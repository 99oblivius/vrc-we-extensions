//! VRChat's user camera (`/usercamera/*`) as `@CAMERA`.
//!
//! `Pose` carries x y z, then pitch yaw roll in degrees; the rotation is
//! the Z, X, Y extrinsic euler order (roll, then pitch, then yaw).
//! `move_local` and `turn_local` act in the camera's own frame (forward is
//! its view), `move_world` and `turn_world` on the world's axes; a turn's
//! angles compose in the same euler order.
//!
//! A path (`fly_path`, a track) is a list of steps, each a move and a turn
//! from the point before (a relative program, like G91): the move on the
//! heading of the pose it started from (its yaw alone), along a cubic
//! Bezier through the keys, the turn as the change of pitch, yaw and roll,
//! each in a line, and both at the step's pace (`motion::Curve`). Plays
//! stack: every step of the clock, each adds how far its own path moved
//! since the last to wherever the camera is, so
//! moves, writes, the person's own movement, held pads and other plays all
//! add up. One `Pose` goes out per step.
//!
//! The library keeps named tracks and named positions in the extension's
//! store: `library` lists them, and each track's steps are under its own
//! key. Actions pick them by key; their data goes out when one fires or
//! travels (`Event::Resolve`), and an arriving mapping's item is kept here,
//! reusing an identical one (`Event::Import`).

use glam::Vec3;
use vrc_we_sdk::{
    Command, Event, Extension, Manifest, Mark, Node, OscArg, OscMessage, UiEvent, Value, TICK_HZ,
};

mod icons;
mod library;
mod manifest;
mod motion;
mod view;
mod widgets;

use library::{Position, Track};
use motion::{
    continuing, euler, pose_of, position, rotation, step_of, Fly, Follow, Optics, Play, Resume,
    Step, MOVE_SPEED, SMOOTH, SMOOTH_SECONDS, TURN_SPEED, WIRE,
};
use view::EDITOR;

vrc_we_sdk::export!(Camera);

pub(crate) const ROOT: &str = "/usercamera/";
pub(crate) const POSE: &str = "/usercamera/Pose";
pub(crate) const MODE: &str = "/usercamera/Mode";
pub(crate) const CAPTURE: &str = "/usercamera/Capture";
pub(crate) const CAPTURE_DELAYED: &str = "/usercamera/CaptureDelayed";
pub(crate) const CLOSE: &str = "/usercamera/Close";

/// The pose fields, in `Pose`'s argument order.
pub(crate) const POSE_FIELDS: [&str; 6] = ["x", "y", "z", "pitch", "yaw", "roll"];

pub(crate) const PAGE: &str = "camera";
/// The signal a play that runs to its end sends; a stopped one doesn't.
pub(crate) const PLAY_ENDED: &str = "play_ended";

/// VRChat's camera modes, by `Mode` value.
pub(crate) const MODES: [&str; 7] = [
    "Off",
    "Photo",
    "Stream",
    "Emoji",
    "Multilayer",
    "Print",
    "Drone",
];

/// VRChat's camera sliders (2025.3.3): field, OSC name, label, range, and
/// the default those notes give. VRChat sends no update when one moves:
/// the client reads its value from VRChat's OSCQuery tree when this starts,
/// and until then a slider reads its default. The notes give
/// Lightness 0–50 with a default of 60; it takes 0–100 and 50 until
/// VRChat's own range is checked.
pub(crate) const SLIDERS: [(&str, &str, &str, f64, f64, f64); 14] = [
    ("zoom", "Zoom", "Zoom", 20.0, 300.0, 45.0),
    ("exposure", "Exposure", "Exposure", -10.0, 4.0, 0.0),
    (
        "focal_distance",
        "FocalDistance",
        "Focal distance",
        0.0,
        10.0,
        1.5,
    ),
    ("aperture", "Aperture", "Aperture", 1.4, 32.0, 15.0),
    ("hue", "Hue", "Hue", 0.0, 360.0, 120.0),
    ("saturation", "Saturation", "Saturation", 0.0, 100.0, 100.0),
    ("lightness", "Lightness", "Lightness", 0.0, 100.0, 50.0),
    (
        "look_at_me_x_offset",
        "LookAtMeXOffset",
        "Look-at X",
        -25.0,
        25.0,
        0.0,
    ),
    (
        "look_at_me_y_offset",
        "LookAtMeYOffset",
        "Look-at Y",
        -25.0,
        25.0,
        0.0,
    ),
    ("fly_speed", "FlySpeed", "Fly speed", 0.1, 15.0, 3.0),
    ("turn_speed", "TurnSpeed", "Turn speed", 0.1, 5.0, 1.0),
    (
        "smoothing_strength",
        "SmoothingStrength",
        "Smoothing",
        0.1,
        10.0,
        5.0,
    ),
    ("photo_rate", "PhotoRate", "Photo rate", 0.1, 2.0, 1.0),
    ("duration", "Duration", "Duration", 0.1, 60.0, 2.0),
];

