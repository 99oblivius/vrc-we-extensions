use crate::library::clean_name;
use crate::motion::{step_of, with_curve, Curve};
use crate::*;

use vrc_we_sdk::{Item, UiEvent};

pub(super) fn pose_in(pose: [f32; 6]) -> Event {
    Event::Osc(OscMessage {
        addr: POSE.into(),
        args: pose.iter().map(|v| OscArg::Float(*v)).collect(),
    })
}

pub(super) fn call(method: &str, args: &[f64]) -> Event {
    Event::Call {
        id: 1,
        method: method.into(),
        args: args.iter().map(|a| Value::Number(*a)).collect(),
        from: None,
    }
}

pub(super) fn named(id: u64, method: &str, name: &str, more: &[f64]) -> Event {
    let mut args = vec![Value::Text(name.into())];
    args.extend(more.iter().map(|a| Value::Number(*a)));
    Event::Call {
        id,
        method: method.into(),
        args,
        from: None,
    }
}

/// A straight step at an even pace.
pub(super) fn lin(s: [f32; 7]) -> Step {
    step_of(&s).expect("7 numbers")
}

pub(super) fn fly(id: u64, path: &[Step]) -> Event {
    Event::Call {
        id,
        method: "fly_path".into(),
        args: vec![steps_value(path)],
        from: None,
    }
}

pub(super) fn replied(out: &[Command]) -> Vec<u64> {
    out.iter()
        .filter_map(|c| match c {
            Command::Reply { id, .. } => Some(*id),
            _ => None,
        })
        .collect()
}

pub(super) fn reply_of(out: &[Command], id: u64) -> Option<Value> {
    out.iter().find_map(|c| match c {
        Command::Reply { id: i, value } if *i == id => Some(value.clone()),
        _ => None,
    })
}

/// The pose a command list sends, if it sends one.
pub(super) fn sent_pose(out: &[Command]) -> Option<[f32; 6]> {
    out.iter().find_map(|c| match c {
        Command::Osc(m) if m.addr == POSE => {
            let v: Vec<f32> = m.args.iter().filter_map(arg_f32).collect();
            v.try_into().ok()
        }
        _ => None,
    })
}

pub(super) fn sent(out: &[Command], addr: &str) -> Option<Vec<OscArg>> {
    out.iter().find_map(|c| match c {
        Command::Osc(m) if m.addr == addr => Some(m.args.clone()),
        _ => None,
    })
}

pub(super) fn fields(out: &[Command]) -> Vec<(&str, Value)> {
    out.iter()
        .filter_map(|c| match c {
            Command::Field { name, value } => Some((name.as_str(), value.clone())),
            _ => None,
        })
        .collect()
}

pub(super) fn stored(out: &[Command]) -> Vec<(String, String)> {
    out.iter()
        .filter_map(|c| match c {
            Command::Store { key, value } => Some((key.clone(), value.clone())),
            _ => None,
        })
        .collect()
}

/// Timer `id` firing; every one at the same time, so each play step is one
/// `FRAME`.
pub(super) fn timer(id: &str) -> Event {
    Event::Timer {
        id: id.into(),
        at_ms: 0,
    }
}

