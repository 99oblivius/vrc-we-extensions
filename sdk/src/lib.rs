//! The boundary between the vrc-WiredExpressions client and an extension.
//!
//! An extension is a pure state machine: the client feeds it [`Event`]s and
//! carries out the [`Command`]s it returns. The client owns every socket,
//! timer, secret and file; an extension never blocks. Every type here
//! crosses the boundary serialized.
//!
//! The client holds an extension to its [`Manifest`]: a call or a write
//! reaches it only after [`Namespace::check_call`] or
//! [`Namespace::check_write`] passed, and its OSC commands are sent only to
//! its write prefixes ([`Manifest::writes`]).

mod command;
mod event;
mod log;
mod manifest;
mod net;
mod node;
mod osc;
mod validate;
mod value;

pub use command::*;
pub use event::*;
pub use log::*;
pub use manifest::*;
pub use net::*;
pub use node::*;
pub use osc::*;
pub use validate::*;
pub use value::*;

/// Whether Windows reserves `name` for a device in any case, whatever
/// extension follows it (`con`, `nul.txt`'s `nul`, `com1`).
pub fn is_device_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let numbered = (upper.starts_with("COM") || upper.starts_with("LPT"))
        && upper.len() == 4
        && upper.as_bytes()[3].is_ascii_digit();
    numbered || ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
}

/// An extension: a pure state machine.
pub trait Extension {
    fn manifest(&self) -> Manifest;
    /// One event in; the commands it causes out, in order.
    fn handle(&mut self, event: Event) -> Vec<Command>;
}

/// The boundary version this SDK speaks, which a module built against it
/// reports. An event, a command, a variant or a field a module built
/// earlier would not read bumps it.
pub const BOUNDARY_VERSION: u32 = 2;

/// The oldest boundary version the client loads. It rises only to drop
/// support.
pub const MIN_BOUNDARY: u32 = 1;

/// Makes `$ext` (an [`Extension`] and `Default`) the module's extension
/// when built for `wasm32`: writes the exports the client calls.
///
/// | Export | Signature | Contract |
/// |---|---|---|
/// | `vrcwe_version` | `() -> u32` | [`BOUNDARY_VERSION`] |
/// | `vrcwe_alloc` | `(len) -> ptr` | a buffer for the next input |
/// | `vrcwe_manifest` | `() -> u64` | the [`Manifest`] |
/// | `vrcwe_handle` | `(ptr, len) -> u64` | one [`Event`] in, its `Vec<Command>` out |
///
/// Everything crosses as JSON. A `u64` is `ptr << 32 | len` of a reply
/// that stays valid until the module's next call. The buffer passed to
/// `vrcwe_handle` is one `vrcwe_alloc` returned; the call frees it.
#[macro_export]
macro_rules! export {
    ($ext:ty) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            ::std::thread_local! {
                static EXTENSION: ::std::cell::RefCell<$ext> =
                    ::std::cell::RefCell::new(<$ext as ::core::default::Default>::default());
            }

            #[no_mangle]
            pub extern "C" fn vrcwe_version() -> u32 {
                $crate::BOUNDARY_VERSION
            }

            #[no_mangle]
            pub extern "C" fn vrcwe_alloc(len: u32) -> u32 {
                $crate::__export::alloc(len)
            }

            #[no_mangle]
            pub extern "C" fn vrcwe_manifest() -> u64 {
                EXTENSION.with(|e| $crate::__export::manifest(&*e.borrow()))
            }

            #[no_mangle]
            pub extern "C" fn vrcwe_handle(ptr: u32, len: u32) -> u64 {
                // SAFETY: the client passes a buffer `vrcwe_alloc` returned
                // for exactly `len` bytes, once.
                EXTENSION
                    .with(|e| unsafe { $crate::__export::handle(&mut *e.borrow_mut(), ptr, len) })
            }
        };
    };
}

