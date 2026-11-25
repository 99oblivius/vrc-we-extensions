//! How the camera moves: offsets, turns, plays, pacing and smoothing.
use crate::*;

use super::support::*;
use crate::motion::{
    height, key_ramp, key_times, pose_at, ramps, Follow, Leg, Path, FRAME, IN_LEN, IN_RAMP, OPTICS,
    OUT_LEN, OUT_RAMP, SMOOTH, WIRE,
};
use glam::Quat;
use vrc_we_sdk::{Drawn, Piece, UiEvent};

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

/// The legs of a path, measured and paced.
fn legs(steps: &[Step], closed: bool) -> Vec<Leg> {
    let mut path = Path::new(steps, closed);
    (0..steps.len())
        .map(|i| path.leg(i).expect("a leg a step").clone())
        .collect()
}

/// Steps along +z, each (metres, seconds, the ramp leaving, arriving).
fn ramped(steps: &[[f32; 4]]) -> Vec<Step> {
    steps
        .iter()
        .map(|[m, s, out, into]| curved([0.0, 0.0, *m, 0.0, 0.0, 0.0, *s], [0.0, 0.0, *out, *into]))
        .collect()
}

/// A track of `keys` ramps, `seconds` and `metres` apart.
fn track(ramps: &[f32], seconds: f32, metres: f32) -> Vec<Step> {
    let steps: Vec<[f32; 4]> = ramps
        .windows(2)
        .map(|r| [metres, seconds, r[0], r[1]])
        .collect();
    ramped(&steps)
}

/// How fast `leg` goes `t` (0–1) of the way through its time.
fn speed(leg: &Leg, t: f32) -> f32 {
    let d = 1e-3;
    let (a, b) = ((t - d).max(0.0), (t + d).min(1.0));
    leg.at(b).pos.distance(leg.at(a).pos) / ((b - a) * leg.seconds)
}

#[test]
fn whatever_the_ramps_a_leg_covers_its_distance_in_its_time_and_never_goes_back() {
    let all = [-100.0, -60.0, 0.0, 35.0, 100.0];
    for out in all {
        for into in all {
            for (before, after) in [(0.02, 4.0), (1.0, 1.0), (4.0, 0.02)] {
                let steps = ramped(&[
                    [1.0, before, into, out],
                    [3.0, 1.0, out, into],
                    [1.0, after, into, out],
                ]);
                for leg in legs(&steps, false) {
                    let mut last = 0.0;
                    for k in 0..=200 {
                        let f = leg.reached(k as f32 / 200.0);
                        assert!(
                            f.is_finite() && f >= last - 1e-5 && f <= 1.0,
                            "{out} {into} {k}: {f} after {last}"
                        );
                        last = f;
                    }
                    assert!((last - 1.0).abs() < 1e-3, "all of it: {out} {into} {last}");
                }
            }
        }
    }
}

#[test]
fn a_key_is_passed_at_one_speed_and_its_ramp_is_its_own() {
    // Three legs of differing speeds.
    let steps = ramped(&[
        [2.0, 2.0, 0.0, 40.0],
        [6.0, 2.0, 40.0, -30.0],
        [1.0, 1.0, -30.0, 0.0],
    ]);
    let legs = legs(&steps, false);
    for pair in legs.windows(2) {
        let (into, out) = (speed(&pair[0], 1.0), speed(&pair[1], 0.0));
        assert!((into - out).abs() < 0.05 * into.max(1.0), "{into} {out}");
    }
    let ramp = ramps(&steps, false);
    for pair in ramp.windows(2) {
        assert_eq!(pair[0].heights[1], pair[1].heights[0]);
        assert_eq!(
            pair[0].slopes[1], pair[1].slopes[0],
            "it leaves as it arrived"
        );
    }
    let mut moved = steps.clone();
    moved[0][IN_RAMP] = -80.0;
    moved[1][OUT_RAMP] = -80.0;
    moved[0][6] = 0.5;
    let after = ramps(&moved, false);
    assert_eq!(after[0].heights[0], ramp[0].heights[0]);
    assert_eq!(after[1].heights[1], ramp[1].heights[1]);
    assert_eq!(after[2].heights, ramp[2].heights, "no other key moves");
    // Four legs away nothing at all.
    let long = track(&[0.0, 30.0, -20.0, 50.0, 0.0, 10.0], 1.0, 1.0);
    let mut other = long.clone();
    other[0][OUT_RAMP] = 90.0;
    assert_eq!(ramps(&long, false)[3..], ramps(&other, false)[3..]);
}

