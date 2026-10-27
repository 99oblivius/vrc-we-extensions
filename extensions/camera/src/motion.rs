//! How the camera moves: a path's steps and the legs they make, plays
//! along them, and flying while a pad is held.

use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};
use vrc_we_sdk::{Command, Key, Value, TICK_HZ};

use crate::{Camera, PLAY, PLAY_ENDED, SETTINGS_TICKS};

/// The most steps a path takes; how far (metres) and how far round
/// (degrees) one step may go; how long one may last.
pub(crate) const PATH_MAX: usize = 1024;
pub(crate) const STEP_REACH: f64 = 1000.0;
pub(crate) const STEP_TURN: f64 = 3600.0;
pub(crate) const STEP_SECONDS: f64 = 600.0;

/// A tick's seconds: a step's when the clock has no earlier tick to
/// measure from.
pub(crate) const FRAME: f32 = 1.0 / TICK_HZ as f32;

/// The longest step one tick takes, in milliseconds: a tick late by more
/// moves no further, so a stall never jumps the camera.
const STEP_MAX_MS: u64 = 100;

/// Degrees a pixel of right-drag turns the camera.
const LOOK_PER_PX: f32 = 0.2;

/// The camera's own smoothing: its time constant at VRChat's strength `s`
/// is `SMOOTH_SECONDS.0 · s ^ SMOOTH_SECONDS.1` seconds, so a low strength
/// stays quick (0.04 s at 1) and 10 trails for about a second.
pub(crate) const SMOOTH_SECONDS: (f32, f32) = (0.04, 1.4);

/// Where the camera is sent while this smooths its moves: an elastic per
/// position and angle toward the pose (whose angles run on,
/// [`continuing`]), closing the same share of what is left each moment.
/// It moves the moment the pose does, as fast as it is far behind: a steady
/// move is followed at its own speed a constant distance behind, a stop is
/// reached without overshoot, and having no momentum of its own, nothing
/// cancels another move's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Follow {
    pub(crate) pose: [f32; 6],
}

impl Follow {
    pub(crate) fn at(pose: [f32; 6]) -> Self {
        Self { pose }
    }

    /// `dt` seconds toward `to` at `rate` a second, exactly as the elastic
    /// closes on a pose held still that long.
    pub(crate) fn step(&mut self, to: &[f32; 6], rate: f32, dt: f32) {
        let left = (-rate * dt).exp();
        for (at, goal) in self.pose.iter_mut().zip(to) {
            *at = goal + (*at - goal) * left;
        }
    }

    /// Whether it rests on `to`: within a millimetre and a twentieth of a
    /// degree, so its last step to `to` is never a visible jump.
    pub(crate) fn settled(&self, to: &[f32; 6]) -> bool {
        position(&self.pose).distance(position(to)) < 1e-3
            && (3..6).all(|k| (self.pose[k] - to[k]).abs() < 0.05)
    }
}

/// Where the person's play of a track goes on from: the track (by key),
/// the step it was on and the seconds into it, and where the camera was
/// then; none when a key was chosen instead, so the play snaps to it.
/// Steps, not seconds: a step of 0 s ends where the next starts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Resume {
    pub(crate) track: String,
    pub(crate) step: usize,
    pub(crate) elapsed: f32,
    pub(crate) pose: Option<[f32; 6]>,
}

impl Resume {
    /// The seconds along its track, on `steps`.
    pub(crate) fn at(&self, steps: &[Step]) -> f32 {
        let before: f32 = steps.iter().take(self.step).map(|s| s[6]).sum();
        before + self.elapsed
    }
}

/// A path step: a move (right, up, forward) from the point before, on the
/// start pose's heading, the change of pitch, yaw and roll in degrees, the
/// seconds it takes (0 jumps), its curve ([`Curve`]), then the camera's
/// optics at the key it reaches ([`Optics`]). The first [`WIRE`] numbers
/// are what crosses to scripts and contacts; the optics stay with the
/// library's track.
pub(crate) type Step = [f32; 15];
pub(crate) const WIRE: usize = 11;

/// VRChat's zoom, focal distance, aperture and exposure ([`OPTIC_FIELDS`]),
/// from [`OPTICS`] on in a step: what a key holds of the camera's settings,
/// eased between keys as the move is.
pub(crate) type Optics = [f32; 4];
pub(crate) const OPTICS: usize = 11;

/// A step's curve, from [`CURVE`] on: the handle lengths at the key it
/// leaves and at the key it reaches (0 along the step's own chord, so
/// straight; 1 the key's automatic direction; a third of the chord long),
/// and its pace dot: where along the step (0–1) and how fast there, as a
/// multiple of the step's average pace.
pub(crate) type Curve = [f32; 4];
pub(crate) const CURVE: usize = 7;
pub(crate) const OUT_LEN: usize = 7;
pub(crate) const IN_LEN: usize = 8;
pub(crate) const DOT_AT: usize = 9;
pub(crate) const DOT_PACE: usize = 10;

/// A straight step at an even pace: what a step of 7 numbers, and a flight
/// to a pose, is.
pub(crate) const LINEAR: Curve = [0.0, 0.0, 0.5, 1.0];
/// A new key's curve before any key was chosen: smooth, at an even pace.
pub(crate) const SMOOTH: Curve = [1.0, 1.0, 0.5, 1.0];

