//! VRChat's user camera (`/usercamera/*`) as `@CAMERA`.
//!
//! `Pose` carries x, y, z, then pitch, yaw and roll in degrees. The rotation is
//! extrinsic: roll, then pitch, then yaw. `move_local` and `turn_local` are
//! relative to the camera, `move_world` and `turn_world` to the world's axes.
//!
//! A path (`fly_path`, a track) is a list of steps, each relative to the
//! previous point: a move along a Bézier curve, relative to the heading of the
//! start, a change of pitch, yaw and roll, and the ramps that set its pace
//! (`motion::Curve`). Plays are cumulative: on each tick every play adds its
//! change to the camera's pose, along with the user's own movement, held pads
//! and other plays. One `Pose` is sent per tick.
//!
//! The library is kept in the extension's store: `library` lists the tracks and
//! positions, and each track's steps have a key of their own. Actions pick an
//! item by key. Its data is sent when the action fires or is shared
//! (`Event::Resolve`), and added to the library of the client that receives it
//! (`Event::Import`).

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
use view::{Tab, EDITOR};

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
/// The signal sent when a play runs to its end. A stopped play doesn't send it.
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

/// VRChat's camera sliders (2025.3.3): field, OSC name, label, minimum,
/// maximum, default and summary.
///
/// The ranges and defaults are those of VRChat's release notes. They give
/// Lightness a range of 0–50 with a default of 60, so 0–100 and 50 are used
/// instead. VRChat sends no update when a slider is moved: the client reads the
/// values from OSCQuery at start, and a slider reads its default until then.
pub(crate) const SLIDERS: [(&str, &str, &str, f64, f64, f64, &str); 14] = [
    (
        "zoom",
        "Zoom",
        "Zoom",
        20.0,
        300.0,
        45.0,
        "The camera's field of view",
    ),
    (
        "exposure",
        "Exposure",
        "Exposure",
        -10.0,
        4.0,
        0.0,
        "How bright the image is",
    ),
    (
        "focal_distance",
        "FocalDistance",
        "Focal distance",
        0.0,
        10.0,
        1.5,
        "Distance in focus, with manual focus",
    ),
    (
        "aperture",
        "Aperture",
        "Aperture",
        1.4,
        32.0,
        15.0,
        "Depth of field aperture",
    ),
    (
        "hue",
        "Hue",
        "Hue",
        0.0,
        360.0,
        120.0,
        "Green screen colour's hue",
    ),
    (
        "saturation",
        "Saturation",
        "Saturation",
        0.0,
        100.0,
        100.0,
        "Green screen colour's saturation",
    ),
    (
        "lightness",
        "Lightness",
        "Lightness",
        0.0,
        100.0,
        50.0,
        "Green screen colour's lightness",
    ),
    (
        "look_at_me_x_offset",
        "LookAtMeXOffset",
        "Look-at X",
        -25.0,
        25.0,
        0.0,
        "Look-at-me image shift, sideways",
    ),
    (
        "look_at_me_y_offset",
        "LookAtMeYOffset",
        "Look-at Y",
        -25.0,
        25.0,
        0.0,
        "Look-at-me image shift, up or down",
    ),
    (
        "fly_speed",
        "FlySpeed",
        "Fly speed",
        0.1,
        15.0,
        3.0,
        "How fast the camera moves when flying",
    ),
    (
        "turn_speed",
        "TurnSpeed",
        "Turn speed",
        0.1,
        5.0,
        1.0,
        "How fast the camera turns when flying",
    ),
    (
        "smoothing_strength",
        "SmoothingStrength",
        "Smoothing",
        0.1,
        10.0,
        5.0,
        "How much camera movement is smoothed",
    ),
    (
        "photo_rate",
        "PhotoRate",
        "Photo rate",
        0.1,
        2.0,
        1.0,
        "The dolly's photo capture rate",
    ),
    (
        "duration",
        "Duration",
        "Duration",
        0.1,
        60.0,
        2.0,
        "How long a new dolly point takes",
    ),
];

/// The sliders a key stores ([`motion::Optics`]), in a step's order.
pub(crate) const OPTIC_FIELDS: [&str; 4] = ["zoom", "focal_distance", "aperture", "exposure"];

/// The toggle that smooths the camera's movement, and the slider for its
/// strength.
pub(crate) const SMOOTH_MOVEMENT: &str = "smooth_movement";
pub(crate) const SMOOTHING_STRENGTH: &str = "smoothing_strength";

