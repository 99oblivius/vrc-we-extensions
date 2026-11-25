//! The camera's page and its editor modal, and what their controls do.

use crate::widgets::*;

use vrc_we_sdk::{
    col, row, significant, Align, Button, Choice, Command, Flow, Frame, Grid, Ink, Key, Node,
    Path3, Point3, Scene, Size, Slider, Stroke, Surface, Text, TextSize, Toggle, UiEvent, Value,
    SLIDER_FIGURES,
};

use crate::library::{clean_name, free_name, Position, Track};
use crate::motion::{
    angles, clamp_step, facing, in_range, insert_key, key_pose, key_times, place_at, pose_at,
    position, ramp_at, restart, retime_key, rotation, same_place, set_key, shapes, step_to,
    with_curve, with_optics, Curve, Fly, Loop, Optics, Play, Resume, Step, MOVE_SPEED, PATH_MAX,
    SPEED_PER_LINE, TURN_SPEED,
};
use crate::{
    wex, wex_number, Camera, CHOOSE_KEY, MODES, MOVE_FIRST, NAME_TAKEN, OPEN_FIRST, SAVE, SAVE_MS,
    SLIDERS, SMOOTHING_STRENGTH, SMOOTH_MOVEMENT, TOGGLES,
};

/// The id of the editor modal.
pub(crate) const EDITOR: &str = "editor";

/// The tabs of the editor's right panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tab {
    #[default]
    Key,
    Fly,
    Lens,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Key, Tab::Fly, Tab::Lens];
    const FACES: [(&'static str, &'static str); 3] =
        [("Key", "key"), ("Fly", "fly"), ("Lens", "lens")];
}

/// The widths of the editor's panels, and the gap between their fields.
const TRACKS_PX: f32 = 224.0;
const TABS_PX: f32 = 288.0;
const PANEL_GAP: f32 = 8.0;

/// The labels of [`Loop::ALL`].
const LOOPS: [&str; 3] = ["Plays once", "Ends at its start", "Loops forever"];

/// VRChat's camera settings in groups, by field.
pub(crate) const SETTINGS: [(&str, &[&str]); 7] = [
    (
        "Lens",
        &[
            "zoom",
            "focal_distance",
            "aperture",
            "exposure",
            "show_focus",
        ],
    ),
    (
        "In the photo",
        &[
            "local_player",
            "remote_player",
            "environment",
            "show_ui_in_camera",
            "orientation_is_landscape",
        ],
    ),
    (
        "Green screen",
        &["green_screen", "hue", "saturation", "lightness"],
    ),
    (
        "Movement",
        &[
            "smooth_movement",
            "smoothing_strength",
            "auto_level_roll",
            "auto_level_pitch",
            "flying",
            "roll_while_flying",
            "fly_speed",
            "turn_speed",
        ],
    ),
    (
        "Look at me",
        &["look_at_me", "look_at_me_x_offset", "look_at_me_y_offset"],
    ),
    (
        "Dolly",
        &["photo_rate", "duration", "dolly_paths_stay_visible"],
    ),
    (
        "Other",
        &["lock", "trigger_takes_photos", "streaming", "camera_ears"],
    ),
];

/// The icons of [`MODES`].
const MODE_ICONS: [&str; 7] = [
    "power", "capture", "stream", "emoji", "layers", "print", "drone",
];