/// The longest handle, in automatic lengths; the slowest and fastest a pace
/// dot goes, as multiples of its step's average.
pub(crate) const HANDLE_MAX: f64 = 1.0;

/// How a track ends: open; closed, its last step the way back to its start
/// and the curve joined there; or looping, closed and played round until
/// stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Loop {
    #[default]
    Open,
    Closed,
    Looping,
}

impl Loop {
    pub(crate) fn closed(self) -> bool {
        self != Loop::Open
    }
}

/// The last of `steps` made the way back to its start: its move and turn
/// undo all the others', and it reaches the start's optics, when given.
pub(crate) fn reclose(steps: &mut [Step], optics: Option<Optics>) {
    let Some((last, rest)) = steps.split_last_mut() else {
        return;
    };
    for k in 0..6 {
        last[k] = -rest.iter().map(|s| s[k]).sum::<f32>();
    }
    if let Some(o) = optics {
        last[OPTICS..].copy_from_slice(&o);
    }
}
pub(crate) const PACE_RANGE: (f64, f64) = (0.05, 2.0);

/// How far a pace dot stays inside its step's ends, as a share of it, so
/// both pieces of the pace keep a width.
const DOT_INSET: f32 = 0.004;

/// How close to a leg's end its time counts as reached: what adding up a
/// tick's seconds rounds off.
const TIME_SLACK: f32 = 1e-4;

/// The samples of a leg's curve its length is measured at.
const ARC_SAMPLES: usize = 32;

/// `values` as a step: 7 numbers are a straight one at an even pace, 11 a
/// whole one without optics (read as none), 15 one with them; anything
/// else is none.
pub(crate) fn step_of(values: &[f32]) -> Option<Step> {
    let mut s = [0.0; 15];
    match values.len() {
        7 => {
            s[..7].copy_from_slice(values);
            s[CURVE..WIRE].copy_from_slice(&LINEAR);
        }
        WIRE => s[..WIRE].copy_from_slice(values),
        15 => s.copy_from_slice(values),
        _ => return None,
    }
    Some(s)
}

/// `step` with `curve`.
pub(crate) fn with_curve(mut step: Step, curve: Curve) -> Step {
    step[CURVE..WIRE].copy_from_slice(&curve);
    step
}

/// `step` reaching `optics`.
pub(crate) fn with_optics(mut step: Step, optics: Optics) -> Step {
    step[OPTICS..].copy_from_slice(&optics);
    step
}

/// The optics each key of `steps` holds, from the start's `first`.
pub(crate) fn key_optics(first: Optics, steps: &[Step]) -> Vec<Optics> {
    std::iter::once(first)
        .chain(steps.iter().map(|s| {
            let mut o = [0.0; 4];
            o.copy_from_slice(&s[OPTICS..]);
            o
        }))
        .collect()
}

/// Where a path has reached, from its start: a position on the start
/// pose's heading, and how far pitch, yaw and roll have changed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Offset {
    pub(crate) pos: Vec3,
    turn: Vec3,
}

impl Offset {
    const ZERO: Offset = Offset {
        pos: Vec3::ZERO,
        turn: Vec3::ZERO,
    };

    /// `self` then `step`.
    pub(crate) fn then(self, step: &Step) -> Offset {
        Offset {
            pos: self.pos + Vec3::new(step[0], step[1], step[2]),
            turn: self.turn + Vec3::new(step[3], step[4], step[5]),
        }
    }
}

/// The pace of a step with its dot at `at`, `pace` times its average, at
/// `t` (0–1) of its time: two cubic Hermite pieces, from 1 at the key it
/// leaves to `pace` at the dot (level there) and on to 1 at the key it
/// reaches, the keys' slopes ±6(1 − `pace`) in each piece's own span. Each
/// piece then averages exactly 1, so the step keeps its time whatever its
/// dot, and the pace stays between about 1 − 0.67(`pace` − 1) and `pace`
/// (never below 0 over [`PACE_RANGE`]).
pub(crate) fn pace(at: f32, pace: f32, t: f32) -> f32 {
    let (c, slope) = (dot_place(at), 6.0 * (1.0 - pace));
    match t.clamp(0.0, 1.0) {
        t if t <= c => hermite(1.0, slope, pace, 0.0, t / c),
        t => hermite(pace, 0.0, 1.0, -slope, (t - c) / (1.0 - c)),
    }
}

/// How far along a step (0–1) its pace has carried it at `t` of its time:
/// the integral of [`pace`]; the dot is reached at `at`.
pub(crate) fn progress(at: f32, pace: f32, t: f32) -> f32 {
    let (c, slope) = (dot_place(at), 6.0 * (1.0 - pace));
    match t.clamp(0.0, 1.0) {
        t if t <= c => c * hermite_area(1.0, slope, pace, 0.0, t / c),
        t => c + (1.0 - c) * hermite_area(pace, 0.0, 1.0, -slope, (t - c) / (1.0 - c)),
    }
}

/// Where a pace dot set at `at` sits: [`DOT_INSET`] inside its step's ends.
pub(crate) fn dot_place(at: f32) -> f32 {
    at.clamp(DOT_INSET, 1.0 - DOT_INSET)
}

