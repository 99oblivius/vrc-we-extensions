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
        summary: method_summary(name).to_owned(),
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
    let field = |name: &str, shape, shared, summary: &str| Field {
        name: name.to_owned(),
        shape,
        writable: true,
        shared,
        summary: summary.to_owned(),
    };
    let mut fields: Vec<Field> = POSE_FIELDS
        .iter()
        .zip(POSE_SUMMARIES)
        .map(|(f, summary)| field(f, Shape::Number(None), true, summary))
        .collect();
    fields.push(field(
        "mode",
        Shape::Number(Some(Range::new(0.0, 6.0))),
        true,
        "Camera mode, 0 Off through 6 Drone",
    ));
    fields.extend(SLIDERS.iter().map(|(f, _, _, lo, hi, _, summary)| {
        field(f, Shape::Number(Some(Range::new(*lo, *hi))), false, summary)
    }));
    fields.extend(
        TOGGLES
            .iter()
            .map(|(f, _, _, _, summary)| field(f, Shape::Bool, false, summary)),
    );
    let (moves, turns) = (["dx", "dy", "dz"], ["dpitch", "dyaw", "droll"]);
    let (at, facing) = (&["x", "y", "z"], &["pitch", "yaw", "roll"]);
    Manifest {
        name: "camera".to_owned(),
        author: vrc_we_sdk::cargo_authors(env!("CARGO_PKG_AUTHORS")),
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
        terms: None,
        status: false,
        game: false,
        players: false,
        description: env!("CARGO_PKG_DESCRIPTION").into(),
        documentation: env!("CARGO_PKG_HOMEPAGE").into(),
        license: env!("CARGO_PKG_LICENSE").into(),
        docs: include_str!("docs.md").into(),
    }
}

/// The pose fields' summaries, in [`POSE_FIELDS`] order.
const POSE_SUMMARIES: [&str; 6] = [
    "Camera position on the world's x axis",
    "Camera position on the world's y axis",
    "Camera position on the world's z axis",
    "Camera pitch, in degrees",
    "Camera yaw, in degrees",
    "Camera roll, in degrees",
];

fn method_summary(name: &str) -> &'static str {
    match name {
        "move_local" => "Moves along the camera's own axes",
        "move_world" => "Moves along the world's axes",
        "turn_local" => "Turns about the camera's own axes",
        "turn_world" => "Turns about the world's axes",
        "capture" => "Takes a photo",
        "capture_delayed" => "Takes a photo after a delay",
        "close" => "Closes the camera",
        "fly_path" => "Flies the camera along a track",
        "fly_to" => "Flies to a saved position over seconds",
        "track" => "A saved track's steps, by name",
        "position" => "A saved position's pose, by name",
        "stop" => "Stops every play",
        "add_key" => "Adds the camera's pose to the chosen track",
        _ => "",
    }
}