impl Camera {
    /// What the user has to do for VRChat to report its camera: open it, or
    /// move it if it is already open.
    pub(crate) fn without_pose(&self) -> &'static str {
        match self.mode {
            Some(mode) if mode > 0 => MOVE_FIRST,
            _ => OPEN_FIRST,
        }
    }

    /// The indices of the tracks whose names match the search.
    pub(crate) fn found(&self) -> Vec<usize> {
        let needle = self.search.to_lowercase();
        (0..self.tracks.len())
            .filter(|i| self.tracks[*i].name.to_lowercase().contains(&needle))
            .collect()
    }

    /// Whether track `key` is being played.
    fn playing(&self, key: &str) -> bool {
        self.plays.iter().any(|p| p.track.as_deref() == Some(key))
    }

    /// The Fly tab: the pads, their speeds, and how the camera moves.
    fn fly_rows(&self) -> Vec<Node> {
        let hold = |fly: Fly| {
            let pad = fly.pad();
            let disabled = self.leveled(fly);
            let leveled_by = fly
                .leveled_by()
                .and_then(|field| TOGGLES.iter().find(|t| t.0 == field));
            let tip = match leveled_by {
                Some(toggle) if disabled => format!("{} (off while {} is on)", pad.tip, toggle.2),
                _ => pad.tip.to_owned(),
            };
            let button = Button::hold(pad.id, pad.icon, tip, Some(pad.key));
            Node::from(button.disabled(disabled))
        };
        let pad = |label: &str, rows: [[Fly; 3]; 2]| {
            let keys = rows.into_iter().flatten().map(hold).collect();
            col![label, Grid::new(3, keys)]
        };
        let mut rows = Vec::new();
        if !self.linked {
            rows.push(format!("{} {STAND_IN}", self.without_pose()).into());
        }
        rows.extend([
            row![
                pad(
                    "Move",
                    [
                        [Fly::Rise, Fly::Forward, Fly::Sink],
                        [Fly::Left, Fly::Back, Fly::Right],
                    ],
                )
                .width(Size::Fill),
                pad(
                    "Turn",
                    [
                        [Fly::RollLeft, Fly::PitchUp, Fly::RollRight],
                        [Fly::YawLeft, Fly::PitchDown, Fly::YawRight],
                    ],
                )
                .width(Size::Fill),
            ]
            .into(),
            row![
                speed_slider("pad_move", "Move speed (m/s)", MOVE_SPEED, self.move_speed),
                speed_slider("pad_turn", "Turn speed (°/s)", TURN_SPEED, self.turn_speed),
            ]
            .into(),
            button("level", "Level the horizon", "level").into(),
            Toggle::new("local_nav", "Local navigation", Some(self.local_nav)).into(),
        ]);
        rows.extend(self.setting(SMOOTHING_STRENGTH));
        rows
    }

    /// The camera's modes in `columns` columns, and the photo buttons.
    fn mode_rows(&self, columns: u8) -> Vec<Node> {
        let modes = MODES.into_iter().zip(MODE_ICONS);
        let mode = self.mode.and_then(|m| usize::try_from(m).ok());
        vec![
            Button::grid("mode", modes, mode, columns).into(),
            row![
                button("capture", "Take a photo", "capture"),
                button("capture_delayed", "With timer", "timer"),
            ]
            .wrap(true)
            .into(),
        ]
    }

    /// The slider or the switch of a camera setting. The smoothing slider shows
    /// 0 while smoothing is off.
    fn setting(&self, field: &str) -> Option<Node> {
        if let Some(i) = TOGGLES.iter().position(|t| t.0 == field) {
            let toggle = Toggle::new(field, TOGGLES[i].2, Some(self.toggles[i]));
            return Some(toggle.into());
        }
        let i = SLIDERS.iter().position(|s| s.0 == field)?;
        let (_, _, label, min, max, ..) = SLIDERS[i];
        let value = f64::from(self.sliders[i]);
        let (min, value) = match field == SMOOTHING_STRENGTH {
            true if !self.smoothing() => (0.0, 0.0),
            true => (0.0, value),
            false => (min, value),
        };
        let slider = Slider::new(field, label, (min, max), Some(value));
        Some(slider.live(true).into())
    }

    /// The Lens tab: VRChat's camera modes and settings.
    fn settings_rows(&self) -> Vec<Node> {
        let mut rows: Vec<Node> = vec![Text::heading("Mode").into()];
        rows.extend(self.mode_rows(2));
        rows.push(Node::Divider);
        for (name, fields) in SETTINGS {
            rows.push(Text::heading(name).into());
            rows.extend(fields.iter().filter_map(|field| self.setting(field)));
            rows.push(Node::Divider);
        }
        rows.push(
            "Read from VRChat as this starts; VRChat doesn't report a slider moved in game.".into(),
        );
        rows
    }

    /// The saved positions as buttons, and a button that saves the current one.
    /// The editor marks the selected position.
    fn positions_rows(&self, id: &str, columns: u8) -> Vec<Node> {
        let mut rows: Vec<Node> = vec![
            Text::heading("Positions").into(),
            "Each belongs to the world it was saved in.".into(),
        ];
        if !self.positions.is_empty() {
            let chosen = (id == "pick_position").then_some(self.spot).flatten();
            let names = self.positions.iter().map(|p| (p.name.as_str(), "pin"));
            rows.push(Button::grid(id, names, chosen, columns).into());
        }
        rows.push(button("save_position", "Save position", "add").into());
        rows
    }

    /// The time of the edited track's playhead, if it is playing or paused.
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

    /// The camera in the 3D view: its position, forward and up.
    pub(crate) fn scene_camera(&self) -> [[f32; 3]; 3] {
        let p = self.shown_pose();
        let rot = rotation(&p);
        [
            position(&p).to_array(),
            (rot * glam::Vec3::Z).to_array(),
            (rot * glam::Vec3::Y).to_array(),
        ]
    }

    /// Shown on the page and in the editor when a stored value can't be read,
    /// with a button that starts a new library.
    fn unreadable_view(&self) -> Node {
        col![UNREADABLE, Button::delete("library_reset", NEW_LIBRARY),].into()
    }

    /// The page: the camera's modes, the tracks and the positions.
    pub(crate) fn page(&self) -> Node {
        if self.unreadable {
            return self.unreadable_view();
        }
        let mut rows = self.mode_rows(3);
        rows.push(Node::Divider);
        rows.push(Text::heading("Tracks").into());
        rows.push(
            name_field("search", "Search", &self.search)
                .live(true)
                .into(),
        );
        for i in self.found() {
            let t = &self.tracks[i];
            let (icon, label) = match self.playing(&t.key) {
                true => ("pause", "Pause"),
                false => ("play", "Play"),
            };
            let name = format!(
                "{}   {} keys   {} s",
                t.name,
                t.keys(),
                wex_number(t.seconds())
            );
            rows.push(
                row![button(&format!("play:{}", t.key), label, icon), name]
                    .align(Align::Middle)
                    .into(),
            );
        }
        rows.push(
            button("open_editor", "Advanced Camera", "lens")
                .primary()
                .wide()
                .large(true)
                .into(),
        );
        rows.push(Node::Divider);
        rows.extend(self.positions_rows("go_position", 3));
        if let Some(note) = &self.note {
            rows.push(Text::new(note).ink(Ink::Marked).into());
        }
        column(rows).into()
    }

    /// The editor: the track panel, the 3D view and the tabs, above the
    /// timeline.
    pub(crate) fn editor(&self) -> Node {
        if self.unreadable {
            return self.unreadable_view();
        }
        let edited = self.track.and_then(|i| self.tracks.get(i));
        let keyed = edited.filter(|t| t.keys() > 0);
        let mut scene = vec![self.scene(keyed).into()];
        scene.extend(self.note.as_ref().map(|note| {
            Frame::new(Text::new(note).ink(Ink::Marked))
                .surface(Surface::Raised)
                .padding(PANEL_GAP)
                .into()
        }));
        let pose_lines = self.pose_lines().map(|line| {
            Text::new(line)
                .size(TextSize::Small)
                .mono(true)
                .clip(true)
                .width(Size::Fill)
                .into()
        });
        let main = col![
            Node::Layers(scene),
            Flow::col(pose_lines.into()).gap(0.0).width(Size::Fill),
        ]
        .width(Size::Fill)
        .height(Size::Fill);
        let mut tracks = col![scrolled(self.library_rows(edited))];
        if let Some(t) = edited {
            tracks = tracks.push(pinned(column(self.control_rows(t))));
        }
        let tabs = Button::grid("tab", Tab::FACES, Some(self.tab as usize), 3);
        let tab = match self.tab {
            Tab::Key => self.key_rows(edited),
            Tab::Fly => self.fly_rows(),
            Tab::Lens => self.settings_rows(),
        };
        let panels = row![
            panel(tracks, TRACKS_PX),
            main,
            panel(col![pinned(tabs), scrolled(tab)], TABS_PX),
        ];
        let steps = keyed.map_or(&[][..], |t| &t.steps);
        let (keys, closed) = keyed.map_or((0, false), |t| (t.keys(), t.looping.closed()));
        let body = col![
            panels.gap(PANEL_PAD).height(Size::Fill),
            timeline(steps, keys, closed, self.key, self.playhead()),
        ];
        Frame::new(body.height(Size::Fill))
            .height(Size::Fill)
            .keys(self.pad_keys())
            .into()
    }

    /// The keys that fly the camera, whichever tab is selected.
    fn pad_keys(&self) -> Vec<(Key, String)> {
        Fly::PADS
            .iter()
            .filter(|pad| !self.leveled(pad.fly))
            .map(|pad| (pad.key, pad.id.to_owned()))
            .collect()
    }

    /// The 3D view: the camera, and the path and keys of `keyed`.
    fn scene(&self, keyed: Option<&Track>) -> Scene {
        let scene = Scene::new("scene")
            .cameras(vec![self.scene_camera()])
            .empty("No keyframes")
            .hint(HINT)
            .height(Size::Fill);
        let Some(track) = keyed else { return scene };
        let closed = track.looping.closed();
        let start = track.start.unwrap_or(self.pose);
        let (origin, heading) = (position(&start), facing(&start));
        let legs = shapes(&track.steps, closed);
        let samples = (PATH_POINTS / legs.len().max(1)).clamp(1, LEG_POINTS);
        let mut path: Vec<[f32; 3]> = Vec::new();
        let mut points = Vec::new();
        // The end of a closed path is its first key, so it has no number.
        let numbered = track.keys() - usize::from(closed);
        let mut push_key = |at: [f32; 3]| {
            let key = points.len();
            let point = Point3::new(at).selected(self.key == Some(key));
            points.push(match key < numbered {
                true => point.label((key + 1).to_string()),
                false => point,
            });
        };
        let world = |offset: glam::Vec3| (origin + heading * offset).to_array();
        for leg in &legs {
            let curve = (0..samples).map(|j| world(leg.point(j as f32 / samples as f32)));
            push_key(world(leg.from.pos));
            path.extend(curve);
        }
        let end = world(legs.last().map_or(glam::Vec3::ZERO, |l| l.to.pos));
        push_key(end);
        path.push(end);
        scene
            .points(points)
            .paths(vec![Path3::new(path, Stroke::new(Ink::Accent, 2.0))])
    }

    /// The track panel: the tracks, the name of the edited one, and the
    /// positions.
    fn library_rows(&self, edited: Option<&Track>) -> Vec<Node> {
        let found = self.found();
        let names = found.iter().map(|i| self.tracks[*i].name.as_str());
        let chosen = self.track.and_then(|t| found.iter().position(|i| *i == t));
        let mut rows: Vec<Node> = vec![
            Text::heading("Tracks").into(),
            name_field("search", "Search", &self.search)
                .live(true)
                .into(),
            Button::list("tracks", names, chosen),
            row![
                button("new", "New", "add"),
                Button::icon_only("duplicate", "duplicate", "Duplicate the track"),
                Button::delete("delete", "").tip("Delete the track"),
            ]
            .into(),
        ];
        if let Some(t) = edited {
            rows.push(name_field("rename", "Name (Enter renames)", &t.name).into());
            rows.push(Button::copy("Copy as Wex", wex(&t.steps)).into());
        }
        rows.push(Node::Divider);
        rows.extend(self.positions_rows("pick_position", 1));
        if let Some(p) = self.spot.and_then(|i| self.positions.get(i)) {
            rows.push(name_field("rename_position", "Name (Enter renames)", &p.name).into());
            rows.push(
                row![
                    button("go", "Fly there", "go"),
                    Button::delete("delete_position", "").tip("Delete the position"),
                ]
                .into(),
            );
        }
        rows
    }

    /// The playback controls of the edited track.
    fn control_rows(&self, t: &Track) -> Vec<Node> {
        let (icon, label) = match self.playing(&t.key) {
            true => ("pause", "Pause"),
            false => ("play", "Play"),
        };
        vec![
            Choice::new(
                "loop",
                LOOPS.map(str::to_owned).to_vec(),
                Some(t.looping as usize),
            )
            .width(Size::Fill)
            .into(),
            row![
                button("play", label, icon).primary().wide(),
                Button::icon_only("stop", "stop", "Stop, back to the start"),
            ]
            .into(),
        ]
    }

    /// The Key tab: adding keys, and the controls of the selected key.
    fn key_rows(&self, edited: Option<&Track>) -> Vec<Node> {
        let add_key = button("add_key", "Add keyframe", "keyframe").primary();
        let keyed = edited.filter(|t| t.keys() > 0).zip(self.key);
        let Some((t, key)) = keyed else {
            return vec![
                add_key.wide().into(),
                Text::heading("No keys yet").into(),
                "Move the camera to where the track starts, then press Add keyframe.".into(),
            ];
        };
        let keys = t.keys();
        let name = match key {
            0 => format!("Start of {keys} keys"),
            k => format!("Key {} of {keys}", k + 1),
        };
        let mut rows: Vec<Node> = vec![
            add_key.wide().into(),
            Text::heading(name).into(),
            button("key_insert", "Insert after this key", "insert")
                .wide()
                .into(),
            row![
                button("key_set", "Set to camera", "target").wide(),
                button("key_go", "Fly here", "go").wide(),
            ]
            .into(),
        ];
        let step = key.checked_sub(1).and_then(|i| Some((i, t.steps.get(i)?)));
        if let Some((i, step)) = step {
            rows.push(
                row![
                    button("key_dup", "Duplicate", "duplicate").wide(),
                    Button::delete("key_del", "Delete").wide(),
                ]
                .into(),
            );
            rows.push(
                row![
                    button("key_earlier", "Earlier", "earlier").wide(),
                    button("key_later", "Later", "later").wide(),
                ]
                .into(),
            );
            let field =
                |k: usize| number_node(&format!("s:{i}:{k}"), STEP_LABELS[k], Some(step[k]));
            rows.push(Node::Divider);
            rows.push(format!("From key {key}, in metres and degrees").into());
            rows.push(
                Grid::new(3, (0..6).map(field).collect())
                    .gap(PANEL_GAP)
                    .into(),
            );
        }
        rows.push(Node::Divider);
        let time = step.map(|_| {
            let time = key_times(&t.steps).get(key).copied();
            number_node("key_time", "Time (s)", time)
        });
        rows.extend(time);
        if t.steps.len() > RAMP_STEPS_MAX {
            rows.push(RAMP_TOO_LONG.into());
        }
        rows.extend(path_rows(&t.steps, key, t.looping.closed()));
        rows.push(Node::Divider);
        rows.push(button("start_here", "Start the track at the camera", "pin").into());
        rows
    }

    /// Edits the selected key with the camera's pose. `edit` takes the steps,
    /// the track's start and its optics, the key and the pose. Returns false,
    /// with a note for the user, if no key is selected.
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
        t.edit(|t| {
            if let Track {
                steps,
                start: Some(start),
                optics,
                ..
            } = t
            {
                edit(steps, start, optics, key, pose);
            }
        });
        self.save_track(i, out);
        self.save_index(out);
        true
    }

    /// The last key of the edited track, not counting the end of a closed one.
    fn last_key(&self) -> usize {
        self.track
            .and_then(|i| self.tracks.get(i))
            .map_or(0, |t| t.steps.len() - usize::from(t.looping.closed()))
    }

    /// Sets how the edited track ends. Closing it adds a step back to the
    /// start, and opening it removes that step.
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
            _ => {}
        }
        t.edit(|t| {
            match (t.looping.closed(), looping.closed()) {
                (false, true) => {
                    let mut back = with_curve([0.0; 15], curve);
                    back[6] = KEY_SECONDS;
                    t.steps.push(back);
                }
                (true, false) => {
                    t.steps.pop();
                }
                _ => {}
            }
            t.looping = looping;
        });
        let last = t.steps.len();
        self.key = self.key.map(|k| k.min(last));
        self.save_track(i, out);
        self.save_index(out);
    }

    /// Selects the track to edit. [`Camera::keep_chosen`] then selects its
    /// first key.
    fn edit(&mut self, i: Option<usize>) {
        self.track = i.filter(|i| *i < self.tracks.len());
        self.key = None;
    }

    /// Plays track `i` if it isn't playing, from where it was paused or from
    /// the selected key. It continues from the camera's pose if that hasn't
    /// changed since the pause, and otherwise jumps to the track.
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

    /// Flies to position `i`.
    fn go(&mut self, i: usize, out: &mut Vec<Command>) {
        if let Some(to) = self.positions.get(i).map(|p| p.pose) {
            self.start(None, |start| Play::to(None, &to, GO_SECONDS, start), out);
        }
    }

    /// Sets the smoothing strength. A value below VRChat's minimum switches
    /// smoothing off, any other switches it on.
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

    /// Pauses track `key`.
    fn pause(&mut self, key: &str, out: &mut Vec<Command>) {
        self.end_plays_where(|p| p.call.is_none() && p.track.as_deref() == Some(key), out);
    }

    /// Stops every play and returns to the start of the edited track.
    fn stop(&mut self, out: &mut Vec<Command>) {
        self.end_plays(out);
        self.go_to_key(0, out);
    }

    /// Selects key `key` of the edited track and moves the camera and its
    /// optics there at once, pausing the track. Returns false if there is no
    /// track with a start.
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

    /// Moves the camera and its optics to `at` seconds into the edited track,
    /// pausing it.
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
        if let Some(o) = optics.and_then(|o| o.get(step).copied()) {
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

    /// Moves key `index` of the edited track to `at` seconds ([`retime_key`]).
    fn retime(&mut self, index: usize, at: f32, out: &mut Vec<Command>) {
        self.edit_track(|steps| retime_key(steps, index, at), out);
    }

    /// Edits the step that leads to key `key`. `edit` takes the steps and the
    /// index of that step.
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

    /// Adds a key at the camera's pose to the end of the selected track, or to
    /// a new track if there is none. The first key is the track's start.
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
                // Before the step back to the start.
                let at = t.steps.len() - usize::from(closed);
                let step = step_to(&start, &t.steps[..at], &pose, KEY_SECONDS);
                t.keep_optics(now);
                let step = clamp_step(with_optics(with_curve(step, curve), now));
                t.edit(|t| t.steps.insert(at, step));
                self.key = Some(at + 1);
                self.save_track(i, out);
                self.save_index(out);
            }
        }
    }

    /// Keeps the selected key's handle lengths for new keys. Returns whether
    /// they changed.
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

    /// Sets the given handle lengths of the selected key, and keeps them for
    /// new keys. A `live` edit is stored after a delay ([`SAVE`]).
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
            true => self.save_later(i, out),
            false => {
                self.save_track(i, out);
                if changed {
                    self.save_index(out);
                }
            }
        }
    }

    /// Stores the track with a live edit, and the handle lengths of new keys.
    pub(crate) fn save_settled(&mut self, out: &mut Vec<Command>) {
        let Some(key) = self.unsaved.take() else {
            return;
        };
        if let Some(i) = self.tracks.iter().position(|t| t.key == key) {
            self.save_track(i, out);
            self.save_index(out);
        }
    }

    /// Handles dragging key `index` in the timeline to `at` seconds and a ramp
    /// height. The track is stored when the key is released (`done`), or after
    /// a delay ([`SAVE`]).
    fn drag_key(&mut self, index: usize, at: f32, height: f32, done: bool, out: &mut Vec<Command>) {
        let Some(i) = self.track.filter(|i| *i < self.tracks.len()) else {
            return;
        };
        let t = &mut self.tracks[i];
        if index >= t.keys() {
            return;
        }
        self.key = Some(index);
        t.edit(|t| retime_key(&mut t.steps, index, at));
        if t.steps.len() <= RAMP_STEPS_MAX {
            // Rounded to less than a pixel of the timeline's height.
            t.set_ramp(index, (ramp_at(height) * 10.0).round() / 10.0);
        }
        match done {
            true => self.save_track(i, out),
            false => self.save_later(i, out),
        }
    }

    /// Stores track `i` when there has been no other live edit for [`SAVE_MS`].
    fn save_later(&mut self, i: usize, out: &mut Vec<Command>) {
        self.unsaved = Some(self.tracks[i].key.clone());
        out.push(Command::Timer {
            id: SAVE.to_owned(),
            after_ms: SAVE_MS,
        });
    }

    /// The curve of a new key.
    fn new_curve(&self) -> Curve {
        let [out, into] = self.key_handles;
        [out, into, 0.0, 0.0]
    }

    /// The camera's position and angles, in columns of a fixed width.
    fn pose_lines(&self) -> [String; 2] {
        let [x, y, z, pitch, yaw, roll] = in_range(self.pose);
        [
            format!("x     {x:>8.2}  y   {y:>8.2}  z    {z:>8.2} m"),
            format!("pitch {pitch:>8.2}  yaw {yaw:>8.2}  roll {roll:>8.2}°"),
        ]
    }

    /// Keeps a track and one of its keys selected, as long as there are any,
    /// and puts the stand-in at the start of a newly selected track unless it
    /// is moving. While the editor is open, a track with steps but no start
    /// gets its start from the camera.
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
        if t.keys() > 0 {
            self.key.get_or_insert(0);
        }
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
            if let Some(key) = self.key {
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
            // The only action that overwrites a library that couldn't be read.
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
            "capture" | "capture_delayed" => self.on_call(None, id, &[], out),
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
            // Selects the next track, or the previous one if it was the last.
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
                    let pose = self.pose;
                    let t = &mut self.tracks[i];
                    t.keep_optics(now);
                    t.edit(|t| {
                        t.start = Some(pose);
                        t.optics = Some(now);
                    });
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
                    "A closed track ends at its start: set it to play once to remove the way back."
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
                if let Some(fly) = Fly::of_id(&id) {
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
            UiEvent::Dragged {
                id,
                index,
                at,
                value,
                done,
            } if id == "timeline" => self.drag_key(index, at as f32, value as f32, done, out),
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
                "tab" => {
                    if let Some(tab) = Tab::ALL.get(index) {
                        self.tab = *tab;
                    }
                }
                "loop" => {
                    if let Some(looping) = Loop::ALL.get(index) {
                        self.set_loop(*looping, out);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}