/// The sliders a key holds ([`motion::Optics`]), in a step's order.
pub(crate) const OPTIC_FIELDS: [&str; 4] = ["zoom", "focal_distance", "aperture", "exposure"];

/// The toggle that smooths the camera's movement, and its strength's
/// slider.
pub(crate) const SMOOTH_MOVEMENT: &str = "smooth_movement";
pub(crate) const SMOOTHING_STRENGTH: &str = "smoothing_strength";

/// VRChat's camera toggles (2025.3.3): field, OSC name, label, and the
/// value it reads until VRChat's is known (the client reads it when this
/// starts) or it is set here. VRChat documents no defaults: these are
/// a fresh camera's as chosen here (every layer shown, landscape, the
/// rest off).
pub(crate) const TOGGLES: [(&str, &str, &str, bool); 18] = [
    ("show_ui_in_camera", "ShowUIInCamera", "UI in camera", false),
    ("lock", "Lock", "Lock", false),
    ("local_player", "LocalPlayer", "Local player", true),
    ("remote_player", "RemotePlayer", "Remote players", true),
    ("environment", "Environment", "Environment", true),
    ("green_screen", "GreenScreen", "Green screen", false),
    (
        "smooth_movement",
        "SmoothMovement",
        "Smooth movement",
        false,
    ),
    ("look_at_me", "LookAtMe", "Look at me", false),
    ("auto_level_roll", "AutoLevelRoll", "Level roll", false),
    ("auto_level_pitch", "AutoLevelPitch", "Level pitch", false),
    ("flying", "Flying", "Flying", false),
    (
        "trigger_takes_photos",
        "TriggerTakesPhotos",
        "Trigger takes photos",
        false,
    ),
    (
        "dolly_paths_stay_visible",
        "DollyPathsStayVisible",
        "Dolly paths visible",
        false,
    ),
    ("camera_ears", "CameraEars", "Camera ears", false),
    ("show_focus", "ShowFocus", "Show focus", false),
    ("streaming", "Streaming", "Streaming", false),
    (
        "roll_while_flying",
        "RollWhileFlying",
        "Roll while flying",
        false,
    ),
    (
        "orientation_is_landscape",
        "OrientationIsLandscape",
        "Landscape",
        true,
    ),
];

/// The play clock: one step of every play, held pad and look, and one
/// `Pose`, each tick of the client's.
pub(crate) const PLAY: &str = "play";

/// VRChat reports the camera's pose as it follows the poses sent to it,
/// late, so a report taken while this drives it would undo the latest
/// steps: reports wait until this has sent nothing for `SETTLE_MS`, and
/// the last one is then taken as where VRChat put the camera.
pub(crate) const SETTLE: &str = "settle";
pub(crate) const SETTLE_MS: u32 = 300;

/// The same for the settings this sends (a live drag, a write, a play's
/// optics): VRChat's reports of one wait until this has sent it nothing
/// for `SETTLE_MS`, so a late echo never pulls it back.
pub(crate) const SETTLE_SETTINGS: &str = "settle_settings";

/// Sliders go to VRChat at most once a tick, each at its latest value: a
/// drag reports every mouse move, faster than VRChat takes them, and a
/// backlog would trail behind the drag.
pub(crate) const SEND_SETTINGS: &str = "send_settings";

/// While the clock runs, one stream goes to VRChat, one message a tick: the
/// pose, but for one tick in this many (12 a second) that carries the
/// settings changed meanwhile instead, the next tick's pose catching up. A
/// pose stream beside a settings stream stepped the motion and the
/// person's own input in game; VRChat documents no limit, so the rate is
/// chosen, not measured.
pub(crate) const SETTINGS_TICKS: u32 = 5;

/// With no clock running, the optics ([`OPTIC_FIELDS`]) rest this many
/// ticks after a send (30 a second), [`REST`] ending it: VRChat takes the
/// zoom more slowly than 60 a second, and a faster stream queues.
pub(crate) const REST_TICKS: u32 = 2;
pub(crate) const REST: &str = "rest";

/// One of VRChat's camera settings, by its place in [`SLIDERS`] or
/// [`TOGGLES`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum Setting {
    Slider(usize),
    Toggle(usize),
}

/// While the clock runs, the editor's view is built anew once in this many
/// motion events (a tick, a report, a look), so its offset line and
/// settings follow, and once more when the clock stops; between, only its
/// camera and playhead move (`Command::Mark`). A whole view each tick would
/// spend a long track's fuel.
pub(crate) const EDITOR_REFRESH: u32 = 15;

/// A live edit (a slider moving) is kept once no other has come for
/// `SAVE_MS`.
pub(crate) const SAVE: &str = "save";
pub(crate) const SAVE_MS: u32 = 400;

pub(crate) const LIBRARY_KEY: &str = "library";

/// The lists actions pick from: tracks by name (their steps), positions by
/// name (their poses).
pub(crate) const TRACKS: &str = "tracks";
pub(crate) const POSITIONS: &str = "positions";

/// The longest name a track or a position takes, in characters.
pub(crate) const NAME_MAX: usize = 64;