/// The cubic Hermite from `y0` (slope `m0`) to `y1` (slope `m1`) over 0–1,
/// at `s`.
fn hermite(y0: f32, m0: f32, y1: f32, m1: f32, s: f32) -> f32 {
    let (s2, s3) = (s * s, s * s * s);
    (2.0 * s3 - 3.0 * s2 + 1.0) * y0
        + (s3 - 2.0 * s2 + s) * m0
        + (-2.0 * s3 + 3.0 * s2) * y1
        + (s3 - s2) * m1
}

/// The area under [`hermite`] from 0 to `s`.
fn hermite_area(y0: f32, m0: f32, y1: f32, m1: f32, s: f32) -> f32 {
    let (s2, s3, s4) = (s * s, s * s * s, s * s * s * s);
    (s - s3 + s4 / 2.0) * y0
        + (s2 / 2.0 - 2.0 * s3 / 3.0 + s4 / 4.0) * m0
        + (s3 - s4 / 2.0) * y1
        + (s4 / 4.0 - s3 / 3.0) * m1
}

/// One leg of a path: from one key's offset to the next on a cubic Bezier
/// (`ctrl` its inner control points, on the start's heading), each angle
/// in a line, at its step's pace. `arc` is the curve's length up to each
/// of [`ARC_SAMPLES`] even steps of its parameter, so it is walked by
/// distance and the pace is a real speed along it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Leg {
    pub(crate) from: Offset,
    pub(crate) to: Offset,
    ctrl: [Vec3; 2],
    pub(crate) seconds: f32,
    dot: (f32, f32),
    arc: [f32; ARC_SAMPLES + 1],
}

impl Leg {
    /// The curve's point at parameter `s` (0–1): where a drawing samples
    /// it.
    pub(crate) fn point(&self, s: f32) -> Vec3 {
        let (p0, p3) = (self.from.pos, self.to.pos);
        let [p1, p2] = self.ctrl;
        let r = 1.0 - s;
        p0 * (r * r * r) + p1 * (3.0 * r * r * s) + p2 * (3.0 * r * s * s) + p3 * (s * s * s)
    }

    /// The point `f` (0–1) of the way along the curve by distance.
    pub(crate) fn along(&self, f: f32) -> Vec3 {
        let total = self.arc[ARC_SAMPLES];
        if total <= f32::EPSILON {
            return self.from.pos.lerp(self.to.pos, f);
        }
        let want = f.clamp(0.0, 1.0) * total;
        let j = self.arc[1..]
            .iter()
            .position(|a| *a >= want)
            .unwrap_or(ARC_SAMPLES - 1);
        let span = self.arc[j + 1] - self.arc[j];
        let part = if span > 0.0 {
            (want - self.arc[j]) / span
        } else {
            0.0
        };
        self.point((j as f32 + part) / ARC_SAMPLES as f32)
    }

    /// How far along (0–1) the leg is `t` (0–1) of its time in, by its pace.
    pub(crate) fn reached(&self, t: f32) -> f32 {
        progress(self.dot.0, self.dot.1, t)
    }

    /// Where the leg is `t` (0–1) of its time in: each angle in a line by
    /// the same progress, so the roll of keys without one stays 0 between
    /// them, and a turn past 180° goes the whole way.
    pub(crate) fn at(&self, t: f32) -> Offset {
        let f = self.reached(t);
        Offset {
            pos: self.along(f),
            turn: self.from.turn.lerp(self.to.turn, f),
        }
    }
}

/// The automatic handle direction at each key of a path through `points`,
/// joined by `steps`: the mean of the unit chords to its neighbours along
/// steps that move and take time (a jump breaks the curve); zero with none.
/// A `closed` path's start and end are one key, neighbours of both ends.
fn handle_dirs(points: &[Vec3], steps: &[Step], closed: bool) -> Vec<Vec3> {
    let chord = |i: usize| -> Option<Vec3> {
        let d = points[i + 1] - points[i];
        (steps[i][6] > 0.0 && d.length() > 1e-6).then(|| d.normalize())
    };
    let n = steps.len();
    (0..points.len())
        .map(|k| {
            let into = match k {
                0 if closed && n > 0 => chord(n - 1),
                _ => k.checked_sub(1).and_then(chord),
            };
            let out = match k {
                _ if k == n && closed && n > 0 => chord(0),
                _ => (k < n).then(|| chord(k)).flatten(),
            };
            (into.unwrap_or(Vec3::ZERO) + out.unwrap_or(Vec3::ZERO)).normalize_or_zero()
        })
        .collect()
}

/// A path playing from the pose it started at.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Play {
    /// The coroutine call it answers when it ends; none for the page's.
    pub(crate) call: Option<u64>,
    /// The track it plays, by key, when it plays one from the library.
    pub(crate) track: Option<String>,
    legs: Vec<Leg>,
    /// The start pose's heading: the offsets move on its axes.
    frame: Quat,
    /// The leg it travels.
    index: usize,
    /// Seconds along it.
    elapsed: f32,
    /// The offset it had reached at the last step.
    at: Offset,
    /// It goes round again from its first leg instead of ending.
    repeat: bool,
    /// Each key's optics, eased between them as it goes; none leaves the
    /// camera's settings alone.
    optics: Vec<Optics>,
}