#[test]
fn a_ramp_stays_in_its_band_and_a_key_at_its_foot_stops() {
    for keys in [
        [0.0, 100.0, -100.0, 100.0, 0.0],
        [-100.0, -100.0, 100.0, 100.0, -100.0],
        [0.0, 80.0, 90.0, -95.0, 100.0],
    ] {
        for seconds in [0.05, 1.0, 9.0] {
            let mut steps = track(&keys, seconds, 1.0);
            steps[1][6] = 0.02;
            for (ramp, step) in ramps(&steps, false).iter().zip(&steps) {
                for k in 0..=100 {
                    let h = ramp.at(k as f32 / 100.0, step[6]);
                    assert!((-1e-4..=1.0 + 1e-4).contains(&h), "{keys:?}: {h}");
                }
            }
        }
    }
    let slowed = legs(&track(&[0.0, -100.0, 0.0], 1.0, 2.0), false);
    assert!(speed(&slowed[0], 1.0) < 0.05, "it stops at the key");
    assert!(speed(&slowed[1], 0.0) < 0.05);
    let stops = legs(&track(&[-100.0, -100.0], 1.0, 2.0), false);
    assert!(
        (stops[0].at(1.0).pos.z - 2.0).abs() < 1e-3,
        "and still arrives"
    );
}

#[test]
fn keys_closer_together_are_flown_between_faster() {
    let peak = |seconds: f32| {
        let mut steps = track(&[0.0, 20.0, -20.0, 0.0], 2.0, 2.0);
        steps[1][6] = seconds;
        let leg = &legs(&steps, false)[1];
        (0..=50)
            .map(|k| speed(leg, k as f32 / 50.0))
            .fold(0.0, f32::max)
    };
    assert!(peak(0.4) > 3.0 * peak(2.0), "{} {}", peak(0.4), peak(2.0));
}