/// What [`export!`] calls. Not for direct use.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub mod __export {
    use std::cell::RefCell;

    use super::{Event, Extension};

    thread_local! {
        static REPLY: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    /// Keeps `bytes` until the next reply; `ptr << 32 | len`.
    fn reply(bytes: Vec<u8>) -> u64 {
        REPLY.with(|r| {
            let mut r = r.borrow_mut();
            *r = bytes;
            ((r.as_ptr() as usize as u64) << 32) | r.len() as u64
        })
    }

    pub fn alloc(len: u32) -> u32 {
        let buf = vec![0u8; len as usize].into_boxed_slice();
        Box::into_raw(buf).cast::<u8>() as usize as u32
    }

    pub fn manifest(ext: &impl Extension) -> u64 {
        reply(serde_json::to_vec(&ext.manifest()).expect("a manifest serializes"))
    }

    /// # Safety
    /// `ptr` is a buffer [`alloc`] returned for exactly `len` bytes, not
    /// yet taken back.
    pub unsafe fn handle(ext: &mut impl Extension, ptr: u32, len: u32) -> u64 {
        let input: Box<[u8]> = Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            ptr as usize as *mut u8,
            len as usize,
        ));
        // An event this version can't read is the client's fault: the
        // panic traps, and the client counts a fault.
        let event: Event =
            serde_json::from_slice(&input).expect("an event of this boundary version");
        drop(input);
        reply(serde_json::to_vec(&ext.handle(event)).expect("commands serialize"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(name: &str, writable: bool, range: Option<Range>) -> Field {
        Field {
            name: name.into(),
            shape: Shape::Number(range),
            writable,
            shared: false,
        }
    }

    /// A list of up to `max` points of three numbers, each within ±10.
    fn path(max: usize) -> Shape {
        Shape::List {
            item: Box::new(Shape::Tuple(vec![
                Shape::Number(Some(Range::new(
                    -10.0, 10.0
                )));
                3
            ])),
            max,
        }
    }

    fn ns() -> Namespace {
        Namespace {
            name: "PROBE".into(),
            fields: vec![
                number("level", true, Some(Range::new(0.0, 1.0))),
                number("seen", false, None),
            ],
            methods: vec![Method {
                name: "nudge".into(),
                params: vec![Param {
                    name: "by".into(),
                    shape: Shape::Number(Some(Range::new(-2.0, 2.0))),
                    options: None,
                    optional: false,
                }],
                returns: Shape::None,
                moves: vec!["level".into()],
                coroutine: false,
                contacts: false,
            }],
            signals: vec![Signal {
                name: "bumped".into(),
                label: "Bumped".into(),
                payload: Shape::Number(None),
            }],
        }
    }

    fn manifest(namespace: Namespace) -> Manifest {
        Manifest {
            name: "probe".into(),
            author: "someone".into(),
            version: "0.1.0".into(),
            namespace: Some(namespace),
            osc_read: vec!["/probe/".into()],
            osc_write: vec!["/probe/".into()],
            pages: vec![Page {
                id: "main".into(),
                title: "Probe".into(),
            }],
            actions: vec![Action {
                label: "Nudge".into(),
                does: Does::Call("nudge".into()),
                in_macros: true,
            }],
            conditions: vec![Condition {
                label: "Level".into(),
                field: "level".into(),
            }],
            secrets: vec!["token".into()],
            notifies: false,
            network: Vec::new(),
            log_prefixes: Vec::new(),
            icons: Vec::new(),
            contacts: false,
        }
    }

    #[test]
    fn a_sound_manifest_validates() {
        assert_eq!(manifest(ns()).validate(), Ok(()));
    }

    #[test]
    fn fields_and_methods_share_one_name_set() {
        let mut n = ns();
        n.methods[0].name = "level".into();
        assert_eq!(
            manifest(n).validate(),
            Err(ManifestError::Duplicate("level".into()))
        );
    }

    #[test]
    fn names_are_identifiers_and_the_namespace_is_in_capitals() {
        let mut n = ns();
        n.name = "Probe".into();
        assert!(matches!(
            manifest(n).validate(),
            Err(ManifestError::BadName(_))
        ));
        let mut n = ns();
        n.fields[0].name = "two words".into();
        assert!(matches!(
            manifest(n).validate(),
            Err(ManifestError::BadName(_))
        ));
        let mut m = manifest(ns());
        m.name = "pro-be".into();
        assert!(matches!(m.validate(), Err(ManifestError::BadName(_))));
        let mut m = manifest(ns());
        m.author = " ".into();
        assert!(matches!(m.validate(), Err(ManifestError::BadName(_))));
    }

    #[test]
    fn a_method_moves_only_writable_fields() {
        let mut n = ns();
        n.methods[0].moves = vec!["seen".into()];
        assert!(matches!(
            manifest(n).validate(),
            Err(ManifestError::MovesUnknown { .. })
        ));
    }

    #[test]
    fn ranges_and_tuples_are_never_empty() {
        let mut n = ns();
        n.fields[0].shape = Shape::Number(Some(Range::new(1.0, 0.0)));
        assert!(matches!(
            manifest(n).validate(),
            Err(ManifestError::BadShape(_))
        ));
        let mut n = ns();
        n.methods[0].params[0].shape = Shape::Number(Some(Range::new(f64::NAN, 1.0)));
        assert!(matches!(
            manifest(n).validate(),
            Err(ManifestError::BadShape(_))
        ));
        let mut n = ns();
        n.methods[0].params[0].shape = Shape::Tuple(Vec::new());
        assert!(matches!(
            manifest(n).validate(),
            Err(ManifestError::BadShape(_))
        ));
        let mut n = ns();
        n.fields[0].shape = Shape::None;
        assert!(matches!(
            manifest(n).validate(),
            Err(ManifestError::Untyped(_))
        ));
    }

    #[test]
    fn shapes_stay_within_the_ceilings() {
        let declare = |shape: Shape| {
            let mut n = ns();
            n.methods[0].params[0].shape = shape;
            manifest(n).validate()
        };
        assert_eq!(declare(path(LIST_MAX)), Ok(()));
        assert!(matches!(
            declare(path(LIST_MAX + 1)),
            Err(ManifestError::BadShape(_))
        ));
        assert_eq!(declare(Shape::Text { max: TEXT_MAX }), Ok(()));
        assert!(matches!(
            declare(Shape::Text { max: TEXT_MAX + 1 }),
            Err(ManifestError::BadShape(_))
        ));
        let deep = Shape::List {
            item: Box::new(path(2)),
            max: 2,
        };
        assert!(matches!(declare(deep), Err(ManifestError::BadShape(_))));
        let wide = Shape::List {
            item: Box::new(Shape::List {
                item: Box::new(Shape::Bool),
                max: LIST_MAX,
            }),
            max: LIST_MAX,
        };
        assert!(matches!(declare(wide), Err(ManifestError::BadShape(_))));
        let nines = Shape::List {
            item: Box::new(Shape::Tuple(vec![Shape::Bool; 9])),
            max: LIST_MAX,
        };
        assert_eq!(declare(nines.clone()), Ok(()));
        let mut n = ns();
        n.methods[0].params[0].shape = nines.clone();
        n.methods[0].params.push(Param {
            optional: false,
            name: "again".into(),
            shape: nines,
            options: None,
        });
        assert!(
            matches!(manifest(n).validate(), Err(ManifestError::BadShape(_))),
            "the values of all arguments count together"
        );
    }

    #[test]
    fn nested_values_are_checked_and_clamped() {
        let shape = path(2);
        let point = |a: f64| Value::List(vec![Value::Number(a); 3]);
        assert_eq!(
            shape.check("p", Value::List(vec![point(1.0), point(50.0)])),
            Ok(Value::List(vec![point(1.0), point(10.0)]))
        );
        assert_eq!(
            shape.check("p", Value::List(vec![point(0.0); 3])),
            Err(CheckError::TooMany {
                name: "p".into(),
                max: 2
            })
        );
        assert_eq!(
            shape.check(
                "p",
                Value::List(vec![Value::List(vec![Value::Number(0.0)])])
            ),
            Err(CheckError::Length {
                name: "p".into(),
                expected: 3,
                got: 1
            })
        );
        let texty = Value::List(vec![Value::List(vec![
            Value::Number(0.0),
            Value::Text("x".into()),
            Value::Number(0.0),
        ])]);
        assert!(matches!(
            shape.check("p", texty),
            Err(CheckError::Type { .. })
        ));
        let nan = Value::List(vec![point(f64::NAN)]);
        assert_eq!(
            shape.check("p", nan),
            Err(CheckError::NotFinite("p".into()))
        );
        assert_eq!(
            Shape::Text { max: 3 }.check("t", Value::Text("four".into())),
            Err(CheckError::TooLong {
                name: "t".into(),
                max: 3
            })
        );
    }

    #[test]
    fn a_published_value_or_a_reply_must_fit_its_declaration() {
        let n = ns();
        assert_eq!(
            n.check_published("level", Value::Number(5.0)),
            Some(Value::Number(1.0))
        );
        assert_eq!(n.check_published("level", Value::Text("x".into())), None);
        assert_eq!(n.check_published("level", Value::None), Some(Value::None));
        assert_eq!(n.check_published("nope", Value::Number(0.0)), None);
        assert_eq!(n.check_reply("nudge", Value::Number(1.0)), None);
        assert_eq!(n.check_reply("nudge", Value::None), Some(Value::None));
    }

    #[test]
    fn an_action_calls_a_method_it_has_or_sets_a_writable_field() {
        let mut m = manifest(ns());
        m.actions[0].does = Does::Call("poke".into());
        assert_eq!(m.validate(), Err(ManifestError::BadAction("Nudge".into())));
        let mut m = manifest(ns());
        m.actions[0].does = Does::Set;
        assert_eq!(m.validate(), Ok(()));
        let n = m.namespace.as_mut().unwrap();
        n.methods[0].moves.clear();
        n.fields.retain(|f| !f.writable);
        assert_eq!(m.validate(), Err(ManifestError::BadAction("Nudge".into())));
        let mut m = manifest(ns());
        m.actions.push(m.actions[0].clone());
        assert_eq!(m.validate(), Err(ManifestError::Duplicate("Nudge".into())));
    }

    #[test]
    fn signals_share_the_name_set_and_their_labels_with_conditions() {
        let mut n = ns();
        n.signals[0].name = "seen".into();
        assert_eq!(
            manifest(n).validate(),
            Err(ManifestError::Duplicate("seen".into()))
        );
        let mut m = manifest(ns());
        m.conditions[0].label = "Bumped".into();
        assert_eq!(m.validate(), Err(ManifestError::Duplicate("Bumped".into())));
        let mut n = ns();
        n.signals[0].payload = Shape::None;
        assert_eq!(manifest(n).validate(), Ok(()));
    }

    #[test]
    fn a_signals_payload_must_fit_its_declaration() {
        let n = ns();
        assert_eq!(
            n.check_signal("bumped", Value::Number(2.0)),
            Some(Value::Number(2.0))
        );
        assert_eq!(n.check_signal("bumped", Value::Text("x".into())), None);
        assert_eq!(n.check_signal("seen", Value::Number(2.0)), None);
    }

    #[test]
    fn a_condition_compares_an_on_off_or_number_field_it_has() {
        let mut m = manifest(ns());
        m.conditions[0].field = "nope".into();
        assert_eq!(
            m.validate(),
            Err(ManifestError::BadCondition("Level".into()))
        );
        let mut m = manifest(ns());
        m.namespace.as_mut().unwrap().fields[0].shape = Shape::Text { max: 8 };
        assert_eq!(
            m.validate(),
            Err(ManifestError::BadCondition("Level".into()))
        );
        let mut m = manifest(ns());
        m.conditions.push(m.conditions[0].clone());
        assert_eq!(m.validate(), Err(ManifestError::Duplicate("Level".into())));
    }

    #[test]
    fn a_network_grant_names_where_it_reaches_and_broad_ones_are_told_apart() {
        let grant = |what, to, from, upto| NetGrant {
            what,
            proto: Proto::Tcp,
            to,
            ports: Ports { from, to: upto },
        };
        let domain = |name: &str| NetTo::Domain {
            name: name.into(),
            lan: false,
        };
        let mut m = manifest(ns());
        m.network = vec![
            grant(NetWhat::Connect, NetTo::Loopback, 12345, 12345),
            grant(NetWhat::Connect, domain("api.twitch.tv"), 443, 443),
            grant(NetWhat::Connect, domain("*.lovense.club"), 30010, 30010),
            grant(
                NetWhat::Connect,
                NetTo::Lan("192.168.1.0/24".into()),
                8123,
                8123,
            ),
            grant(NetWhat::Connect, NetTo::Lan("*.local".into()), 80, 80),
            grant(NetWhat::Listen, NetTo::Loopback, 9000, 9010),
        ];
        assert_eq!(m.validate(), Ok(()));
        assert!(m.network.iter().all(|g| !g.broad()));
        for (i, bad) in [
            grant(NetWhat::Connect, domain("10.0.0.1"), 80, 80),
            grant(NetWhat::Connect, domain("printer.local"), 80, 80),
            grant(NetWhat::Connect, domain("bad_name.com"), 80, 80),
            grant(NetWhat::Listen, domain("api.twitch.tv"), 80, 80),
            grant(
                NetWhat::Connect,
                NetTo::Lan("192.168.1.0/33".into()),
                80,
                80,
            ),
            grant(NetWhat::Connect, NetTo::Loopback, 0, 80),
            grant(NetWhat::Connect, NetTo::Loopback, 90, 80),
        ]
        .into_iter()
        .enumerate()
        {
            let mut m = manifest(ns());
            m.network = vec![bad];
            assert_eq!(m.validate(), Err(ManifestError::BadNetGrant(0)), "{i}");
        }
        for broad in [
            grant(NetWhat::Connect, NetTo::Any, 80, 80),
            grant(NetWhat::Connect, NetTo::Lan("10.0.0.0/8".into()), 80, 80),
            grant(NetWhat::Connect, NetTo::Loopback, 1, u16::MAX),
            grant(NetWhat::Listen, NetTo::Lan("192.168.1.0/24".into()), 80, 80),
        ] {
            assert!(broad.broad(), "{broad:?}");
        }
    }

    #[test]
    fn bytes_cross_as_base64() {
        let data = Data::Bytes(vec![0, 1, 254, 255]);
        let json = serde_json::to_string(&data).unwrap();
        assert_eq!(json, r#"{"Bytes":"AAH+/w=="}"#);
        assert_eq!(serde_json::from_str::<Data>(&json).unwrap(), data);
    }

    #[test]
    fn cut_to_keeps_whole_characters() {
        assert_eq!(cut_to("héllo", 2), "h");
        assert_eq!(cut_to("héllo", 3), "hé");
        assert_eq!(cut_to("hi", 9), "hi");
    }

    #[test]
    fn log_prefixes_are_text_without_control_characters_and_few() {
        let mut m = manifest(ns());
        m.log_prefixes = vec!["[Behaviour] OnPlayerJoined ".into()];
        assert_eq!(m.validate(), Ok(()));
        assert!(m.reads_log("[Behaviour] OnPlayerJoined Amy (usr_1)"));
        assert!(!m.reads_log("[Behaviour] OnPlayerLeft Amy"));
        m.log_prefixes = vec!["a\nb".into()];
        assert_eq!(
            m.validate(),
            Err(ManifestError::BadLogPrefix("a\nb".into()))
        );
        m.log_prefixes = vec![String::new()];
        assert!(matches!(m.validate(), Err(ManifestError::BadLogPrefix(_))));
        m.log_prefixes = vec!["x".into(); LOG_PREFIXES_MAX + 1];
        assert_eq!(m.validate(), Err(ManifestError::TooManyLogPrefixes));
    }

    #[test]
    fn a_list_items_data_fits_the_parameters_picking_from_it() {
        let mut n = ns();
        n.methods[0].params[0].options = Some("levels".into());
        assert_eq!(
            n.check_item("levels", Value::Number(5.0)),
            Some(Value::Number(2.0)),
            "clamped as a call would be"
        );
        assert_eq!(n.check_item("levels", Value::Text("x".into())), None);
        assert_eq!(
            n.check_item("other", Value::Bool(true)),
            Some(Value::Bool(true)),
            "a list no parameter picks from"
        );
    }

    #[test]
    fn an_icon_draws_from_itself_alone() {
        let ok = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><defs><linearGradient id="g"/></defs><path fill="url(#g)" d="M0 0h16v16z"/><use href="#g"/></svg>"##;
        assert!(icon_is_self_contained(ok));
        for bad in [
            r#"<svg><image href="/dev/zero"/></svg>"#,
            r#"<svg><IMAGE xlink:href="a.png"/></svg>"#,
            r#"<svg><use href = 'other.svg#a'/></svg>"#,
            r##"<svg><filter><feImage href="#a"/></filter></svg>"##,
            r#"<svg><rect fill="url( 'file:///etc/x')"/></svg>"#,
            r#"<svg><style>@import "x.css";</style></svg>"#,
            r#"<!DOCTYPE svg [<!ENTITY x SYSTEM "file:///etc/passwd">]><svg/>"#,
            r#"<svg><script>1</script></svg>"#,
            r#"<svg><foreignObject/></svg>"#,
            r#"<svg><a href="data:text/plain,x"/></svg>"#,
        ] {
            assert!(!icon_is_self_contained(bad), "{bad}");
        }
    }

    #[test]
    fn a_manifests_icons_are_few_named_small_and_self_contained() {
        let mut m = manifest(ns());
        let icon = |name: &str, svg: &str| Icon {
            name: name.into(),
            svg: svg.into(),
        };
        m.icons = vec![icon("play", "<svg/>"), icon("stop", "<svg/>")];
        assert_eq!(m.validate(), Ok(()));
        m.icons.push(icon("play", "<svg/>"));
        assert_eq!(m.validate(), Err(ManifestError::BadIcon("play".into())));
        m.icons = vec![icon("no good", "<svg/>")];
        assert!(matches!(m.validate(), Err(ManifestError::BadIcon(_))));
        m.icons = vec![icon("big", &"x".repeat(ICON_MAX + 1))];
        assert!(matches!(m.validate(), Err(ManifestError::BadIcon(_))));
        m.icons = vec![icon("far", r#"<svg><image href="a.png"/></svg>"#)];
        assert!(matches!(m.validate(), Err(ManifestError::BadIcon(_))));
        m.icons = (0..=ICONS_MAX)
            .map(|i| icon(&format!("i{i}"), "<svg/>"))
            .collect();
        assert_eq!(m.validate(), Err(ManifestError::TooManyIcons));
    }

    #[test]
    fn a_mark_moves_its_own_kind_on_its_own_id_only() {
        let camera = [[1.0, 2.0, 3.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]];
        let mut view = Node::Split {
            main: Box::new(Node::Column(vec![
                Node::Scene3d {
                    id: "s".into(),
                    path: Vec::new(),
                    keys: Vec::new(),
                    selected: None,
                    camera: None,
                    overlay: None,
                    closed: false,
                    hint: None,
                },
                Node::Timeline {
                    id: "t".into(),
                    keys: vec![0.0],
                    length: 1.0,
                    selected: None,
                    playhead: None,
                    lane: None,
                },
            ])),
            side: Box::new(Node::Divider),
        };
        let before = view.clone();
        assert!(!view.mark("s", &Mark::Playhead(Some(1.0))), "another kind");
        assert!(!view.mark("x", &Mark::Camera(Some(camera))), "another id");
        assert_eq!(view, before);
        assert!(view.mark("s", &Mark::Camera(Some(camera))));
        assert!(!view.mark("s", &Mark::Camera(Some(camera))), "no change");
        assert!(view.mark("t", &Mark::Playhead(Some(0.5))));
        let Node::Split { main, .. } = &view else {
            unreachable!()
        };
        assert!(matches!(
            main.as_ref(),
            Node::Column(n) if matches!(n[0], Node::Scene3d { camera: Some(c), .. } if c == camera)
                && matches!(n[1], Node::Timeline { playhead: Some(0.5), .. })
        ));
        assert!(!Mark::Playhead(Some(f32::NAN)).finite());
        assert!(!Mark::Camera(Some([[f32::INFINITY; 3]; 3])).finite());
        assert!(Mark::Camera(None).finite());
    }

    #[test]
    fn the_editor_nodes_fit_only_finite_and_within_the_limits() {
        let hinted = |path: Vec<[f32; 3]>, hint: usize| Node::Scene3d {
            id: "s".into(),
            path,
            keys: vec![0],
            selected: None,
            camera: Some([[0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]]),
            overlay: None,
            closed: false,
            hint: Some("x".repeat(hint)),
        };
        let scene = |path: Vec<[f32; 3]>| hinted(path, 8);
        assert!(scene(vec![[0.0; 3]; 10]).fits());
        assert!(
            !hinted(vec![[0.0; 3]], TEXT_MAX + 1).fits(),
            "a hint is text like any"
        );
        assert!(!scene(vec![[f32::NAN, 0.0, 0.0]]).fits());
        assert!(!scene(vec![[0.0; 3]; VIEW_NODES_MAX]).fits());
        let timeline = |length: f32| Node::Timeline {
            id: "t".into(),
            keys: vec![0.0, 2.0],
            length,
            selected: Some(1),
            playhead: Some(1.0),
            lane: None,
        };
        assert!(timeline(4.0).fits());
        assert!(!timeline(f32::INFINITY).fits());
        let laned = |points: usize, dot: LaneDot| Node::Timeline {
            id: "t".into(),
            keys: vec![0.0, 2.0],
            length: 4.0,
            selected: None,
            playhead: None,
            lane: Some(Lane {
                min: 0.0,
                max: 2.0,
                base: 1.0,
                label: "1×".into(),
                curves: vec![LaneCurve {
                    points: vec![[0.0, 1.0]; points],
                    faint: false,
                }],
                dots: vec![dot],
            }),
        };
        let dot = |at: [f32; 2], from: f32, to: f32| LaneDot { at, from, to };
        assert!(laned(10, dot([1.0, 1.5], 0.0, 2.0)).fits());
        assert!(!laned(10, dot([f32::NAN, 1.5], 0.0, 2.0)).fits());
        assert!(
            !laned(10, dot([1.0, 1.5], 2.0, 0.0)).fits(),
            "a dot's span is ordered"
        );
        assert!(
            !laned(VIEW_NODES_MAX, dot([1.0, 1.5], 0.0, 2.0)).fits(),
            "a lane's points count as nodes"
        );
        let overlaid = |text: usize| Node::Scene3d {
            id: "s".into(),
            path: vec![[0.0; 3]; 2],
            keys: vec![0],
            selected: None,
            camera: None,
            overlay: Some(Box::new(Node::Text("x".repeat(text)))),
            closed: false,
            hint: None,
        };
        assert!(overlaid(8).fits());
        assert!(
            !overlaid(TEXT_MAX + 1).fits(),
            "a scene's overlay is checked too"
        );
        let dragged = |at: f64| UiEvent::Dragged {
            id: "t".into(),
            index: 0,
            at,
            value: 1.0,
            done: true,
        };
        assert!(dragged(1.0).fits());
        assert!(!dragged(f64::INFINITY).fits());
        let split = Node::Split {
            main: Box::new(timeline(4.0)),
            side: Box::new(Node::Text("x".repeat(TEXT_MAX + 1))),
        };
        assert!(!split.fits(), "a split's sides are checked too");
        assert!(UiEvent::Moved {
            id: "t".into(),
            index: 0,
            value: 2.0
        }
        .fits());
        assert!(!UiEvent::Moved {
            id: "t".into(),
            index: 0,
            value: f64::NAN
        }
        .fits());
    }

    #[test]
    fn a_slider_needs_an_ordered_range_above_0_by_ratio_and_a_hold_key_a_bindable_one() {
        let slider = |min: f64, max: f64, log: bool| Node::Slider {
            id: "speed".into(),
            label: "Speed".into(),
            min,
            max,
            value: Some(1.0),
            log,
            live: false,
        };
        assert!(slider(0.01, 100.0, true).fits());
        assert!(slider(-1.0, 1.0, false).fits());
        assert!(!slider(0.0, 100.0, true).fits(), "a ratio from 0");
        assert!(!slider(5.0, 5.0, false).fits());
        assert!(!slider(0.0, f64::INFINITY, false).fits());
        let hold = |key| Node::HoldButton {
            id: "forward".into(),
            icon: "up".into(),
            tip: "Forward".into(),
            key: Some(key),
            disabled: false,
        };
        for key in [
            Key::Char('w'),
            Key::Char('7'),
            Key::Space,
            Key::Shift,
            Key::Left,
        ] {
            assert!(hold(key).fits(), "{key:?}");
        }
        for key in [
            Key::Char('W'),
            Key::Char('é'),
            Key::Char(' '),
            Key::Char('\u{1b}'),
        ] {
            assert!(!hold(key).fits(), "{key:?}");
        }
        assert!(UiEvent::Wheel { lines: -2.5 }.fits());
        assert!(!UiEvent::Wheel { lines: f64::NAN }.fits());
    }

    #[test]
    fn significant_keeps_the_leading_figures_at_any_scale() {
        for (value, want) in [
            (0.01234, 0.012),
            (2.5, 2.5),
            (3.125, 3.1),
            (38.4, 38.0),
            (360.0, 360.0),
            (99.6, 100.0),
            (-1.25, -1.3),
            (0.0, 0.0),
        ] {
            let got = significant(value, 2);
            assert!((got - want).abs() < 1e-12, "{value}: {got}");
        }
        assert!(significant(f64::NAN, 2).is_nan());
    }

    #[test]
    fn an_item_has_a_store_key_and_a_label() {
        let item = |key: &str, label: &str| Item {
            key: key.into(),
            label: label.into(),
        };
        assert!(item("track_1", "Orbit").fits());
        assert!(!item("track 1", "Orbit").fits());
        assert!(!item("con", "Orbit").fits(), "a Windows device name");
        assert!(!item("track_1", " ").fits());
        assert!(!item("track_1", "a\nb").fits());
    }

    #[test]
    fn an_imported_label_is_numbered_past_the_taken_ones() {
        let taken = ["Orbit", "Orbit (2)"];
        assert_eq!(unique_label("Pan", |l| taken.contains(&l)), "Pan");
        assert_eq!(unique_label("Orbit", |l| taken.contains(&l)), "Orbit (3)");
        let long = "é".repeat(OPTION_LABEL_MAX);
        let numbered = unique_label(&long, |l| l == cut_to(&long, OPTION_LABEL_MAX));
        assert!(numbered.len() <= OPTION_LABEL_MAX && numbered.ends_with(" (2)"));
    }

    #[test]
    fn a_ui_event_fits_only_within_the_limits() {
        let text = |n| UiEvent::Text {
            id: "name".into(),
            value: "x".repeat(n),
        };
        assert!(text(TEXT_MAX).fits());
        assert!(!text(TEXT_MAX + 1).fits());
        assert!(!UiEvent::Number {
            id: "n".into(),
            value: f64::NAN
        }
        .fits());
    }

    #[test]
    fn text_is_found_at_any_depth() {
        assert!(Value::List(vec![Value::List(vec![Value::Text("a".into())])]).holds_text());
        assert!(!Value::List(vec![Value::Number(1.0)]).holds_text());
        assert!(Shape::Tuple(vec![Shape::Bool, Shape::Text { max: 1 }]).holds_text());
        assert!(!path(4).holds_text());
    }

    #[test]
    fn prefixes_never_reach_avatar_addresses_or_the_root() {
        for bad in [
            "/",
            "//",
            "/avatar/",
            "/avatar/parameters/",
            "/Avatar/",
            "/probe",
            "probe/",
            "/a//b/",
            "/*/",
            "/{avatar,x}/",
            "/a b/",
            "/caf\u{e9}/",
        ] {
            let mut m = manifest(ns());
            m.osc_write = vec![bad.into()];
            assert_eq!(
                m.validate(),
                Err(ManifestError::BadPrefix(bad.into())),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_namespace_is_held_to_its_fields_methods_and_parameters() {
        let mut n = ns();
        n.fields
            .extend((n.fields.len()..FIELDS_MAX).map(|i| number(&format!("f{i}"), false, None)));
        assert_eq!(n.validate(), Ok(()));
        n.fields.push(number("one_more", false, None));
        assert_eq!(n.validate(), Err(ManifestError::TooLarge("PROBE".into())));
        let mut n = ns();
        let method = n.methods[0].clone();
        n.methods = (0..=METHODS_MAX)
            .map(|i| Method {
                name: format!("m{i}"),
                moves: Vec::new(),
                ..method.clone()
            })
            .collect();
        assert_eq!(n.validate(), Err(ManifestError::TooLarge("PROBE".into())));
        let mut n = ns();
        let param = n.methods[0].params[0].clone();
        n.methods[0].params = (0..=PARAMS_MAX)
            .map(|i| Param {
                optional: false,
                name: format!("p{i}"),
                ..param.clone()
            })
            .collect();
        assert_eq!(n.validate(), Err(ManifestError::TooLarge("PROBE".into())));
    }

    #[test]
    fn a_manifest_names_at_most_sixteen_prefixes_each_way() {
        let mut m = manifest(ns());
        m.osc_read = (0..OSC_PREFIXES_MAX).map(|i| format!("/p{i}/")).collect();
        assert_eq!(m.validate(), Ok(()));
        m.osc_write = (0..=OSC_PREFIXES_MAX).map(|i| format!("/p{i}/")).collect();
        assert_eq!(m.validate(), Err(ManifestError::TooManyPrefixes));
    }

    #[test]
    fn an_osc_message_fits_with_a_literal_address_finite_numbers_and_its_size() {
        let message = |addr: &str, args: Vec<OscArg>| OscMessage {
            addr: addr.into(),
            args,
        };
        let pose = message("/usercamera/Pose", vec![OscArg::Float(0.5); 6]);
        assert_eq!(pose.encoded_len(), 20 + 8 + 24);
        assert!(pose.fits());
        for addr in [
            "/usercamera/*",
            "/usercamera//Pose",
            "/usercamera/",
            "usercamera",
            "/a b",
        ] {
            assert!(!message(addr, Vec::new()).fits(), "{addr}");
        }
        assert!(!message("/a", vec![OscArg::Float(f32::NAN)]).fits());
        assert!(!message("/a", vec![OscArg::Float(f32::INFINITY)]).fits());
        assert!(!message("/a", vec![OscArg::Text("a\0b".into())]).fits());
        let edge = OSC_MESSAGE_MAX - 4 - 4;
        assert!(message("/a", vec![OscArg::Text("x".repeat(edge - 1))]).fits());
        assert!(!message("/a", vec![OscArg::Text("x".repeat(edge))]).fits());
        let long = format!("/{}", "a".repeat(OSC_ADDR_MAX));
        assert!(!message(&long, Vec::new()).fits());
    }

    #[test]
    fn a_store_key_is_a_short_identifier_and_never_a_windows_device() {
        assert!(is_store_key("library") && is_store_key(&"k".repeat(64)));
        for key in [
            "con",
            "Nul",
            "aux",
            "PRN",
            "com1",
            "LPT9",
            "a.b",
            "",
            &"k".repeat(65),
        ] {
            assert!(!is_store_key(key), "{key}");
        }
        assert!(is_store_key("com10") && is_store_key("console"));
    }

    #[test]
    fn pages_are_few_with_short_ids_and_label_titles() {
        let page = |id: &str, title: &str| Page {
            id: id.into(),
            title: title.into(),
        };
        let mut m = manifest(ns());
        m.pages = (0..PAGES_MAX)
            .map(|i| page(&format!("p{i}"), "Page"))
            .collect();
        assert_eq!(m.validate(), Ok(()));
        m.pages.push(page("one_more", "Page"));
        assert_eq!(m.validate(), Err(ManifestError::TooManyPages));
        for (id, title) in [(&*"p".repeat(65), "Page"), ("p", ""), ("p", "two\nlines")] {
            m.pages = vec![page(id, title)];
            assert!(m.validate().is_err(), "{id:?} {title:?}");
        }
    }

    #[test]
    fn page_ids_are_unique_identifiers() {
        let mut m = manifest(ns());
        m.pages.push(m.pages[0].clone());
        assert_eq!(m.validate(), Err(ManifestError::Duplicate("main".into())));
        let mut m = manifest(ns());
        m.pages[0].id = "two words".into();
        assert!(matches!(m.validate(), Err(ManifestError::BadName(_))));
        assert!(manifest(ns()).has_page("main"));
    }

    #[test]
    fn store_keys_are_short_identifiers() {
        assert!(is_store_key("sequence"));
        assert!(!is_store_key("../escape"));
        assert!(!is_store_key("a/b"));
        assert!(!is_store_key(&"k".repeat(65)));
    }

    #[test]
    fn prefixes_match_whole_segments() {
        let m = manifest(ns());
        assert!(m.reads("/probe/value"));
        assert!(!m.reads("/prober/value"));
        assert!(m.writes("/probe/value"));
        assert!(!m.writes("/avatar/parameters/value"));
    }

    #[test]
    fn a_write_is_typed_finite_and_clamped() {
        let n = ns();
        assert_eq!(
            n.check_write("level", Value::Number(3.0)),
            Ok(Value::Number(1.0))
        );
        assert_eq!(
            n.check_write("seen", Value::Number(0.5)),
            Err(CheckError::ReadOnly("seen".into()))
        );
        assert_eq!(
            n.check_write("nope", Value::Number(0.5)),
            Err(CheckError::UnknownField("nope".into()))
        );
        assert!(matches!(
            n.check_write("level", Value::Bool(true)),
            Err(CheckError::Type { .. })
        ));
        assert_eq!(
            n.check_write("level", Value::Number(f64::INFINITY)),
            Err(CheckError::NotFinite("level".into()))
        );
    }

    #[test]
    fn a_call_is_counted_typed_and_clamped() {
        let n = ns();
        assert_eq!(
            n.check_call("nudge", vec![Value::Number(-5.0)]),
            Ok(vec![Value::Number(-2.0)])
        );
        assert!(matches!(
            n.check_call("nudge", vec![]),
            Err(CheckError::Arity {
                expected: 1,
                got: 0,
                ..
            })
        ));
        assert!(matches!(
            n.check_call("nudge", vec![Value::None]),
            Err(CheckError::Type { .. })
        ));
        assert_eq!(
            n.check_call("poke", vec![]),
            Err(CheckError::UnknownMethod("poke".into()))
        );
    }

    #[test]
    fn events_and_commands_cross_serialized() {
        let event = Event::Call {
            id: 7,
            method: "nudge".into(),
            args: vec![
                Value::Number(1.5),
                Value::List(vec![Value::Text("a".into())]),
            ],
            from: Some(Contact {
                id: "c1".into(),
                label: "Amy".into(),
            }),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), event);
        let command = Command::Osc(OscMessage {
            addr: "/probe/value".into(),
            args: vec![OscArg::Float(0.5), OscArg::Int(2), OscArg::Bool(true)],
        });
        let json = serde_json::to_string(&command).unwrap();
        assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), command);
        let m = manifest(ns());
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Manifest>(&json).unwrap(), m);
    }

    #[test]
    fn secret_slots_are_store_keys_declared_once_and_few() {
        let mut m = manifest(ns());
        assert_eq!(m.validate(), Ok(()));
        assert!(m.has_secret("token") && !m.has_secret("other"));
        m.secrets.push("token".into());
        assert_eq!(m.validate(), Err(ManifestError::Duplicate("token".into())));
        m.secrets = vec!["../token".into()];
        assert_eq!(m.validate(), Err(ManifestError::BadName("../token".into())));
        m.secrets = (0..=SECRETS_MAX).map(|i| format!("s{i}")).collect();
        assert_eq!(m.validate(), Err(ManifestError::TooManySecrets));
        let mut json = serde_json::to_value(manifest(ns())).unwrap();
        json.as_object_mut().unwrap().remove("secrets");
        let older: Manifest = serde_json::from_value(json).unwrap();
        assert!(
            older.secrets.is_empty(),
            "a manifest from before secrets declares none"
        );
    }

    #[test]
    fn a_view_fits_within_its_nodes_and_depth() {
        let column = |n: usize| Node::Column(vec![Node::Text("x".into()); n]);
        assert!(column(VIEW_NODES_MAX - 1).fits());
        assert!(!column(VIEW_NODES_MAX).fits());
        let list = Node::List {
            id: "l".into(),
            items: vec!["row".into(); VIEW_NODES_MAX],
            selected: None,
        };
        assert!(!list.fits(), "a list's rows count as nodes");
        let nest =
            |depth: usize| (1..depth).fold(Node::Text("x".into()), |n, _| Node::Row(vec![n]));
        assert!(nest(VIEW_DEPTH_MAX).fits());
        assert!(!nest(VIEW_DEPTH_MAX + 1).fits());
    }

    #[test]
    fn a_views_text_is_held_per_string_and_in_all() {
        let text = |n: usize| Node::Text("x".repeat(n));
        assert!(text(TEXT_MAX).fits());
        assert!(!text(TEXT_MAX + 1).fits());
        let column = |n: usize| Node::Column(vec![text(TEXT_MAX); n]);
        assert!(column(VIEW_TEXT_MAX / TEXT_MAX).fits());
        assert!(!column(VIEW_TEXT_MAX / TEXT_MAX + 1).fits());
        let copy = |n: usize| Node::Copy {
            label: "Copy".into(),
            text: "x".repeat(n),
        };
        assert!(copy(VIEW_COPY_MAX).fits(), "a copy text isn't shown");
        assert!(!Node::Column(vec![copy(VIEW_COPY_MAX), copy(1)]).fits());
    }

    #[test]
    fn a_list_is_one_a_parameter_picks_from() {
        let mut n = ns();
        assert!(!n.picks_from("levels"));
        n.methods[0].params[0].options = Some("levels".into());
        assert!(n.picks_from("levels") && !n.picks_from("other"));
    }
}