impl Play {
    pub(crate) fn new(call: Option<u64>, legs: Vec<Leg>, start: &[f32; 6]) -> Self {
        Self {
            call,
            track: None,
            legs,
            frame: facing(start),
            index: 0,
            elapsed: 0.0,
            at: Offset::ZERO,
            repeat: false,
            optics: Vec::new(),
        }
    }

    /// `steps` from `start`.
    pub(crate) fn path(call: Option<u64>, steps: &[Step], start: &[f32; 6]) -> Self {
        Self::path_at(call, steps, false, 0, 0.0, start)
    }

    /// `steps` (`closed` when its end is its start) from `elapsed` seconds
    /// into step `index`, the camera at `pose` there: on the heading its
    /// start would have had to reach `pose` by then.
    pub(crate) fn path_at(
        call: Option<u64>,
        steps: &[Step],
        closed: bool,
        index: usize,
        elapsed: f32,
        pose: &[f32; 6],
    ) -> Self {
        let legs = legs(steps, closed);
        let reached = match legs.get(index) {
            Some(leg) if leg.seconds > 0.0 => leg.at(elapsed / leg.seconds),
            Some(leg) => leg.from,
            None => legs.last().map_or(Offset::ZERO, |l| l.to),
        };
        Self {
            call,
            track: None,
            frame: euler(0.0, pose[4] - reached.turn.y, 0.0),
            legs,
            index,
            elapsed,
            at: reached,
            repeat: false,
            optics: Vec::new(),
        }
    }

    /// It eases the camera's optics through `optics`, one a key.
    pub(crate) fn with_optics(self, optics: Vec<Optics>) -> Self {
        Self { optics, ..self }
    }

    /// The optics of the key it last reached, whole: a key's optics apply
    /// as the play reaches it, never eased between keys (VRChat stepped a
    /// pose stream beside an eased zoom). None without any.
    pub(crate) fn optics(&self) -> Option<Optics> {
        self.optics.get(self.index).copied()
    }

    /// It goes round again from its first leg instead of ending.
    pub(crate) fn repeating(self) -> Self {
        Self {
            repeat: true,
            ..self
        }
    }

    /// One leg from `start` to the pose `to`, in `seconds`, each angle
    /// turning the short way.
    pub(crate) fn to(call: Option<u64>, to: &[f32; 6], seconds: f32, start: &[f32; 6]) -> Self {
        Self::new(
            call,
            legs(&[step_to(start, &[], to, seconds)], false),
            start,
        )
    }

    /// One step of `dt` seconds: the world-space move and the change of
    /// pitch, yaw and roll its path made since the last step; none once it
    /// has ended. Time past a leg's end runs on into the next legs (a jump
    /// at once), so every key is reached at its time, never a tick late.
    pub(crate) fn step(&mut self, dt: f32) -> Option<(Vec3, Vec3)> {
        self.legs.get(self.index)?;
        self.elapsed += dt;
        let mut to = self.at;
        for _ in 0..=self.legs.len() {
            let Some(leg) = self.legs.get(self.index) else {
                break;
            };
            if leg.seconds > 0.0 && self.elapsed < leg.seconds - TIME_SLACK {
                to = leg.at(self.elapsed / leg.seconds);
                break;
            }
            to = leg.to;
            self.elapsed = (self.elapsed - leg.seconds).max(0.0);
            self.index += 1;
            if self.repeat && self.index == self.legs.len() {
                self.index = 0;
            }
        }
        let moved = self.frame * (to.pos - self.at.pos);
        let turned = to.turn - self.at.turn;
        self.at = to;
        Some((moved, turned))
    }

    pub(crate) fn ended(&self) -> bool {
        self.legs.is_empty() || (!self.repeat && self.index >= self.legs.len())
    }

    /// The step it is on, and the seconds into it.
    pub(crate) fn place(&self) -> (usize, f32) {
        (self.index, self.elapsed)
    }

    /// It plays library track `key`.
    pub(crate) fn of_track(self, key: String) -> Self {
        Self {
            track: Some(key),
            ..self
        }
    }

    /// The seconds it has played.
    pub(crate) fn at(&self) -> f32 {
        let before: f32 = self.legs[..self.index.min(self.legs.len())]
            .iter()
            .map(|l| l.seconds)
            .sum();
        before + self.elapsed
    }
}

/// Each key's offset on a path, from its start (key 0) to its end.
pub(crate) fn key_offsets(steps: &[Step]) -> Vec<Offset> {
    let mut at = Offset::ZERO;
    std::iter::once(at)
        .chain(steps.iter().map(|s| {
            at = at.then(s);
            at
        }))
        .collect()
}

/// The legs of a path (`closed` when its end is its start), each measured
/// along its length ([`shapes`]) so a play walks it at its pace.
pub(crate) fn legs(steps: &[Step], closed: bool) -> Vec<Leg> {
    let mut legs = shapes(steps, closed);
    for leg in &mut legs {
        let mut last = leg.from.pos;
        for j in 1..=ARC_SAMPLES {
            let p = leg.point(j as f32 / ARC_SAMPLES as f32);
            leg.arc[j] = leg.arc[j - 1] + p.distance(last);
            last = p;
        }
    }
    legs
}