pub(crate) const NAME_TAKEN: &str = "Give it a name nothing else here has.";
pub(crate) const OPEN_FIRST: &str = "Open the camera in VRChat first.";
pub(crate) const MOVE_FIRST: &str =
    "Move the camera in VRChat once: VRChat tells where it is only as it moves.";
pub(crate) const CHOOSE_KEY: &str = "Choose a key of a track first.";

fn number(v: &Value) -> f32 {
    match v {
        Value::Number(n) => *n as f32,
        _ => 0.0,
    }
}

fn arg_f32(arg: &OscArg) -> Option<f32> {
    match arg {
        OscArg::Float(f) => Some(*f),
        OscArg::Int(i) => Some(*i as f32),
        _ => None,
    }
}

fn arg_bool(arg: &OscArg) -> Option<bool> {
    match arg {
        OscArg::Bool(b) => Some(*b),
        OscArg::Int(i) => Some(*i != 0),
        OscArg::Float(f) => Some(*f != 0.0),
        OscArg::Text(_) => None,
    }
}

/// A path as a script passes it, once checked against its shape.
pub(crate) fn steps_of(value: &Value) -> Vec<Step> {
    let Value::List(items) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Value::List(v) => step_of(&v.iter().map(number).collect::<Vec<_>>()),
            _ => None,
        })
        .collect()
}

/// A pose as a script or an action passes it.
pub(crate) fn pose_value(value: &Value) -> Option<[f32; 6]> {
    let Value::List(v) = value else { return None };
    <[f32; 6]>::try_from(v.iter().map(number).collect::<Vec<_>>()).ok()
}

pub(crate) fn pose_list(pose: &[f32; 6]) -> Value {
    Value::List(pose.iter().map(|n| Value::Number(f64::from(*n))).collect())
}

/// `steps` as they cross: each its first [`WIRE`] numbers.
pub(crate) fn steps_value(steps: &[Step]) -> Value {
    Value::List(
        steps
            .iter()
            .map(|s| {
                Value::List(
                    s[..WIRE]
                        .iter()
                        .map(|n| Value::Number(f64::from(*n)))
                        .collect(),
                )
            })
            .collect(),
    )
}

