//! What it declares and shows: fields, OSC, the page, the pads and the editor.
use crate::*;

use super::support::*;
use crate::motion::{key_times, pose_at, FRAME, MOVE_SPEED, TURN_SPEED};
use crate::widgets::{HINT, STAND_IN};
use vrc_we_sdk::UiEvent;

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