#[test]
fn a_hold_a_jump_and_a_turn_in_place_break_the_speed_from_leg_to_leg() {
    let turn = lin([0.0, 0.0, 0.0, 0.0, 90.0, 0.0, 1.0]);
    let hold = lin([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    let jump = lin([0.0, 0.0, 5.0, 0.0, 0.0, 0.0, 0.0]);
    for beside in [turn, hold, jump] {
        let steps = [lin([0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0]), beside];
        let legs = legs(&steps, false);
        for k in 0..=10 {
            let t = k as f32 / 10.0;
            assert!((legs[0].reached(t) - t).abs() < 1e-5, "even to its end");
        }
    }
    // Two turns in place run on from one into the other.
    let turns = [turn, lin([0.0, 0.0, 0.0, 0.0, 30.0, 0.0, 1.0])];
    let legs = legs(&turns, false);
    assert!(legs[0].reached(0.9) > 0.9, "the faster slows into the key");
}

#[test]
fn a_place_on_a_track_is_where_a_play_of_it_is() {
    let steps = track(&[0.0, 60.0, -40.0, 0.0], 1.0, 2.0);
    for closed in [false, true] {
        let whole = legs(&steps, closed);
        for (i, leg) in whole.iter().enumerate() {
            for t in [0.0, 0.3, 0.7, 1.0] {
                let at = pose_at(&[0.0; 6], &steps, closed, i, t);
                assert!((at[2] - leg.at(t).pos.z).abs() < 1e-4, "{i} {t}");
            }
        }
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
fn every_key_is_reached_at_its_time_whatever_its_ramp() {
    let mut cam = known([0.0; 6]);
    cam.handle(fly(
        1,
        &ramped(&[[1.0, 0.5, 0.0, 90.0], [1.0, 0.5, 90.0, -70.0]]),
    ));
    let (poses, ids) = run_plays(&mut cam, 1000);
    let ticks = (0.5 / FRAME).round() as usize;
    assert!(
        (poses[ticks - 1][2] - 1.0).abs() < 1e-3,
        "key 1 at 0.5 s: {:?}",
        poses[ticks - 1]
    );
    assert!((poses.last().unwrap()[2] - 2.0).abs() < 1e-3);
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
    let sliders = |cam: &Camera| sliders_shown(cam, &["in_len", "out_len"]);
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
fn a_dragged_key_sets_its_time_and_ramp_and_the_track_is_kept_once_let_go() {
    let (mut cam, _) = with_track("Orbit", &track(&[0.0, 0.0, 0.0, 0.0], 2.0, 2.0));
    cam.handle(press("open_editor"));
    let plot = |cam: &Camera| timeline_of(cam.shown_editor.as_ref().unwrap()).cloned();
    let at = |cam: &Camera| -> Vec<[f32; 2]> {
        plot(cam)
            .expect("a timeline")
            .points()
            .map(|p| p.at)
            .collect()
    };
    assert_eq!(at(&cam), [[0.0, 0.5], [2.0, 0.5], [4.0, 0.5], [6.0, 0.5]]);
    let out = cam.handle(drag_key(1, 2.5, 0.8, false));
    assert_eq!(at(&cam), [[0.0, 0.5], [2.5, 0.8], [4.0, 0.5], [6.0, 0.5]]);
    assert!(stored(&out).is_empty(), "nothing kept while it moves");
    let out = cam.handle(drag_key(1, 2.5, 0.8, true));
    assert!(
        stored(&out).iter().any(|(k, _)| k == "track_0"),
        "kept once let go"
    );
    let steps = cam.tracks[0].steps.clone();
    assert_eq!(steps[0][IN_RAMP], steps[1][OUT_RAMP], "one ramp a key");
    cam.handle(drag_key(0, 9.0, 0.0, true));
    assert_eq!(at(&cam)[0], [0.0, 0.0], "the start in height alone");
    // Its curve is one Bezier a leg, through every key.
    let curve = plot(&cam).unwrap().items.iter().find_map(|i| match i {
        Drawn::Curve(c) => Some(c.clone()),
        _ => None,
    });
    let ends: Vec<[f32; 2]> = curve
        .expect("a curve")
        .pieces
        .iter()
        .map(|p| match p {
            Piece::Cubic(_, _, to) => *to,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(ends, at(&cam)[1..]);
    let (mut long, _) = with_track("Long", &vec![FORWARD; 257]);
    long.handle(press("open_editor"));
    let long_plot = plot(&long).unwrap();
    assert!(
        long_plot
            .points()
            .all(|p| p.drag.is_some_and(|d| d.y.is_none() && !d.live)),
        "too long a track has no ramp, and a key moves once let go"
    );
    let before = long.tracks[0].steps[0];
    long.handle(drag_key(1, 0.1, 0.0, true));
    assert_eq!(long.tracks[0].steps[0][IN_RAMP], before[IN_RAMP]);
    assert!((key_times(&long.tracks[0].steps)[1] - 0.1).abs() < 1e-6);
    long.handle(chosen(EDITOR, "timeline", 1));
    assert!(shown(&long, true)
        .iter()
        .any(|t| t.starts_with("Ramps are edited")));
}

#[test]
fn a_keys_ramp_stays_its_own_through_every_edit() {
    let of = |cam: &Camera| -> Vec<f32> {
        let steps = &cam.tracks[0].steps;
        (0..=steps.len()).map(|k| key_ramp(steps, k)).collect()
    };
    let joined = |cam: &Camera| {
        let steps = &cam.tracks[0].steps;
        steps.windows(2).all(|w| w[0][IN_RAMP] == w[1][OUT_RAMP])
    };
    let ramps = [-100.0, 10.0, 20.0, 30.0];
    let make = || {
        let (mut cam, _) = with_track("Orbit", &track(&ramps, 1.0, 1.0));
        cam.handle(press("open_editor"));
        cam
    };
    let mut cam = make();
    cam.handle(chosen(EDITOR, "timeline", 1));
    cam.handle(edit("key_del"));
    assert_eq!(of(&cam), [-100.0, 20.0, 30.0], "the start keeps its own");
    assert!(joined(&cam));
    let mut cam = make();
    cam.handle(chosen(EDITOR, "timeline", 2));
    cam.handle(edit("key_earlier"));
    assert_eq!(of(&cam), [-100.0, 20.0, 10.0, 30.0]);
    cam.handle(edit("key_dup"));
    assert_eq!(of(&cam), [-100.0, 20.0, 20.0, 10.0, 30.0]);
    assert!(joined(&cam));
    let mut cam = make();
    cam.handle(chosen(EDITOR, "timeline", 0));
    cam.handle(edit("key_insert"));
    assert_eq!(
        of(&cam),
        [-100.0, 0.0, 10.0, 20.0, 30.0],
        "a new key's is even"
    );
    assert!(joined(&cam));
    let mut cam = make();
    cam.handle(chosen(EDITOR, "loop", 1));
    assert_eq!(of(&cam), [-100.0, 10.0, 20.0, 30.0, -100.0]);
    cam.handle(drag_key(4, 4.0, 0.6, true));
    assert_eq!(
        of(&cam)[0],
        of(&cam)[4],
        "a closed track's end is its start"
    );
    assert!((of(&cam)[0] - 20.0).abs() < 1e-3);
    cam.handle(chosen(EDITOR, "loop", 0));
    assert_eq!(of(&cam), [20.0, 10.0, 20.0, 30.0]);
}

#[test]
fn a_track_kept_with_pace_dots_loads_with_the_ramps_closest_to_them_once() {
    let (mut cam, _) = with_track("Orbit", &[FORWARD]);
    let step = |at: f32, pace: f32| format!("[0,0,1,0,0,0,1,1,1,{at},{pace},0,0,0,0]");
    let value = format!(
        "[{},{},{},{}]",
        step(0.0, 0.05),
        step(1.0, 0.05),
        step(0.0, 0.05),
        step(0.5, 1.0)
    );
    let load = |cam: &mut Camera, value: String| {
        cam.handle(Event::Loaded {
            key: "track_0".into(),
            value: Some(value),
        })
    };
    let out = load(&mut cam, value);
    let steps = cam.tracks[0].steps.clone();
    let heights: Vec<f32> = (0..=4).map(|k| height(key_ramp(&steps, k))).collect();
    for slow in [0, 2] {
        assert!(heights[slow] < 0.15, "a slow key: {heights:?}");
    }
    for even in [1, 3, 4] {
        assert!(
            (heights[even] - 0.5).abs() < 1e-4,
            "an even one: {heights:?}"
        );
    }
    let kept = stored(&out);
    let (_, value) = kept.iter().find(|(k, _)| k == "track_0").expect("kept");
    let out = load(&mut cam, value.clone());
    assert_eq!(cam.tracks[0].steps, steps, "kept as it is now");
    assert!(stored(&out).is_empty(), "converted once");
}

#[test]
fn an_old_track_of_7_numbers_loads_straight_on_an_even_ramp_and_steps_round_trip() {
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
        [0.5, 0.75, -25.0, 75.0],
    )];
    assert_eq!(steps_of(&steps_value(&steps)), steps);
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
    let sliders = |cam: &Camera| sliders_shown(cam, &["in_len", "out_len"]);
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
    assert!(cam.note.as_deref().is_some_and(|t| t.contains("play once")));
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
    cam.handle(chosen(EDITOR, "tab", 2));
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