/// `v` as Wex reads it, onto `into`: at most four decimals, no trailing
/// zeros. Written digit by digit: a float formatted to a precision takes an
/// exact path, and each formatted piece an allocation, that interpreted
/// spend a long track's view's fuel.
fn push_number(into: &mut String, v: f32) {
    let n = (f64::from(v) * 10_000.0).round() as i64;
    if n < 0 {
        into.push('-');
    }
    let (whole, part) = (n.unsigned_abs() / 10_000, n.unsigned_abs() % 10_000);
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    let mut rest = whole;
    loop {
        at -= 1;
        digits[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    into.extend(digits[at..].iter().map(|d| char::from(*d)));
    if part > 0 {
        into.push('.');
        let mut place = 1_000;
        let mut rest = part;
        while rest > 0 {
            into.push(char::from(b'0' + (rest / place) as u8));
            rest %= place;
            place /= 10;
        }
    }
}

/// A number as Wex reads it ([`push_number`]).
pub(crate) fn wex_number(v: f32) -> String {
    let mut s = String::new();
    push_number(&mut s, v);
    s
}

/// Steps as the Wex list `fly_path` takes, one step a line, each its first
/// [`WIRE`] numbers.
pub(crate) fn wex(steps: &[Step]) -> String {
    let mut s = String::with_capacity(steps.len() * 64 + 4);
    s.push('[');
    for (i, step) in steps.iter().enumerate() {
        s.push_str(if i == 0 { "\n    [" } else { ",\n    [" });
        for (k, v) in step[..WIRE].iter().enumerate() {
            if k > 0 {
                s.push_str(", ");
            }
            push_number(&mut s, *v);
        }
        s.push(']');
    }
    s.push_str("\n]");
    s
}

#[derive(Debug)]
pub struct Camera {
    /// The camera's pose: VRChat's while it reports one (`linked`), else a
    /// stand-in the pads, keys and plays move here, placed at the chosen
    /// track's start (`placed_for`). Only a linked pose goes to VRChat, and
    /// the pose fields, scripts' moves and their writes wait for one: a
    /// pose sent from the stand-in would move VRChat's camera there.
    pose: [f32; 6],
    linked: bool,
    placed_for: Option<String>,
    /// The pose fields as last published.
    published: Option<[f32; 6]>,
    /// Where the camera is sent while this smooths it toward the pose
    /// (`SmoothMovement` on), until it rests there; and whether VRChat's
    /// own smoothing is switched off meanwhile, to come back on once VRChat
    /// settles. VRChat's eases each angle within 0–360 and restarts on
    /// every pose sent, so a turn past 360 swung back and a move cancelled
    /// a turn's momentum.
    follow: Option<Follow>,
    overriding: bool,
    mode: Option<i32>,
    sliders: [f32; SLIDERS.len()],
    toggles: [bool; TOGGLES.len()],
    tracks: Vec<Track>,
    positions: Vec<Position>,
    next_key: u32,
    /// The track the editor edits, chosen whenever there is one, and its
    /// key chosen there: 0 its start, `n` the end of its step `n - 1`.
    track: Option<usize>,
    key: Option<usize>,
    /// The position chosen in the editor.
    spot: Option<usize>,
    /// What the track lists show: the names holding it.
    search: String,
    plays: Vec<Play>,
    /// The pads held down, flying the camera.
    flying: Vec<Fly>,
    /// The pads' speeds: metres and degrees a second (not VRChat's own
    /// fly and turn speeds).
    move_speed: f32,
    turn_speed: f32,
    /// The pads move on the camera's own axes, not level.
    local_nav: bool,
    /// The play clock runs, and when it last stepped (`Event::Timer`'s
    /// `at_ms`).
    ticking: bool,
    last_tick: Option<u64>,
    /// Pixels a right-drag in the 3D view moved since the last step.
    look: [f32; 2],
    /// Reports wait (`SETTLE`); the last one that arrived meanwhile.
    settling: bool,
    reported: Option<[f32; 6]>,
    /// The settings sent since they last settled, each with VRChat's last
    /// report of it meanwhile (a toggle's as 0 or 1); and the sliders set
    /// since the last send ([`SEND_SETTINGS`]).
    settings_sent: Vec<(Setting, Option<f32>)>,
    sliders_due: Vec<usize>,
    /// Ticks before the optics may go again with no clock running
    /// ([`REST_TICKS`]); the clock's place in its round of
    /// [`SETTINGS_TICKS`]; and the pose it holds for its next send.
    resting: u32,
    beat: u32,
    held_pose: Option<[f32; 6]>,
    /// Where the person's play of a track goes on from.
    resume: Option<Resume>,
    /// The handle lengths (out, in) a new key takes: the last chosen key's.
    key_handles: [f32; 2],
    /// The track a live edit changed, kept once the edits settle (`SAVE`).
    unsaved: Option<String>,
    /// The chosen track's key as the library last kept it.
    kept_choice: Option<String>,
    editor_open: bool,
    /// Motion events since the editor's view was last built, the plays
    /// running then, and its camera and playhead as last shown.
    since_built: u32,
    built_plays: usize,
    marked: ([[f32; 3]; 3], Option<f32>),
    /// One line for the person about what the last action did.
    note: Option<String>,
    shown: Option<Node>,
    shown_editor: Option<Node>,
    /// The library changed since its lists were last published.
    library_changed: bool,
    /// A stored library value couldn't be read. Nothing is shown and nothing
    /// is saved over it until the person starts a new library here.
    unreadable: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            pose: [0.0; 6],
            linked: false,
            placed_for: None,
            published: None,
            follow: None,
            overriding: false,
            mode: None,
            sliders: SLIDERS.map(|s| s.5 as f32),
            toggles: TOGGLES.map(|t| t.3),
            tracks: Vec::new(),
            positions: Vec::new(),
            next_key: 0,
            track: None,
            key: None,
            spot: None,
            search: String::new(),
            plays: Vec::new(),
            flying: Vec::new(),
            move_speed: MOVE_SPEED.2,
            turn_speed: TURN_SPEED.2,
            local_nav: false,
            ticking: false,
            last_tick: None,
            look: [0.0; 2],
            settling: false,
            reported: None,
            settings_sent: Vec::new(),
            sliders_due: Vec::new(),
            resting: 0,
            beat: 0,
            held_pose: None,
            resume: None,
            key_handles: [SMOOTH[0], SMOOTH[1]],
            unsaved: None,
            kept_choice: None,
            editor_open: false,
            since_built: 0,
            built_plays: 0,
            marked: ([[0.0; 3]; 3], None),
            note: None,
            shown: None,
            shown_editor: None,
            library_changed: false,
            unreadable: false,
        }
    }
}

impl Camera {
    /// Takes `pose` as the camera's, publishing the fields it changes
    /// while linked.
    fn set_pose(&mut self, pose: [f32; 6], out: &mut Vec<Command>) {
        if self.linked {
            for (i, name) in POSE_FIELDS.iter().enumerate() {
                if self.published.is_none_or(|old| old[i] != pose[i]) {
                    out.push(Command::Field {
                        name: (*name).to_owned(),
                        value: Value::Number(f64::from(pose[i])),
                    });
                }
            }
            self.published = Some(pose);
        }
        self.pose = pose;
    }

    /// Moves the camera to `pose`, its angles going on from the last pose's
    /// ([`continuing`]): at once, or, while smoothing, by the clock
    /// ([`Follow`]).
    fn send_pose(&mut self, pose: [f32; 6], out: &mut Vec<Command>) {
        let before = self.pose;
        let pose = continuing(&before, pose);
        self.set_pose(pose, out);
        match self.smooth_rate() {
            Some(_) => {
                self.follow.get_or_insert(Follow::at(before));
                self.wind(out);
            }
            None => self.transmit(pose, out),
        }
    }