/// Each step's curve between its keys, unmeasured: for drawing by
/// [`Leg::point`]. A handle a third of the chord long blends from the
/// chord (length 0: the leg's straight line, at an even parameter) to the
/// key's automatic direction (length 1); a jump draws straight.
pub(crate) fn shapes(steps: &[Step], closed: bool) -> Vec<Leg> {
    let keys = key_offsets(steps);
    let points: Vec<Vec3> = keys.iter().map(|o| o.pos).collect();
    let dirs = handle_dirs(&points, steps, closed);
    steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (from, to) = (keys[i], keys[i + 1]);
            let chord = to.pos - from.pos;
            let unit = chord.normalize_or_zero();
            let reach = chord.length() / 3.0;
            let handle = |dir: Vec3, len: f32| {
                let dir = if dir == Vec3::ZERO { unit } else { dir };
                (unit * (1.0 - len) + dir * len) * reach
            };
            let ctrl = match s[6] > 0.0 {
                true => [
                    from.pos + handle(dirs[i], s[OUT_LEN]),
                    to.pos - handle(dirs[i + 1], s[IN_LEN]),
                ],
                false => [from.pos + chord / 3.0, to.pos - chord / 3.0],
            };
            Leg {
                from,
                to,
                ctrl,
                seconds: s[6],
                dot: (s[DOT_AT], s[DOT_PACE]),
                arc: [0.0; ARC_SAMPLES + 1],
            }
        })
        .collect()
}

/// A rotation from pitch, yaw and roll in degrees, in `Pose`'s order.
pub(crate) fn euler(pitch: f32, yaw: f32, roll: f32) -> Quat {
    Quat::from_euler(
        EulerRot::ZXYEx,
        roll.to_radians(),
        pitch.to_radians(),
        yaw.to_radians(),
    )
}

/// Pitch, yaw and roll in degrees.
pub(crate) fn angles(q: Quat) -> [f32; 3] {
    let (roll, pitch, yaw) = q.to_euler(EulerRot::ZXYEx);
    [pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees()]
}

/// A pose from a position and a rotation.
pub(crate) fn pose_of(pos: Vec3, rot: Quat) -> [f32; 6] {
    let [pitch, yaw, roll] = angles(rot);
    [pos.x, pos.y, pos.z, pitch, yaw, roll]
}

pub(crate) fn position(pose: &[f32; 6]) -> Vec3 {
    Vec3::new(pose[0], pose[1], pose[2])
}

pub(crate) fn rotation(pose: &[f32; 6]) -> Quat {
    euler(pose[3], pose[4], pose[5])
}

/// Whether two poses are one place, within what VRChat's reports round
/// off: a centimetre, and about half a degree of view or up.
pub(crate) fn same_place(a: &[f32; 6], b: &[f32; 6]) -> bool {
    let (ra, rb) = (rotation(a), rotation(b));
    position(a).distance(position(b)) < 0.01
        && [Vec3::Y, Vec3::Z]
            .into_iter()
            .all(|v| (ra * v).distance(rb * v) < 0.01)
}

/// The pose's heading: its yaw alone.
pub(crate) fn facing(pose: &[f32; 6]) -> Quat {
    euler(0.0, pose[4], 0.0)
}