/// VRChat's camera toggles (2025.3.3): field, OSC name, label, default and
/// summary. VRChat documents no defaults; these match a new camera. The client
/// reads the actual values from OSCQuery at start.
pub(crate) const TOGGLES: [(&str, &str, &str, bool, &str); 18] = [
    (
        "show_ui_in_camera",
        "ShowUIInCamera",
        "UI in camera",
        false,
        "Photos include the interface",
    ),
    (
        "lock",
        "Lock",
        "Lock",
        false,
        "Stops the camera being picked up (VR)",
    ),
    (
        "local_player",
        "LocalPlayer",
        "Local player",
        true,
        "Photos include you",
    ),
    (
        "remote_player",
        "RemotePlayer",
        "Remote players",
        true,
        "Photos include other players",
    ),
    (
        "environment",
        "Environment",
        "Environment",
        true,
        "Photos include the world",
    ),
    (
        "green_screen",
        "GreenScreen",
        "Green screen",
        false,
        "A green screen replaces the world",
    ),
    (
        "smooth_movement",
        "SmoothMovement",
        "Smooth movement",
        false,
        "Smooths the camera's movement",
    ),
    (
        "look_at_me",
        "LookAtMe",
        "Look at me",
        false,
        "The camera follows where your head faces",
    ),
    (
        "auto_level_roll",
        "AutoLevelRoll",
        "Level roll",
        false,
        "Keeps the camera's roll level",
    ),
    (
        "auto_level_pitch",
        "AutoLevelPitch",
        "Level pitch",
        false,
        "Keeps the camera's pitch level",
    ),
    (
        "flying",
        "Flying",
        "Flying",
        false,
        "The camera flies apart from your avatar",
    ),
    (
        "trigger_takes_photos",
        "TriggerTakesPhotos",
        "Trigger takes photos",
        false,
        "The controller trigger takes photos",
    ),
    (
        "dolly_paths_stay_visible",
        "DollyPathsStayVisible",
        "Dolly paths visible",
        false,
        "Dolly paths stay shown",
    ),
    (
        "camera_ears",
        "CameraEars",
        "Camera ears",
        false,
        "Audio comes from the camera's position",
    ),
    (
        "show_focus",
        "ShowFocus",
        "Show focus",
        false,
        "Shows where the camera focuses",
    ),
    (
        "streaming",
        "Streaming",
        "Streaming",
        false,
        "Sends the view out over Spout",
    ),
    (
        "roll_while_flying",
        "RollWhileFlying",
        "Roll while flying",
        false,
        "The camera rolls while flying (VR)",
    ),
    (
        "orientation_is_landscape",
        "OrientationIsLandscape",
        "Landscape",
        true,
        "Landscape, not portrait",
    ),
];

/// The timer of the play clock: one tick of every play, held pad and look.
pub(crate) const PLAY: &str = "play";

/// VRChat reports the camera's pose with a delay as it follows the poses sent
/// to it. Reports therefore wait until nothing has been sent for `SETTLE_MS`,
/// and then the last one is applied.
pub(crate) const SETTLE: &str = "settle";
pub(crate) const SETTLE_MS: u32 = 300;

/// The same for settings: reports of a setting wait until it hasn't been sent
/// for `SETTLE_MS`.
pub(crate) const SETTLE_SETTINGS: &str = "settle_settings";

/// Sliders are sent at most once per tick, with their latest value, since a
/// drag reports every movement of the mouse.
pub(crate) const SEND_SETTINGS: &str = "send_settings";

/// While the clock runs, one message is sent per tick: the pose, except every
/// this many ticks, when changed settings are sent instead. Sending settings
/// next to the poses makes the motion uneven in VRChat. VRChat documents no
/// limit, so the rate is a choice.
pub(crate) const SETTINGS_TICKS: u32 = 5;

/// While the clock doesn't run, the ticks between two sends of the optics
/// ([`OPTIC_FIELDS`]): VRChat can't keep up with 60 zoom changes per second.
/// [`REST`] is the timer for it.
pub(crate) const REST_TICKS: u32 = 2;
pub(crate) const REST: &str = "rest";

/// A camera setting, by its index in [`SLIDERS`] or [`TOGGLES`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum Setting {
    Slider(usize),
    Toggle(usize),
}

/// While the clock runs, the editor's view is rebuilt once in this many motion
/// events. In between, only its camera and playhead move (`Command::Mark`),
/// since rebuilding a long track's view on each tick takes too much fuel.
pub(crate) const EDITOR_REFRESH: u32 = 15;

/// The timer that stores a live edit, such as a slider being moved, once there
/// has been no other for `SAVE_MS`.
pub(crate) const SAVE: &str = "save";
pub(crate) const SAVE_MS: u32 = 400;

pub(crate) const LIBRARY_KEY: &str = "library";

/// The lists actions pick from: tracks and positions.
pub(crate) const TRACKS: &str = "tracks";
pub(crate) const POSITIONS: &str = "positions";

