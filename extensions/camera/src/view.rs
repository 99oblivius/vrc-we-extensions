//! The camera's views: its page, the editor modal, and what a press on
//! either does.

use crate::widgets::*;

use vrc_we_sdk::{significant, Command, Node, UiEvent, Value, SLIDER_FIGURES};

use crate::library::{clean_name, free_name, Position, Track};
use crate::motion::{
    angles, clamp_step, facing, insert_key, key_pose, key_times, place_at, pose_at, position,
    restart, rotation, same_place, set_key, shapes, step_to, with_curve, with_optics, Curve, Fly,
    Loop, Optics, Play, Resume, Step, DOT_AT, DOT_PACE, MOVE_SPEED, PATH_MAX, SPEED_PER_LINE,
    STEP_SECONDS, TURN_SPEED,
};
use crate::{
    wex, wex_number, Camera, CHOOSE_KEY, MODES, MOVE_FIRST, NAME_TAKEN, OPEN_FIRST, SAVE, SAVE_MS,
    SLIDERS, SMOOTHING_STRENGTH, SMOOTH_MOVEMENT, TOGGLES,
};

/// The editor modal's id.
pub(crate) const EDITOR: &str = "editor";

impl Camera {
    /// What has to happen before VRChat reports its camera: open it, or,
    /// open but not moved since this started, move it once.
    pub(crate) fn without_pose(&self) -> &'static str {
        match self.mode {
            Some(mode) if mode > 0 => MOVE_FIRST,
            _ => OPEN_FIRST,
        }
    }

    /// The tracks whose names hold the search, by index.
    pub(crate) fn found(&self) -> Vec<usize> {
        let needle = self.search.to_lowercase();
        (0..self.tracks.len())
            .filter(|i| self.tracks[*i].name.to_lowercase().contains(&needle))
            .collect()
    }

    /// Whether track `key` is playing.
    fn playing(&self, key: &str) -> bool {
        self.plays.iter().any(|p| p.track.as_deref() == Some(key))
    }

    /// The pads, hold-to-fly: moves on the camera's axes and turns, their
    /// speeds and how they move; first a line while they move a stand-in.
    fn pads(&self) -> Vec<Node> {
        let hold = |fly: Fly| {
            let (_, id, icon, tip, key) = Fly::PADS
                .iter()
                .find(|p| p.0 == fly)
                .copied()
                .expect("every fly has a pad");
            let disabled = self.leveled(fly);
            let tip = match fly
                .leveled_by()
                .and_then(|f| TOGGLES.iter().find(|t| t.0 == f))
            {
                Some(t) if disabled => format!("{tip} (off while {} is on)", t.2),
                _ => tip.to_owned(),
            };
            Node::HoldButton {
                id: format!("fly:{id}"),
                icon: icon.to_owned(),
                tip,
                key: Some(key),
                disabled,
            }
        };
        let pad = |label: &str, rows: [[Fly; 3]; 2]| {
            let mut col = vec![text(label)];
            col.extend(rows.map(|row| Node::Row(row.map(hold).to_vec())));
            Node::Column(col)
        };
        let mut rows = Vec::new();
        if !self.linked {
            rows.push(text(format!("{} {STAND_IN}", self.without_pose())));
        }
        rows.extend([
            Node::Row(vec![
                pad(
                    "Move",
                    [
                        [Fly::Rise, Fly::Forward, Fly::Sink],
                        [Fly::Left, Fly::Back, Fly::Right],
                    ],
                ),
                pad(
                    "Turn",
                    [
                        [Fly::RollLeft, Fly::PitchUp, Fly::RollRight],
                        [Fly::YawLeft, Fly::PitchDown, Fly::YawRight],
                    ],
                ),
            ]),
            Node::Row(vec![
                speed_slider("pad_move", "Move (m/s)", MOVE_SPEED, self.move_speed),
                speed_slider("pad_turn", "Turn (°/s)", TURN_SPEED, self.turn_speed),
            ]),
            Node::Row(vec![
                Node::Toggle {
                    id: "local_nav".to_owned(),
                    label: "Local navigation".to_owned(),
                    value: Some(self.local_nav),
                },
                button("level", "Level the horizon", Some("level"), false),
            ]),
        ]);
        rows
    }

    /// VRChat's camera settings: its sliders, then its toggles, two a row.
    /// Smoothing reads 0 while Smooth movement is off.
    fn settings_rows(&self) -> Vec<Node> {
        let sliders: Vec<Node> = SLIDERS
            .iter()
            .zip(self.sliders)
            .map(|((field, _, label, min, max, _), v)| {
                let (min, v) = match *field == SMOOTHING_STRENGTH {
                    true if !self.smoothing() => (0.0, 0.0),
                    true => (0.0, f64::from(v)),
                    false => (*min, f64::from(v)),
                };
                Node::Slider {
                    id: (*field).to_owned(),
                    label: (*label).to_owned(),
                    min,
                    max: *max,
                    value: Some(v),
                    log: false,
                    live: true,
                }
            })
            .collect();
        let toggles: Vec<Node> = TOGGLES
            .iter()
            .zip(self.toggles)
            .map(|((field, _, label, _), v)| Node::Toggle {
                id: (*field).to_owned(),
                label: (*label).to_owned(),
                value: Some(v),
            })
            .collect();
        let mut rows = vec![
            text(
                "VRChat's settings, read from VRChat when this starts; VRChat doesn't report a slider moved in-game.",
            ),
        ];
        rows.extend(sliders.chunks(2).map(|pair| Node::Row(pair.to_vec())));
        rows.extend(toggles.chunks(2).map(|pair| Node::Row(pair.to_vec())));
        rows
    }

    /// Positions to fly to, and saving the camera's.
    fn positions_rows(&self, id: &str) -> Vec<Node> {
        let mut rows = vec![
            heading("Positions"),
            text("Positions belong to the world they were saved in."),
        ];
        if !self.positions.is_empty() {
            rows.push(Node::Buttons {
                id: id.to_owned(),
                options: self.positions.iter().map(|p| p.name.clone()).collect(),
                selected: (id == "pick_position").then_some(self.spot).flatten(),
                columns: 3,
            });
        }
        rows.push(button(
            "save_position",
            "Save the camera's position",
            None,
            false,
        ));
        rows
    }

    /// The track playing's (or a pause's) place in seconds, on the edited
    /// track.
    pub(crate) fn playhead(&self) -> Option<f32> {
        let t = self.track.and_then(|i| self.tracks.get(i))?;
        self.plays
            .iter()
            .find(|p| p.track.as_deref() == Some(t.key.as_str()))
            .map(Play::at)
            .or(self
                .resume
                .as_ref()
                .filter(|r| r.track == t.key)
                .map(|r| r.at(&t.steps)))
    }

    /// The 3D view's camera: where it is shown, the way it faces, its top.
    pub(crate) fn scene_camera(&self) -> [[f32; 3]; 3] {
        let p = self.shown_pose();
        let rot = rotation(&p);
        [
            position(&p).to_array(),
            (rot * glam::Vec3::Z).to_array(),
            (rot * glam::Vec3::Y).to_array(),
        ]
    }

    /// What the page and the editor show while a stored value couldn't be
    /// read: the reason, and the one press that replaces it (a fresh, empty
    /// index written over it).
    fn unreadable_view(&self) -> Node {
        Node::Column(vec![
            heading("Camera"),
            text(UNREADABLE),
            button("library_reset", NEW_LIBRARY, None, false),
        ])
    }

    /// The page: the camera's mode and actions, the pads, the tracks to
    /// play and the positions to fly to.
    pub(crate) fn page(&self) -> Node {
        if self.unreadable {
            return self.unreadable_view();
        }
        let mut rows = vec![
            heading("Camera"),
            Node::Buttons {
                id: "mode".to_owned(),
                options: MODES.iter().map(|m| (*m).to_owned()).collect(),
                selected: self.mode.and_then(|m| usize::try_from(m).ok()),
                columns: 4,
            },
            Node::Row(vec![
                icon_button("capture", "capture", "Capture"),
                icon_button("capture_delayed", "timer", "Capture after a delay"),
                icon_button("close", "close", "Close the camera"),
            ]),
        ];
        rows.extend(self.pads());
        rows.push(Node::Divider);
        rows.push(heading("Tracks"));
        rows.push(name_field("search", "Search", &self.search, true));
        for i in self.found() {
            let t = &self.tracks[i];
            let (icon, tip) = match self.playing(&t.key) {
                true => ("pause", "Pause"),
                false => ("play", "Play"),
            };
            rows.push(Node::Row(vec![
                icon_button(&format!("play:{}", t.key), icon, tip),
                text(format!(
                    "{}   {} keys   {} s",
                    t.name,
                    t.keys(),
                    wex_number(t.seconds())
                )),
            ]));
        }
        rows.push(button("open_editor", "Track Editor", Some("edit"), true));
        rows.push(Node::Divider);
        rows.extend(self.positions_rows("go_position"));
        if let Some(note) = &self.note {
            rows.push(text(note.clone()));
        }
        Node::Column(rows)
    }

    /// The editor: the edited track in 3D and on its timeline, beside a
    /// panel of every tool.
    pub(crate) fn editor(&self) -> Node {
        if self.unreadable {
            return self.unreadable_view();
        }
        let edited = self.track.and_then(|i| self.tracks.get(i));
        let keyed = edited.filter(|t| t.keys() > 0);
        let start = keyed.and_then(|t| t.start).unwrap_or(self.pose);
        let (origin, frame) = (position(&start), facing(&start));
        let (mut path, mut key_points) = (Vec::new(), Vec::new());
        if let Some(t) = keyed {
            let legs = shapes(&t.steps, t.looping.closed());
            let per = (PATH_POINTS / legs.len().max(1)).clamp(1, LEG_POINTS);
            for leg in &legs {
                key_points.push(path.len());
                path.extend((0..per).map(|j| leg.point(j as f32 / per as f32)));
            }
            key_points.push(path.len());
            path.push(legs.last().map_or(glam::Vec3::ZERO, |l| l.to.pos));
        }
        let path: Vec<[f32; 3]> = path
            .into_iter()
            .map(|p| (origin + frame * p).to_array())
            .collect();
        let times = keyed.map(|t| key_times(&t.steps)).unwrap_or_default();
        let steps: &[Step] = keyed.map_or(&[], |t| &t.steps);
        let closed = keyed.is_some_and(|t| t.looping.closed());
        let main = Node::Column(vec![
            Node::Scene3d {
                id: "scene".to_owned(),
                keys: key_points,
                path,
                selected: self.key,
                camera: Some(self.scene_camera()),
                overlay: key_panel(steps, self.key, closed).map(Box::new),
                closed,
                hint: Some(HINT.to_owned()),
            },
            Node::Timeline {
                id: "timeline".to_owned(),
                length: times.last().copied().unwrap_or(0.0),
                keys: times.clone(),
                selected: self.key,
                playhead: self.playhead(),
                lane: pace_lane(steps),
            },
        ]);
        let found = self.found();
        let mut side = vec![
            heading("Track"),
            name_field("search", "Search", &self.search, true),
            Node::List {
                id: "tracks".to_owned(),
                items: found.iter().map(|i| self.tracks[*i].name.clone()).collect(),
                selected: self.track.and_then(|t| found.iter().position(|i| *i == t)),
            },
            Node::Row(vec![
                icon_button("new", "add", "New track"),
                icon_button("duplicate", "duplicate", "Duplicate the track"),
                icon_button("delete", "delete", "Delete the track"),
            ]),
        ];
        if let Some(t) = edited {
            let (icon, tip) = match self.playing(&t.key) {
                true => ("pause", "Pause"),
                false => ("play", "Play from the chosen key"),
            };
            side.extend([
                Node::Divider,
                name_field("rename", "Name (Enter renames)", &t.name, false),
                Node::Buttons {
                    id: "loop".to_owned(),
                    options: ["Open", "Closed", "Looping"].map(str::to_owned).to_vec(),
                    selected: Some(t.looping as usize),
                    columns: 3,
                },
                Node::Row(vec![
                    icon_button("play", icon, tip),
                    icon_button("stop", "stop", "Stop, back to the start"),
                    Node::Copy {
                        label: "Copy as Wex".to_owned(),
                        text: wex(&t.steps),
                    },
                ]),
                Node::Divider,
            ]);
            let keys = t.keys();
            side.push(heading(&match self.key {
                Some(0) => format!("Start of {keys} keys"),
                Some(k) => format!("Key {} of {keys}", k + 1),
                None => format!("{keys} keys"),
            }));
            if let Some(s) = self
                .key
                .and_then(|k| k.checked_sub(1))
                .and_then(|i| t.steps.get(i).map(|s| (i, s)))
            {
                let (i, step) = s;
                let field =
                    |k: usize| number_node(&format!("s:{i}:{k}"), STEP_LABELS[k], Some(step[k]));
                side.push(Node::Row((0..3).map(field).collect()));
                side.push(Node::Row((3..6).map(field).collect()));
                side.push(number_node(
                    "key_time",
                    "Time (s)",
                    times.get(i + 1).copied(),
                ));
            }
            side.push(Node::Row(vec![
                icon_button("key_set", "target", "Set this key to the camera"),
                icon_button(
                    "key_insert",
                    "keyframe",
                    "Insert a key at the camera after this one",
                ),
                icon_button("key_go", "go", "Fly the camera to this key"),
            ]));
            side.push(Node::Row(vec![
                icon_button("key_dup", "duplicate", "Duplicate this key's step"),
                icon_button("key_earlier", "earlier", "Move this key earlier"),
                icon_button("key_later", "later", "Move this key later"),
                icon_button("key_del", "delete", "Remove this key"),
            ]));
        }
        side.push(Node::Row(vec![
            button("add_key", "Add keyframe", Some("keyframe"), false),
            icon_button("start_here", "pin", "Start the track at the camera"),
        ]));
        side.push(text(self.offset_line()));
        if steps.len() > LANE_STEPS_MAX {
            side.push(text(LANE_TOO_LONG));
        }
        side.push(Node::Divider);
        side.push(heading("Camera"));
        side.extend(self.pads());
        side.push(Node::Divider);
        side.extend(self.settings_rows());
        side.push(Node::Divider);
        side.extend(self.positions_rows("pick_position"));
        if let Some(p) = self.spot.and_then(|i| self.positions.get(i)) {
            side.push(name_field(
                "rename_position",
                "Name (Enter renames)",
                &p.name,
                false,
            ));
            side.push(Node::Row(vec![
                icon_button("go", "go", "Fly there"),
                icon_button("delete_position", "delete", "Delete the position"),
            ]));
        }
        if let Some(note) = &self.note {
            side.push(text(note.clone()));
        }
        Node::Split {
            main: Box::new(main),
            side: Box::new(Node::Column(side)),
        }
    }

    /// Edits the edited track at its chosen key with the camera's pose:
    /// `edit` gets the steps, the track's start (changed in place), the key
    /// and the pose. Without a camera, a chosen key or the track's start,
    /// it notes what is missing and edits nothing; true when it edited.
    fn edit_at_camera(
        &mut self,
        edit: impl FnOnce(&mut Vec<Step>, &mut [f32; 6], &mut Option<Optics>, usize, [f32; 6]),
        out: &mut Vec<Command>,
    ) -> bool {
        let (pose, now) = (self.pose, self.optics_now());
        let i = self.track.filter(|i| *i < self.tracks.len());
        let Some((i, key)) = i.zip(self.key) else {
            self.note = Some(CHOOSE_KEY.to_owned());
            return false;
        };
        let t = &mut self.tracks[i];
        if t.start.is_none() {
            self.note = Some(CHOOSE_KEY.to_owned());
            return false;
        }
        t.keep_optics(now);
        let Track {
            steps,
            start: Some(start),
            optics,
            ..
        } = t
        else {
            return false;
        };
        edit(steps, start, optics, key, pose);
        t.reclose();
        self.save_track(i, out);
        self.save_index(out);
        true
    }

    /// The edited track's last key a person moves: the end of its last
    /// step, or of the one before a closed track's way back.
    fn last_key(&self) -> usize {
        self.track
            .and_then(|i| self.tracks.get(i))
            .map_or(0, |t| t.steps.len() - usize::from(t.looping.closed()))
    }

    /// Sets the edited track's loop: closing it adds the way back to its
    /// start, a step like any other; opening it takes that step away.
    fn set_loop(&mut self, looping: Loop, out: &mut Vec<Command>) {
        let curve = self.new_curve();
        let Some(i) = self.track.filter(|i| *i < self.tracks.len()) else {
            return;
        };
        let t = &mut self.tracks[i];
        match (t.looping.closed(), looping.closed()) {
            (false, true) if t.steps.is_empty() => {
                self.note = Some("Add a key before closing the track.".to_owned());
                return;
            }
            (false, true) if t.steps.len() >= PATH_MAX => {
                self.note = Some(track_full());
                return;
            }
            (false, true) => {
                let mut back = with_curve([0.0; 15], curve);
                back[6] = KEY_SECONDS;
                t.steps.push(back);
                t.looping = looping;
                t.reclose();
            }
            (true, false) => {
                t.steps.pop();
                let last = t.steps.len();
                self.key = self.key.map(|k| k.min(last));
            }
            _ => {}
        }
        t.looping = looping;
        self.save_track(i, out);
        self.save_index(out);
    }

    /// Chooses track `i` to edit, from its start.
    fn edit(&mut self, i: Option<usize>) {
        self.track = i.filter(|i| *i < self.tracks.len());
        self.key = None;
    }

    /// Plays track `i`, unless it plays: on from where the person paused
    /// it or a key chosen since, else from the edited track's chosen key
    /// (its start at the last key). It goes on from the camera when it
    /// hasn't moved since the pause, else snaps to that place's key.
    fn play_track(&mut self, i: usize, out: &mut Vec<Command>) {
        let Some(t) = self.tracks.get(i).filter(|t| !self.playing(&t.key)) else {
            return;
        };
        let (steps, key, start, looping) = (t.steps.clone(), t.key.clone(), t.start, t.looping);
        let optics = t.key_optics();
        let chosen = self
            .key
            .filter(|k| self.track == Some(i) && *k < steps.len())
            .map(|step| Resume {
                track: key.clone(),
                step,
                elapsed: 0.0,
                pose: None,
            });
        let resume = self
            .resume
            .take()
            .filter(|r| r.track == key && r.step < steps.len())
            .or(chosen);
        let (step, elapsed) = resume.as_ref().map_or((0, 0.0), |r| (r.step, r.elapsed));
        let still = resume
            .and_then(|r| r.pose)
            .is_some_and(|then| same_place(&then, &self.pose));
        let elapsed = match (still, start) {
            (false, Some(start)) => {
                self.jump_pose(key_pose(&start, &steps, step), out);
                if let Some(o) = optics.as_ref().and_then(|o| o.get(step)) {
                    self.set_optics(*o, out);
                    self.send_settings_now(out);
                }
                0.0
            }
            _ => elapsed,
        };
        self.start(
            None,
            |from| {
                let play = Play::path_at(None, &steps, looping.closed(), step, elapsed, from)
                    .with_optics(optics.unwrap_or_default());
                match looping {
                    Loop::Looping => play.repeating().of_track(key),
                    _ => play.of_track(key),
                }
            },
            out,
        );
    }

    /// Flies to position `i` in `GO_SECONDS`.
    fn go(&mut self, i: usize, out: &mut Vec<Command>) {
        if let Some(to) = self.positions.get(i).map(|p| p.pose) {
            self.start(None, |start| Play::to(None, &to, GO_SECONDS, start), out);
        }
    }

    /// The Smoothing slider at `v`: below VRChat's least strength it turns
    /// Smooth movement off, else sets the strength and turns it on.
    fn slide_smoothing(&mut self, v: f32, out: &mut Vec<Command>) {
        let least = SLIDERS
            .iter()
            .find(|s| s.0 == SMOOTHING_STRENGTH)
            .map_or(0.0, |s| s.3 as f32);
        if v < least / 2.0 {
            return self.on_write(SMOOTH_MOVEMENT, &Value::Bool(false), out);
        }
        self.on_write(SMOOTHING_STRENGTH, &Value::Number(f64::from(v)), out);
        if !self.smoothing() {
            self.on_write(SMOOTH_MOVEMENT, &Value::Bool(true), out);
        }
    }

    /// Pauses the person's play of track `key`: Play goes on from there.
    fn pause(&mut self, key: &str, out: &mut Vec<Command>) {
        self.end_plays_where(|p| p.call.is_none() && p.track.as_deref() == Some(key), out);
    }

    /// Ends every play and puts the edited track back at its start: the
    /// camera there and the playhead at 0.
    fn stop(&mut self, out: &mut Vec<Command>) {
        self.end_plays(out);
        self.go_to_key(0, out);
    }

    /// Chooses the edited track's key `key` and puts the camera there at
    /// once, its pose and its optics, pausing the track's play; Play goes
    /// on from there. False without a track with a start.
    fn go_to_key(&mut self, key: usize, out: &mut Vec<Command>) -> bool {
        let t = self.track.and_then(|i| self.tracks.get(i));
        let optics = t.and_then(|t| t.key_optics());
        let Some((track, start, steps)) =
            t.and_then(|t| Some((t.key.clone(), t.start?, t.steps.clone())))
        else {
            return false;
        };
        let key = key.min(steps.len());
        let to = key_pose(&start, &steps, key);
        self.end_plays_where(|p| p.track.as_ref() == Some(&track), out);
        self.key = Some(key);
        self.jump_pose(to, out);
        if let Some(o) = optics.as_ref().and_then(|o| o.get(key)) {
            self.set_optics(*o, out);
            self.send_settings_now(out);
        }
        self.resume = Some(Resume {
            track,
            step: key,
            elapsed: 0.0,
            pose: Some(self.pose),
        });
        true
    }

    /// The edited track scrubbed to `at` seconds: its play paused, the
    /// camera put there at once with its optics, and Play going on from
    /// there.
    fn scrub(&mut self, at: f32, out: &mut Vec<Command>) {
        let Some(t) = self.track.and_then(|i| self.tracks.get(i)) else {
            return;
        };
        let (Some(start), false) = (t.start, t.steps.is_empty()) else {
            return;
        };
        let (track, steps, closed) = (t.key.clone(), t.steps.clone(), t.looping.closed());
        let optics = t.key_optics();
        self.pause(&track, out);
        let (step, into) = place_at(&steps, at);
        let pose = pose_at(&start, &steps, closed, step, into);
        self.jump_pose(pose, out);
        let there = optics.and_then(|o| {
            Play::path_at(None, &steps, closed, step, into, &pose)
                .with_optics(o)
                .optics()
        });
        if let Some(o) = there {
            self.set_optics(o, out);
            self.send_settings_now(out);
        }
        self.resume = Some(Resume {
            track,
            step,
            elapsed: into,
            pose: Some(self.pose),
        });
    }

    /// Moves key `index` (1 or more) of the edited track to `at` seconds
    /// from its start, held between its neighbours, the others staying
    /// put: the step into it and the step out of it share the time.
    fn retime(&mut self, index: usize, at: f32, out: &mut Vec<Command>) {
        let Some(times) = self
            .track
            .and_then(|i| self.tracks.get(i))
            .map(|t| key_times(&t.steps))
        else {
            return;
        };
        let Some(before) = index.checked_sub(1).and_then(|i| times.get(i)).copied() else {
            return;
        };
        let after = times.get(index + 1).copied();
        let at = at.max(before).min(after.unwrap_or(f32::INFINITY));
        let cap = STEP_SECONDS as f32;
        let i = index - 1;
        self.edit_track(
            |steps| {
                if let Some(s) = steps.get_mut(i) {
                    s[6] = (at - before).min(cap);
                }
                if let (Some(after), Some(next)) = (after, steps.get_mut(i + 1)) {
                    next[6] = (after - at).min(cap);
                }
            },
            out,
        );
    }

    /// Edits the edited track's key `key` (its step, `key - 1`): `edit`
    /// gets the steps and that step's index.
    fn edit_key(&mut self, edit: impl FnOnce(&mut Vec<Step>, usize), out: &mut Vec<Command>) {
        let Some(at) = self.key.and_then(|k| k.checked_sub(1)) else {
            return;
        };
        self.edit_track(
            |steps| {
                if at < steps.len() {
                    edit(steps, at);
                }
            },
            out,
        );
    }

    /// The camera's pose as a key at the end of the chosen track (a new
    /// track when there is none): its start when it has none, else a step
    /// from its last key.
    pub(crate) fn add_key(&mut self, out: &mut Vec<Command>) {
        if self.unreadable {
            self.note = Some(UNREADABLE.to_owned());
            return;
        }
        let (pose, now) = (self.pose, self.optics_now());
        if self.track.is_none() {
            let name = self.copy_name("Track");
            self.insert_track(name, Vec::new(), None, out);
        }
        let Some(i) = self.track.filter(|i| *i < self.tracks.len()) else {
            return;
        };
        let curve = self.new_curve();
        let t = &mut self.tracks[i];
        let closed = t.looping.closed();
        match t.start {
            None => {
                t.start = Some(pose);
                t.optics = Some(now);
                self.key = Some(0);
                self.note = Some(
                    "The track starts at the camera: move it and add the next keyframe.".to_owned(),
                );
                self.save_index(out);
            }
            Some(_) if t.steps.len() >= PATH_MAX => {
                self.note = Some(track_full());
            }
            Some(start) => {
                // A closed track's new key goes before its way back.
                let at = t.steps.len() - usize::from(closed);
                let step = step_to(&start, &t.steps[..at], &pose, KEY_SECONDS);
                t.keep_optics(now);
                t.steps
                    .insert(at, clamp_step(with_optics(with_curve(step, curve), now)));
                t.reclose();
                self.key = Some(at + 1);
                self.save_track(i, out);
                self.save_index(out);
            }
        }
    }

    /// Takes the chosen key's handle lengths as a new key's, the sides it
    /// lacks kept as they were; true when they changed.
    fn remember_handles(&mut self) -> bool {
        let t = self.track.and_then(|i| self.tracks.get(i));
        let sides = t
            .zip(self.key)
            .map(|(t, k)| key_sides(&t.steps, k, t.looping.closed()));
        let Some((into, onward)) = sides else {
            return false;
        };
        let handles = [
            onward.unwrap_or(self.key_handles[0]),
            into.unwrap_or(self.key_handles[1]),
        ];
        std::mem::replace(&mut self.key_handles, handles) != handles
    }

    /// Sets the chosen key's handles (the sides given) and keeps them as a
    /// new key's. A `live` edit (a slider moving) is kept once the edits
    /// settle ([`SAVE`]), not each time.
    fn shape_key(
        &mut self,
        into: Option<f32>,
        onward: Option<f32>,
        live: bool,
        out: &mut Vec<Command>,
    ) {
        let Some(key) = self.key else { return };
        let Some(i) = self.track.filter(|i| *i < self.tracks.len()) else {
            return;
        };
        let t = &mut self.tracks[i];
        set_key_sides(&mut t.steps, key, t.looping.closed(), into, onward);
        let changed = self.remember_handles();
        match live {
            true => {
                self.unsaved = Some(self.tracks[i].key.clone());
                out.push(Command::Timer {
                    id: SAVE.to_owned(),
                    after_ms: SAVE_MS,
                });
            }
            false => {
                self.save_track(i, out);
                if changed {
                    self.save_index(out);
                }
            }
        }
    }

    /// Keeps the track whose live edits have settled, and the handles a
    /// new key takes.
    pub(crate) fn save_settled(&mut self, out: &mut Vec<Command>) {
        let Some(key) = self.unsaved.take() else {
            return;
        };
        if let Some(i) = self.tracks.iter().position(|t| t.key == key) {
            self.save_track(i, out);
            self.save_index(out);
        }
    }

    /// The pace lane's dot `index` dragged to `at` seconds and `value`
    /// times its step's average: where along its step and how fast there;
    /// the track is kept once it is let go (`done`).
    fn drag_dot(&mut self, index: usize, at: f32, value: f32, done: bool, out: &mut Vec<Command>) {
        let Some(i) = self.track.filter(|i| *i < self.tracks.len()) else {
            return;
        };
        let steps = &mut self.tracks[i].steps;
        let Some(s) = timed(steps).get(index).copied() else {
            return;
        };
        let from: f32 = steps[..s].iter().map(|s| s[6]).sum();
        steps[s][DOT_AT] = (at - from) / steps[s][6];
        steps[s][DOT_PACE] = value;
        steps[s] = clamp_step(steps[s]);
        if done {
            self.save_track(i, out);
        }
    }

    /// A new key's curve: the last chosen key's handle lengths, at an even
    /// pace.
    fn new_curve(&self) -> Curve {
        let [out, into] = self.key_handles;
        [out, into, 0.5, 1.0]
    }

    /// Where the camera is from the chosen track's last key, as a step:
    /// what Add keyframe adds.
    fn offset_line(&self) -> String {
        let pose = self.pose;
        let Some(t) = self.track.and_then(|i| self.tracks.get(i)) else {
            return "Add keyframe starts a track at the camera.".to_owned();
        };
        let Some(start) = t.start else {
            return "Add keyframe starts the track at the camera.".to_owned();
        };
        let last = t.steps.len() - usize::from(t.looping.closed());
        let s = step_to(&start, &t.steps[..last], &pose, 0.0);
        let n = |v: f32| wex_number((v * 100.0).round() / 100.0);
        format!(
            "From key {}: right {}, up {}, forward {} m; pitch {}, yaw {}, roll {}°",
            last + 1,
            n(s[0]),
            n(s[1]),
            n(s[2]),
            n(s[3]),
            n(s[4]),
            n(s[5]),
        )
    }

    /// Keeps a track chosen while any exist, and the stand-in at a newly
    /// chosen track's start unless something moves it; while the editor is
    /// open, a chosen track with steps but no start starts at the camera.
    pub(crate) fn keep_chosen(&mut self, out: &mut Vec<Command>) {
        match self.tracks.len().checked_sub(1) {
            None => {
                self.track = None;
                self.key = None;
            }
            Some(last) if self.track.is_none_or(|i| i > last) => {
                self.track = Some(self.track.map_or(0, |i| i.min(last)));
                self.key = None;
            }
            Some(_) => {}
        }
        let chosen = self.track.map(|i| &self.tracks[i]);
        let key = chosen.map(|t| t.key.clone());
        if !self.linked && key != self.placed_for {
            let still = self.plays.is_empty() && self.flying.is_empty();
            if let Some(start) = chosen.and_then(|t| t.start).filter(|_| still) {
                self.pose = start;
            }
            self.placed_for = key;
        }
        let Some(i) = self.track else { return };
        let t = &mut self.tracks[i];
        if self.editor_open && t.start.is_none() && !t.steps.is_empty() {
            t.start = Some(self.pose);
            self.save_index(out);
        }
    }

    fn on_number(&mut self, id: &str, value: f64, out: &mut Vec<Command>) {
        let v = value as f32;
        match id {
            "in_len" => return self.shape_key(Some(v), None, true, out),
            "out_len" => return self.shape_key(None, Some(v), true, out),
            _ => {}
        }
        let speed = match id {
            "pad_move" => Some((&mut self.move_speed, MOVE_SPEED)),
            "pad_turn" => Some((&mut self.turn_speed, TURN_SPEED)),
            _ => None,
        };
        if let Some((speed, (min, max, _))) = speed {
            *speed = v.clamp(min, max);
            return;
        }
        if id == SMOOTHING_STRENGTH {
            return self.slide_smoothing(v, out);
        }
        if id == "key_time" {
            if let Some(key) = self.key.filter(|k| *k >= 1) {
                self.retime(key, v, out);
            }
            return;
        }
        let at = id.strip_prefix("s:").and_then(|r| {
            let (i, k) = r.split_once(':')?;
            Some((i.parse::<usize>().ok()?, k.parse::<usize>().ok()?))
        });
        match at {
            Some((i, k)) => self.edit_track(
                |steps| {
                    if let Some(s) = steps.get_mut(i).filter(|_| k < 6) {
                        s[k] = v;
                        *s = clamp_step(*s);
                    }
                },
                out,
            ),
            None => self.on_write(id, &Value::Number(value), out),
        }
    }

    fn on_text(&mut self, id: &str, value: String, out: &mut Vec<Command>) {
        match id {
            "search" => self.search = value,
            "rename" => {
                let Some(i) = self.track.filter(|i| *i < self.tracks.len()) else {
                    return;
                };
                match clean_name(&value) {
                    Some(name) if name == self.tracks[i].name => {}
                    Some(name) if !self.name_taken(&name) => {
                        self.tracks[i].name = name;
                        self.save_index(out);
                    }
                    _ => self.note = Some(NAME_TAKEN.to_owned()),
                }
            }
            "rename_position" => {
                let Some(i) = self.spot.filter(|i| *i < self.positions.len()) else {
                    return;
                };
                match clean_name(&value) {
                    Some(name) if name == self.positions[i].name => {}
                    Some(name) if !self.position_taken(&name) => {
                        self.positions[i].name = name;
                        self.save_index(out);
                    }
                    _ => self.note = Some(NAME_TAKEN.to_owned()),
                }
            }
            _ => {}
        }
    }

    fn on_pressed(&mut self, id: &str, out: &mut Vec<Command>) {
        if let Some(key) = id.strip_prefix("play:") {
            match self.playing(key) {
                true => self.pause(key, out),
                false => {
                    if let Some(i) = self.tracks.iter().position(|t| t.key == key) {
                        self.play_track(i, out);
                    }
                }
            }
            return;
        }
        match id {
            // The person's explicit choice, the only thing that writes over a
            // library this client couldn't read.
            "library_reset" => {
                self.unreadable = false;
                self.tracks.clear();
                self.positions.clear();
                self.track = None;
                self.key = None;
                self.spot = None;
                self.resume = None;
                self.library_changed = true;
                self.save_index(out);
            }
            "capture" | "capture_delayed" | "close" => self.on_call(None, id, &[], out),
            "open_editor" => self.editor_open = true,
            "new" => {
                let name = self.copy_name("Track");
                self.insert_track(name, Vec::new(), None, out);
                self.edit(Some(self.tracks.len() - 1));
            }
            "duplicate" => {
                if let Some(t) = self.track.and_then(|i| self.tracks.get(i)).cloned() {
                    let name = self.copy_name(&t.name);
                    self.insert_track(name, t.steps, t.start, out);
                    let last = self.tracks.len() - 1;
                    self.tracks[last].looping = t.looping;
                    self.tracks[last].optics = t.optics;
                    self.save_index(out);
                    self.edit(Some(last));
                }
            }
            // The track below takes its place, or the one above the last.
            "delete" => {
                if let Some(i) = self.track.filter(|i| *i < self.tracks.len()) {
                    let t = self.tracks.remove(i);
                    self.end_plays_where(|p| p.track.as_deref() == Some(t.key.as_str()), out);
                    out.push(Command::Unstore { key: t.key });
                    let below = i.min(self.tracks.len().saturating_sub(1));
                    self.edit((!self.tracks.is_empty()).then_some(below));
                    self.save_index(out);
                }
            }
            "play" => {
                let edited = self.track.and_then(|i| self.tracks.get(i));
                match edited.map(|t| t.key.clone()) {
                    Some(key) if self.playing(&key) => self.pause(&key, out),
                    Some(_) => self.play_track(self.track.unwrap_or(0), out),
                    None => {}
                }
            }
            "stop" => self.stop(out),
            "add_key" => self.add_key(out),
            "start_here" => {
                let now = self.optics_now();
                if let Some(i) = self.track.filter(|i| *i < self.tracks.len()) {
                    let t = &mut self.tracks[i];
                    t.keep_optics(now);
                    t.start = Some(self.pose);
                    t.optics = Some(now);
                    t.reclose();
                    self.key = Some(0);
                    self.save_track(i, out);
                    self.save_index(out);
                }
            }
            "key_set" => {
                let now = self.optics_now();
                self.edit_at_camera(
                    |steps, start, optics, key, pose| match key {
                        0 => {
                            restart(steps, start, &pose);
                            *start = pose;
                            *optics = Some(now);
                        }
                        _ => {
                            set_key(start, steps, key, &pose);
                            if let Some(s) = steps.get_mut(key - 1) {
                                *s = with_optics(*s, now);
                            }
                        }
                    },
                    out,
                );
            }
            "key_insert" => {
                let curve = self.new_curve();
                let full = self.track.and_then(|i| self.tracks.get(i));
                if full.is_some_and(|t| t.steps.len() >= PATH_MAX) {
                    self.note = Some(track_full());
                    return;
                }
                let (last, now) = (self.last_key(), self.optics_now());
                let inserted = self.edit_at_camera(
                    |steps, start, _, key, pose| {
                        let at = key.min(last).min(steps.len());
                        insert_key(start, steps, at, &pose, KEY_SECONDS, curve);
                        steps[at] = with_optics(steps[at], now);
                    },
                    out,
                );
                if inserted {
                    self.key = self.key.map(|k| k.min(last) + 1);
                }
            }
            "key_go" => {
                let gone = self.key.is_some_and(|key| self.go_to_key(key, out));
                if !gone {
                    self.note = Some(CHOOSE_KEY.to_owned());
                }
            }
            "level" => {
                let pose = self.pose;
                let [pitch, yaw, _] = angles(rotation(&pose));
                self.send_pose([pose[0], pose[1], pose[2], pitch, yaw, 0.0], out);
            }
            "key_dup" if self.key.is_some_and(|k| k >= 1 && k <= self.last_key()) => {
                self.edit_key(
                    |steps, i| {
                        if steps.len() < PATH_MAX {
                            steps.insert(i + 1, steps[i]);
                        }
                    },
                    out,
                );
                self.key = self.key.map(|k| k + 1);
            }
            "key_del" if self.key.is_some_and(|k| k > self.last_key()) => {
                self.note = Some(
                    "A closed track ends at its start: set it to Open to remove the way back."
                        .to_owned(),
                );
            }
            "key_del" if self.key.is_some_and(|k| k >= 1) => {
                self.edit_key(
                    |steps, i| {
                        steps.remove(i);
                    },
                    out,
                );
                self.key = self.key.map(|k| k - 1);
            }
            "key_earlier" if self.key.is_some_and(|k| k >= 2 && k <= self.last_key()) => {
                self.edit_key(|steps, i| steps.swap(i - 1, i), out);
                self.key = self.key.map(|k| k - 1);
            }
            "key_later" if self.key.is_some_and(|k| k >= 1 && k < self.last_key()) => {
                self.edit_key(|steps, i| steps.swap(i, i + 1), out);
                self.key = self.key.map(|k| k + 1);
            }
            "save_position" => {
                let name = free_name("Position", |n| self.position_taken(n));
                let key = self.fresh_key("pos");
                let pose = self.pose;
                self.positions.push(Position { name, key, pose });
                self.spot = Some(self.positions.len() - 1);
                self.save_index(out);
            }
            "go" => {
                if let Some(i) = self.spot {
                    self.go(i, out);
                }
            }
            "delete_position" => {
                if let Some(i) = self.spot.filter(|i| *i < self.positions.len()) {
                    self.positions.remove(i);
                    self.spot = None;
                    self.save_index(out);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn on_ui(&mut self, event: UiEvent, out: &mut Vec<Command>) {
        self.note = None;
        match event {
            UiEvent::Number { id, value } => self.on_number(&id, value, out),
            UiEvent::Pressed { id } => self.on_pressed(&id, out),
            UiEvent::Text { id, value } => self.on_text(&id, value, out),
            UiEvent::Toggled { id, value } if id == "local_nav" => self.local_nav = value,
            UiEvent::Toggled { id, value } => self.on_write(&id, &Value::Bool(value), out),
            UiEvent::Held { id, down } => {
                if let Some(fly) = id.strip_prefix("fly:").and_then(Fly::of) {
                    self.fly_held(fly, down, out);
                }
            }
            UiEvent::Wheel { lines } => {
                let (min, max, _) = MOVE_SPEED;
                let scaled = self.move_speed * SPEED_PER_LINE.powf(lines as f32);
                let snapped = significant(f64::from(scaled), SLIDER_FIGURES) as f32;
                self.move_speed = snapped.clamp(min, max);
            }
            UiEvent::Look { id, x, y } if id == "scene" => {
                self.look[0] += x as f32;
                self.look[1] += y as f32;
                self.wind(out);
            }
            UiEvent::Moved { id, index, value } if id == "timeline" && index >= 1 => {
                self.key = Some(index);
                self.retime(index, value as f32, out);
            }
            UiEvent::Dragged {
                id,
                index,
                at,
                value,
                done,
            } if id == "timeline" => self.drag_dot(index, at as f32, value as f32, done, out),
            UiEvent::Scrubbed { id, at } if id == "timeline" => self.scrub(at as f32, out),
            UiEvent::Chosen { id, index } => match id.as_str() {
                "key_shape" => {
                    let len = if index == 0 { 0.0 } else { 1.0 };
                    self.shape_key(Some(len), Some(len), false, out);
                }
                "mode" if index < MODES.len() => {
                    if index > 0 && self.mode.is_none_or(|m| m == 0) {
                        self.note = Some(OPEN_FIRST.to_owned());
                    }
                    self.on_write("mode", &Value::Number(index as f64), out)
                }
                "go_position" => self.go(index, out),
                "pick_position" => {
                    self.spot = (index < self.positions.len()).then_some(index);
                }
                "tracks" => self.edit(self.found().get(index).copied()),
                "scene" | "timeline" => {
                    let t = self.track.and_then(|i| self.tracks.get(i));
                    self.key = (index < t.map_or(0, |t| t.keys())).then_some(index);
                    if self.key.is_some() {
                        self.go_to_key(index, out);
                    }
                    if self.remember_handles() {
                        self.save_index(out);
                    }
                }
                "loop" => {
                    let looping = [Loop::Open, Loop::Closed, Loop::Looping];
                    if let Some(l) = looping.get(index) {
                        self.set_loop(*l, out);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}