/// `degrees` in -180..180.
fn wrapped(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

/// `to` with each angle moved by whole turns to within half a turn of
/// `from`'s: a turn past 180° or 360° goes on past it instead of jumping a
/// turn back, which VRChat's smoothing would ease the long way round.
pub(crate) fn continuing(from: &[f32; 6], mut to: [f32; 6]) -> [f32; 6] {
    for k in 3..6 {
        to[k] = from[k] + wrapped(to[k] - from[k]);
    }
    to
}

/// `pose` with each angle within -180..180.
pub(crate) fn in_range(mut pose: [f32; 6]) -> [f32; 6] {
    for angle in &mut pose[3..] {
        *angle = wrapped(*angle);
    }
    pose
}

/// The step from the end of `steps`, played from `start`, to the world
/// pose `to`, in `seconds`: its move on the start's heading, and each
/// angle turning the short way.
pub(crate) fn step_to(start: &[f32; 6], steps: &[Step], to: &[f32; 6], seconds: f32) -> Step {
    let end = key_offsets(steps)[steps.len()];
    let pos = facing(start).inverse() * (position(to) - position(start)) - end.pos;
    // Both through one rotation, so the same orientation always reads the
    // same angles whatever form VRChat sent them in.
    let (from, to) = (angles(rotation(start)), angles(rotation(to)));
    let turn: [f32; 3] = std::array::from_fn(|k| wrapped(to[k] - from[k] - end.turn[k]));
    with_curve(
        [
            pos.x, pos.y, pos.z, turn[0], turn[1], turn[2], seconds, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0,
        ],
        LINEAR,
    )
}

/// The world pose `elapsed` seconds into step `index` of `steps` (`closed`
/// when its end is its start) played from `start`.
pub(crate) fn pose_at(
    start: &[f32; 6],
    steps: &[Step],
    closed: bool,
    index: usize,
    elapsed: f32,
) -> [f32; 6] {
    let legs = legs(steps, closed);
    let o = match legs.get(index) {
        Some(leg) if leg.seconds > 0.0 => leg.at((elapsed / leg.seconds).min(1.0)),
        Some(leg) => leg.to,
        None => return key_pose(start, steps, index),
    };
    let at = position(start) + facing(start) * o.pos;
    let turn = Vec3::new(start[3], start[4], start[5]) + o.turn;
    [at.x, at.y, at.z, turn.x, turn.y, turn.z]
}

/// The step `seconds` into `steps` falls in and how far into it, held to
/// the path.
pub(crate) fn place_at(steps: &[Step], seconds: f32) -> (usize, f32) {
    let times = key_times(steps);
    let i = (0..steps.len())
        .rfind(|i| times[*i] <= seconds)
        .unwrap_or(0);
    let into = (seconds - times[i]).clamp(0.0, steps.get(i).map_or(0.0, |s| s[6]));
    (i, into)
}

/// Key `key`'s world pose on `steps` played from `start` (key 0 is the
/// start, key `n` the end of step `n - 1`).
pub(crate) fn key_pose(start: &[f32; 6], steps: &[Step], key: usize) -> [f32; 6] {
    let o = key_offsets(steps)[key.min(steps.len())];
    let at = position(start) + facing(start) * o.pos;
    let turn = Vec3::new(start[3], start[4], start[5]) + o.turn;
    [at.x, at.y, at.z, turn.x, turn.y, turn.z]
}

/// Adds `sign` times `change` to the move and turn of `step`, its seconds
/// kept.
fn shift(step: &mut Step, change: &[f32; 6], sign: f32) {
    for (v, c) in step.iter_mut().zip(change) {
        *v += sign * c;
    }
}

/// Key `key` (1 or more) of `steps` played from `start` moved to the world
/// pose `to`, every other key kept where it is: the step into it takes the
/// change, the step out of it gives it back, so a turn typed past 180°
/// keeps its whole turns.
pub(crate) fn set_key(start: &[f32; 6], steps: &mut [Step], key: usize, to: &[f32; 6]) {
    let Some(i) = key.checked_sub(1).filter(|i| *i < steps.len()) else {
        return;
    };
    let new = step_to(start, &steps[..i], to, steps[i][6]);
    let change: [f32; 6] = std::array::from_fn(|k| new[k] - steps[i][k]);
    shift(&mut steps[i], &change, 1.0);
    if let Some(next) = steps.get_mut(i + 1) {
        shift(next, &change, -1.0);
    }
}

/// A key at the world pose `to` inserted after key `key` of `steps`
/// played from `start`, every other key kept where it is and when it is:
/// the new step, of `curve`, takes half the following step's seconds, or
/// `seconds` after the last key.
pub(crate) fn insert_key(
    start: &[f32; 6],
    steps: &mut Vec<Step>,
    key: usize,
    to: &[f32; 6],
    seconds: f32,
    curve: Curve,
) {
    let at = key.min(steps.len());
    let mut new = with_curve(step_to(start, &steps[..at], to, seconds), curve);
    if let Some(next) = steps.get_mut(at) {
        let change: [f32; 6] = std::array::from_fn(|k| new[k]);
        shift(next, &change, -1.0);
        new[6] = next[6] / 2.0;
        next[6] -= new[6];
    }
    steps.insert(at, new);
}

/// `steps` played from `from` rewritten to play from `to`, every key after
/// the start kept where it is: each move turns onto `to`'s heading, and the
/// first step takes the change of place and angles.
pub(crate) fn restart(steps: &mut [Step], from: &[f32; 6], to: &[f32; 6]) {
    let onto = facing(to).inverse() * facing(from);
    for s in steps.iter_mut() {
        let m = onto * Vec3::new(s[0], s[1], s[2]);
        s[..3].copy_from_slice(&m.to_array());
    }
    if let Some(first) = steps.first_mut() {
        let moved = facing(to).inverse() * (position(from) - position(to));
        let (a, b) = (angles(rotation(from)), angles(rotation(to)));
        let turned: [f32; 3] = std::array::from_fn(|k| wrapped(a[k] - b[k]));
        let change = [moved.x, moved.y, moved.z, turned[0], turned[1], turned[2]];
        shift(first, &change, 1.0);
    }
}

/// A step's values within their ranges; its optics are held to theirs as
/// they go to VRChat.
pub(crate) fn clamp_step(mut s: Step) -> Step {
    for (k, v) in s[..WIRE].iter_mut().enumerate() {
        let (low, high) = match k {
            0..=2 => (-STEP_REACH, STEP_REACH),
            3..=5 => (-STEP_TURN, STEP_TURN),
            6 => (0.0, STEP_SECONDS),
            OUT_LEN | IN_LEN => (0.0, HANDLE_MAX),
            DOT_AT => (0.0, 1.0),
            _ => PACE_RANGE,
        };
        *v = match v.is_finite() {
            true => v.clamp(low as f32, high as f32),
            false => 0.0,
        };
    }
    s
}

/// A way a held pad flies the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fly {
    Forward,
    Back,
    Left,
    Right,
    Rise,
    Sink,
    PitchUp,
    PitchDown,
    YawLeft,
    YawRight,
    RollLeft,
    RollRight,
}