pub(super) fn close(a: [f32; 6], b: [f32; 6]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

pub(super) fn known(pose: [f32; 6]) -> Camera {
    let mut cam = Camera::default();
    cam.handle(pose_in(pose));
    cam
}

/// Steps the plays until none is left (or `max` steps): the camera's pose
/// after each step and the calls answered, in order.
pub(super) fn run_plays(cam: &mut Camera, max: usize) -> (Vec<[f32; 6]>, Vec<u64>) {
    let (mut poses, mut ids) = (Vec::new(), Vec::new());
    for _ in 0..max {
        if cam.plays.is_empty() {
            break;
        }
        let out = cam.handle(timer(PLAY));
        poses.push(cam.pose);
        ids.extend(replied(&out));
    }
    (poses, ids)
}

pub(super) fn ui(page: &str, event: UiEvent) -> Event {
    Event::Ui {
        page: page.into(),
        event,
    }
}

pub(super) fn press(id: &str) -> Event {
    ui(PAGE, UiEvent::Pressed { id: id.into() })
}

pub(super) fn edit(id: &str) -> Event {
    ui(EDITOR, UiEvent::Pressed { id: id.into() })
}

pub(super) fn chosen(page: &str, id: &str, index: usize) -> Event {
    ui(
        page,
        UiEvent::Chosen {
            id: id.into(),
            index,
        },
    )
}

pub(super) fn typed(id: &str, value: &str) -> Event {
    ui(
        PAGE,
        UiEvent::Text {
            id: id.into(),
            value: value.into(),
        },
    )
}

pub(super) fn number_in(page: &str, id: &str, value: f64) -> Event {
    ui(
        page,
        UiEvent::Number {
            id: id.into(),
            value,
        },
    )
}

pub(super) fn texts(node: &Node, into: &mut Vec<String>) {
    match node {
        Node::Column(items) | Node::Row(items) => items.iter().for_each(|n| texts(n, into)),
        Node::Heading(t) | Node::Text(t) => into.push(t.clone()),
        Node::Link { label, url } => into.push(format!("{label} -> {url}")),
        Node::Number { id, value, .. } | Node::Slider { id, value, .. } => {
            into.push(format!("{id}={value:?}"))
        }
        Node::Button { id, .. } | Node::Choice { id, .. } => into.push(id.clone()),
        Node::Scene3d { path, .. } => into.push(format!("path of {}", path.len())),
        Node::TextInput { id, value, .. } => into.push(format!("{id}={value}")),
        Node::Toggle { id, value, .. } => into.push(format!("{id}={value:?}")),
        Node::List { id, items, .. } => into.push(format!("{id}: {}", items.join(" | "))),
        Node::Copy { text, .. } => into.push(format!("copy {text}")),
        Node::Split { main, side } => [main, side].into_iter().for_each(|n| texts(n, into)),
        Node::IconButton { id, .. }
        | Node::HoldButton { id, .. }
        | Node::Buttons { id, .. }
        | Node::Timeline { id, .. } => into.push(id.clone()),
        Node::Icon { icon } => into.push(icon.clone()),
        Node::Divider => {}
    }
}

/// What the last published view of the page (or of the editor) shows.
pub(super) fn shown(cam: &Camera, editor: bool) -> Vec<String> {
    let node = match editor {
        true => cam.shown_editor.as_ref(),
        false => cam.shown.as_ref(),
    };
    let mut out = Vec::new();
    texts(node.expect("a view"), &mut out);
    out
}

/// The 3D view's key points.
pub(super) fn keys_of(node: &Node) -> Option<Vec<[f32; 3]>> {
    match node {
        Node::Scene3d { path, keys, .. } => Some(keys.iter().map(|k| path[*k]).collect()),
        Node::Column(items) | Node::Row(items) => items.iter().find_map(keys_of),
        Node::Split { main, side } => keys_of(main).or_else(|| keys_of(side)),
        _ => None,
    }
}

pub(super) fn playhead_of(node: &Node) -> Option<Option<f32>> {
    match node {
        Node::Timeline { playhead, .. } => Some(*playhead),
        Node::Column(items) | Node::Row(items) => items.iter().find_map(playhead_of),
        Node::Split { main, side } => playhead_of(main).or_else(|| playhead_of(side)),
        _ => None,
    }
}

pub(super) const FORWARD: Step = [
    0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0,
];

/// A camera at the origin with a saved track `name` of `steps`.
pub(super) fn with_track(name: &str, steps: &[Step]) -> (Camera, Vec<Command>) {
    let mut cam = known([0.0; 6]);
    let mut out = Vec::new();
    cam.insert_track(clean_name(name).unwrap(), steps.to_vec(), None, &mut out);
    out.extend(cam.handle(timer("none")));
    (cam, out)
}

pub(super) fn options(out: &[Command], list: &str) -> Option<Vec<Item>> {
    out.iter().find_map(|c| match c {
        Command::Options { list: l, items } if l == list => Some(items.clone()),
        _ => None,
    })
}

/// A track of `keys` world poses recorded from `start` with the editor,
/// open and chosen.
pub(super) fn recorded(start: [f32; 6], keys: &[[f32; 6]]) -> Camera {
    let mut cam = known(start);
    cam.handle(press("open_editor"));
    cam.handle(edit("add_key"));
    for k in keys {
        cam.handle(pose_in(*k));
        cam.handle(edit("add_key"));
    }
    cam
}

/// Every key's world pose of the edited track.
pub(super) fn key_poses(cam: &Camera) -> Vec<[f32; 6]> {
    let t = &cam.tracks[cam.track.unwrap()];
    (0..t.keys())
        .map(|k| crate::motion::key_pose(&t.start.unwrap(), &t.steps, k))
        .collect()
}

/// The same place, and the same view and up within about 0.06°.
pub(super) fn same_pose(a: [f32; 6], b: [f32; 6]) -> bool {
    let (ra, rb) = (rotation(&a), rotation(&b));
    position(&a).distance(position(&b)) < 1e-3
        && [Vec3::Y, Vec3::Z]
            .into_iter()
            .all(|v| (ra * v).distance(rb * v) < 1e-3)
}

pub(super) const START: [f32; 6] = [1.0, 2.0, 3.0, 10.0, 20.0, 0.0];

pub(super) const KEYS: [[f32; 6]; 3] = [
    [2.0, 2.0, 5.0, -15.0, 80.0, 0.0],
    [0.0, 1.0, 4.0, 30.0, -150.0, 10.0],
    [-3.0, 1.5, 0.0, 0.0, 170.0, 0.0],
];

pub(super) const HERE: [f32; 6] = [5.0, 3.0, -2.0, 5.0, 45.0, 0.0];

/// VRChat reporting `pose`, settled.
pub(super) fn settle(cam: &mut Camera) {
    cam.handle(timer(SETTLE));
}

/// `s` with `curve`.
pub(super) fn curved(s: [f32; 7], curve: Curve) -> Step {
    with_curve(lin(s), curve)
}

/// The editor's scene and timeline as last published.
pub(super) fn editor_nodes(cam: &Camera) -> (Option<Node>, Option<vrc_we_sdk::Lane>) {
    fn walk(n: &Node, scene: &mut Option<Node>, lane: &mut Option<vrc_we_sdk::Lane>) {
        match n {
            Node::Column(items) | Node::Row(items) => {
                items.iter().for_each(|i| walk(i, scene, lane))
            }
            Node::Split { main, side } => {
                [main, side].into_iter().for_each(|i| walk(i, scene, lane))
            }
            Node::Scene3d { overlay, .. } => *scene = overlay.as_deref().cloned(),
            Node::Timeline { lane: l, .. } => *lane = l.clone(),
            _ => {}
        }
    }
    let (mut scene, mut lane) = (None, None);
    walk(
        cam.shown_editor.as_ref().expect("an editor"),
        &mut scene,
        &mut lane,
    );
    (scene, lane)
}

/// A track of `keys` keys along +z, 2 m and 1 s apart, open, in the editor.
pub(super) fn straight_track(keys: usize) -> Camera {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    cam.handle(edit("add_key"));
    for k in 1..keys {
        let x = if k % 2 == 1 { 1.0 } else { -1.0 };
        cam.handle(pose_in([x, 0.0, 2.0 * k as f32, 0.0, 0.0, 0.0]));
        cam.handle(edit("add_key"));
    }
    cam
}

/// The editor's 3D view as last built.
pub(super) fn scene_of(node: &Node) -> Option<&Node> {
    match node {
        Node::Scene3d { .. } => Some(node),
        Node::Column(items) | Node::Row(items) => items.iter().find_map(scene_of),
        Node::Split { main, side } => scene_of(main).or_else(|| scene_of(side)),
        _ => None,
    }
}

pub(super) fn write(field: &str, value: f64) -> Event {
    Event::Write {
        field: field.into(),
        value: Value::Number(value),
    }
}

/// The out of the optics' rest ending, as its timer does with no clock.
pub(super) fn rests_end(cam: &mut Camera) -> Vec<Command> {
    cam.handle(timer(REST))
}

/// Every value of `addr` sent while the plays run, and once they end.
pub(super) fn sent_while_playing(cam: &mut Camera, addr: &str) -> Vec<f32> {
    let mut values = Vec::new();
    for _ in 1..10_000 {
        if cam.plays.is_empty() {
            break;
        }
        let out = cam.handle(timer(PLAY));
        values.extend(sent(&out, addr).and_then(|a| arg_f32(&a[0])));
    }
    values.extend(sent(&rests_end(cam), addr).and_then(|a| arg_f32(&a[0])));
    values
}

/// What `event` sends, the tick's settings with it.
pub(super) fn with_send(cam: &mut Camera, event: Event) -> Vec<Command> {
    let mut out = cam.handle(event);
    out.extend(cam.handle(timer(SEND_SETTINGS)));
    out
}