    /// Sends `pose` to VRChat while linked, VRChat's own smoothing switched
    /// off first while this smooths; its reports wait until it settles
    /// (`SETTLE`).
    /// While the clock runs, it goes with the clock's next pose tick
    /// ([`SETTINGS_TICKS`]).
    fn transmit(&mut self, pose: [f32; 6], out: &mut Vec<Command>) {
        if !self.linked {
            return;
        }
        match self.ticking {
            true => self.held_pose = Some(pose),
            false => self.transmit_now(pose, out),
        }
    }

    /// Sends the pose the clock holds, if any.
    pub(crate) fn release_pose(&mut self, out: &mut Vec<Command>) {
        if let Some(pose) = self.held_pose.take() {
            self.transmit_now(pose, out);
        }
    }

    fn transmit_now(&mut self, pose: [f32; 6], out: &mut Vec<Command>) {
        if self.smoothing() && !self.overriding {
            self.overriding = true;
            Self::osc_out(&format!("{ROOT}SmoothMovement"), OscArg::Bool(false), out);
        }
        out.push(Command::Osc(OscMessage {
            addr: POSE.to_owned(),
            args: pose.iter().map(|v| OscArg::Float(*v)).collect(),
        }));
        out.push(Command::Timer {
            id: SETTLE.to_owned(),
            after_ms: SETTLE_MS,
        });
        self.settling = true;
        self.reported = None;
    }

    /// The camera `dt` seconds on toward the pose while smoothing, sent as
    /// it goes; it rests on the pose once there, or at once when smoothing
    /// stops.
    pub(crate) fn follow_step(&mut self, dt: f32, out: &mut Vec<Command>) {
        let Some(mut f) = self.follow.take() else {
            return;
        };
        let to = self.pose;
        match self.smooth_rate() {
            Some(rate) => f.step(&to, rate, dt),
            None => f = Follow::at(to),
        }
        match f.settled(&to) {
            true => self.transmit(to, out),
            false => {
                self.transmit(f.pose, out);
                self.follow = Some(f);
            }
        }
    }

    /// The spring's rate a second while smoothing ([`SMOOTH_SECONDS`]).
    pub(crate) fn smooth_rate(&self) -> Option<f32> {
        let strength = SLIDERS
            .iter()
            .position(|s| s.0 == SMOOTHING_STRENGTH)
            .map_or(1.0, |i| self.sliders[i]);
        let (scale, power) = SMOOTH_SECONDS;
        self.smoothing()
            .then(|| 1.0 / (scale * strength.max(0.01).powf(power)))
    }

    /// Moves the camera to `pose` at once, smoothing or not: a jump to a
    /// key or a track's start.
    pub(crate) fn jump_pose(&mut self, pose: [f32; 6], out: &mut Vec<Command>) {
        let pose = continuing(&self.pose, pose);
        self.set_pose(pose, out);
        self.follow = None;
        self.transmit(pose, out);
    }

    /// The camera's optics as they stand.
    pub(crate) fn optics_now(&self) -> Optics {
        OPTIC_FIELDS.map(|f| {
            SLIDERS
                .iter()
                .position(|s| s.0 == f)
                .map_or(0.0, |i| self.sliders[i])
        })
    }

    /// Sets the camera's optics, sending VRChat those that change.
    pub(crate) fn set_optics(&mut self, optics: Optics, out: &mut Vec<Command>) {
        for (field, v) in OPTIC_FIELDS.iter().zip(optics) {
            let Some(i) = SLIDERS.iter().position(|s| s.0 == *field) else {
                continue;
            };
            if (self.sliders[i] - v).abs() > 1e-4 {
                self.send_slider(i, v, out);
            }
        }
    }

    /// Sends VRChat slider `i` at `v`, held to its range.
    /// It goes out with the tick's others ([`SEND_SETTINGS`]); an unchanged
    /// value sends nothing.
    fn send_slider(&mut self, i: usize, v: f32, out: &mut Vec<Command>) {
        let (_, _, _, lo, hi, _) = SLIDERS[i];
        let v = v.clamp(lo as f32, hi as f32);
        if self.sliders[i] == v && !self.sliders_due.contains(&i) {
            return;
        }
        self.set_slider(i, v, out);
        self.hold(Setting::Slider(i), out);
        if !self.ticking {
            out.push(Command::Timer {
                id: SEND_SETTINGS.to_owned(),
                after_ms: 0,
            });
        }
        if !self.sliders_due.contains(&i) {
            self.sliders_due.push(i);
        }
    }

    /// The sliders set since the last send, each at its latest value; the
    /// optics wait while they rest ([`REST_TICKS`]).
    pub(crate) fn send_settings(&mut self, out: &mut Vec<Command>) {
        let optic = |i: &usize| OPTIC_FIELDS.contains(&SLIDERS[*i].0);
        let (waiting, due): (Vec<usize>, Vec<usize>) = std::mem::take(&mut self.sliders_due)
            .into_iter()
            .partition(|i| self.resting > 0 && optic(i));
        self.sliders_due = waiting;
        for i in due {
            Self::osc_out(
                &format!("{ROOT}{}", SLIDERS[i].1),
                OscArg::Float(self.sliders[i]),
                out,
            );
            if optic(&i) {
                self.resting = REST_TICKS;
            }
        }
        self.rest_later(out);
    }