impl Fly {
    /// The toggle (a `TOGGLES` field) under which VRChat levels what this
    /// pad turns, undoing it.
    pub(crate) fn leveled_by(self) -> Option<&'static str> {
        match self {
            Fly::PitchUp | Fly::PitchDown => Some("auto_level_pitch"),
            Fly::RollLeft | Fly::RollRight => Some("auto_level_roll"),
            _ => None,
        }
    }

    /// Each pad: its id on a view (`fly:<id>`), its icon, its tip and the
    /// key that holds it.
    pub(crate) const PADS: [(Fly, &'static str, &'static str, &'static str, Key); 12] = [
        (Fly::Forward, "forward", "up", "Forward", Key::Char('w')),
        (Fly::Back, "back", "down", "Back", Key::Char('s')),
        (Fly::Left, "left", "left", "Left", Key::Char('a')),
        (Fly::Right, "right", "right", "Right", Key::Char('d')),
        (Fly::Rise, "rise", "rise", "Up", Key::Space),
        (Fly::Sink, "sink", "sink", "Down", Key::Shift),
        (Fly::PitchUp, "pitch_up", "tilt_up", "Tilt up", Key::Up),
        (
            Fly::PitchDown,
            "pitch_down",
            "tilt_down",
            "Tilt down",
            Key::Down,
        ),
        (Fly::YawLeft, "yaw_left", "pan_left", "Turn left", Key::Left),
        (
            Fly::YawRight,
            "yaw_right",
            "pan_right",
            "Turn right",
            Key::Right,
        ),
        (
            Fly::RollLeft,
            "roll_left",
            "roll_left",
            "Roll left",
            Key::Char('q'),
        ),
        (
            Fly::RollRight,
            "roll_right",
            "roll_right",
            "Roll right",
            Key::Char('e'),
        ),
    ];

    /// The pad `id` names.
    pub(crate) fn of(id: &str) -> Option<Fly> {
        Self::PADS.iter().find(|p| p.1 == id).map(|p| p.0)
    }

    /// Its move (right, up, forward: on the camera's axes when navigating
    /// locally, else level on its heading) and its turn (pitch and roll on
    /// the camera's axes, yaw about the world's vertical), each a unit a
    /// second.
    fn rate(self) -> (Vec3, [f32; 3]) {
        let mv = |x, y, z| (Vec3::new(x, y, z), [0.0; 3]);
        let turn = |p, y, r| (Vec3::ZERO, [p, y, r]);
        match self {
            Fly::Forward => mv(0.0, 0.0, 1.0),
            Fly::Back => mv(0.0, 0.0, -1.0),
            Fly::Left => mv(-1.0, 0.0, 0.0),
            Fly::Right => mv(1.0, 0.0, 0.0),
            Fly::Rise => mv(0.0, 1.0, 0.0),
            Fly::Sink => mv(0.0, -1.0, 0.0),
            Fly::PitchUp => turn(-1.0, 0.0, 0.0),
            Fly::PitchDown => turn(1.0, 0.0, 0.0),
            Fly::YawLeft => turn(0.0, -1.0, 0.0),
            Fly::YawRight => turn(0.0, 1.0, 0.0),
            Fly::RollLeft => turn(0.0, 0.0, 1.0),
            Fly::RollRight => turn(0.0, 0.0, -1.0),
        }
    }
}

/// The pads' move speed in metres a second, and their turn speed in
/// degrees a second: (lowest, highest, at first). Both are set on a slider
/// spread by ratio.
pub(crate) const MOVE_SPEED: (f32, f32, f32) = (0.01, 100.0, 2.0);
pub(crate) const TURN_SPEED: (f32, f32, f32) = (0.1, 360.0, 45.0);

/// How much one wheel line, while a pad is held, multiplies the move speed.
pub(crate) const SPEED_PER_LINE: f32 = 1.25;

/// When each point of a path is reached, from its start: 0, then the end of
/// each step.
pub(crate) fn key_times(steps: &[Step]) -> Vec<f32> {
    let mut at = 0.0;
    std::iter::once(0.0)
        .chain(steps.iter().map(|s| {
            at += s[6];
            at
        }))
        .collect()
}

impl Camera {
    /// Starts the play `make` builds from the camera's pose. A play with no
    /// legs answers at once; one with legs always runs — on VRChat's camera
    /// while it reports one, else on the stand-in — so a mapping's play never
    /// depends on the game being open.
    pub(crate) fn start(
        &mut self,
        call: Option<u64>,
        make: impl FnOnce(&[f32; 6]) -> Play,
        out: &mut Vec<Command>,
    ) {
        let play = make(&self.pose);
        if play.legs.is_empty() {
            out.extend(call.map(|id| Command::Reply {
                id,
                value: Value::None,
            }));
            return;
        }
        self.wind(out);
        self.plays.push(play);
    }

    /// Starts the clock if it isn't running.
    pub(crate) fn wind(&mut self, out: &mut Vec<Command>) {
        if !self.ticking {
            self.ticking = true;
            // Its first tick sends the pose.
            self.beat = 0;
            out.push(Command::Timer {
                id: PLAY.to_owned(),
                after_ms: 0,
            });
        }
    }

    /// Stops the clock.
    fn unwind(&mut self, out: &mut Vec<Command>) {
        self.release_pose(out);
        self.rest();
        out.push(Command::CancelTimer {
            id: PLAY.to_owned(),
        });
    }

    /// The clock stops. Angles a turn took past a whole turn come back in
    /// range: a stand-in's here, a linked camera's as VRChat next reports
    /// it once it settles.
    fn rest(&mut self) {
        self.ticking = false;
        self.last_tick = None;
        if !self.linked {
            self.pose = in_range(self.pose);
        }
    }