/// The longest name of a track or a position, in characters.
pub(crate) const NAME_MAX: usize = 64;

pub(crate) const NAME_TAKEN: &str = "That name is taken or empty.";
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

/// Reads a path passed by a script, which the client has checked against its
/// shape.
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

/// Reads a pose passed by a script or an action.
pub(crate) fn pose_value(value: &Value) -> Option<[f32; 6]> {
    let Value::List(v) = value else { return None };
    <[f32; 6]>::try_from(v.iter().map(number).collect::<Vec<_>>()).ok()
}

pub(crate) fn pose_list(pose: &[f32; 6]) -> Value {
    Value::List(pose.iter().map(|n| Value::Number(f64::from(*n))).collect())
}

/// `steps` as a value: the first [`WIRE`] numbers of each.
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

/// Appends `v` in Wex notation, with at most four decimals. Written by hand
/// because formatting floats takes too much fuel for a long track.
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

/// `v` in Wex notation ([`push_number`]).
pub(crate) fn wex_number(v: f32) -> String {
    let mut s = String::new();
    push_number(&mut s, v);
    s
}

/// Steps as the Wex list that `fly_path` takes, one step per line.
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
    /// The camera's pose: VRChat's if it reports one (`linked`), otherwise that
    /// of a stand-in at the start of the selected track (`placed_for`). The
    /// stand-in's pose is never sent to VRChat or published.
    pose: [f32; 6],
    linked: bool,
    placed_for: Option<String>,
    /// The pose fields as last published.
    published: Option<[f32; 6]>,
    /// The smoothed pose sent to VRChat while `SmoothMovement` is on, until it
    /// reaches the camera's pose. `overriding` is set while VRChat's own
    /// smoothing is switched off in its place: that one works on angles in
    /// 0–360 and restarts with every pose, so it turns back at 360° and cancels
    /// momentum.
    follow: Option<Follow>,
    overriding: bool,
    mode: Option<i32>,
    sliders: [f32; SLIDERS.len()],
    toggles: [bool; TOGGLES.len()],
    tracks: Vec<Track>,
    positions: Vec<Position>,
    next_key: u32,
    /// The selected track, and its selected key: 0 is the start, `n` the end of
    /// step `n - 1`.
    track: Option<usize>,
    key: Option<usize>,
    /// The selected position.
    spot: Option<usize>,
    /// The search text of the track lists.
    search: String,
    /// The selected tab of the editor.
    tab: Tab,
    plays: Vec<Play>,
    /// The directions being flown in.
    flying: Vec<Fly>,
    /// The flying speeds in metres and degrees per second.
    move_speed: f32,
    turn_speed: f32,
    /// Whether flying is relative to the camera instead of level.
    local_nav: bool,
    /// Whether the clock runs, and the time of its last tick (`Event::Timer`).
    ticking: bool,
    last_tick: Option<u64>,
    /// Pixels looked since the last tick, by dragging with the right button.
    look: [f32; 2],
    /// Whether reports wait (`SETTLE`), and the last one received meanwhile.
    settling: bool,
    reported: Option<[f32; 6]>,
    /// The settings sent that haven't settled, each with VRChat's last report
    /// (a toggle as 0 or 1), and the sliders changed since the last send
    /// ([`SEND_SETTINGS`]).
    settings_sent: Vec<(Setting, Option<f32>)>,
    sliders_due: Vec<usize>,
    /// Ticks until the optics can be sent again ([`REST_TICKS`]), the clock's
    /// position in its cycle of [`SETTINGS_TICKS`], and the pose that the next
    /// pose tick sends.
    resting: u32,
    beat: u32,
    held_pose: Option<[f32; 6]>,
    /// Where playing a track resumes.
    resume: Option<Resume>,
    /// The handle lengths (out, in) of a new key: those of the last selected
    /// key.
    key_handles: [f32; 2],
    /// The key of the track with a live edit to store (`SAVE`).
    unsaved: Option<String>,
    /// The key of the selected track as last stored.
    kept_choice: Option<String>,
    editor_open: bool,
    /// Motion events since the editor was rebuilt, the number of plays at the
    /// time, and its camera and playhead as last shown.
    since_built: u32,
    built_plays: usize,
    marked: ([[f32; 3]; 3], Option<f32>),
    /// A message for the user about the last action.
    note: Option<String>,
    shown: Option<Node>,
    shown_editor: Option<Node>,
    /// The library has changed since its lists were published.
    library_changed: bool,
    /// A stored value couldn't be read. The library isn't shown or saved until
    /// the user starts a new one.
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
            tab: Tab::default(),
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
    /// Sets the camera's pose and, while linked, publishes the fields that
    /// changed.
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

    /// Moves the camera to `pose`: at once, or over time while smoothing. Its
    /// angles are continuous ([`continuing`]).
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

    /// Sends `pose` to VRChat if linked: at once, or with the next pose tick
    /// while the clock runs. VRChat's own smoothing is switched off if this
    /// smooths instead.
    fn transmit(&mut self, pose: [f32; 6], out: &mut Vec<Command>) {
        if !self.linked {
            return;
        }
        match self.ticking {
            true => self.held_pose = Some(pose),
            false => self.transmit_now(pose, out),
        }
    }

    /// Sends the pose that waits for the next pose tick, if any.
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

    /// Advances smoothing by `dt` seconds. It ends at the pose, or at once when
    /// smoothing is switched off.
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

    /// The rate of smoothing per second, if it is on ([`SMOOTH_SECONDS`]).
    pub(crate) fn smooth_rate(&self) -> Option<f32> {
        let strength = SLIDERS
            .iter()
            .position(|s| s.0 == SMOOTHING_STRENGTH)
            .map_or(1.0, |i| self.sliders[i]);
        let (scale, power) = SMOOTH_SECONDS;
        self.smoothing()
            .then(|| 1.0 / (scale * strength.max(0.01).powf(power)))
    }

    /// Moves the camera to `pose` at once, even while smoothing.
    pub(crate) fn jump_pose(&mut self, pose: [f32; 6], out: &mut Vec<Command>) {
        let pose = continuing(&self.pose, pose);
        self.set_pose(pose, out);
        self.follow = None;
        self.transmit(pose, out);
    }

    /// The camera's current optics.
    pub(crate) fn optics_now(&self) -> Optics {
        OPTIC_FIELDS.map(|f| {
            SLIDERS
                .iter()
                .position(|s| s.0 == f)
                .map_or(0.0, |i| self.sliders[i])
        })
    }

    /// Sets the camera's optics, sending those that changed.
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

    /// Sets slider `i` to `v`, limited to its range, and sends it with the next
    /// tick if it changed ([`SEND_SETTINGS`]).
    fn send_slider(&mut self, i: usize, v: f32, out: &mut Vec<Command>) {
        let (_, _, _, lo, hi, ..) = SLIDERS[i];
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

    /// Sends the sliders that changed. Optics wait until they can be sent again
    /// ([`REST_TICKS`]).
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

    /// Sends all sliders that changed, without waiting.
    pub(crate) fn send_settings_now(&mut self, out: &mut Vec<Command>) {
        self.resting = 0;
        self.send_settings(out);
    }

    /// Starts the timer until the optics can be sent again. It is due half a
    /// tick early, so that jitter doesn't delay it by a whole tick.
    pub(crate) fn rest_later(&mut self, out: &mut Vec<Command>) {
        if self.resting > 0 && !self.ticking {
            out.push(Command::Timer {
                id: REST.to_owned(),
                after_ms: (2 * REST_TICKS - 3) * 500 / TICK_HZ,
            });
        }
    }

    /// The optics can be sent again.
    fn rested(&mut self, out: &mut Vec<Command>) {
        self.resting = 0;
        self.send_settings(out);
    }

    /// Makes VRChat's reports of `setting` wait until it has settled
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

    /// Handles VRChat's report of a setting: at once, or when it has settled.
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

    /// Whether movement smoothing is on.
    pub(crate) fn smoothing(&self) -> bool {
        TOGGLES
            .iter()
            .position(|t| t.0 == SMOOTH_MOVEMENT)
            .is_some_and(|i| self.toggles[i])
    }

    /// The camera's pose in the 3D view: the smoothed one while smoothing.
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

    /// Whether VRChat levels the rotation of `fly`, which disables its pad.
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
                    // While this smooths, it has switched off VRChat's
                    // smoothing itself.
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
                // VRChat's smoothing is switched back on when this stops.
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

    /// Handles a method call. `id` is `None` for a call from the page. A play
    /// answers its call when it ends; all others are answered here.
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
                        None => Play::path(id, &[], start),
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
    /// Updates the editor. After a `motion` event while the clock runs, only
    /// its camera and playhead ([`EDITOR_REFRESH`]).
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
                    mark: Mark::Camera(0, shown.0),
                });
            }
            if shown.1 != self.marked.1 {
                out.push(Command::Mark {
                    view: EDITOR.to_owned(),
                    id: "timeline".to_owned(),
                    mark: Mark::Line(0, shown.1),
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
                title: "Advanced Camera".to_owned(),
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
        // These events change nothing in the editor except what moves.
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
            // Switches VRChat's smoothing back on at the current pose, so that
            // it doesn't continue toward an earlier one.
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