    /// The sliders set since the last send, now, rest or not: a jump's,
    /// which lands whole.
    pub(crate) fn send_settings_now(&mut self, out: &mut Vec<Command>) {
        self.resting = 0;
        self.send_settings(out);
    }

    /// With no clock running, a timer ends the optics' rest: it counts
    /// from the tick that applies it, the one after the send, so it ends on
    /// the tick [`REST_TICKS`] after, due half a tick early so a tick's
    /// jitter never slips it one.
    pub(crate) fn rest_later(&mut self, out: &mut Vec<Command>) {
        if self.resting > 0 && !self.ticking {
            out.push(Command::Timer {
                id: REST.to_owned(),
                after_ms: (2 * REST_TICKS - 3) * 500 / TICK_HZ,
            });
        }
    }

    /// The optics' rest ended: values set meanwhile go now.
    fn rested(&mut self, out: &mut Vec<Command>) {
        self.resting = 0;
        self.send_settings(out);
    }

    /// VRChat's reports of `setting`, just sent, wait until it settles
    /// ([`SETTLE_SETTINGS`]).
    fn hold(&mut self, setting: Setting, out: &mut Vec<Command>) {
        if !self.settings_sent.iter().any(|(s, _)| *s == setting) {
            self.settings_sent.push((setting, None));
        }
        out.push(Command::Timer {
            id: SETTLE_SETTINGS.to_owned(),
            after_ms: SETTLE_MS,
        });
    }

    /// VRChat reported `setting` at `v`: kept for when it settles while it
    /// is held, else taken now.
    fn reported_setting(&mut self, setting: Setting, v: f32, out: &mut Vec<Command>) {
        match self.settings_sent.iter_mut().find(|(s, _)| *s == setting) {
            Some((_, report)) => *report = Some(v),
            None => self.take_setting(setting, v, out),
        }
    }

    fn take_setting(&mut self, setting: Setting, v: f32, out: &mut Vec<Command>) {
        match setting {
            Setting::Slider(i) => self.set_slider(i, v, out),
            Setting::Toggle(i) => self.set_toggle(i, v != 0.0, out),
        }
    }

    /// Whether VRChat smooths the camera's movement.
    pub(crate) fn smoothing(&self) -> bool {
        TOGGLES
            .iter()
            .position(|t| t.0 == SMOOTH_MOVEMENT)
            .is_some_and(|i| self.toggles[i])
    }

    /// Where the view shows the camera: as sent while smoothing, else the
    /// pose.
    pub(crate) fn shown_pose(&self) -> [f32; 6] {
        self.follow.map_or(self.pose, |f| f.pose)
    }

    fn set_mode(&mut self, mode: i32, out: &mut Vec<Command>) {
        if self.mode != Some(mode) {
            out.push(Command::Field {
                name: "mode".to_owned(),
                value: Value::Number(f64::from(mode)),
            });
        }
        self.mode = Some(mode);
    }

    fn set_slider(&mut self, i: usize, value: f32, out: &mut Vec<Command>) {
        if self.sliders[i] != value {
            out.push(Self::slider_field(i, value));
        }
        self.sliders[i] = value;
    }

    fn set_toggle(&mut self, i: usize, value: bool, out: &mut Vec<Command>) {
        if self.toggles[i] != value {
            out.push(Self::toggle_field(i, value));
        }
        self.toggles[i] = value;
        let leveled: Vec<Fly> = self
            .flying
            .iter()
            .copied()
            .filter(|f| self.leveled(*f))
            .collect();
        self.flying.retain(|f| !leveled.contains(f));
    }

    /// Whether VRChat levels what `fly` turns: its pad is off.
    pub(crate) fn leveled(&self, fly: Fly) -> bool {
        fly.leveled_by()
            .and_then(|field| TOGGLES.iter().position(|t| t.0 == field))
            .is_some_and(|i| self.toggles[i])
    }

    fn slider_field(i: usize, value: f32) -> Command {
        Command::Field {
            name: SLIDERS[i].0.to_owned(),
            value: Value::Number(f64::from(value)),
        }
    }

    fn toggle_field(i: usize, value: bool) -> Command {
        Command::Field {
            name: TOGGLES[i].0.to_owned(),
            value: Value::Bool(value),
        }
    }

    fn osc_out(addr: &str, arg: OscArg, out: &mut Vec<Command>) {
        out.push(Command::Osc(OscMessage {
            addr: addr.to_owned(),
            args: vec![arg],
        }));
    }

