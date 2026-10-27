//! What it keeps: tracks, positions, names and the store.
use super::support::*;
use crate::library::clean_name;
use crate::widgets::UNREADABLE;
use crate::*;
use vrc_we_sdk::Item;

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
