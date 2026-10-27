use super::*;
use crate::library::clean_name;
use crate::motion::{
    dot_place, key_times, legs, pace, pose_at, progress, step_of, with_curve, Curve, Follow,
    DOT_AT, DOT_PACE, FRAME, IN_LEN, MOVE_SPEED, OPTICS, OUT_LEN, SMOOTH, TURN_SPEED, WIRE,
};
use crate::widgets::{HINT, STAND_IN, UNREADABLE};
use glam::Quat;
use vrc_we_sdk::{Item, UiEvent};

fn pose_in(pose: [f32; 6]) -> Event {
    Event::Osc(OscMessage {
        addr: POSE.into(),
        args: pose.iter().map(|v| OscArg::Float(*v)).collect(),
    })
}

fn call(method: &str, args: &[f64]) -> Event {
    Event::Call {
        id: 1,
        method: method.into(),
        args: args.iter().map(|a| Value::Number(*a)).collect(),
        from: None,
    }
}

fn named(id: u64, method: &str, name: &str, more: &[f64]) -> Event {
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
fn lin(s: [f32; 7]) -> Step {
    step_of(&s).expect("7 numbers")
}

fn fly(id: u64, path: &[Step]) -> Event {
    Event::Call {
        id,
        method: "fly_path".into(),
        args: vec![steps_value(path)],
        from: None,
    }
}

fn replied(out: &[Command]) -> Vec<u64> {
    out.iter()
        .filter_map(|c| match c {
            Command::Reply { id, .. } => Some(*id),
            _ => None,
        })
        .collect()
}

fn reply_of(out: &[Command], id: u64) -> Option<Value> {
    out.iter().find_map(|c| match c {
        Command::Reply { id: i, value } if *i == id => Some(value.clone()),
        _ => None,
    })
}

/// The pose a command list sends, if it sends one.
fn sent_pose(out: &[Command]) -> Option<[f32; 6]> {
    out.iter().find_map(|c| match c {
        Command::Osc(m) if m.addr == POSE => {
            let v: Vec<f32> = m.args.iter().filter_map(arg_f32).collect();
            v.try_into().ok()
        }
        _ => None,
    })
}

fn sent(out: &[Command], addr: &str) -> Option<Vec<OscArg>> {
    out.iter().find_map(|c| match c {
        Command::Osc(m) if m.addr == addr => Some(m.args.clone()),
        _ => None,
    })
}

fn fields(out: &[Command]) -> Vec<(&str, Value)> {
    out.iter()
        .filter_map(|c| match c {
            Command::Field { name, value } => Some((name.as_str(), value.clone())),
            _ => None,
        })
        .collect()
}

fn stored(out: &[Command]) -> Vec<(String, String)> {
    out.iter()
        .filter_map(|c| match c {
            Command::Store { key, value } => Some((key.clone(), value.clone())),
            _ => None,
        })
        .collect()
}

/// Timer `id` firing; every one at the same time, so each play step is one
/// `FRAME`.
fn timer(id: &str) -> Event {
    Event::Timer {
        id: id.into(),
        at_ms: 0,
    }
}

fn close(a: [f32; 6], b: [f32; 6]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

fn known(pose: [f32; 6]) -> Camera {
    let mut cam = Camera::default();
    cam.handle(pose_in(pose));
    cam
}

/// Steps the plays until none is left (or `max` steps): the camera's pose
/// after each step and the calls answered, in order.
fn run_plays(cam: &mut Camera, max: usize) -> (Vec<[f32; 6]>, Vec<u64>) {
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

fn ui(page: &str, event: UiEvent) -> Event {
    Event::Ui {
        page: page.into(),
        event,
    }
}

fn press(id: &str) -> Event {
    ui(PAGE, UiEvent::Pressed { id: id.into() })
}

fn edit(id: &str) -> Event {
    ui(EDITOR, UiEvent::Pressed { id: id.into() })
}

fn chosen(page: &str, id: &str, index: usize) -> Event {
    ui(
        page,
        UiEvent::Chosen {
            id: id.into(),
            index,
        },
    )
}

fn typed(id: &str, value: &str) -> Event {
    ui(
        PAGE,
        UiEvent::Text {
            id: id.into(),
            value: value.into(),
        },
    )
}

fn number_in(page: &str, id: &str, value: f64) -> Event {
    ui(
        page,
        UiEvent::Number {
            id: id.into(),
            value,
        },
    )
}

fn texts(node: &Node, into: &mut Vec<String>) {
    match node {
        Node::Column(items) | Node::Row(items) => items.iter().for_each(|n| texts(n, into)),
        Node::Heading(t) | Node::Text(t) => into.push(t.clone()),
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
fn shown(cam: &Camera, editor: bool) -> Vec<String> {
    let node = match editor {
        true => cam.shown_editor.as_ref(),
        false => cam.shown.as_ref(),
    };
    let mut out = Vec::new();
    texts(node.expect("a view"), &mut out);
    out
}

/// The 3D view's key points.
fn keys_of(node: &Node) -> Option<Vec<[f32; 3]>> {
    match node {
        Node::Scene3d { path, keys, .. } => Some(keys.iter().map(|k| path[*k]).collect()),
        Node::Column(items) | Node::Row(items) => items.iter().find_map(keys_of),
        Node::Split { main, side } => keys_of(main).or_else(|| keys_of(side)),
        _ => None,
    }
}

fn playhead_of(node: &Node) -> Option<Option<f32>> {
    match node {
        Node::Timeline { playhead, .. } => Some(*playhead),
        Node::Column(items) | Node::Row(items) => items.iter().find_map(playhead_of),
        Node::Split { main, side } => playhead_of(main).or_else(|| playhead_of(side)),
        _ => None,
    }
}

const FORWARD: Step = [
    0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0,
];

#[test]
fn the_manifest_validates() {
    assert_eq!(Camera::default().manifest().validate(), Ok(()));
}

#[test]
fn a_reported_pose_sets_every_field_then_only_changes() {
    let mut cam = Camera::default();
    let out = cam.handle(pose_in([1.0, 2.0, 3.0, 10.0, 20.0, 30.0]));
    let names: Vec<&str> = fields(&out).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, POSE_FIELDS);
    let out = cam.handle(pose_in([1.0, 2.0, 4.0, 10.0, 20.0, 30.0]));
    assert_eq!(fields(&out), [("z", Value::Number(4.0))]);
}

#[test]
fn nothing_moves_before_vrchat_reports_a_pose() {
    let mut cam = Camera::default();
    let out = cam.handle(call("move_local", &[0.0, 0.0, 1.0]));
    assert_eq!(sent_pose(&out), None);
    assert_eq!(replied(&out), [1]);
    let out = cam.handle(Event::Write {
        field: "x".into(),
        value: Value::Number(1.0),
    });
    assert_eq!(sent_pose(&out), None);
}

#[test]
fn move_local_moves_along_the_camera_axes() {
    let mut cam = known([5.0, 1.0, 0.0, 0.0, 90.0, 0.0]);
    let pose = sent_pose(&cam.handle(call("move_local", &[0.0, 0.0, 1.0]))).unwrap();
    assert!(close(pose, [6.0, 1.0, 0.0, 0.0, 90.0, 0.0]), "{pose:?}");
    let mut cam = known([0.0, 1.0, 0.0, 90.0, 0.0, 0.0]);
    let pose = sent_pose(&cam.handle(call("move_local", &[0.0, 0.0, 2.0]))).unwrap();
    assert!(close(pose, [0.0, -1.0, 0.0, 90.0, 0.0, 0.0]), "{pose:?}");
}

#[test]
fn turn_local_turns_about_the_camera_axes() {
    let mut cam = known([0.0, 0.0, 0.0, 0.0, 90.0, 0.0]);
    let pose = sent_pose(&cam.handle(call("turn_local", &[0.0, 45.0, 0.0]))).unwrap();
    assert!(close(pose, [0.0, 0.0, 0.0, 0.0, 135.0, 0.0]), "{pose:?}");
    // Looking straight down, a local yaw swings the view sideways.
    let mut cam = known([0.0, 0.0, 0.0, 90.0, 0.0, 0.0]);
    let pose = sent_pose(&cam.handle(call("turn_local", &[0.0, 30.0, 0.0]))).unwrap();
    let forward = rotation(&pose) * Vec3::Z;
    let swung = Vec3::new(0.5, -(3.0f32.sqrt()) / 2.0, 0.0);
    assert!((forward - swung).length() < 1e-4, "{forward:?}");
}

#[test]
fn the_world_moves_and_turns_ignore_where_the_camera_faces() {
    let mut cam = known([5.0, 1.0, 0.0, 0.0, 90.0, 0.0]);
    let pose = sent_pose(&cam.handle(call("move_world", &[0.0, 0.0, 1.0]))).unwrap();
    assert!(close(pose, [5.0, 1.0, 1.0, 0.0, 90.0, 0.0]), "{pose:?}");
    // Looking straight down, a world yaw keeps the view down and turns the
    // picture about it.
    let mut cam = known([0.0, 0.0, 0.0, 90.0, 0.0, 0.0]);
    let pose = sent_pose(&cam.handle(call("turn_world", &[0.0, 30.0, 0.0]))).unwrap();
    let forward = rotation(&pose) * Vec3::Z;
    assert!((forward - Vec3::NEG_Y).length() < 1e-4, "{forward:?}");
    let up = rotation(&pose) * Vec3::Y;
    let turned = Quat::from_rotation_y(30f32.to_radians()) * Vec3::Z;
    assert!((up - turned).length() < 1e-4, "{up:?}");
}

#[test]
fn a_path_is_a_coroutine_answered_when_it_ends() {
    let mut cam = known([0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    let out = cam.handle(fly(7, &[lin([0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.5])]));
    assert!(replied(&out).is_empty(), "no reply while it flies");
    let (poses, ids) = run_plays(&mut cam, 100);
    assert_eq!(ids, [7]);
    assert!(close(
        *poses.last().unwrap(),
        [0.0, 1.0, 2.0, 0.0, 0.0, 0.0]
    ));
    assert!(poses.iter().any(|p| p[2] > 0.5 && p[2] < 1.5), "in between");
}

#[test]
fn each_step_goes_on_from_the_last_on_the_starts_axes() {
    // Facing +X (yaw 90): forward 1 m, forward 1 m more while turning 30,
    // then 1 m right; the start's right is -Z.
    let mut cam = known([5.0, 1.0, 0.0, 0.0, 90.0, 0.0]);
    cam.handle(fly(
        1,
        &[
            FORWARD,
            lin([0.0, 0.0, 1.0, 0.0, 30.0, 0.0, 0.2]),
            lin([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.2]),
        ],
    ));
    let (poses, _) = run_plays(&mut cam, 100);
    let end = *poses.last().unwrap();
    assert!(close(end, [7.0, 1.0, -1.0, 0.0, 120.0, 0.0]), "{end:?}");
}

#[test]
fn a_track_moves_on_its_starts_heading_whatever_its_pitch() {
    let mut cam = known([0.0, 1.0, 0.0, 45.0, 90.0, 0.0]);
    cam.handle(fly(1, &[FORWARD]));
    let (poses, _) = run_plays(&mut cam, 100);
    let end = *poses.last().unwrap();
    assert!(close(end, [1.0, 1.0, 0.0, 45.0, 90.0, 0.0]), "{end:?}");
}

#[test]
fn a_turn_recorded_from_a_pitched_start_has_no_roll() {
    let mut cam = known([0.0, 1.0, 0.0, 30.0, 0.0, 0.0]);
    cam.handle(press("open_editor"));
    cam.handle(edit("add_key"));
    cam.handle(pose_in([0.0, 1.0, 0.0, 30.0, 90.0, 0.0]));
    cam.handle(edit("add_key"));
    let step = cam.tracks[0].steps[0];
    let want = [0.0, 0.0, 0.0, 0.0, 90.0, 0.0, 2.0];
    assert!(
        step.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-3),
        "{step:?}"
    );
}

#[test]
fn no_roll_appears_between_keys_without_one() {
    let mut cam = known([0.0, 1.0, 0.0, -20.0, 0.0, 0.0]);
    cam.handle(fly(1, &[lin([0.0, 0.0, 0.0, 40.0, 170.0, 0.0, 1.0])]));
    let (poses, _) = run_plays(&mut cam, 100);
    assert!(poses.len() > 10);
    assert!(poses.iter().all(|p| p[5].abs() < 1e-3), "{poses:?}");
    assert!(close(
        *poses.last().unwrap(),
        [0.0, 1.0, 0.0, 20.0, 170.0, 0.0]
    ));
}

#[test]
fn a_turn_past_half_a_circle_goes_the_whole_way() {
    let mut cam = known([0.0; 6]);
    cam.handle(fly(1, &[lin([0.0, 0.0, 0.0, 0.0, 270.0, 0.0, 0.5])]));
    let (poses, _) = run_plays(&mut cam, 100);
    assert!(poses.windows(2).all(|w| w[1][4] > w[0][4]), "{poses:?}");
    assert!(close(
        *poses.last().unwrap(),
        [0.0, 0.0, 0.0, 0.0, 270.0, 0.0]
    ));
}

#[test]
fn recorded_keys_play_back_onto_their_poses() {
    let start = [1.0, 2.0, 3.0, 10.0, 20.0, 5.0];
    let keys = [
        [2.0, 2.5, 5.0, -15.0, 80.0, 0.0],
        [0.0, 1.0, 4.0, 60.0, -150.0, -30.0],
        [-3.0, 1.5, 0.0, 0.0, 170.0, 10.0],
    ];
    let mut cam = known(start);
    cam.handle(press("open_editor"));
    cam.handle(edit("add_key"));
    for k in keys {
        cam.handle(pose_in(k));
        cam.handle(edit("add_key"));
    }
    let steps = cam.tracks[0].steps.clone();
    cam.handle(pose_in(start));
    cam.handle(fly(1, &steps));
    let (poses, _) = run_plays(&mut cam, 1000);
    let end = *poses.last().unwrap();
    assert!(same_pose(end, keys[2]), "{end:?} against {:?}", keys[2]);
}

#[test]
fn plays_stack_and_add_to_what_moves_the_camera_meanwhile() {
    let mut cam = known([0.0; 6]);
    cam.handle(fly(1, &[lin([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.3])]));
    cam.handle(fly(2, &[lin([0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.6])]));
    cam.handle(timer(PLAY));
    cam.handle(call("move_world", &[0.0, 0.0, 3.0]));
    let (poses, ids) = run_plays(&mut cam, 100);
    assert_eq!(ids, [1, 2], "each answered when its own path ends");
    let end = *poses.last().unwrap();
    assert!(close(end, [1.0, 2.0, 3.0, 0.0, 0.0, 0.0]), "{end:?}");
}

#[test]
fn a_cancelled_play_stops_unanswered_and_stop_ends_every_play() {
    let mut cam = known([0.0; 6]);
    cam.handle(fly(1, &[lin([0.0, 0.0, 9.0, 0.0, 0.0, 0.0, 9.0])]));
    cam.handle(fly(2, &[lin([0.0, 9.0, 0.0, 0.0, 0.0, 0.0, 9.0])]));
    cam.handle(Event::Cancel { id: 1 });
    assert_eq!(cam.plays.len(), 1);
    let out = cam.handle(Event::Call {
        id: 3,
        method: "stop".into(),
        args: Vec::new(),
        from: None,
    });
    assert_eq!(replied(&out), [2, 3], "the play ends, then stop answers");
    assert!(cam.plays.is_empty());
}

#[test]
fn a_play_run_to_its_end_emits_play_ended_and_a_stopped_one_doesnt() {
    let ended = |out: &[Command]| {
        out.iter()
            .filter(|c| matches!(c, Command::Signal { name, .. } if name == PLAY_ENDED))
            .count()
    };
    let mut cam = known([0.0; 6]);
    cam.handle(fly(1, &[lin([0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.2])]));
    let mut emitted = 0;
    while !cam.plays.is_empty() {
        emitted += ended(&cam.handle(timer(PLAY)));
    }
    assert_eq!(emitted, 1);
    cam.handle(fly(2, &[lin([0.0, 0.0, 9.0, 0.0, 0.0, 0.0, 9.0])]));
    let out = cam.handle(Event::Call {
        id: 3,
        method: "stop".into(),
        args: Vec::new(),
        from: None,
    });
    assert_eq!(ended(&out), 0);
}

#[test]
fn a_play_with_no_steps_ends_at_once() {
    let mut cam = known([0.0; 6]);
    assert_eq!(replied(&cam.handle(fly(5, &[]))), [5], "no steps");
}

#[test]
fn a_play_runs_on_the_stand_in_before_vrchat_reports_a_pose() {
    // A mapping's play is independent of the game: the stand-in flies, the
    // call answers when the play ends, and VRChat is sent nothing.
    let mut cam = Camera::default();
    let out = cam.handle(fly(4, &[lin([0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0])]));
    assert!(replied(&out).is_empty(), "the play is still running");
    assert!(
        sent_pose(&out).is_none(),
        "an unlinked camera sends no pose"
    );
    let (_, ids) = run_plays(&mut cam, 3);
    assert_eq!(ids, vec![4]);
    assert!(close(cam.pose, [0.0, 0.0, 4.0, 0.0, 0.0, 0.0]));
}

#[test]
fn a_zero_second_step_jumps() {
    let mut cam = known([0.0; 6]);
    cam.handle(fly(1, &[lin([0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0])]));
    let (poses, ids) = run_plays(&mut cam, 2);
    assert_eq!((poses.len(), ids), (1, vec![1]));
    assert!(close(poses[0], [0.0, 0.0, 4.0, 0.0, 0.0, 0.0]));
}

#[test]
fn a_field_write_sends_the_whole_pose() {
    let mut cam = known([1.0, 2.0, 3.0, 10.0, 20.0, 30.0]);
    let out = cam.handle(Event::Write {
        field: "yaw".into(),
        value: Value::Number(45.0),
    });
    assert_eq!(sent_pose(&out), Some([1.0, 2.0, 3.0, 10.0, 45.0, 30.0]));
    assert_eq!(fields(&out), [("yaw", Value::Number(45.0))]);
}

#[test]
fn mode_is_read_and_written_as_an_integer() {
    let mut cam = Camera::default();
    let out = cam.handle(Event::Osc(OscMessage {
        addr: MODE.into(),
        args: vec![OscArg::Int(1)],
    }));
    assert_eq!(fields(&out), [("mode", Value::Number(1.0))]);
    let out = cam.handle(Event::Write {
        field: "mode".into(),
        value: Value::Number(2.4),
    });
    assert_eq!(sent(&out, MODE), Some(vec![OscArg::Int(2)]));
    assert_eq!(fields(&out), [("mode", Value::Number(2.0))]);
}

#[test]
fn vrchats_settings_are_written_over_osc_and_read_back() {
    let mut cam = Camera::default();
    let out = with_send(
        &mut cam,
        Event::Write {
            field: "zoom".into(),
            value: Value::Number(500.0),
        },
    );
    assert_eq!(
        sent(&out, "/usercamera/Zoom"),
        Some(vec![OscArg::Float(300.0)]),
        "clamped to VRChat's range"
    );
    assert_eq!(fields(&out), [("zoom", Value::Number(300.0))]);
    let out = cam.handle(Event::Osc(OscMessage {
        addr: "/usercamera/Lock".into(),
        args: vec![OscArg::Int(1)],
    }));
    assert_eq!(fields(&out), [("lock", Value::Bool(true))]);
    cam.handle(press("open_editor"));
    let out = cam.handle(ui(
        EDITOR,
        UiEvent::Toggled {
            id: "lock".into(),
            value: false,
        },
    ));
    assert_eq!(
        sent(&out, "/usercamera/Lock"),
        Some(vec![OscArg::Bool(false)])
    );
    assert!(
        shown(&cam, true).contains(&"lock=Some(false)".to_owned()),
        "the editor shows the settings"
    );
}

#[test]
fn every_call_but_a_playing_coroutine_gets_one_reply_at_once() {
    let mut cam = known([0.0; 6]);
    for event in [
        call("move_local", &[1.0, 0.0, 0.0]),
        call("move_world", &[1.0, 0.0, 0.0]),
        call("turn_local", &[0.0, 1.0, 0.0]),
        call("turn_world", &[0.0, 1.0, 0.0]),
        call("capture", &[]),
        call("capture_delayed", &[]),
        call("close", &[]),
        call("stop", &[]),
        call("add_key", &[]),
        named(1, "track", "none", &[]),
    ] {
        assert_eq!(replied(&cam.handle(event)), [1]);
    }
}

#[test]
fn captures_and_close_send_their_osc() {
    let mut cam = Camera::default();
    for (method, addr) in [
        ("capture", CAPTURE),
        ("capture_delayed", CAPTURE_DELAYED),
        ("close", CLOSE),
    ] {
        let out = cam.handle(call(method, &[]));
        assert_eq!(sent(&out, addr), Some(vec![OscArg::Bool(true)]), "{method}");
    }
}

#[test]
fn keyframes_follow_the_camera_from_the_tracks_start() {
    // Facing +X: the first keyframe is the start, then 2 m along +X is 2 m
    // forward, then a turn in place.
    let mut cam = known([0.0, 1.0, 0.0, 0.0, 90.0, 0.0]);
    cam.handle(press("open_editor"));
    cam.handle(edit("add_key"));
    assert_eq!(
        (cam.tracks.len(), cam.track, cam.tracks[0].keys()),
        (1, Some(0), 1),
        "a new track, its start at the camera"
    );
    assert_eq!(cam.tracks[0].start, Some([0.0, 1.0, 0.0, 0.0, 90.0, 0.0]));
    cam.handle(pose_in([2.0, 1.0, 0.0, 0.0, 90.0, 0.0]));
    assert!(
        shown(&cam, true)
            .iter()
            .any(|t| t.starts_with("From key 1: right 0, up 0, forward 2 m")),
        "{:?}",
        shown(&cam, true)
    );
    cam.handle(edit("add_key"));
    cam.handle(pose_in([2.0, 1.0, 0.0, 0.0, 120.0, 0.0]));
    assert_eq!(
        replied(&cam.handle(call("add_key", &[]))),
        [1],
        "the method"
    );
    let near = |a: Step, b: [f32; 7]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
    let steps = cam.tracks[0].steps.clone();
    assert_eq!(steps.len(), 2);
    assert!(
        near(steps[0], [0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0]),
        "{:?}",
        steps[0]
    );
    assert!(
        near(steps[1], [0.0, 0.0, 0.0, 0.0, 30.0, 0.0, 2.0]),
        "{:?}",
        steps[1]
    );
    assert_eq!(cam.key, Some(2), "the key added last is chosen");
    cam.handle(edit("new"));
    cam.handle(chosen(EDITOR, "tracks", 0));
    cam.handle(pose_in([3.0, 1.0, 0.0, 0.0, 120.0, 0.0]));
    cam.handle(edit("add_key"));
    let step = cam.tracks[0].steps[2];
    assert!(
        near(step, [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 2.0]),
        "choosing another track and back keeps where it goes on from: {step:?}"
    );
}

#[test]
fn start_here_moves_a_track_and_the_view_holds_still_while_the_camera_moves() {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    cam.handle(edit("add_key"));
    cam.handle(pose_in([0.0, 0.0, 2.0, 0.0, 0.0, 0.0]));
    cam.handle(edit("add_key"));
    let path = |cam: &Camera| match cam.editor() {
        Node::Split { main, .. } => match *main {
            Node::Column(items) => match &items[0] {
                Node::Scene3d {
                    path, keys, camera, ..
                } => (keys.iter().map(|k| path[*k]).collect::<Vec<_>>(), *camera),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    let (before, _) = path(&cam);
    assert_eq!(before, [[0.0, 0.0, 0.0], [0.0, 0.0, 2.0]]);
    cam.handle(pose_in([5.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
    let (after, camera) = path(&cam);
    assert_eq!(after, before, "the camera moving moves no key");
    assert_eq!(camera.unwrap()[0], [5.0, 0.0, 0.0], "only the camera");
    cam.handle(edit("start_here"));
    assert_eq!(path(&cam).0, [[5.0, 0.0, 0.0], [5.0, 0.0, 2.0]]);
}

#[test]
fn keys_are_edited_in_place_and_reordered() {
    let (mut cam, _) = with_track(
        "Orbit",
        &[FORWARD, lin([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0])],
    );
    cam.handle(press("open_editor"));
    cam.handle(chosen(EDITOR, "timeline", 2));
    cam.handle(number_in(EDITOR, "key_time", 3.2));
    assert!(
        (cam.tracks[0].steps[1][6] - 3.0).abs() < 1e-5,
        "a key's time is from the start"
    );
    cam.handle(edit("key_earlier"));
    assert_eq!(cam.key, Some(1));
    assert_eq!(cam.tracks[0].steps[0][0], 1.0);
    cam.handle(edit("key_dup"));
    assert_eq!((cam.tracks[0].steps.len(), cam.key), (3, Some(2)));
    cam.handle(edit("key_del"));
    assert_eq!((cam.tracks[0].steps.len(), cam.key), (2, Some(1)));
    cam.handle(chosen(EDITOR, "timeline", 0));
    cam.handle(edit("key_del"));
    assert_eq!(cam.tracks[0].steps.len(), 2, "the start is no step");
}

#[test]
fn a_dragged_key_moves_between_its_neighbours_and_the_last_sets_the_length() {
    let step = |s: f32| lin([0.0, 0.0, 1.0, 0.0, 0.0, 0.0, s]);
    let (mut cam, _) = with_track("Orbit", &[step(2.0), step(4.0), step(2.0)]);
    cam.handle(press("open_editor"));
    let drag = |index: usize, value: f64| {
        ui(
            EDITOR,
            UiEvent::Moved {
                id: "timeline".into(),
                index,
                value,
            },
        )
    };
    let times = |cam: &Camera| crate::motion::key_times(&cam.tracks[0].steps);
    assert_eq!(times(&cam), [0.0, 2.0, 6.0, 8.0]);
    cam.handle(drag(2, 3.0));
    assert_eq!(times(&cam), [0.0, 2.0, 3.0, 8.0], "the keys after stay");
    cam.handle(drag(2, 99.0));
    assert_eq!(times(&cam), [0.0, 2.0, 8.0, 8.0], "held before the next");
    cam.handle(drag(3, 20.0));
    assert_eq!(times(&cam), [0.0, 2.0, 8.0, 20.0], "the last runs on");
    cam.handle(drag(0, 5.0));
    assert_eq!(times(&cam)[0], 0.0, "the start stays");
}

#[test]
fn a_track_stays_chosen_and_a_delete_chooses_the_one_below() {
    let (mut cam, _) = with_track("A", &[FORWARD]);
    let mut out = Vec::new();
    cam.insert_track("B".into(), vec![FORWARD], None, &mut out);
    cam.insert_track("C".into(), vec![FORWARD], None, &mut out);
    cam.track = None;
    cam.handle(press("open_editor"));
    assert_eq!(cam.track, Some(0), "one is chosen on opening");
    cam.handle(edit("delete"));
    assert_eq!(cam.tracks[cam.track.unwrap()].name, "B", "the one below");
    cam.handle(chosen(EDITOR, "tracks", 1));
    cam.handle(edit("delete"));
    assert_eq!(
        cam.tracks[cam.track.unwrap()].name,
        "B",
        "the last: the one above"
    );
    cam.handle(edit("delete"));
    assert_eq!(cam.track, None, "none left");
}

/// A camera at the origin with a saved track `name` of `steps`.
fn with_track(name: &str, steps: &[Step]) -> (Camera, Vec<Command>) {
    let mut cam = known([0.0; 6]);
    let mut out = Vec::new();
    cam.insert_track(clean_name(name).unwrap(), steps.to_vec(), None, &mut out);
    out.extend(cam.handle(timer("none")));
    (cam, out)
}

#[test]
fn a_saved_track_reads_back_by_name_and_flies() {
    let (mut cam, out) = with_track("Orbit", &[FORWARD, FORWARD]);
    let keys: Vec<String> = stored(&out).into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["track_0", LIBRARY_KEY]);
    let out = cam.handle(named(3, "track", "Orbit", &[]));
    let steps = reply_of(&out, 3).unwrap();
    assert_eq!(steps, steps_value(&[FORWARD, FORWARD]));
    let out = cam.handle(Event::Call {
        id: 4,
        method: "fly_path".into(),
        args: vec![steps],
        from: None,
    });
    assert!(replied(&out).is_empty());
    let (poses, ids) = run_plays(&mut cam, 100);
    assert_eq!(ids, [4]);
    assert!(close(
        *poses.last().unwrap(),
        [0.0, 0.0, 2.0, 0.0, 0.0, 0.0]
    ));
}

#[test]
fn track_names_are_cleaned_and_kept_apart() {
    let (mut cam, _) = with_track("  Orbit\n", &[FORWARD]);
    assert_eq!(cam.tracks[0].name, "Orbit");
    cam.track = Some(0);
    cam.handle(press("duplicate"));
    assert_eq!(cam.tracks[1].name, "Orbit (2)");
    cam.track = Some(0);
    cam.handle(press("duplicate"));
    assert_eq!(cam.tracks[2].name, "Orbit (3)");
    cam.track = Some(2);
    cam.handle(typed("rename", "Orbit"));
    assert_eq!(cam.tracks[2].name, "Orbit (3)", "a rename to a name in use");
    assert!(cam.note.is_some());
    cam.handle(typed("rename", "   "));
    assert_eq!(cam.tracks[2].name, "Orbit (3)", "a blank rename");
    let out = cam.handle(typed("rename", "Sweep"));
    assert_eq!(cam.tracks[2].name, "Sweep");
    assert!(stored(&out).iter().any(|(k, _)| k == LIBRARY_KEY));
    let long = "x".repeat(NAME_MAX + 10);
    assert_eq!(clean_name(&long).unwrap().chars().count(), NAME_MAX);
}

#[test]
fn the_library_comes_back_from_the_store() {
    let (mut cam, mut out) = with_track("Orbit", &[FORWARD]);
    cam.handle(pose_in([1.0, 2.0, 3.0, 0.0, 0.0, 0.0]));
    out.extend(cam.handle(press("save_position")));
    let mut store: std::collections::HashMap<String, String> = stored(&out).into_iter().collect();
    let mut fresh = Camera::default();
    let mut pending = fresh.handle(Event::Start);
    while let Some(key) = pending.iter().find_map(|c| match c {
        Command::Load { key } => Some(key.clone()),
        _ => None,
    }) {
        let value = store.remove(&key);
        pending = fresh.handle(Event::Loaded { key, value });
    }
    assert_eq!(fresh.tracks, cam.tracks);
    assert_eq!(fresh.positions, cam.positions);
    assert_eq!(fresh.next_key, cam.next_key);
}

#[test]
fn deleting_a_track_removes_its_stored_steps() {
    let (mut cam, _) = with_track("Orbit", &[FORWARD]);
    let out = cam.handle(press("delete"));
    assert!(out.contains(&Command::Unstore {
        key: "track_0".into()
    }));
    assert!(cam.tracks.is_empty() && cam.track.is_none());
}

#[test]
fn a_position_is_flown_to_in_its_seconds_or_jumped_to() {
    let stage = [1.0, 2.0, 3.0, 0.0, 45.0, 0.0];
    let mut cam = known(stage);
    cam.handle(press("save_position"));
    let to = reply_of(&cam.handle(named(5, "position", "Position", &[])), 5).unwrap();
    assert_eq!(
        reply_of(&cam.handle(named(6, "position", "Nowhere", &[])), 6),
        Some(Value::None)
    );
    let fly_to = |id: u64, seconds: f64| Event::Call {
        id,
        method: "fly_to".into(),
        args: vec![to.clone(), Value::Number(seconds)],
        from: None,
    };
    cam.handle(pose_in([9.0, 0.0, 9.0, 10.0, -90.0, 0.0]));
    cam.handle(fly_to(1, 0.0));
    let (poses, ids) = run_plays(&mut cam, 2);
    assert_eq!(ids, [1]);
    assert!(close(poses[0], stage), "{:?}", poses[0]);
    cam.handle(pose_in([9.0, 0.0, 9.0, 10.0, -90.0, 0.0]));
    cam.handle(fly_to(2, 0.3));
    let (poses, ids) = run_plays(&mut cam, 100);
    assert_eq!(ids, [2]);
    assert!(poses.len() > 2, "it flies there");
    assert!(close(*poses.last().unwrap(), stage), "{:?}", poses.last());
}

#[test]
fn the_editor_searches_chooses_and_edits_a_tracks_keys() {
    let (mut cam, _) = with_track("Orbit", &[FORWARD, FORWARD]);
    let mut out = Vec::new();
    cam.insert_track("Dolly".into(), vec![FORWARD], None, &mut out);
    let out = cam.handle(press("open_editor"));
    assert!(out
        .iter()
        .any(|c| matches!(c, Command::Modal { id, .. } if id == EDITOR)));
    cam.handle(ui(
        EDITOR,
        UiEvent::Text {
            id: "search".into(),
            value: "orb".into(),
        },
    ));
    let listed = shown(&cam, true);
    assert!(
        listed.iter().any(|l| l.starts_with("tracks: Orbit")),
        "{listed:?}"
    );
    assert!(
        !listed
            .iter()
            .any(|l| l.starts_with("tracks:") && l.contains("Dolly")),
        "{listed:?}"
    );
    cam.handle(chosen(EDITOR, "tracks", 0));
    assert_eq!(cam.track, Some(0), "the row is the found track");
    cam.handle(chosen(EDITOR, "timeline", 2));
    assert_eq!(cam.key, Some(2));
    assert!(shown(&cam, true).contains(&"s:1:0=Some(0.0)".to_owned()));
    let out = cam.handle(number_in(EDITOR, "s:1:0", 2.0));
    assert_eq!(cam.tracks[0].steps[1][0], 2.0);
    assert!(stored(&out).iter().any(|(k, _)| k == "track_0"));
    cam.handle(edit("key_earlier"));
    assert_eq!(cam.tracks[0].steps[0][0], 2.0, "moved earlier");
    cam.handle(edit("key_del"));
    assert_eq!(cam.tracks[0].steps, [FORWARD]);
    cam.handle(ui(
        EDITOR,
        UiEvent::Moved {
            id: "timeline".into(),
            index: 1,
            value: 1.5,
        },
    ));
    assert_eq!(
        cam.tracks[0].steps[0][6], 1.5,
        "a key dragged on the timeline"
    );
    assert_eq!(key_times(&cam.tracks[0].steps), [0.0, 1.5]);
    let copy = format!("copy {}", wex(&cam.tracks[0].steps));
    assert!(shown(&cam, true).contains(&copy));
    assert_eq!(
        wex(&[FORWARD]),
        "[\n    [0, 0, 1, 0, 0, 0, 0.2, 0, 0, 0.5, 1]\n]"
    );
    // A track with no start starts where the camera stood when the editor
    // showed it, and stays there as the camera moves.
    assert_eq!(cam.tracks[0].start, Some([0.0; 6]));
    cam.handle(pose_in([5.0, 1.0, 0.0, 0.0, 90.0, 0.0]));
    let path = keys_of(cam.shown_editor.as_ref().unwrap()).unwrap();
    assert_eq!(path, [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
    cam.handle(edit("play"));
    let out = cam.handle(timer(PLAY));
    let playhead = out.iter().find_map(|c| match c {
        Command::Mark {
            id,
            mark: Mark::Playhead(at),
            ..
        } if id == "timeline" => *at,
        _ => None,
    });
    assert!(playhead.is_some_and(|p| p > 0.0), "{playhead:?}");
    cam.handle(Event::ModalClosed { id: EDITOR.into() });
    let out = cam.handle(pose_in([6.0, 1.0, 0.0, 0.0, 90.0, 0.0]));
    assert!(
        !out.iter().any(|c| matches!(c, Command::Modal { .. })),
        "closed stays closed"
    );
}

#[test]
fn the_page_lists_its_tracks_to_play_and_stop() {
    let (mut cam, _) = with_track("Orbit", &[FORWARD, FORWARD]);
    cam.handle(Event::Start);
    assert!(shown(&cam, false)
        .iter()
        .any(|t| t.starts_with("Orbit   3 keys")));
    cam.handle(press("play:track_0"));
    assert_eq!(cam.plays.len(), 1);
    assert_eq!(cam.plays[0].track.as_deref(), Some("track_0"));
    cam.handle(press("play:track_0"));
    assert!(cam.plays.is_empty(), "the same row pauses it");
}

#[test]
fn a_pose_write_before_vrchat_reports_moves_the_stand_in_only() {
    let mut cam = Camera::default();
    let out = cam.handle(Event::Write {
        field: "x".into(),
        value: Value::Number(2.0),
    });
    assert!(sent_pose(&out).is_none(), "nothing goes to VRChat unlinked");
    assert!(close(cam.pose, [2.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
    assert!(fields(&out).is_empty(), "no field is published unlinked");
}

#[test]
fn an_unreadable_library_shows_a_note_and_saves_nothing() {
    let mut cam = Camera::default();
    cam.handle(Event::Start);
    let out = cam.handle(Event::Loaded {
        key: LIBRARY_KEY.into(),
        value: Some("{not an index".into()),
    });
    assert!(cam.unreadable);
    assert!(stored(&out).is_empty(), "nothing written over it");
    let page = shown(&cam, false);
    assert!(page.contains(&UNREADABLE.to_owned()), "{page:?}");
    assert!(page.contains(&"library_reset".to_owned()), "{page:?}");
    let mut into = Vec::new();
    cam.add_key(&mut into);
    assert!(cam.tracks.is_empty(), "no track is made over it");
    assert!(stored(&into).is_empty());
    let out = cam.handle(press("library_reset"));
    assert!(!cam.unreadable);
    assert!(stored(&out).iter().any(|(k, _)| k == LIBRARY_KEY));
}

#[test]
fn the_pads_move_a_stand_in_until_vrchat_reports_a_pose() {
    let mut cam = Camera::default();
    let out = cam.handle(Event::Start);
    assert!(out.contains(&Command::Load {
        key: LIBRARY_KEY.into()
    }));
    let says = |cam: &Camera, first: &str| {
        shown(cam, false)
            .iter()
            .any(|t| t.starts_with(first) && t.ends_with(STAND_IN))
    };
    assert!(says(&cam, OPEN_FIRST));
    assert!(shown(&cam, false).contains(&"fly:forward".to_owned()));
    cam.handle(Event::Osc(OscMessage {
        addr: MODE.into(),
        args: vec![OscArg::Int(1)],
    }));
    assert!(
        says(&cam, MOVE_FIRST),
        "open, it asks for a move: VRChat reports none before"
    );
    cam.handle(pose_in([1.0, 2.0, 3.0, 0.0, 90.0, 0.0]));
    assert!(!shown(&cam, false).iter().any(|t| t.ends_with(STAND_IN)));
    let again = cam.handle(pose_in([1.0, 2.0, 3.0, 0.0, 90.0, 0.0]));
    assert!(
        !again.iter().any(|c| matches!(c, Command::Page { .. })),
        "an unchanged view isn't sent again"
    );
}

#[test]
fn a_held_pad_flies_the_camera_until_it_is_let_go() {
    let mut cam = known([5.0, 1.0, 0.0, 0.0, 90.0, 0.0]);
    let hold = |id: &str, down| {
        ui(
            PAGE,
            UiEvent::Held {
                id: id.into(),
                down,
            },
        )
    };
    let out = cam.handle(hold("fly:forward", true));
    assert!(out
        .iter()
        .any(|c| matches!(c, Command::Timer { id, .. } if id == PLAY)));
    let pose = sent_pose(&cam.handle(timer(PLAY))).expect("a pose a step");
    // At the normal speed, 2 m a second over one step, facing +X.
    let step = FRAME;
    assert!(
        close(pose, [5.0 + 2.0 * step, 1.0, 0.0, 0.0, 90.0, 0.0]),
        "{pose:?}"
    );
    cam.handle(hold("fly:forward", false));
    let out = cam.handle(timer(PLAY));
    assert!(
        !out.iter()
            .any(|c| matches!(c, Command::Timer { id, .. } if id == PLAY)),
        "let go, the clock stops"
    );
    let out = cam.handle(number_in(PAGE, "pad_turn", 90.0));
    assert!(fields(&out).is_empty() && sent(&out, "/usercamera/TurnSpeed").is_none());
    let out = with_send(&mut cam, number_in(EDITOR, "turn_speed", 2.0));
    assert_eq!(
        (sent(&out, "/usercamera/TurnSpeed"), cam.turn_speed),
        (Some(vec![OscArg::Float(2.0)]), 90.0),
        "VRChat's turn speed and the pads' are two"
    );
    cam.handle(hold("fly:yaw_right", true));
    let pose = sent_pose(&cam.handle(timer(PLAY))).unwrap();
    assert!(
        (pose[4] - (90.0 + 90.0 * step)).abs() < 1e-2,
        "90° a second turns 90° a second: {pose:?}"
    );
    let out = cam.handle(chosen(PAGE, "mode", 1));
    assert_eq!(sent(&out, MODE), Some(vec![OscArg::Int(1)]));
}

#[test]
fn the_roll_pads_roll_the_camera_from_any_start() {
    for start in [[0.0; 6], [0.0, 1.0, 0.0, 30.0, 120.0, 0.0]] {
        for (pad, sign) in [("roll_left", 1.0), ("roll_right", -1.0)] {
            let mut cam = known(start);
            cam.handle(number_in(PAGE, "pad_turn", 90.0));
            cam.handle(ui(
                PAGE,
                UiEvent::Held {
                    id: format!("fly:{pad}"),
                    down: true,
                },
            ));
            let mut pose = start;
            for _ in 0..10 {
                cam.handle(timer(PLAY));
                pose = cam.pose;
            }
            let rolled = 90.0 * FRAME * 10.0;
            assert!(
                (pose[5] - sign * rolled).abs() < 0.1,
                "{pad} from {start:?} rolls {rolled}°: {pose:?}"
            );
            assert!(
                (pose[3] - start[3]).abs() < 0.1 && (pose[4] - start[4]).abs() < 0.1,
                "and keeps pitch and yaw: {pose:?}"
            );
        }
    }
}

#[test]
fn a_leveled_turn_disables_its_pads_and_keys_but_poses_keep_their_roll() {
    fn holds(node: &Node, into: &mut Vec<(String, bool, String)>) {
        match node {
            Node::Column(items) | Node::Row(items) => items.iter().for_each(|n| holds(n, into)),
            Node::Split { main, side } => [main, side].into_iter().for_each(|n| holds(n, into)),
            Node::HoldButton {
                id, disabled, tip, ..
            } => into.push((id.clone(), *disabled, tip.clone())),
            _ => {}
        }
    }
    let off = |cam: &Camera| {
        let mut all = Vec::new();
        holds(cam.shown.as_ref().unwrap(), &mut all);
        all.into_iter().filter(|h| h.1).collect::<Vec<_>>()
    };
    let level = |field: &str, on: bool| {
        Event::Osc(OscMessage {
            addr: format!("/usercamera/{field}"),
            args: vec![OscArg::Bool(on)],
        })
    };
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    assert!(off(&cam).is_empty(), "nothing is off while nothing levels");
    cam.handle(ui(
        PAGE,
        UiEvent::Held {
            id: "fly:roll_left".into(),
            down: true,
        },
    ));
    cam.handle(level("AutoLevelRoll", true));
    let ids: Vec<String> = off(&cam).into_iter().map(|h| h.0).collect();
    assert_eq!(ids, ["fly:roll_left", "fly:roll_right"]);
    assert!(
        off(&cam)[0].2.ends_with("(off while Level roll is on)"),
        "{:?}",
        off(&cam)
    );
    assert!(cam.flying.is_empty(), "a roll held as it levels stops");
    cam.handle(level("AutoLevelPitch", true));
    assert_eq!(off(&cam).len(), 4, "pitch too: {:?}", off(&cam));
    cam.handle(fly(1, &[lin([0.0, 0.0, 0.0, 0.0, 0.0, 30.0, 1.0])]));
    let (poses, _) = run_plays(&mut cam, 1000);
    assert!(
        (poses.last().unwrap()[5] - 30.0).abs() < 1e-2,
        "a track's roll still goes out: {:?}",
        poses.last()
    );
}

#[test]
fn the_wheel_scales_the_move_speed_within_its_range_and_leaves_the_turn() {
    let mut cam = known([0.0; 6]);
    let wheel = |lines: f64| ui(PAGE, UiEvent::Wheel { lines });
    cam.handle(wheel(2.0));
    assert_eq!(cam.move_speed, 3.1, "2 × 1.25 twice, held to two figures");
    assert_eq!(cam.turn_speed, TURN_SPEED.2);
    cam.handle(wheel(-1000.0));
    assert_eq!(cam.move_speed, MOVE_SPEED.0);
    cam.handle(number_in(PAGE, "pad_move", 1e6));
    assert_eq!(
        cam.move_speed, MOVE_SPEED.1,
        "a slider value is clamped too"
    );
    cam.handle(number_in(PAGE, "pad_turn", 0.0));
    assert_eq!(cam.turn_speed, TURN_SPEED.0);
}

#[test]
fn pads_fly_level_by_default_and_on_the_cameras_axes_when_local() {
    let tilted = [0.0, 1.0, 0.0, 45.0, 90.0, 30.0];
    let step = FRAME;
    let flown = |pad: &str, local: bool| {
        let mut cam = known(tilted);
        if local {
            cam.handle(ui(
                PAGE,
                UiEvent::Toggled {
                    id: "local_nav".into(),
                    value: true,
                },
            ));
        }
        let held = |down| {
            ui(
                PAGE,
                UiEvent::Held {
                    id: format!("fly:{pad}"),
                    down,
                },
            )
        };
        cam.handle(held(true));
        let pose = sent_pose(&cam.handle(timer(PLAY))).unwrap();
        (position(&pose) - position(&tilted)) / (MOVE_SPEED.2 * step)
    };
    let near = |a: Vec3, b: Vec3| a.distance(b) < 1e-3;
    assert!(
        near(flown("forward", false), Vec3::X),
        "level, along the heading"
    );
    assert!(near(flown("rise", false), Vec3::Y), "level, straight up");
    assert!(
        near(flown("right", false), Vec3::NEG_Z),
        "level, the heading's right"
    );
    let view = rotation(&tilted) * Vec3::Z;
    assert!(near(flown("forward", true), view), "local, along the view");
    assert!(flown("forward", true).y < -0.5, "pitched down, it sinks");
}

#[test]
fn its_commands_stay_inside_its_prefixes() {
    let mut cam = known([0.0; 6]);
    let manifest = cam.manifest();
    let mut out = cam.handle(call("move_local", &[1.0, 0.0, 0.0]));
    out.extend(cam.handle(call("capture", &[])));
    out.extend(cam.handle(fly(2, &[FORWARD])));
    while !cam.plays.is_empty() {
        out.extend(cam.handle(timer(PLAY)));
    }
    for (field, ..) in SLIDERS {
        out.extend(cam.handle(Event::Write {
            field: field.into(),
            value: Value::Number(1.0),
        }));
    }
    for c in out {
        if let Command::Osc(m) = c {
            assert!(manifest.writes(&m.addr), "{}", m.addr);
        }
    }
}

fn options(out: &[Command], list: &str) -> Option<Vec<Item>> {
    out.iter().find_map(|c| match c {
        Command::Options { list: l, items } if l == list => Some(items.clone()),
        _ => None,
    })
}

#[test]
fn the_library_is_published_as_the_lists_actions_pick_from() {
    let mut cam = Camera::default();
    let out = cam.handle(Event::Start);
    assert_eq!(options(&out, TRACKS), Some(Vec::new()));
    assert_eq!(options(&out, POSITIONS), Some(Vec::new()));
    let item = |key: &str, label: &str| Item {
        key: key.into(),
        label: label.into(),
    };
    let (mut cam, out) = with_track("Orbit", &[FORWARD]);
    assert_eq!(options(&out, TRACKS), Some(vec![item("track_0", "Orbit")]));
    let unchanged = cam.handle(pose_in([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
    assert_eq!(
        options(&unchanged, TRACKS),
        None,
        "only when the library changes"
    );
    let out = cam.handle(press("save_position"));
    assert_eq!(
        options(&out, POSITIONS),
        Some(vec![item("pos_1", "Position")])
    );
}

#[test]
fn an_item_resolves_to_its_data_by_key() {
    let (mut cam, _) = with_track("Orbit", &[FORWARD]);
    cam.handle(press("save_position"));
    let resolve = |cam: &mut Camera, id: u64, list: &str, key: &str| {
        cam.handle(Event::Resolve {
            id,
            list: list.into(),
            key: key.into(),
        })
        .into_iter()
        .find_map(|c| match c {
            Command::Resolved { id: i, data } if i == id => Some(data),
            _ => None,
        })
    };
    assert_eq!(
        resolve(&mut cam, 1, TRACKS, "track_0"),
        Some(steps_value(&[FORWARD]))
    );
    assert_eq!(
        resolve(&mut cam, 2, POSITIONS, "pos_1"),
        Some(pose_list(&[0.0; 6]))
    );
    cam.track = Some(0);
    cam.handle(typed("rename", "Sweep"));
    assert_eq!(
        resolve(&mut cam, 3, TRACKS, "track_0"),
        Some(steps_value(&[FORWARD])),
        "a rename keeps the key"
    );
    cam.handle(press("delete"));
    assert_eq!(resolve(&mut cam, 4, TRACKS, "track_0"), Some(Value::None));
}

#[test]
fn fly_to_flies_to_a_pose_it_is_given() {
    let mut cam = known([0.0; 6]);
    let to = Value::List(
        [3.0, 1.0, 2.0, 0.0, 90.0, 0.0]
            .iter()
            .map(|n| Value::Number(*n))
            .collect(),
    );
    cam.handle(Event::Call {
        id: 9,
        method: "fly_to".into(),
        args: vec![to, Value::Number(0.2)],
        from: None,
    });
    let (poses, ids) = run_plays(&mut cam, 100);
    assert_eq!(ids, [9]);
    assert!(close(
        *poses.last().unwrap(),
        [3.0, 1.0, 2.0, 0.0, 90.0, 0.0]
    ));
}

#[test]
fn an_import_reuses_an_identical_item_and_numbers_a_taken_name() {
    let (mut cam, _) = with_track("Orbit", &[FORWARD]);
    cam.track = None;
    let import = |cam: &mut Camera, id: u64, list: &str, label: &str, data: Value| {
        let out = cam.handle(Event::Import {
            id,
            list: list.into(),
            label: label.into(),
            data,
        });
        let item = out.iter().find_map(|c| match c {
            Command::Imported { id: i, item } if *i == id => Some(item.clone()),
            _ => None,
        });
        (item.expect("answered"), out)
    };
    let item = |key: &str, label: &str| {
        Some(Item {
            key: key.into(),
            label: label.into(),
        })
    };
    let (same, _) = import(&mut cam, 1, TRACKS, "Spin", steps_value(&[FORWARD]));
    assert_eq!(same, item("track_0", "Orbit"), "identical steps: ours");
    let (other, out) = import(
        &mut cam,
        2,
        TRACKS,
        "Orbit",
        steps_value(&[FORWARD, FORWARD]),
    );
    assert_eq!(other, item("track_1", "Orbit (2)"));
    assert_eq!(cam.tracks[1].steps, [FORWARD, FORWARD]);
    assert!(stored(&out).iter().any(|(k, _)| k == "track_1"));
    assert_eq!(cam.track, Some(0), "an import doesn't change the choice");
    let (again, _) = import(
        &mut cam,
        3,
        TRACKS,
        "Orbit",
        steps_value(&[FORWARD, FORWARD]),
    );
    assert_eq!(again, other, "a re-sent track lands on the first import");
    let long = "L".repeat(NAME_MAX + 10);
    let (cut, _) = import(&mut cam, 4, TRACKS, &long, steps_value(&[]));
    assert_eq!(cut, item("track_2", &"L".repeat(NAME_MAX)));
    let (stage, _) = import(&mut cam, 5, POSITIONS, "Stage", pose_list(&[1.0; 6]));
    assert_eq!(stage, item("pos_3", "Stage"));
    assert_eq!(cam.positions[0].pose, [1.0; 6]);
    let (bad, _) = import(&mut cam, 6, POSITIONS, "Bad", Value::None);
    assert_eq!(bad, None);
}

#[test]
fn positions_saved_before_keys_get_keys_when_they_load() {
    let mut cam = Camera::default();
    let index = r#"{"next":2,"tracks":[],"positions":[{"name":"Stage","pose":[1,2,3,0,0,0]}]}"#;
    let out = cam.handle(Event::Loaded {
        key: LIBRARY_KEY.into(),
        value: Some(index.into()),
    });
    assert_eq!(cam.positions[0].key, "pos_2");
    assert_eq!(cam.next_key, 3);
    assert!(
        stored(&out)
            .iter()
            .any(|(k, v)| k == LIBRARY_KEY && v.contains("pos_2")),
        "kept, so the keys hold"
    );
}

#[test]
fn settings_read_vrchats_defaults_until_set_and_start_publishes_them() {
    let mut cam = Camera::default();
    let out = cam.handle(Event::Start);
    let published = fields(&out);
    assert_eq!(published.len(), SLIDERS.len() + TOGGLES.len());
    assert!(published.contains(&("zoom", Value::Number(45.0))));
    assert!(published.contains(&("orientation_is_landscape", Value::Bool(true))));
    assert!(published.contains(&("green_screen", Value::Bool(false))));
    cam.handle(press("open_editor"));
    let listed = shown(&cam, true);
    assert!(listed.contains(&"zoom=Some(45.0)".to_owned()), "{listed:?}");
    assert!(listed.contains(&"environment=Some(true)".to_owned()));
}

#[test]
fn the_page_opens_the_track_editor_from_one_wide_button() {
    let mut cam = known([0.0; 6]);
    cam.handle(timer("none"));
    let wide: Vec<String> = match cam.shown.as_ref().unwrap() {
        Node::Column(rows) => rows
            .iter()
            .filter_map(|n| match n {
                Node::Button { id, wide: true, .. } => Some(id.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    assert_eq!(wide, ["open_editor"]);
    cam.handle(press("open_editor"));
    assert!(cam.editor_open);
}

/// A track of `keys` world poses recorded from `start` with the editor,
/// open and chosen.
fn recorded(start: [f32; 6], keys: &[[f32; 6]]) -> Camera {
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
fn key_poses(cam: &Camera) -> Vec<[f32; 6]> {
    let t = &cam.tracks[cam.track.unwrap()];
    (0..t.keys())
        .map(|k| crate::motion::key_pose(&t.start.unwrap(), &t.steps, k))
        .collect()
}

/// The same place, and the same view and up within about 0.06°.
fn same_pose(a: [f32; 6], b: [f32; 6]) -> bool {
    let (ra, rb) = (rotation(&a), rotation(&b));
    position(&a).distance(position(&b)) < 1e-3
        && [Vec3::Y, Vec3::Z]
            .into_iter()
            .all(|v| (ra * v).distance(rb * v) < 1e-3)
}

const START: [f32; 6] = [1.0, 2.0, 3.0, 10.0, 20.0, 0.0];
const KEYS: [[f32; 6]; 3] = [
    [2.0, 2.0, 5.0, -15.0, 80.0, 0.0],
    [0.0, 1.0, 4.0, 30.0, -150.0, 10.0],
    [-3.0, 1.5, 0.0, 0.0, 170.0, 0.0],
];
const HERE: [f32; 6] = [5.0, 3.0, -2.0, 5.0, 45.0, 0.0];

#[test]
fn setting_a_key_to_the_camera_keeps_every_other_key_where_it_is() {
    for key in 0..=3 {
        let mut cam = recorded(START, &KEYS);
        let before = key_poses(&cam);
        cam.handle(chosen(EDITOR, "scene", key));
        cam.handle(pose_in(HERE));
        settle(&mut cam);
        cam.handle(edit("key_set"));
        let after = key_poses(&cam);
        assert_eq!(after.len(), before.len());
        for (k, (a, b)) in after.iter().zip(&before).enumerate() {
            let want = if k == key { HERE } else { *b };
            assert!(
                same_pose(*a, want),
                "set {key}, key {k}: {a:?} against {want:?}"
            );
        }
        let times = key_times(&cam.tracks[0].steps);
        assert_eq!(times, [0.0, 2.0, 4.0, 6.0], "and when it is");
    }
}

#[test]
fn a_set_key_keeps_the_whole_turns_typed_into_the_next_step() {
    let mut cam = recorded(START, &KEYS[..2]);
    cam.handle(chosen(EDITOR, "scene", 2));
    cam.handle(number_in(EDITOR, "s:1:4", 500.0));
    cam.handle(chosen(EDITOR, "scene", 1));
    cam.handle(pose_in(HERE));
    settle(&mut cam);
    cam.handle(edit("key_set"));
    let total: f32 = cam.tracks[0].steps.iter().map(|s| s[4]).sum();
    assert!((START[4] + total - (80.0 + 500.0)).abs() < 1e-3, "{total}");
}

#[test]
fn a_key_inserted_at_the_camera_keeps_the_later_keys_where_and_when_they_are() {
    let mut cam = recorded(START, &KEYS);
    let before = key_poses(&cam);
    cam.handle(chosen(EDITOR, "scene", 1));
    cam.handle(pose_in(HERE));
    settle(&mut cam);
    cam.handle(edit("key_insert"));
    let after = key_poses(&cam);
    assert_eq!(after.len(), 5);
    assert!(same_pose(after[2], HERE));
    for (a, b) in [(0, 0), (1, 1), (3, 2), (4, 3)] {
        assert!(same_pose(after[a], before[b]), "key {a}");
    }
    assert_eq!(key_times(&cam.tracks[0].steps), [0.0, 2.0, 3.0, 4.0, 6.0]);
    assert_eq!(cam.key, Some(2), "the inserted key is chosen");
    cam.handle(chosen(EDITOR, "scene", 4));
    cam.handle(edit("key_insert"));
    assert_eq!(
        key_times(&cam.tracks[0].steps).last(),
        Some(&8.0),
        "after the last"
    );
}

#[test]
fn go_to_key_snaps_the_camera_to_the_chosen_key() {
    let mut cam = recorded(START, &KEYS);
    cam.handle(chosen(EDITOR, "scene", 2));
    let pose = sent_pose(&cam.handle(edit("key_go"))).expect("at once");
    assert!(same_pose(pose, KEYS[1]), "{pose:?}");
    assert!(cam.plays.is_empty());
    let mut fresh = known(START);
    fresh.handle(edit("key_go"));
    assert_eq!(fresh.note.as_deref(), Some(CHOOSE_KEY));
}

#[test]
fn level_zeroes_the_roll_and_keeps_where_the_camera_looks() {
    let mut cam = known([1.0, 2.0, 3.0, 20.0, 30.0, 15.0]);
    let pose = sent_pose(&cam.handle(press("level"))).expect("a pose");
    assert!(close(pose, [1.0, 2.0, 3.0, 20.0, 30.0, 0.0]), "{pose:?}");
    let mut closed = Camera {
        pose: [0.0, 0.0, 0.0, 10.0, 0.0, 20.0],
        ..Camera::default()
    };
    assert!(
        sent_pose(&closed.handle(press("level"))).is_none(),
        "not linked"
    );
    assert!(
        close(closed.pose, [0.0, 0.0, 0.0, 10.0, 0.0, 0.0]),
        "the stand-in levels"
    );
}

#[test]
fn a_mode_while_the_camera_is_closed_says_to_open_it_and_still_asks() {
    let mut cam = known([0.0; 6]);
    cam.handle(Event::Osc(OscMessage {
        addr: MODE.into(),
        args: vec![OscArg::Int(0)],
    }));
    let out = cam.handle(chosen(PAGE, "mode", 1));
    assert_eq!(sent(&out, MODE), Some(vec![OscArg::Int(1)]));
    assert_eq!(cam.note.as_deref(), Some(OPEN_FIRST));
    cam.handle(chosen(PAGE, "mode", 0));
    assert_eq!(cam.note, None, "closing needs nothing open");
}

#[test]
fn every_icon_a_view_names_is_packaged() {
    let mut cam = recorded(START, &KEYS);
    cam.handle(chosen(EDITOR, "scene", 1));
    cam.handle(timer("none"));
    let packaged: Vec<String> = crate::icons::icons().into_iter().map(|i| i.name).collect();
    fn named(n: &Node, into: &mut Vec<String>) {
        match n {
            Node::Column(items) | Node::Row(items) => items.iter().for_each(|i| named(i, into)),
            Node::Split { main, side } => [main, side].into_iter().for_each(|n| named(n, into)),
            Node::Icon { icon } | Node::IconButton { icon, .. } | Node::HoldButton { icon, .. } => {
                into.push(icon.clone())
            }
            Node::Button {
                icon: Some(icon), ..
            } => into.push(icon.clone()),
            _ => {}
        }
    }
    let mut used = Vec::new();
    named(cam.shown.as_ref().unwrap(), &mut used);
    named(cam.shown_editor.as_ref().unwrap(), &mut used);
    assert!(used.len() > 20, "{used:?}");
    let missing: Vec<&String> = used.iter().filter(|u| !packaged.contains(u)).collect();
    assert!(missing.is_empty(), "{missing:?}");
}

/// VRChat reporting `pose`, settled.
fn settle(cam: &mut Camera) {
    cam.handle(timer(SETTLE));
}

#[test]
fn reports_wait_while_poses_go_out_and_the_last_one_is_taken_after() {
    let mut cam = known([0.0; 6]);
    cam.handle(fly(1, &[lin([0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.5])]));
    let mut poses = Vec::new();
    for _ in 0..100 {
        if cam.plays.is_empty() {
            break;
        }
        let out = cam.handle(timer(PLAY));
        // VRChat follows a send behind.
        if let Some(sent) = sent_pose(&out) {
            cam.handle(pose_in(poses.last().copied().unwrap_or([0.0; 6])));
            poses.push(sent);
        }
    }
    assert!(
        close(*poses.last().unwrap(), [0.0, 0.0, 4.0, 0.0, 0.0, 0.0]),
        "the late reports take nothing from the path: {:?}",
        poses.last()
    );
    cam.handle(pose_in([0.0, 0.0, 3.9, 0.0, 0.0, 0.0]));
    assert!(close(cam.pose, [0.0, 0.0, 4.0, 0.0, 0.0, 0.0]));
    settle(&mut cam);
    assert!(
        close(cam.pose, [0.0, 0.0, 3.9, 0.0, 0.0, 0.0]),
        "settled, VRChat's last word is taken"
    );
    cam.handle(pose_in([1.0, 0.0, 3.9, 0.0, 0.0, 0.0]));
    assert_eq!(cam.pose[0], 1.0, "and its moves after");
}

#[test]
fn a_step_moves_as_far_as_the_time_since_the_last_tick() {
    let mut cam = known([0.0; 6]);
    cam.handle(fly(1, &[lin([0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0])]));
    let at = |cam: &mut Camera, ms| {
        cam.handle(Event::Timer {
            id: PLAY.into(),
            at_ms: ms,
        });
        cam.pose[2]
    };
    let first = at(&mut cam, 1000);
    assert!((first - FRAME).abs() < 1e-4, "a first tick is one frame");
    assert!(
        (at(&mut cam, 1050) - first - 0.05).abs() < 1e-4,
        "50 ms later, 5 cm"
    );
    let before = at(&mut cam, 1060);
    assert!(
        (at(&mut cam, 5000) - before - 0.1).abs() < 1e-4,
        "a stall moves 100 ms"
    );
}

#[test]
fn a_right_drag_in_the_view_turns_the_camera() {
    let mut cam = known([0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    let look = |x: f64, y: f64| {
        ui(
            EDITOR,
            UiEvent::Look {
                id: "scene".into(),
                x,
                y,
            },
        )
    };
    cam.handle(look(30.0, 10.0));
    cam.handle(look(20.0, 0.0));
    let pose = sent_pose(&cam.handle(timer(PLAY))).unwrap();
    assert!(close(pose, [0.0, 1.0, 0.0, 2.0, 10.0, 0.0]), "{pose:?}");
    let out = cam.handle(timer(PLAY));
    assert!(sent_pose(&out).is_none() && !cam.ticking, "nothing moved");
}

#[test]
fn play_starts_at_the_chosen_key_pauses_and_stops_back_at_the_start() {
    let mut cam = recorded(START, &KEYS);
    settle(&mut cam);
    cam.handle(pose_in(HERE));
    let out = cam.handle(edit("play"));
    assert!(
        sent_pose(&out).is_some_and(|p| same_pose(p, START)),
        "at the last key chosen, snapped to the start"
    );
    for _ in 0..150 {
        cam.handle(timer(PLAY));
    }
    cam.handle(edit("play"));
    assert!(cam.plays.is_empty(), "Play again pauses");
    let paused = cam.pose;
    let resume = cam.resume.clone().unwrap();
    assert_eq!(resume.step, 1, "2.5 s in: the second step");
    let out = cam.handle(edit("play"));
    assert!(sent_pose(&out).is_none(), "not moved since: on from there");
    assert_eq!(cam.plays.len(), 1);
    cam.handle(edit("play"));
    cam.handle(edit("play"));
    cam.handle(press("play:track_0"));
    cam.handle(press("play:track_0"));
    assert_eq!(cam.plays.len(), 1, "a track never plays twice at once");
    let next = sent_pose(&cam.handle(timer(PLAY))).unwrap();
    assert!(
        position(&next).distance(position(&paused)) < 0.2,
        "{next:?}"
    );
    cam.handle(edit("play"));
    settle(&mut cam);
    cam.handle(pose_in(HERE));
    let out = cam.handle(edit("play"));
    assert!(
        sent_pose(&out).is_some_and(|p| same_pose(p, KEYS[0])),
        "moved since: snapped to the last key reached"
    );
    let out = cam.handle(edit("stop"));
    assert!(
        cam.plays.is_empty() && sent_pose(&out).is_some_and(|p| same_pose(p, START)),
        "Stop goes back to the start"
    );
    assert_eq!(
        playhead_of(cam.shown_editor.as_ref().unwrap()),
        Some(Some(0.0))
    );
    assert_eq!(cam.key, Some(0), "and chooses the first key");
    let out = cam.handle(chosen(EDITOR, "timeline", 2));
    assert!(
        sent_pose(&out).is_some_and(|p| same_pose(p, KEYS[1])),
        "choosing a key puts the camera there"
    );
    let out = cam.handle(edit("play"));
    assert!(sent_pose(&out).is_none(), "and Play goes on from it");
    assert_eq!(cam.plays.len(), 1);
    let (_, _) = run_plays(&mut cam, 1000);
    assert!(cam.resume.is_none(), "played to its end, it starts over");
}

/// `s` with `curve`.
fn curved(s: [f32; 7], curve: Curve) -> Step {
    with_curve(lin(s), curve)
}

/// The editor's scene and timeline as last published.
fn editor_nodes(cam: &Camera) -> (Option<Node>, Option<vrc_we_sdk::Lane>) {
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

#[test]
fn a_dots_pace_keeps_its_steps_time_passes_its_dot_and_mirrors_about_1() {
    for at in [0.0, 0.1, 0.5, 0.9, 1.0] {
        for p in [0.05, 0.5, 1.0, 1.5, 2.0] {
            assert!(
                (progress(at, p, 1.0) - 1.0).abs() < 1e-5,
                "on time: {at} {p}"
            );
            let c = dot_place(at);
            assert!(
                (progress(at, p, c) - c).abs() < 1e-5,
                "the dot at its share"
            );
            assert!((pace(at, p, c) - p).abs() < 1e-5, "through the dot");
            assert!((pace(at, p, 0.0) - 1.0).abs() < 1e-5 && (pace(at, p, 1.0) - 1.0).abs() < 1e-5);
            let mut last = 0.0;
            for k in 0..=200 {
                let t = k as f32 / 200.0;
                let v = pace(at, p, t);
                assert!(v.is_finite() && v >= 0.0, "never below 0: {at} {p} {t} {v}");
                assert!(
                    (v + pace(at, 2.0 - p, t) - 2.0).abs() < 1e-4,
                    "mirrors about 1"
                );
                let f = progress(at, p, t);
                assert!(f >= last - 1e-6, "never back: {at} {p} {t}");
                last = f;
            }
        }
    }
    for k in 0..=10 {
        let t = k as f32 / 10.0;
        assert!(
            (progress(0.3, 1.0, t) - t).abs() < 1e-6,
            "a dot at 1 is an even pace"
        );
    }
}

#[test]
fn a_linear_track_plays_its_straight_lines_at_an_even_pace() {
    let legs = legs(
        &[
            lin([0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 2.0]),
            lin([2.0, 0.0, 0.0, 0.0, 90.0, 0.0, 1.0]),
        ],
        false,
    );
    for k in 0..=20 {
        let t = k as f32 / 20.0;
        let (a, b) = (legs[0].at(t), legs[1].at(t));
        assert!(
            a.pos.distance(Vec3::new(0.0, 0.0, 4.0 * t)) < 1e-4,
            "{t} {:?}",
            a.pos
        );
        assert!(
            b.pos.distance(Vec3::new(2.0 * t, 0.0, 4.0)) < 1e-4,
            "{t} {:?}",
            b.pos
        );
    }
}

#[test]
fn a_smooth_orbit_curves_and_keeps_its_speed_along_each_leg() {
    let ring: Vec<Vec3> = (0..=6)
        .map(|k| {
            let a = (k as f32 * 60.0).to_radians();
            Vec3::new(5.0 * a.cos(), 0.0, 5.0 * a.sin())
        })
        .collect();
    let steps: Vec<Step> = ring
        .windows(2)
        .map(|w| {
            let d = w[1] - w[0];
            curved([d.x, d.y, d.z, 0.0, 0.0, 0.0, 1.0], SMOOTH)
        })
        .collect();
    let legs = legs(&steps, false);
    for leg in &legs[1..5] {
        let mid = leg.at(0.5).pos;
        let chord = (leg.from.pos + leg.to.pos) / 2.0;
        assert!(
            mid.length() > chord.length() + 0.1,
            "it bulges out: {mid:?}"
        );
        let dt = 0.005;
        let speeds: Vec<f32> = (0..200)
            .map(|k| {
                let t = k as f32 * dt;
                leg.at(t + dt).pos.distance(leg.at(t).pos) / dt
            })
            .collect();
        let mean = speeds.iter().sum::<f32>() / speeds.len() as f32;
        let worst = speeds
            .iter()
            .map(|s| (s - mean).abs() / mean)
            .fold(0.0, f32::max);
        assert!(worst < 0.05, "an even pace is an even speed: {worst}");
    }
}

#[test]
fn a_hold_gets_no_handle_and_a_hold_or_a_jump_breaks_the_curve() {
    let held = legs(
        &[
            curved([0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0], SMOOTH),
            curved([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0], SMOOTH),
            curved([2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0], SMOOTH),
        ],
        false,
    );
    let jumped = legs(
        &[
            curved([0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0], SMOOTH),
            curved([2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], SMOOTH),
            curved([0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0], SMOOTH),
        ],
        false,
    );
    for k in 0..=10 {
        let t = k as f32 / 10.0;
        assert_eq!(held[1].at(t).pos, held[1].from.pos, "a hold stays put");
        assert!(held[0].at(t).pos.x.abs() < 1e-5, "into a hold, straight");
        assert!(
            (held[2].at(t).pos.z - 2.0).abs() < 1e-5,
            "out of a hold, straight"
        );
        assert!(jumped[0].at(t).pos.x.abs() < 1e-5, "into a jump, straight");
        assert!(
            (jumped[2].at(t).pos.x - 2.0).abs() < 1e-5,
            "out of a jump, straight"
        );
    }
}

#[test]
fn every_key_is_reached_at_its_time_whatever_its_dot() {
    let mut cam = known([0.0; 6]);
    cam.handle(fly(
        1,
        &[
            curved([0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5], [0.0, 0.0, 0.2, 1.8]),
            curved([0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5], [0.0, 0.0, 0.9, 0.3]),
        ],
    ));
    let (poses, ids) = run_plays(&mut cam, 1000);
    let ticks = (0.5 / FRAME).round() as usize;
    assert!(
        (poses[ticks - 1][2] - 1.0).abs() < 1e-4,
        "key 1 at 0.5 s: {:?}",
        poses[ticks - 1]
    );
    assert!((poses.last().unwrap()[2] - 2.0).abs() < 1e-4);
    assert_eq!(poses.len(), 2 * ticks, "the end at 1 s, not a tick late");
    assert_eq!(ids, [1]);
}

#[test]
fn linear_and_smooth_reset_a_keys_handles_and_a_new_key_takes_the_last_chosen() {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    cam.handle(edit("add_key"));
    for z in [2.0, 4.0, 6.0] {
        cam.handle(pose_in([0.0, 0.0, z, 0.0, 0.0, 0.0]));
        cam.handle(edit("add_key"));
    }
    let steps = |cam: &Camera| cam.tracks[0].steps.clone();
    assert!(
        steps(&cam)
            .iter()
            .all(|s| s[OUT_LEN] == 1.0 && s[IN_LEN] == 1.0),
        "smooth at first"
    );
    cam.handle(chosen(EDITOR, "timeline", 1));
    let out = cam.handle(number_in(EDITOR, "in_len", 0.4));
    assert!(stored(&out).is_empty(), "a slider moving keeps nothing yet");
    cam.handle(number_in(EDITOR, "out_len", 1.7));
    assert_eq!(
        (steps(&cam)[0][IN_LEN], steps(&cam)[1][OUT_LEN]),
        (0.4, 1.0),
        "one side each, at most 1"
    );
    let out = cam.handle(timer(SAVE));
    assert!(
        stored(&out).iter().any(|(k, _)| k == "track_0"),
        "kept once it settles"
    );
    cam.handle(chosen(EDITOR, "key_shape", 1));
    assert_eq!(
        (steps(&cam)[0][IN_LEN], steps(&cam)[1][OUT_LEN]),
        (1.0, 1.0),
        "Smooth puts an edited key back to its defaults"
    );
    let out = cam.handle(chosen(EDITOR, "key_shape", 0));
    assert_eq!(
        (steps(&cam)[0][IN_LEN], steps(&cam)[1][OUT_LEN]),
        (0.0, 0.0),
        "Linear"
    );
    assert!(
        stored(&out)
            .iter()
            .any(|(k, v)| k == LIBRARY_KEY && v.contains("\"key_handles\":[0.0,0.0]")),
        "kept for the next key: {:?}",
        stored(&out)
    );
    cam.handle(pose_in([0.0, 0.0, 8.0, 0.0, 0.0, 0.0]));
    cam.handle(edit("add_key"));
    let last = *steps(&cam).last().unwrap();
    assert_eq!(
        (last[OUT_LEN], last[IN_LEN]),
        (0.0, 0.0),
        "the new key takes them"
    );
    let sliders = |cam: &Camera| {
        let mut ids = Vec::new();
        if let (Some(Node::Column(rows)), _) = editor_nodes(cam) {
            for r in rows {
                if let Node::Slider { id, .. } = r {
                    ids.push(id);
                }
            }
        }
        ids
    };
    cam.handle(chosen(EDITOR, "timeline", 0));
    assert_eq!(sliders(&cam), ["out_len"], "the start has no way in");
    cam.handle(chosen(EDITOR, "timeline", 4));
    assert_eq!(sliders(&cam), ["in_len"], "the last key no way out");
    cam.handle(chosen(EDITOR, "timeline", 2));
    cam.handle(pose_in([1.0, 0.0, 4.0, 0.0, 0.0, 0.0]));
    settle(&mut cam);
    let before = (steps(&cam)[1][IN_LEN], steps(&cam)[2][OUT_LEN]);
    cam.handle(edit("key_set"));
    assert_eq!(
        (steps(&cam)[1][IN_LEN], steps(&cam)[2][OUT_LEN]),
        before,
        "set to the camera, a key keeps its shape"
    );
}

#[test]
fn a_dragged_dot_paces_its_step_and_the_track_is_kept_once_let_go() {
    let (mut cam, _) = with_track(
        "Orbit",
        &[
            lin([0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0]),
            lin([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            lin([0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0]),
        ],
    );
    cam.handle(press("open_editor"));
    let (_, lane) = editor_nodes(&cam);
    let lane = lane.expect("a lane");
    assert_eq!(lane.dots.len(), 2, "a jump has no dot");
    assert_eq!((lane.dots[1].from, lane.dots[1].to), (2.0, 4.0));
    let drag = |done| {
        ui(
            EDITOR,
            UiEvent::Dragged {
                id: "timeline".into(),
                index: 1,
                at: 2.5,
                value: 1.5,
                done,
            },
        )
    };
    let out = cam.handle(drag(false));
    let s = cam.tracks[0].steps[2];
    assert!(
        (s[DOT_AT] - 0.25).abs() < 1e-5 && s[DOT_PACE] == 1.5,
        "{s:?}"
    );
    assert!(stored(&out).is_empty(), "nothing kept while it moves");
    let out = cam.handle(drag(true));
    assert!(
        stored(&out).iter().any(|(k, _)| k == "track_0"),
        "kept once let go"
    );
    let (mut long, _) = with_track("Long", &vec![FORWARD; 257]);
    long.handle(press("open_editor"));
    assert!(
        editor_nodes(&long).1.is_none(),
        "too long a track has no lane"
    );
    assert!(shown(&long, true)
        .iter()
        .any(|t| t.starts_with("Pace is edited")));
}

#[test]
fn an_old_track_of_7_numbers_loads_straight_at_an_even_pace_and_steps_round_trip() {
    let (mut cam, _) = with_track("Orbit", &[FORWARD]);
    cam.handle(Event::Loaded {
        key: "track_0".into(),
        value: Some("[[0,0,1,0,0,0,2]]".into()),
    });
    assert_eq!(
        cam.tracks[0].steps,
        [lin([0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 2.0])]
    );
    let steps = vec![curved(
        [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
        [0.5, 0.75, 0.25, 1.75],
    )];
    assert_eq!(steps_of(&steps_value(&steps)), steps);
}

/// A track of `keys` keys along +z, 2 m and 1 s apart, open, in the editor.
fn straight_track(keys: usize) -> Camera {
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

#[test]
fn closing_a_track_adds_the_way_back_kept_by_every_edit_and_joins_its_start() {
    let mut cam = straight_track(4);
    let n = |cam: &Camera| cam.tracks[0].steps.len();
    assert_eq!(n(&cam), 3);
    let ends_at_start = |cam: &Camera| {
        let s = &cam.tracks[0].steps;
        (0..6).all(|k| s.iter().map(|s| s[k]).sum::<f32>().abs() < 1e-4)
    };
    let out = cam.handle(chosen(EDITOR, "loop", 1));
    assert_eq!(n(&cam), 4, "the way back is a step");
    assert!(ends_at_start(&cam));
    assert!(
        stored(&out)
            .iter()
            .any(|(k, v)| k == LIBRARY_KEY && v.contains("\"looping\":\"closed\"")),
        "kept"
    );
    let sliders = |cam: &Camera| match editor_nodes(cam).0 {
        Some(Node::Column(rows)) => rows
            .into_iter()
            .filter_map(|r| match r {
                Node::Slider { id, .. } => Some(id),
                _ => None,
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    cam.handle(chosen(EDITOR, "timeline", 0));
    assert_eq!(
        sliders(&cam),
        ["in_len", "out_len"],
        "the start has a way in"
    );
    cam.handle(number_in(EDITOR, "in_len", 0.25));
    assert_eq!(
        cam.tracks[0].steps[3][IN_LEN], 0.25,
        "the way back's end is its in"
    );
    let joined = legs(&cam.tracks[0].steps, true);
    let open = legs(&cam.tracks[0].steps, false);
    assert!(
        joined[0].at(0.1).pos.distance(open[0].at(0.1).pos) > 1e-3,
        "the start's direction joins both its legs"
    );
    cam.handle(chosen(EDITOR, "timeline", 3));
    cam.handle(pose_in([3.0, 0.0, 9.0, 0.0, 0.0, 0.0]));
    settle(&mut cam);
    cam.handle(edit("add_key"));
    assert_eq!(n(&cam), 5, "a new key goes before the way back");
    assert_eq!(cam.key, Some(4));
    assert!(ends_at_start(&cam), "and the way back still leads home");
    cam.handle(chosen(EDITOR, "timeline", 5));
    cam.handle(edit("key_del"));
    assert_eq!(n(&cam), 5, "the end of a closed track stays");
    assert!(cam.note.as_deref().is_some_and(|t| t.contains("Open")));
    cam.handle(chosen(EDITOR, "timeline", 4));
    cam.handle(edit("key_later"));
    assert_eq!(cam.key, Some(4), "no key moves past the way back");
    cam.handle(chosen(EDITOR, "loop", 0));
    assert_eq!(n(&cam), 4, "opened, the way back is gone");
}

#[test]
fn a_looping_track_goes_round_until_stopped() {
    let mut cam = straight_track(3);
    cam.handle(chosen(EDITOR, "loop", 2));
    cam.handle(chosen(EDITOR, "timeline", 0));
    let total = cam.tracks[0].seconds();
    cam.handle(edit("play"));
    let ticks = (total / FRAME).round() as usize;
    let (poses, _) = run_plays(&mut cam, ticks + 10);
    assert!(!cam.plays.is_empty(), "still playing after one round");
    assert!(
        close(poses[ticks - 1], [0.0; 6]),
        "a round ends at the start, on time: {:?}",
        poses[ticks - 1]
    );
    let (more, _) = run_plays(&mut cam, ticks / 2);
    assert!(more.last().unwrap()[2] > 1.0, "and goes round again");
    cam.handle(edit("stop"));
    assert!(cam.plays.is_empty());
    cam.handle(chosen(EDITOR, "loop", 1));
    cam.handle(edit("play"));
    run_plays(&mut cam, 3 * ticks);
    assert!(cam.plays.is_empty(), "closed, it plays once");
}

/// The editor's 3D view as last built.
fn scene_of(node: &Node) -> Option<&Node> {
    match node {
        Node::Scene3d { .. } => Some(node),
        Node::Column(items) | Node::Row(items) => items.iter().find_map(scene_of),
        Node::Split { main, side } => scene_of(main).or_else(|| scene_of(side)),
        _ => None,
    }
}

#[test]
fn the_stand_in_sits_at_the_chosen_tracks_start_and_moves_until_vrchat_reports() {
    let mut cam = Camera::default();
    cam.handle(Event::Start);
    let index = r#"{"next":2,"tracks":[
        {"name":"A","key":"track_0","start":[1,2,3,0,90,0],"looping":"open"},
        {"name":"B","key":"track_1","start":[5,0,0,0,0,0],"looping":"open"}],
        "positions":[],"chosen":"track_1"}"#;
    cam.handle(Event::Loaded {
        key: LIBRARY_KEY.into(),
        value: Some(index.into()),
    });
    assert_eq!(cam.track, Some(1), "the chosen track is remembered");
    assert_eq!(cam.pose, [5.0, 0.0, 0.0, 0.0, 0.0, 0.0], "at its start");
    let out = cam.handle(chosen(EDITOR, "tracks", 0));
    assert_eq!(cam.pose, [1.0, 2.0, 3.0, 0.0, 90.0, 0.0]);
    assert!(
        stored(&out)
            .iter()
            .any(|(k, v)| k == LIBRARY_KEY && v.contains(r#""chosen":"track_0""#)),
        "a choice is kept"
    );
    cam.handle(ui(
        PAGE,
        UiEvent::Held {
            id: "fly:forward".into(),
            down: true,
        },
    ));
    let out = cam.handle(timer(PLAY));
    assert!(
        sent_pose(&out).is_none() && fields(&out).is_empty(),
        "nothing to VRChat"
    );
    assert!(cam.pose[0] > 1.0, "the stand-in flies: {:?}", cam.pose);
    let out = cam.handle(call("move_local", &[0.0, 0.0, 1.0]));
    assert!(
        sent_pose(&out).is_none(),
        "a script's move waits for VRChat"
    );
    cam.handle(ui(
        PAGE,
        UiEvent::Held {
            id: "fly:forward".into(),
            down: false,
        },
    ));
    let out = cam.handle(pose_in([9.0, 1.0, 0.0, 0.0, 0.0, 0.0]));
    assert_eq!(fields(&out).len(), 6, "linked, every pose field");
    assert_eq!(
        cam.pose,
        [9.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        "VRChat's pose wins"
    );
    cam.handle(chosen(EDITOR, "tracks", 1));
    assert_eq!(
        cam.pose[0], 9.0,
        "a linked camera stays where VRChat has it"
    );
    let mut out = cam.handle(call("move_local", &[0.0, 0.0, 1.0]));
    out.extend(cam.handle(timer(PLAY)));
    assert!(sent_pose(&out).is_some(), "with the clock's next send");
    cam.handle(Event::Osc(OscMessage {
        addr: MODE.into(),
        args: vec![OscArg::Int(0)],
    }));
    let out = cam.handle(press("level"));
    assert!(
        sent_pose(&out).is_none(),
        "closed in VRChat: a stand-in again"
    );
}

#[test]
fn the_elastic_moves_at_once_keeps_up_with_a_steady_move_and_stops_without_overshoot() {
    let (rate, speed) = (4.0, 2.0);
    let left = (-rate * FRAME).exp();
    let mut jump = Follow::at([0.0; 6]);
    jump.step(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0], rate, FRAME);
    assert!(
        (jump.pose[0] - (1.0 - left)).abs() < 1e-6,
        "the first tick closes its share at once, no build-up: {:?}",
        jump.pose
    );
    let mut f = Follow::at([0.0; 6]);
    let mut target = [0.0; 6];
    let mut last = 0.0;
    for k in 1..=600 {
        target[0] = speed * k as f32 * FRAME;
        target[4] = 60.0 * k as f32 * FRAME;
        last = f.pose[0];
        f.step(&target, rate, FRAME);
    }
    let lag = target[0] - f.pose[0];
    let steady = speed * FRAME * left / (1.0 - left);
    assert!(
        (lag - steady).abs() < 1e-3,
        "a constant lag: {lag} against {steady}"
    );
    assert!(
        ((f.pose[0] - last) / FRAME - speed).abs() < 0.01,
        "at the move's speed"
    );
    assert!(
        f.pose[4] > 580.0,
        "a turn runs on past 360° untouched: {:?}",
        f.pose
    );
    let mut most: f32 = 0.0;
    for _ in 0..600 {
        f.step(&target, rate, FRAME);
        most = most.max(f.pose[0]);
    }
    assert!(most <= target[0] + 1e-3, "no overshoot: {most}");
    assert!(f.settled(&target));
}

#[test]
fn smoothing_is_done_here_with_vrchats_off_while_this_drives_and_back_on_at_rest() {
    let mut cam = known([0.0, 0.0, 0.0, 0.0, 300.0, 0.0]);
    cam.handle(number_in(EDITOR, SMOOTHING_STRENGTH, 3.0));
    cam.handle(number_in(PAGE, "pad_turn", 120.0));
    let smooth = |out: &[Command]| sent(out, "/usercamera/SmoothMovement");
    let held = |id: &str, down| {
        ui(
            PAGE,
            UiEvent::Held {
                id: id.into(),
                down,
            },
        )
    };
    cam.handle(held("fly:yaw_right", true));
    let (mut offs, mut yaws) = (0, Vec::new());
    for k in 0..120 {
        if k == 60 {
            cam.handle(held("fly:forward", true));
        }
        let out = cam.handle(timer(PLAY));
        offs += usize::from(smooth(&out) == Some(vec![OscArg::Bool(false)]));
        yaws.extend(sent_pose(&out).map(|p| p[4]));
    }
    assert_eq!(offs, 1, "VRChat's smoothing goes off once, as this starts");
    assert!(
        yaws.windows(2).all(|w| w[1] > w[0]) && *yaws.last().unwrap() > 360.0,
        "the turn eases on past 360° without a swing back, a move beside it: {yaws:?}"
    );
    assert!(
        cam.pose[4] - cam.shown_pose()[4] > 1.0,
        "what is sent trails the pose"
    );
    cam.handle(Event::Osc(OscMessage {
        addr: "/usercamera/SmoothMovement".into(),
        args: vec![OscArg::Bool(false)],
    }));
    assert!(
        cam.smoothing(),
        "VRChat's echo of this switching it off is not the person's"
    );
    cam.handle(held("fly:yaw_right", false));
    cam.handle(held("fly:forward", false));
    let mut ticks = 0;
    while cam.ticking && ticks < 2000 {
        cam.handle(timer(PLAY));
        ticks += 1;
    }
    assert!(
        !cam.ticking && ticks > 5,
        "the clock runs until the camera rests"
    );
    assert_eq!(cam.shown_pose(), cam.pose);
    let out = cam.handle(timer(SETTLE));
    assert_eq!(
        smooth(&out),
        Some(vec![OscArg::Bool(true)]),
        "back on at rest"
    );
    assert_eq!(sent_pose(&out), Some(cam.pose), "aimed at where it rests");
}

#[test]
fn the_smoothing_slider_reaches_0_to_switch_smoothing_off() {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    let slider = |cam: &Camera| {
        shown(cam, true)
            .into_iter()
            .find(|t| t.starts_with("smoothing_strength="))
    };
    assert_eq!(
        slider(&cam).as_deref(),
        Some("smoothing_strength=Some(0.0)")
    );
    let out = with_send(&mut cam, number_in(EDITOR, SMOOTHING_STRENGTH, 3.0));
    assert_eq!(
        (
            sent(&out, "/usercamera/SmoothingStrength"),
            sent(&out, "/usercamera/SmoothMovement")
        ),
        (
            Some(vec![OscArg::Float(3.0)]),
            Some(vec![OscArg::Bool(true)])
        ),
        "above 0 it sets the strength and turns smoothing on"
    );
    assert_eq!(
        slider(&cam).as_deref(),
        Some("smoothing_strength=Some(3.0)")
    );
    let out = cam.handle(number_in(EDITOR, SMOOTHING_STRENGTH, 0.0));
    assert_eq!(
        sent(&out, "/usercamera/SmoothMovement"),
        Some(vec![OscArg::Bool(false)])
    );
    assert!(
        sent(&out, "/usercamera/SmoothingStrength").is_none(),
        "its strength kept"
    );
    assert_eq!(
        slider(&cam).as_deref(),
        Some("smoothing_strength=Some(0.0)")
    );
}

#[test]
fn while_the_clock_runs_the_editor_moves_its_camera_and_playhead_alone() {
    let mut cam = straight_track(4);
    cam.handle(chosen(EDITOR, "timeline", 0));
    cam.handle(edit("play"));
    let (mut modals, mut marks) = (0, 0);
    for _ in 0..30 {
        for c in cam.handle(timer(PLAY)) {
            match c {
                Command::Modal { .. } => modals += 1,
                Command::Mark { .. } => marks += 1,
                _ => {}
            }
        }
    }
    assert!(marks >= 50, "a camera and a playhead a tick: {marks}");
    assert_eq!(modals, 2, "the whole view now and then");
    run_plays(&mut cam, 10_000);
    let built = scene_of(cam.shown_editor.as_ref().unwrap()).cloned();
    let Some(Node::Scene3d { camera, hint, .. }) = built else {
        panic!("a scene");
    };
    assert_eq!(camera, Some(cam.scene_camera()), "built anew once it stops");
    assert_eq!(hint.as_deref(), Some(HINT));
}

#[test]
fn a_closed_track_draws_its_end_as_its_start() {
    let mut cam = straight_track(3);
    let closed = |cam: &Camera| match scene_of(cam.shown_editor.as_ref().unwrap()) {
        Some(Node::Scene3d { closed, .. }) => *closed,
        _ => panic!("a scene"),
    };
    assert!(!closed(&cam));
    cam.handle(chosen(EDITOR, "loop", 1));
    assert!(closed(&cam));
}

#[test]
fn wex_numbers_take_four_decimals_at_most() {
    assert_eq!(
        [1.5, -2.25, 3.0, 0.12345, -0.00001, 1234.5, -1000.0].map(wex_number),
        ["1.5", "-2.25", "3", "0.1235", "0", "1234.5", "-1000"]
    );
}

fn write(field: &str, value: f64) -> Event {
    Event::Write {
        field: field.into(),
        value: Value::Number(value),
    }
}

/// The out of the optics' rest ending, as its timer does with no clock.
fn rests_end(cam: &mut Camera) -> Vec<Command> {
    cam.handle(timer(REST))
}

/// Every value of `addr` sent while the plays run, and once they end.
fn sent_while_playing(cam: &mut Camera, addr: &str) -> Vec<f32> {
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

#[test]
fn every_key_holds_the_cameras_optics_and_a_play_applies_them_at_each_key() {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    cam.handle(write("zoom", 30.0));
    cam.handle(edit("add_key"));
    cam.handle(pose_in([0.0, 0.0, 2.0, 0.0, 0.0, 0.0]));
    cam.handle(write("zoom", 90.0));
    cam.handle(write("aperture", 8.0));
    cam.handle(edit("add_key"));
    let t = &cam.tracks[0];
    assert_eq!(t.optics.map(|o| o[0]), Some(30.0), "the start's");
    assert_eq!(
        t.steps[0][OPTICS..],
        [90.0, 1.5, 8.0, 0.0],
        "each key's own"
    );
    let out = cam.handle(chosen(EDITOR, "timeline", 0));
    assert_eq!(
        sent(&out, "/usercamera/Zoom"),
        Some(vec![OscArg::Float(30.0)]),
        "choosing the start puts its optics on the camera"
    );
    cam.handle(edit("play"));
    let zooms = sent_while_playing(&mut cam, "/usercamera/Zoom");
    assert_eq!(
        zooms,
        [90.0],
        "whole as the play reaches the key, never eased"
    );
    let Some(Value::List(steps)) = reply_of(&cam.handle(named(9, "track", "Track", &[])), 9) else {
        panic!("a track's steps");
    };
    assert!(
        steps
            .iter()
            .all(|s| matches!(s, Value::List(v) if v.len() == WIRE)),
        "what crosses has no optics"
    );
}

#[test]
fn a_track_kept_without_optics_leaves_the_settings_alone_until_a_key_is_set() {
    let (mut cam, _) = with_track(
        "Orbit",
        &[FORWARD, lin([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0])],
    );
    cam.handle(press("open_editor"));
    cam.handle(edit("play"));
    assert!(sent_while_playing(&mut cam, "/usercamera/Zoom").is_empty());
    cam.handle(write("zoom", 70.0));
    cam.handle(edit("start_here"));
    let t = &cam.tracks[0];
    assert_eq!(t.optics.map(|o| o[0]), Some(70.0));
    assert!(
        t.steps.iter().all(|s| s[OPTICS] == 70.0),
        "every key takes the camera's, so nothing changes until one differs"
    );
}

#[test]
fn a_jump_to_a_key_or_a_tracks_start_is_exact_while_smoothing() {
    let mut cam = recorded(START, &KEYS);
    settle(&mut cam);
    cam.handle(number_in(EDITOR, SMOOTHING_STRENGTH, 5.0));
    cam.handle(pose_in(HERE));
    cam.handle(chosen(EDITOR, "timeline", 2));
    let out = cam.handle(edit("key_go"));
    assert!(
        sent_pose(&out).is_some_and(|p| same_pose(p, KEYS[1])),
        "at once"
    );
    assert!(cam.follow.is_none());
    cam.handle(pose_in(HERE));
    settle(&mut cam);
    let out = cam.handle(chosen(EDITOR, "timeline", 0));
    assert!(
        sent_pose(&out).is_some_and(|p| same_pose(p, START)),
        "the start at once"
    );
    cam.handle(edit("play"));
    let first = sent_pose(&cam.handle(timer(PLAY))).unwrap();
    assert!(
        position(&first).distance(position(&START)) < 0.05,
        "and the play goes on from there, smoothed: {first:?}"
    );
}

#[test]
fn scrubbing_the_ruler_puts_the_camera_there_and_play_goes_on_from_there() {
    let mut cam = recorded(START, &KEYS);
    settle(&mut cam);
    cam.handle(chosen(EDITOR, "timeline", 0));
    cam.handle(edit("play"));
    cam.handle(timer(PLAY));
    let scrub = |at: f64| {
        ui(
            EDITOR,
            UiEvent::Scrubbed {
                id: "timeline".into(),
                at,
            },
        )
    };
    let out = cam.handle(scrub(3.0));
    assert!(cam.plays.is_empty(), "a play pauses");
    let t = &cam.tracks[0];
    let there = pose_at(&START, &t.steps, false, 1, 1.0);
    assert!(
        sent_pose(&out).is_some_and(|p| same_pose(p, there)),
        "half way along the second step"
    );
    assert_eq!(
        cam.resume.as_ref().map(|r| (r.step, r.elapsed)),
        Some((1, 1.0))
    );
    let out = cam.handle(scrub(-5.0));
    assert!(
        sent_pose(&out).is_some_and(|p| same_pose(p, START)),
        "held to the start"
    );
    cam.handle(scrub(3.0));
    let out = cam.handle(edit("play"));
    assert!(sent_pose(&out).is_none(), "on from the scrubbed place");
    assert_eq!(cam.plays.len(), 1);
}

#[test]
fn the_settings_sliders_move_vrchats_camera_as_they_are_dragged() {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    fn live(n: &Node, id: &str) -> Option<bool> {
        match n {
            Node::Slider { id: at, live, .. } if at == id => Some(*live),
            Node::Column(items) | Node::Row(items) => items.iter().find_map(|i| live(i, id)),
            Node::Split { main, side } => live(main, id).or_else(|| live(side, id)),
            _ => None,
        }
    }
    let editor = cam.shown_editor.as_ref().unwrap();
    assert_eq!(live(editor, "zoom"), Some(true));
    assert_eq!(live(editor, "exposure"), Some(true));
}

#[test]
fn a_slider_dragged_live_is_not_pulled_back_by_vrchats_late_echoes() {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    let echo = |v: f32| {
        Event::Osc(OscMessage {
            addr: "/usercamera/Zoom".into(),
            args: vec![OscArg::Float(v)],
        })
    };
    let zoom = |cam: &Camera| cam.optics_now()[0];
    cam.handle(number_in(EDITOR, "zoom", 50.0));
    cam.handle(number_in(EDITOR, "zoom", 60.0));
    cam.handle(echo(50.0));
    assert_eq!(zoom(&cam), 60.0, "an echo of an older value waits");
    cam.handle(echo(60.0));
    cam.handle(timer(SETTLE_SETTINGS));
    assert_eq!(zoom(&cam), 60.0, "settled, VRChat's last word");
    cam.handle(echo(80.0));
    assert_eq!(zoom(&cam), 80.0, "at rest, VRChat's moves come through");
}

#[test]
fn a_tracks_optics_and_a_written_toggle_are_not_pulled_back_by_late_echoes() {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    cam.handle(write("zoom", 30.0));
    cam.handle(edit("add_key"));
    cam.handle(pose_in([0.0, 0.0, 2.0, 0.0, 0.0, 0.0]));
    cam.handle(write("zoom", 90.0));
    cam.handle(edit("add_key"));
    cam.handle(timer(SETTLE_SETTINGS));
    cam.handle(chosen(EDITOR, "timeline", 0));
    cam.handle(edit("play"));
    for _ in 0..60 {
        cam.handle(timer(PLAY));
    }
    let mid = cam.optics_now()[0];
    cam.handle(Event::Osc(OscMessage {
        addr: "/usercamera/Zoom".into(),
        args: vec![OscArg::Float(31.0)],
    }));
    assert_eq!(
        cam.optics_now()[0],
        mid,
        "a play's zoom holds against an echo"
    );
    cam.handle(Event::Write {
        field: "lock".into(),
        value: Value::Bool(true),
    });
    cam.handle(Event::Osc(OscMessage {
        addr: "/usercamera/Lock".into(),
        args: vec![OscArg::Bool(false)],
    }));
    let lock = TOGGLES.iter().position(|t| t.0 == "lock").unwrap();
    assert!(cam.toggles[lock], "a toggle written holds against an echo");
}

/// What `event` sends, the tick's settings with it.
fn with_send(cam: &mut Camera, event: Event) -> Vec<Command> {
    let mut out = cam.handle(event);
    out.extend(cam.handle(timer(SEND_SETTINGS)));
    out
}

#[test]
fn a_drag_sends_a_slider_once_a_tick_at_its_latest_and_nothing_unchanged() {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    let mut out = Vec::new();
    for v in [50.0, 51.0, 52.0, 52.0] {
        out.extend(cam.handle(number_in(EDITOR, "zoom", v)));
    }
    assert!(
        sent(&out, "/usercamera/Zoom").is_none(),
        "none before the tick"
    );
    let out = cam.handle(timer(SEND_SETTINGS));
    let zooms: Vec<_> = out
        .iter()
        .filter_map(|c| match c {
            Command::Osc(m) if m.addr == "/usercamera/Zoom" => Some(m.args.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(zooms, [vec![OscArg::Float(52.0)]], "the latest, once");
    let out = with_send(&mut cam, number_in(EDITOR, "zoom", 52.0));
    assert!(
        sent(&out, "/usercamera/Zoom").is_none(),
        "an unchanged value sends nothing"
    );
}

#[test]
fn a_play_rebuilds_no_view_and_arms_no_rest_timer_and_ends_on_its_optics() {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    cam.handle(write("zoom", 30.0));
    cam.handle(edit("add_key"));
    cam.handle(pose_in([0.0, 0.0, 2.0, 0.0, 0.0, 0.0]));
    cam.handle(write("zoom", 90.0));
    cam.handle(edit("add_key"));
    rests_end(&mut cam);
    cam.handle(chosen(EDITOR, "timeline", 0));
    cam.handle(edit("play"));
    let (mut sends, mut ticks, mut last, mut rests, mut views) = (0, 0, None, 0, 0);
    while !cam.plays.is_empty() && ticks < 1000 {
        let out = cam.handle(timer(PLAY));
        ticks += 1;
        let playing = !cam.plays.is_empty();
        rests += out
            .iter()
            .filter(|c| playing && matches!(c, Command::Timer { id, .. } if id == REST))
            .count();
        views += out
            .iter()
            .filter(|c| matches!(c, Command::Modal { .. }))
            .count();
        if let Some(z) = sent(&out, "/usercamera/Zoom").and_then(|a| arg_f32(&a[0])) {
            sends += 1;
            last = Some(z);
        }
    }
    assert!(sends <= 1, "a key's optics go once: {sends}");
    assert_eq!(rests, 0, "while it plays, the clock counts the rest down");
    assert!(
        views <= ticks / EDITOR_REFRESH as usize + 2,
        "{views} whole views over {ticks} ticks: its own timers rebuild nothing"
    );
    let out = rests_end(&mut cam);
    let last = sent(&out, "/usercamera/Zoom")
        .and_then(|a| arg_f32(&a[0]))
        .or(last);
    assert_eq!(last, Some(90.0), "the last value still goes out");
}

#[test]
fn while_the_clock_runs_one_message_goes_a_tick_the_settings_in_a_poses_place() {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    cam.handle(write("zoom", 30.0));
    cam.handle(edit("add_key"));
    cam.handle(pose_in([0.0, 0.0, 2.0, 0.0, 0.0, 0.0]));
    cam.handle(write("zoom", 60.0));
    cam.handle(edit("add_key"));
    cam.handle(pose_in([0.0, 0.0, 4.0, 0.0, 0.0, 0.0]));
    cam.handle(write("zoom", 90.0));
    cam.handle(edit("add_key"));
    rests_end(&mut cam);
    cam.handle(chosen(EDITOR, "timeline", 0));
    cam.handle(edit("play"));
    let (mut ticks, mut settings, mut last) = (0, Vec::new(), None);
    while ticks < 1000 {
        let out = cam.handle(timer(PLAY));
        ticks += 1;
        let (pose, zoom) = (sent_pose(&out), sent(&out, "/usercamera/Zoom"));
        if cam.plays.is_empty() {
            last = pose;
            break;
        }
        assert!(
            pose.is_some() != zoom.is_some(),
            "tick {ticks}: the pose or the settings, never both or neither"
        );
        if zoom.is_some() {
            settings.push(ticks);
        }
    }
    assert_eq!(
        settings.len(),
        1,
        "the middle key's zoom, once: {settings:?}"
    );
    assert!(!settings.contains(&1), "the first tick sends the pose");
    assert!(
        settings
            .windows(2)
            .all(|w| w[1] - w[0] == SETTINGS_TICKS as usize),
        "one tick in {SETTINGS_TICKS}: {settings:?}"
    );
    let last = last.expect("the end goes out as the clock stops");
    assert!(close(last, cam.pose), "{last:?}");
}

#[test]
fn a_play_whose_keys_share_their_optics_sends_no_setting_and_only_poses() {
    let mut cam = known([0.0; 6]);
    cam.handle(press("open_editor"));
    cam.handle(write("zoom", 60.0));
    cam.handle(edit("add_key"));
    cam.handle(pose_in([0.0, 0.0, 2.0, 0.0, 0.0, 0.0]));
    cam.handle(edit("add_key"));
    rests_end(&mut cam);
    cam.handle(timer(SETTLE_SETTINGS));
    cam.handle(chosen(EDITOR, "timeline", 0));
    let mut out = cam.handle(edit("play"));
    while !cam.plays.is_empty() {
        out.extend(cam.handle(timer(PLAY)));
    }
    let addrs: std::collections::BTreeSet<&str> = out
        .iter()
        .filter_map(|c| match c {
            Command::Osc(m) => Some(m.addr.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        addrs.into_iter().collect::<Vec<_>>(),
        [POSE],
        "a static zoom is never sent: only poses go out"
    );
}