    fn on_osc(&mut self, msg: OscMessage, out: &mut Vec<Command>) {
        let first = msg.args.first();
        match msg.addr.as_str() {
            POSE => {
                let values: Vec<f32> = msg.args.iter().filter_map(arg_f32).collect();
                if let Ok(pose) = <[f32; 6]>::try_from(values) {
                    self.linked = true;
                    match self.settling {
                        true => self.reported = Some(pose),
                        false => self.set_pose(pose, out),
                    }
                }
            }
            MODE => {
                if let Some(mode) = first.and_then(arg_f32) {
                    self.set_mode(mode.round() as i32, out);
                    if mode.round() as i32 == 0 {
                        self.linked = false;
                        self.settling = false;
                        self.overriding = false;
                    }
                }
            }
            addr => {
                let Some(name) = addr.strip_prefix(ROOT) else {
                    return;
                };
                if let Some(i) = SLIDERS.iter().position(|s| s.1 == name) {
                    if let Some(v) = first.and_then(arg_f32) {
                        self.reported_setting(Setting::Slider(i), v, out);
                    }
                } else if let Some(i) = TOGGLES.iter().position(|t| t.1 == name) {
                    // While this smooths, VRChat's smoothing is off by its
                    // doing, not the person's.
                    let ours = TOGGLES[i].0 == SMOOTH_MOVEMENT && self.overriding;
                    if let Some(v) = first.and_then(arg_bool).filter(|_| !ours) {
                        self.reported_setting(Setting::Toggle(i), f32::from(u8::from(v)), out);
                    }
                }
            }
        }
    }

    pub(crate) fn on_write(&mut self, field: &str, value: &Value, out: &mut Vec<Command>) {
        if field == "mode" {
            let mode = number(value).round() as i32;
            Self::osc_out(MODE, OscArg::Int(mode), out);
            self.set_mode(mode, out);
            return;
        }
        if let Some(i) = SLIDERS.iter().position(|s| s.0 == field) {
            self.send_slider(i, number(value), out);
            return;
        }
        if let Some(i) = TOGGLES.iter().position(|t| t.0 == field) {
            let on = matches!(value, Value::Bool(true));
            match (field == SMOOTH_MOVEMENT && self.overriding, on) {
                // VRChat's comes back on once this stops driving.
                (true, true) => {}
                (true, false) => {
                    self.overriding = false;
                    Self::osc_out(&format!("{ROOT}{}", TOGGLES[i].1), OscArg::Bool(on), out);
                }
                (false, _) => {
                    Self::osc_out(&format!("{ROOT}{}", TOGGLES[i].1), OscArg::Bool(on), out);
                    self.hold(Setting::Toggle(i), out);
                }
            }
            self.set_toggle(i, on, out);
            return;
        }
        let Some(i) = POSE_FIELDS.iter().position(|f| *f == field) else {
            return;
        };
        let mut pose = self.pose;
        pose[i] = number(value);
        self.send_pose(pose, out);
    }

    /// A call; `id` is a script's, none for the page's. Every call is
    /// answered here, but a coroutine's that plays, when it ends.
    pub(crate) fn on_call(
        &mut self,
        id: Option<u64>,
        method: &str,
        args: &[Value],
        out: &mut Vec<Command>,
    ) {
        let arg = |i: usize| args.get(i).map_or(0.0, number);
        let name = match args.first() {
            Some(Value::Text(t)) => t.as_str(),
            _ => "",
        };
        let by = Vec3::new(arg(0), arg(1), arg(2));
        let turn = euler(arg(0), arg(1), arg(2));
        let mut reply = Value::None;
        let pose = self.pose;
        match method {
            "move_local" => {
                let rot = rotation(&pose);
                self.send_pose(pose_of(position(&pose) + rot * by, rot), out);
            }
            "move_world" => {
                self.send_pose(pose_of(position(&pose) + by, rotation(&pose)), out);
            }
            "turn_local" => {
                self.send_pose(pose_of(position(&pose), rotation(&pose) * turn), out);
            }
            "turn_world" => {
                self.send_pose(pose_of(position(&pose), turn * rotation(&pose)), out);
            }
            "capture" => Self::osc_out(CAPTURE, OscArg::Bool(true), out),
            "capture_delayed" => Self::osc_out(CAPTURE_DELAYED, OscArg::Bool(true), out),
            "close" => Self::osc_out(CLOSE, OscArg::Bool(true), out),
            "stop" => self.end_plays(out),
            "add_key" => self.add_key(out),
            "track" => {
                if let Some(t) = self.tracks.iter().find(|t| t.name == name) {
                    reply = steps_value(&t.steps);
                }
            }
            "position" => {
                if let Some(p) = self.positions.iter().find(|p| p.name == name) {
                    reply = pose_list(&p.pose);
                }
            }
            "fly_path" => {
                let steps = args.first().map(steps_of).unwrap_or_default();
                return self.start(id, |start| Play::path(id, &steps, start), out);
            }
            "fly_to" => {
                let (target, seconds) = (args.first().and_then(pose_value), arg(1));
                return self.start(
                    id,
                    |start| match target {
                        Some(to) => Play::to(id, &to, seconds, start),
                        None => Play::new(id, Vec::new(), start),
                    },
                    out,
                );
            }
            _ => {}
        }
        out.extend(id.map(|id| Command::Reply { id, value: reply }));
    }
}

