//! What the module declares: the `@CAMERA` namespace, its fields and methods,
//! the OSC prefixes it reads and writes, the page, the actions, the condition
//! and the packaged icons. `lib.rs` keeps the flow.

use vrc_we_sdk::{
    Action, Condition, Does, Field, Manifest, Method, Namespace, Page, Param, Range, Shape, Signal,
};

use crate::icons;
use crate::motion::{HANDLE_MAX, PACE_RANGE, PATH_MAX, STEP_REACH, STEP_SECONDS, STEP_TURN};
use crate::{NAME_MAX, PAGE, PLAY_ENDED, POSE_FIELDS, POSITIONS, ROOT, SLIDERS, TOGGLES, TRACKS};

pub(crate) fn build() -> Manifest {
    let within = |limit: f64| Shape::Number(Some(Range::new(-limit, limit)));
    let param = |name: &str, shape: Shape| Param {
        name: name.to_owned(),
        shape,
        options: None,
        optional: false,
    };
    let picked = |name: &str, shape: Shape, list: &str| Param {
        options: Some(list.to_owned()),
        ..param(name, shape)
    };
    // What a contact controlling this client may call: everything but
    // reading or editing the library.
    let method = |name: &str, params: Vec<Param>, returns, moves: &[&str], coroutine| Method {
        name: name.to_owned(),
        params,
        returns,
        moves: moves.iter().map(|m| (*m).to_owned()).collect(),
        coroutine,
        contacts: !matches!(name, "track" | "position" | "add_key"),
    };
    let axes = |names: [&str; 3], limit: f64| {
        names
            .iter()
            .map(|n| param(n, within(limit)))
            .collect::<Vec<_>>()
    };
    let mut step = vec![within(STEP_REACH); 3];
    step.extend([within(STEP_TURN), within(STEP_TURN), within(STEP_TURN)]);
    step.push(Shape::Number(Some(Range::new(0.0, STEP_SECONDS))));
    let handle = Shape::Number(Some(Range::new(0.0, HANDLE_MAX)));
    step.extend([handle.clone(), handle]);
    step.push(Shape::Number(Some(Range::new(0.0, 1.0))));
    step.push(Shape::Number(Some(Range::new(PACE_RANGE.0, PACE_RANGE.1))));
    let path = Shape::List {
        item: Box::new(Shape::Tuple(step)),
        max: PATH_MAX,
    };
    // A name is at most NAME_MAX characters of up to 4 bytes each.
    let name = || param("name", Shape::Text { max: NAME_MAX * 4 });
    let seconds = || {
        param(
            "seconds",
            Shape::Number(Some(Range::new(0.0, STEP_SECONDS))),
        )
    };
    // Only what VRChat reports is shared: a slider or a toggle reads
    // what was last set on this client.
    let field = |name: &str, shape, shared| Field {
        name: name.to_owned(),
        shape,
        writable: true,
        shared,
    };
    let mut fields: Vec<Field> = POSE_FIELDS
        .iter()
        .map(|f| field(f, Shape::Number(None), true))
        .collect();
    fields.push(field(
        "mode",
        Shape::Number(Some(Range::new(0.0, 6.0))),
        true,
    ));
    fields.extend(
        SLIDERS
            .iter()
            .map(|(f, _, _, lo, hi, _)| field(f, Shape::Number(Some(Range::new(*lo, *hi))), false)),
    );
    fields.extend(TOGGLES.iter().map(|(f, ..)| field(f, Shape::Bool, false)));
    let (moves, turns) = (["dx", "dy", "dz"], ["dpitch", "dyaw", "droll"]);
    let (at, facing) = (&["x", "y", "z"], &["pitch", "yaw", "roll"]);
    Manifest {
        name: "camera".to_owned(),
        author: "oblivius".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        namespace: Some(Namespace {
            name: "CAMERA".to_owned(),
            fields,
            methods: vec![
                method("move_local", axes(moves, 10.0), Shape::None, at, false),
                method("move_world", axes(moves, 10.0), Shape::None, at, false),
                method("turn_local", axes(turns, 180.0), Shape::None, facing, false),
                method("turn_world", axes(turns, 180.0), Shape::None, facing, false),
                method("capture", Vec::new(), Shape::None, &[], false),
                method("capture_delayed", Vec::new(), Shape::None, &[], false),
                method("close", Vec::new(), Shape::None, &[], false),
                method(
                    "fly_path",
                    vec![picked("steps", path.clone(), TRACKS)],
                    Shape::None,
                    &POSE_FIELDS,
                    true,
                ),
                method(
                    "fly_to",
                    vec![
                        picked(
                            "pose",
                            Shape::Tuple(vec![Shape::Number(None); 6]),
                            POSITIONS,
                        ),
                        seconds(),
                    ],
                    Shape::None,
                    &POSE_FIELDS,
                    true,
                ),
                method("track", vec![name()], path, &[], false),
                method(
                    "position",
                    vec![name()],
                    Shape::Tuple(vec![Shape::Number(None); 6]),
                    &[],
                    false,
                ),
                method("stop", Vec::new(), Shape::None, &[], false),
                method("add_key", Vec::new(), Shape::None, &[], false),
            ],
            signals: vec![Signal {
                name: PLAY_ENDED.to_owned(),
                label: "Play ended".to_owned(),
                payload: Shape::None,
            }],
        }),
        osc_read: vec![ROOT.to_owned()],
        osc_write: vec![ROOT.to_owned()],
        pages: vec![Page {
            id: PAGE.to_owned(),
            title: "Camera".to_owned(),
        }],
        actions: [
            ("Capture", Does::Call("capture".into())),
            ("Capture after delay", Does::Call("capture_delayed".into())),
            ("Close the camera", Does::Call("close".into())),
            ("Set a camera setting", Does::Set),
            ("Go to position (same world)", Does::Call("fly_to".into())),
            ("Play track", Does::Call("fly_path".into())),
            ("Stop playing", Does::Call("stop".into())),
            ("Add keyframe", Does::Call("add_key".into())),
            ("Move (camera axes)", Does::Call("move_local".into())),
            ("Move (world axes)", Does::Call("move_world".into())),
            ("Turn (camera axes)", Does::Call("turn_local".into())),
            ("Turn (world axes)", Does::Call("turn_world".into())),
        ]
        .into_iter()
        .map(|(label, does)| Action {
            label: label.to_owned(),
            does,
            in_macros: true,
        })
        .collect(),
        conditions: vec![Condition {
            label: "Camera mode".to_owned(),
            field: "mode".to_owned(),
        }],
        secrets: Vec::new(),
        notifies: false,
        network: Vec::new(),
        log_prefixes: Vec::new(),
        contacts: false,
        icons: icons::icons(),
    }
}