    /// A pad went down (`down`) or was let go: the camera flies that way
    /// while it is held.
    pub(crate) fn fly_held(&mut self, fly: Fly, down: bool, out: &mut Vec<Command>) {
        match down {
            true if !self.flying.contains(&fly) => {
                self.wind(out);
                self.flying.push(fly);
            }
            true => {}
            false => self.flying.retain(|f| *f != fly),
        }
    }

    /// One step of every play, held pad and look, added up into one pose
    /// sent at `at_ms` (the tick's time), as far as the time since the last
    /// tick; a play that ended answers its call. The clock runs while
    /// anything moves.
    pub(crate) fn step(&mut self, at_ms: u64, out: &mut Vec<Command>) {
        let dt = match self.last_tick {
            Some(last) if at_ms > last => (at_ms - last).min(STEP_MAX_MS) as f32 / 1000.0,
            _ => FRAME,
        };
        self.last_tick = Some(at_ms);
        if !self.plays.is_empty() || !self.flying.is_empty() || self.look != [0.0; 2] {
            self.move_step(dt, out);
        }
        self.follow_step(dt, out);
        let stops = self.plays.is_empty() && self.flying.is_empty() && self.follow.is_none();
        self.beat = (self.beat + 1) % SETTINGS_TICKS;
        let settings = self.beat == 0 && !self.sliders_due.is_empty();
        if stops || settings {
            self.resting = 0;
            self.send_settings(out);
        }
        if stops || !settings {
            self.release_pose(out);
        }
        match stops {
            true => {
                self.rest();
                self.rest_later(out);
            }
            false => out.push(Command::Timer {
                id: PLAY.to_owned(),
                after_ms: 0,
            }),
        }
    }

    /// `dt` seconds of every play, held pad and look, sent as one pose.
    fn move_step(&mut self, dt: f32, out: &mut Vec<Command>) {
        let pose = self.pose;
        let (mut pos, mut turn) = (position(&pose), Vec3::new(pose[3], pose[4], pose[5]));
        for play in &mut self.plays {
            if let Some((moved, turned)) = play.step(dt) {
                pos += moved;
                turn += turned;
            }
        }
        if let Some(o) = self.plays.iter().rev().find_map(Play::optics) {
            self.set_optics(o, out);
        }
        let mut pose = [pos.x, pos.y, pos.z, turn.x, turn.y, turn.z];
        let [right, down] = std::mem::take(&mut self.look).map(|px| px * LOOK_PER_PX);
        if !self.flying.is_empty() || right != 0.0 || down != 0.0 {
            let (metres, degrees) = (self.move_speed, self.turn_speed);
            let mut rot = rotation(&pose);
            for fly in &self.flying {
                let (along, [pitch, yaw, roll]) = fly.rate();
                // Level, a move keeps its height: forward and right lie on
                // the heading, up is the world's.
                let axes = match self.local_nav {
                    true => rot,
                    false => euler(0.0, angles(rot)[1], 0.0),
                };
                pos += axes * along * metres * dt;
                let [pitch, yaw, roll] = [pitch, yaw, roll].map(|d| d * degrees * dt);
                rot = (euler(0.0, yaw, 0.0) * rot * euler(pitch, 0.0, roll)).normalize();
            }
            rot = (euler(0.0, right, 0.0) * rot * euler(down, 0.0, 0.0)).normalize();
            pose = pose_of(pos, rot);
        }
        let (ended, going): (Vec<Play>, Vec<Play>) = std::mem::take(&mut self.plays)
            .into_iter()
            .partition(Play::ended);
        self.plays = going;
        for play in ended {
            if self
                .resume
                .as_ref()
                .is_some_and(|r| play.track.as_ref() == Some(&r.track))
            {
                self.resume = None;
            }
            out.push(Command::Signal {
                name: PLAY_ENDED.to_owned(),
                value: Value::None,
            });
            out.extend(play.call.map(|id| Command::Reply {
                id,
                value: Value::None,
            }));
        }
        self.send_pose(pose, out);
    }

    /// Ends every play, answering their calls; the clock stops unless a
    /// pad is held.
    pub(crate) fn end_plays(&mut self, out: &mut Vec<Command>) {
        self.end_plays_where(|_| true, out);
    }

    /// Ends the plays `hit` picks, answering their calls; the person's
    /// play of a track is kept to go on from. The clock stops once nothing
    /// runs on it.
    pub(crate) fn end_plays_where(&mut self, hit: impl Fn(&Play) -> bool, out: &mut Vec<Command>) {
        let (ended, going): (Vec<Play>, Vec<Play>) = std::mem::take(&mut self.plays)
            .into_iter()
            .partition(|p| hit(p));
        self.plays = going;
        for play in ended {
            match (play.call, &play.track) {
                (Some(id), _) => out.push(Command::Reply {
                    id,
                    value: Value::None,
                }),
                (None, Some(track)) => {
                    let (step, elapsed) = play.place();
                    self.resume = Some(Resume {
                        track: track.clone(),
                        step,
                        elapsed,
                        pose: Some(self.pose),
                    })
                }
                (None, None) => {}
            }
        }
        if self.plays.is_empty() && self.flying.is_empty() && self.follow.is_none() && self.ticking
        {
            self.unwind(out);
        }
    }
}