impl Camera {
    /// Shows the editor as it stands: after a `motion` event while the
    /// clock runs, its camera and playhead alone ([`EDITOR_REFRESH`]).
    fn show_editor(&mut self, motion: bool, out: &mut Vec<Command>) {
        let shown = (self.scene_camera(), self.playhead());
        let whole = self.shown_editor.is_none()
            || !motion
            || !self.ticking
            || self.since_built + 1 >= EDITOR_REFRESH
            || self.plays.len() != self.built_plays;
        if !whole {
            self.since_built += 1;
            if shown.0 != self.marked.0 {
                out.push(Command::Mark {
                    view: EDITOR.to_owned(),
                    id: "scene".to_owned(),
                    mark: Mark::Camera(Some(shown.0)),
                });
            }
            if shown.1 != self.marked.1 {
                out.push(Command::Mark {
                    view: EDITOR.to_owned(),
                    id: "timeline".to_owned(),
                    mark: Mark::Playhead(shown.1),
                });
            }
            self.marked = shown;
            return;
        }
        self.since_built = 0;
        self.built_plays = self.plays.len();
        self.marked = shown;
        let editor = self.editor();
        if self.shown_editor.as_ref() != Some(&editor) {
            out.push(Command::Modal {
                id: EDITOR.to_owned(),
                title: "Camera editor".to_owned(),
                view: editor.clone(),
            });
            self.shown_editor = Some(editor);
        }
    }
}

impl Extension for Camera {
    fn manifest(&self) -> Manifest {
        crate::manifest::build()
    }

    fn handle(&mut self, event: Event) -> Vec<Command> {
        let mut out = Vec::new();
        // Its own timers change nothing the editor shows but what moves or
        // what the next whole view takes up.
        let motion = matches!(
            &event,
            Event::Timer { .. }
                | Event::Osc(_)
                | Event::Ui {
                    event: UiEvent::Look { .. },
                    ..
                }
        );
        match event {
            Event::Start => {
                out.push(Command::Load {
                    key: LIBRARY_KEY.to_owned(),
                });
                out.extend((0..SLIDERS.len()).map(|i| Self::slider_field(i, self.sliders[i])));
                out.extend((0..TOGGLES.len()).map(|i| Self::toggle_field(i, self.toggles[i])));
                self.library_changed = true;
            }
            Event::Ui { page, event } if page == PAGE || page == EDITOR => {
                self.on_ui(event, &mut out);
            }
            Event::Ui { .. } => {}
            Event::Timer { id, at_ms } if id == PLAY => self.step(at_ms, &mut out),
            // VRChat's smoothing comes back once this rests, aimed at the
            // pose it rests at, not one it smoothed toward before.
            Event::Timer { id, .. } if id == SETTLE => {
                self.settling = false;
                if std::mem::take(&mut self.overriding) && self.smoothing() {
                    Self::osc_out(
                        &format!("{ROOT}SmoothMovement"),
                        OscArg::Bool(true),
                        &mut out,
                    );
                    out.push(Command::Osc(OscMessage {
                        addr: POSE.to_owned(),
                        args: self.pose.iter().map(|v| OscArg::Float(*v)).collect(),
                    }));
                }
                if let Some(pose) = self.reported.take() {
                    self.set_pose(pose, &mut out);
                }
            }
            Event::Timer { id, .. } if id == SEND_SETTINGS => self.send_settings(&mut out),
            Event::Timer { id, .. } if id == REST => self.rested(&mut out),
            Event::Timer { id, .. } if id == SETTLE_SETTINGS => {
                for (setting, report) in std::mem::take(&mut self.settings_sent) {
                    if let Some(v) = report {
                        self.take_setting(setting, v, &mut out);
                    }
                }
            }
            Event::Timer { id, .. } if id == SAVE => self.save_settled(&mut out),
            Event::Timer { .. } => {}
            Event::Loaded { key, value } => self.on_loaded(&key, value, &mut out),
            Event::ModalClosed { id } if id == EDITOR => {
                self.editor_open = false;
                self.shown_editor = None;
            }
            Event::Osc(msg) => self.on_osc(msg, &mut out),
            Event::Write { field, value } => self.on_write(&field, &value, &mut out),
            Event::Call {
                id, method, args, ..
            } => self.on_call(Some(id), &method, &args, &mut out),
            Event::Cancel { id } => self.end_plays_where(|p| p.call == Some(id), &mut out),
            Event::Resolve { id, list, key } => out.push(Command::Resolved {
                id,
                data: self.resolve(&list, &key),
            }),
            Event::Import {
                id,
                list,
                label,
                data,
            } => {
                let item = self.on_import(&list, &label, &data, &mut out);
                out.push(Command::Imported { id, item });
            }
            _ => {}
        }
        self.keep_chosen(&mut out);
        if self.chosen_key() != self.kept_choice {
            self.save_index(&mut out);
        }
        if std::mem::take(&mut self.library_changed) {
            self.publish_lists(&mut out);
        }
        let view = self.page();
        if self.shown.as_ref() != Some(&view) {
            out.push(Command::Page {
                page: PAGE.to_owned(),
                view: view.clone(),
            });
            self.shown = Some(view);
        }
        if self.editor_open {
            self.show_editor(motion, &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests;
