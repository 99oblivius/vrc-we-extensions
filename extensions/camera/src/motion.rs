//! Camera motion: paths, plays along them, smoothing, and flying by hand.

use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};
use vrc_we_sdk::{Command, Key, Value, TICK_HZ};

use crate::{Camera, PLAY, PLAY_ENDED, SETTINGS_TICKS};

/// The limits of a path: its number of steps, and the metres, degrees and
/// seconds of one step.
pub(crate) const PATH_MAX: usize = 1024;
pub(crate) const STEP_REACH: f64 = 1000.0;
pub(crate) const STEP_TURN: f64 = 3600.0;
pub(crate) const STEP_SECONDS: f64 = 600.0;

/// The duration of one tick, for a step of the clock with no previous tick.
pub(crate) const FRAME: f32 = 1.0 / TICK_HZ as f32;

/// The most time one tick advances, in milliseconds, so that a stall never
/// makes the camera jump.
const STEP_MAX_MS: u64 = 100;

/// Degrees the camera turns per pixel of a right-button drag.
const LOOK_PER_PX: f32 = 0.2;

/// Smoothing time constant: at VRChat's smoothing strength `s` it is
/// `SMOOTH_SECONDS.0 * s.powf(SMOOTH_SECONDS.1)` seconds.
pub(crate) const SMOOTH_SECONDS: (f32, f32) = (0.04, 1.4);

/// The smoothed pose sent to VRChat, which approaches the camera's pose
/// exponentially.
///
/// The approach has no momentum: it starts as soon as the pose moves, follows a
/// steady move at a constant distance, and stops without overshoot. Its angles
/// are continuous ([`continuing`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Follow {
    pub(crate) pose: [f32; 6],
}

impl Follow {
    pub(crate) fn at(pose: [f32; 6]) -> Self {
        Self { pose }
    }

    /// Advances `dt` seconds toward `to`, at `rate` per second.
    pub(crate) fn step(&mut self, to: &[f32; 6], rate: f32, dt: f32) {
        let left = (-rate * dt).exp();
        for (at, goal) in self.pose.iter_mut().zip(to) {
            *at = goal + (*at - goal) * left;
        }
    }

    /// Whether it is close enough to `to` (1 mm and 0.05°) to stop without a
    /// visible jump.
    pub(crate) fn settled(&self, to: &[f32; 6]) -> bool {
        position(&self.pose).distance(position(to)) < 1e-3
            && (3..6).all(|k| (self.pose[k] - to[k]).abs() < 0.05)
    }
}

/// Where playing a track resumes: the track's key, the step and the seconds
/// into it, and the camera's pose at the time. `pose` is `None` if a keyframe
/// was selected instead, so the camera starts by jumping to it.
///
/// The step is stored because the time alone is ambiguous next to steps of 0 s.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Resume {
    pub(crate) track: String,
    pub(crate) step: usize,
    pub(crate) elapsed: f32,
    pub(crate) pose: Option<[f32; 6]>,
}

impl Resume {
    /// The time along the track, in seconds.
    pub(crate) fn at(&self, steps: &[Step]) -> f32 {
        let before: f32 = steps.iter().take(self.step).map(|s| s[6]).sum();
        before + self.elapsed
    }
}

/// A step of a path: the move (right, up, forward) from the previous key
/// relative to the start's heading, the change of pitch, yaw and roll in
/// degrees, the duration in seconds (0 jumps), the curve ([`Curve`]), and the
/// camera's optics at the key it reaches ([`Optics`]).
///
/// Scripts and contacts only see the first [`WIRE`] values. The optics stay in
/// the library.
pub(crate) type Step = [f32; 15];
pub(crate) const WIRE: usize = 11;

/// Zoom, focal distance, aperture and exposure ([`OPTIC_FIELDS`]) at a key,
/// stored in a step from index [`OPTICS`].
pub(crate) type Optics = [f32; 4];
pub(crate) const OPTICS: usize = 11;

/// The curve of a step, stored from index [`CURVE`]: the handle lengths at the
/// key it leaves and the key it reaches (0 straight, 1 the automatic
/// direction), and the ramps at both ([`RAMP_RANGE`]). In a library track, the
/// two steps at a key share its ramp ([`join_ramps`]).
pub(crate) type Curve = [f32; 4];
pub(crate) const CURVE: usize = 7;
pub(crate) const OUT_LEN: usize = 7;
pub(crate) const IN_LEN: usize = 8;
pub(crate) const OUT_RAMP: usize = 9;
pub(crate) const IN_RAMP: usize = 10;

/// A straight step at an even pace.
pub(crate) const LINEAR: Curve = [0.0, 0.0, 0.0, 0.0];
/// The curve of a new key when none has been selected yet.
pub(crate) const SMOOTH: Curve = [1.0, 1.0, 0.0, 0.0];

/// The longest handle.
pub(crate) const HANDLE_MAX: f64 = 1.0;

/// The range of a ramp: the lowest stops, 0 is an even pace, the highest is the
/// fastest.
pub(crate) const RAMP_RANGE: (f64, f64) = (-100.0, 100.0);

/// The exponent that maps a ramp's height to a pace ([`pace_at`]).
const RAMP_POWER: f32 = 1.585;

/// The minimum pace between two keys that both stop, relative to the average.
const PACE_FLOOR: f32 = 0.02;

/// The number of samples of a step's pace.
const PACE_SAMPLES: usize = 32;

/// Tolerance when comparing a time to the end of a leg, for rounding errors of
/// summed ticks.
const TIME_SLACK: f32 = 1e-4;

/// The number of samples used to measure a curve's length.
const ARC_SAMPLES: usize = 32;

/// How a track ends: at its last key, back at its start (`Closed`), or back at
/// its start and over again (`Looping`). The last step of a closed or looping
/// track leads back to its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Loop {
    #[default]
    Open,
    Closed,
    Looping,
}

impl Loop {
    pub(crate) const ALL: [Loop; 3] = [Loop::Open, Loop::Closed, Loop::Looping];

    pub(crate) fn closed(self) -> bool {
        self != Loop::Open
    }
}

/// Makes the last of `steps` return to the start: its move and turn undo those
/// of all the others, and it reaches the start's optics if given.
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

/// Makes the steps at each key share its ramp. The ramp of the start is
/// `start`, which is also the ramp of the end if `closed`.
pub(crate) fn join_ramps(steps: &mut [Step], start: f32, closed: bool) {
    if let Some(last) = steps.last_mut().filter(|_| closed) {
        last[IN_RAMP] = start;
    }
    let mut ramp = start;
    for s in steps {
        s[OUT_RAMP] = ramp;
        ramp = s[IN_RAMP];
    }
}

/// The ramp at key `key` (0 is the start).
pub(crate) fn key_ramp(steps: &[Step], key: usize) -> f32 {
    match key.checked_sub(1) {
        Some(i) => steps.get(i).map_or(0.0, |s| s[IN_RAMP]),
        None => steps.first().map_or(0.0, |s| s[OUT_RAMP]),
    }
}

/// Parses a step: 7 values for a straight one at an even pace, [`WIRE`] for one
/// without optics, 15 for one with optics.
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

/// `step` with the given curve.
pub(crate) fn with_curve(mut step: Step, curve: Curve) -> Step {
    step[CURVE..WIRE].copy_from_slice(&curve);
    step
}

/// `step` with the given optics.
pub(crate) fn with_optics(mut step: Step, optics: Optics) -> Step {
    step[OPTICS..].copy_from_slice(&optics);
    step
}

/// The optics at each key, where `first` are those of the start.
pub(crate) fn key_optics(first: Optics, steps: &[Step]) -> Vec<Optics> {
    std::iter::once(first)
        .chain(steps.iter().map(|s| {
            let mut o = [0.0; 4];
            o.copy_from_slice(&s[OPTICS..]);
            o
        }))
        .collect()
}

/// A point on a path relative to its start: the position relative to the
/// start's heading, and the change of pitch, yaw and roll.
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

    /// This offset followed by `step`.
    pub(crate) fn then(self, step: &Step) -> Offset {
        Offset {
            pos: self.pos + Vec3::new(step[0], step[1], step[2]),
            turn: self.turn + Vec3::new(step[3], step[4], step[5]),
        }
    }
}

/// The height of a ramp, from 0 (stopped) over 0.5 (an even pace) to 1 (the
/// fastest).
pub(crate) fn height(ramp: f32) -> f32 {
    (ramp / 200.0 + 0.5).clamp(0.0, 1.0)
}

/// The ramp of a height.
pub(crate) fn ramp_at(height: f32) -> f32 {
    (height - 0.5) * 200.0
}

/// The pace at a ramp height, as a multiple of the even pace.
fn pace_at(height: f32) -> f32 {
    (2.0 * height.max(0.0)).powf(RAMP_POWER)
}

/// The ramp of a pace, given as a multiple of the even pace.
pub(crate) fn ramp_of_pace(pace: f32) -> f32 {
    ramp_at((pace.max(0.0).powf(1.0 / RAMP_POWER) / 2.0).min(1.0))
}

/// The ramp curve over a step: the heights at both ends, and the slopes there
/// in height per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ramp {
    pub(crate) heights: [f32; 2],
    pub(crate) slopes: [f32; 2],
}

impl Ramp {
    /// The height at `u` (0–1) of a step that takes `seconds`.
    pub(crate) fn at(&self, u: f32, seconds: f32) -> f32 {
        let ([from, to], [out, into]) = (self.heights, self.slopes);
        hermite(from, out * seconds, to, into * seconds, u)
    }
}

/// The steps before and after key `key`, if the pace continues from one to the
/// other: both take time and have the same ramp there. The start and end of a
/// `closed` path are the same key.
fn joined(steps: &[Step], key: usize, closed: bool) -> Option<(usize, usize)> {
    let n = steps.len();
    let (into, out) = match key {
        0 if closed && n > 1 => (n - 1, 0),
        k if k == n && closed && n > 1 => (n - 1, 0),
        k if k > 0 && k < n => (k - 1, k),
        _ => return None,
    };
    let (a, b) = (&steps[into], &steps[out]);
    (a[6] > 0.0 && b[6] > 0.0 && a[IN_RAMP] == b[OUT_RAMP]).then_some((into, out))
}

/// The ramp curve of each step, as cubic Hermite splines.
///
/// The slope at a key is the mean of the two adjacent chords, each weighted by
/// the other step's duration. It is limited to 3 times the shallower chord, or
/// half of it at a turning point, to avoid overshoot. An end that isn't joined
/// to another step takes its slope from the other end. All slopes are limited
/// so that the Bézier control points, and with them the curve, stay between 0
/// and 1.
pub(crate) fn ramps(steps: &[Step], closed: bool) -> Vec<Ramp> {
    let heights: Vec<[f32; 2]> = steps
        .iter()
        .map(|s| [height(s[OUT_RAMP]), height(s[IN_RAMP])])
        .collect();
    let chord = |i: usize| match steps[i][6] > 0.0 {
        true => (heights[i][1] - heights[i][0]) / steps[i][6],
        false => 0.0,
    };
    let at_key = |key: usize| {
        let (a, b) = joined(steps, key, closed)?;
        let (l, r, tl, tr) = (chord(a), chord(b), steps[a][6], steps[b][6]);
        let most = if l * r > 0.0 { 3.0 } else { 0.5 } * l.abs().min(r.abs());
        Some(((l * tr + r * tl) / (tl + tr)).clamp(-most, most))
    };
    let keys: Vec<Option<f32>> = (0..=steps.len()).map(at_key).collect();
    let free = |i: usize, other: Option<f32>| {
        let c = chord(i);
        let bent = other.map_or(c, |m| 1.5 * c - 0.5 * m);
        if bent * c < 0.0 {
            0.0
        } else {
            bent
        }
    };
    // Limited by the longer of the two steps at the key, so that both get the
    // same slope.
    let held = |key: usize, i: usize, slope: f32, h: f32| {
        let longest =
            joined(steps, key, closed).map_or(steps[i][6], |(a, b)| steps[a][6].max(steps[b][6]));
        let most = if longest > 0.0 {
            3.0 * h.min(1.0 - h) / longest
        } else {
            0.0
        };
        slope.clamp(-most, most)
    };
    (0..steps.len())
        .map(|i| {
            let (out, into) = (keys[i], keys[i + 1]);
            let h = heights[i];
            Ramp {
                heights: h,
                slopes: [
                    held(i, i, out.unwrap_or_else(|| free(i, into)), h[0]),
                    held(i + 1, i, into.unwrap_or_else(|| free(i, out)), h[1]),
                ],
            }
        })
        .collect()
}

/// The cubic Hermite spline from `y0` (slope `m0`) to `y1` (slope `m1`) at `s`
/// (0–1).
fn hermite(y0: f32, m0: f32, y1: f32, m1: f32, s: f32) -> f32 {
    let (s2, s3) = (s * s, s * s * s);
    (2.0 * s3 - 3.0 * s2 + 1.0) * y0
        + (s3 - 2.0 * s2 + s) * m0
        + (-2.0 * s3 + 3.0 * s2) * y1
        + (s3 - s2) * m1
}

/// One leg of a path, between two keys: a cubic Bézier curve in position
/// (`ctrl` are its inner control points), linear in each angle.
///
/// `arc` is the curve's length up to each of [`ARC_SAMPLES`] even parameter
/// steps. `pace` is the speed at each of [`PACE_SAMPLES`] even time steps, as a
/// multiple of the average, and `run` is the fraction of the leg covered by
/// then.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Leg {
    pub(crate) from: Offset,
    pub(crate) to: Offset,
    ctrl: [Vec3; 2],
    pub(crate) seconds: f32,
    arc: [f32; ARC_SAMPLES + 1],
    measured: bool,
    pace: [f32; PACE_SAMPLES + 1],
    run: [f32; PACE_SAMPLES + 1],
    paced: bool,
}

/// The quantity a leg's speed is measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    Metres,
    Degrees,
}

impl Leg {
    /// Measures the curve's length, once.
    fn measure(&mut self) {
        if std::mem::replace(&mut self.measured, true) {
            return;
        }
        let mut last = self.from.pos;
        for j in 1..=ARC_SAMPLES {
            let p = self.point(j as f32 / ARC_SAMPLES as f32);
            self.arc[j] = self.arc[j - 1] + p.distance(last);
            last = p;
        }
    }

    /// The distance the leg covers, or the angle if it only turns. `None` if it
    /// does neither or takes no time.
    fn reach(&self) -> Option<(Reach, f32)> {
        let (far, turned) = (self.arc[ARC_SAMPLES], self.from.turn.distance(self.to.turn));
        if self.seconds <= 0.0 {
            return None;
        }
        if far > 1e-4 {
            return Some((Reach::Metres, far));
        }
        (turned > 1e-3).then_some((Reach::Degrees, turned))
    }

    /// Computes the pace: the pace of `ramp`, scaled by `keys` (the paces at
    /// both keys on an even ramp, blended along the leg) and by a bump in the
    /// middle, solved so that the average is 1. The leg therefore always takes
    /// its duration, and the pace at a key is the same in both of its legs.
    fn set_pace(&mut self, ramp: &Ramp, keys: [f32; 2]) {
        let even = ramp.heights == [0.5; 2] && ramp.slopes == [0.0; 2];
        if self.seconds <= 0.0 || (even && keys == [1.0; 2]) {
            return;
        }
        let part = |j: usize| j as f32 / PACE_SAMPLES as f32;
        let swell = |u: f32| 16.0 * (u * (1.0 - u)).powi(2);
        let (from, to) = (keys[0].ln(), keys[1].ln());
        let base: [f32; PACE_SAMPLES + 1] = std::array::from_fn(|j| {
            let u = part(j);
            let along = (from + (to - from) * u * u * (3.0 - 2.0 * u)).exp();
            along * (pace_at(ramp.at(u, self.seconds)) + PACE_FLOOR * swell(u))
        });
        let swells: [f32; PACE_SAMPLES + 1] = std::array::from_fn(|j| swell(part(j)));
        let weight = |j: usize| match j {
            0 | PACE_SAMPLES => 0.5 / PACE_SAMPLES as f32,
            _ => 1.0 / PACE_SAMPLES as f32,
        };
        // The logarithm of the average is convex in `k`, so Newton's method
        // converges.
        let mut k = 0.0f32;
        for _ in 0..16 {
            let (mut mean, mut rise) = (0.0, 0.0);
            for j in 0..=PACE_SAMPLES {
                let v = weight(j) * base[j] * (k * swells[j]).exp();
                mean += v;
                rise += v * swells[j];
            }
            let off = mean.ln();
            if off.abs() < 1e-4 {
                break;
            }
            k = (k - (off * mean / rise).clamp(-8.0, 8.0)).clamp(-60.0, 60.0);
        }
        let pace: [f32; PACE_SAMPLES + 1] =
            std::array::from_fn(|j| base[j] * (k * swells[j]).exp());
        let mean: f32 = (0..=PACE_SAMPLES).map(|j| weight(j) * pace[j]).sum();
        self.pace = pace.map(|v| v / mean);
        for j in 1..=PACE_SAMPLES {
            let mid = (self.pace[j - 1] + self.pace[j]) / 2.0;
            self.run[j] = self.run[j - 1] + mid / PACE_SAMPLES as f32;
        }
    }

    /// The point at curve parameter `s` (0–1).
    pub(crate) fn point(&self, s: f32) -> Vec3 {
        let (p0, p3) = (self.from.pos, self.to.pos);
        let [p1, p2] = self.ctrl;
        let r = 1.0 - s;
        p0 * (r * r * r) + p1 * (3.0 * r * r * s) + p2 * (3.0 * r * s * s) + p3 * (s * s * s)
    }

    /// The point at the fraction `f` (0–1) of the curve's length.
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

    /// The fraction of the leg covered after the fraction `t` of its duration.
    pub(crate) fn reached(&self, t: f32) -> f32 {
        let at = t.clamp(0.0, 1.0) * PACE_SAMPLES as f32;
        let j = (at as usize).min(PACE_SAMPLES - 1);
        let (f, v) = (at - j as f32, &self.pace[j..=j + 1]);
        let more = v[0] * f + (v[1] - v[0]) * f * f / 2.0;
        (self.run[j] + more / PACE_SAMPLES as f32).min(1.0)
    }

    /// The offset after the fraction `t` of the leg's duration. Angles are
    /// interpolated linearly, so a turn past 180° isn't shortened and the roll
    /// between keys without roll stays 0.
    pub(crate) fn at(&self, t: f32) -> Offset {
        let f = self.reached(t);
        Offset {
            pos: self.along(f),
            turn: self.from.turn.lerp(self.to.turn, f),
        }
    }
}

/// The automatic handle direction at each key: the mean of the unit chords to
/// its neighbors, along steps that move and take time, or zero. The start and
/// end of a `closed` path are the same key.
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

/// A path being played.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Play {
    /// The call it answers when it ends, if any.
    pub(crate) call: Option<u64>,
    /// The key of the library track it plays, if any.
    pub(crate) track: Option<String>,
    path: Path,
    /// The heading of the start pose, which offsets are relative to.
    frame: Quat,
    /// The current leg.
    index: usize,
    /// Seconds into the current leg.
    elapsed: f32,
    /// The offset at the last tick.
    at: Offset,
    /// Starts over instead of ending.
    repeat: bool,
    /// The optics at each key. Empty if the play doesn't change them.
    optics: Vec<Optics>,
}

impl Play {
    fn new(call: Option<u64>, path: Path, start: &[f32; 6]) -> Self {
        Self {
            call,
            track: None,
            path,
            frame: facing(start),
            index: 0,
            elapsed: 0.0,
            at: Offset::ZERO,
            repeat: false,
            optics: Vec::new(),
        }
    }

    /// A play of `steps` from `start`.
    pub(crate) fn path(call: Option<u64>, steps: &[Step], start: &[f32; 6]) -> Self {
        Self::path_at(call, steps, false, 0, 0.0, start)
    }

    /// A play that starts `elapsed` seconds into step `index`, with the camera
    /// at `pose` there. Its heading is the one that leads to `pose` at that
    /// point.
    pub(crate) fn path_at(
        call: Option<u64>,
        steps: &[Step],
        closed: bool,
        index: usize,
        elapsed: f32,
        pose: &[f32; 6],
    ) -> Self {
        let mut path = Path::new(steps, closed);
        let reached = match path.leg(index) {
            Some(leg) if leg.seconds > 0.0 => leg.at(elapsed / leg.seconds),
            Some(leg) => leg.from,
            None => path.end(),
        };
        Self {
            call,
            track: None,
            frame: euler(0.0, pose[4] - reached.turn.y, 0.0),
            path,
            index,
            elapsed,
            at: reached,
            repeat: false,
            optics: Vec::new(),
        }
    }

    /// The play, applying these optics at its keys.
    pub(crate) fn with_optics(self, optics: Vec<Optics>) -> Self {
        Self { optics, ..self }
    }

    /// The optics of the last key reached. They are applied at the key and not
    /// eased between keys, because a stream of settings next to the poses makes
    /// the motion uneven in VRChat.
    pub(crate) fn optics(&self) -> Option<Optics> {
        self.optics.get(self.index).copied()
    }

    /// The play, starting over instead of ending.
    pub(crate) fn repeating(self) -> Self {
        Self {
            repeat: true,
            ..self
        }
    }

    /// A play to the pose `to` in a straight line, each angle turning the
    /// shorter way.
    pub(crate) fn to(call: Option<u64>, to: &[f32; 6], seconds: f32, start: &[f32; 6]) -> Self {
        Self::new(
            call,
            Path::new(&[step_to(start, &[], to, seconds)], false),
            start,
        )
    }

    /// Advances `dt` seconds and returns the move in world space and the change
    /// of pitch, yaw and roll since the last tick, or `None` if it has ended.
    /// Time past the end of a leg carries over into the next one.
    pub(crate) fn step(&mut self, dt: f32) -> Option<(Vec3, Vec3)> {
        if self.index >= self.path.len() {
            return None;
        }
        self.elapsed += dt;
        let mut to = self.at;
        for _ in 0..=self.path.len() {
            let Some(leg) = self.path.leg(self.index) else {
                break;
            };
            if leg.seconds > 0.0 && self.elapsed < leg.seconds - TIME_SLACK {
                to = leg.at(self.elapsed / leg.seconds);
                break;
            }
            to = leg.to;
            self.elapsed = (self.elapsed - leg.seconds).max(0.0);
            self.index += 1;
            if self.repeat && self.index == self.path.len() {
                self.index = 0;
            }
        }
        let moved = self.frame * (to.pos - self.at.pos);
        let turned = to.turn - self.at.turn;
        self.at = to;
        Some((moved, turned))
    }

    pub(crate) fn ended(&self) -> bool {
        self.path.len() == 0 || (!self.repeat && self.index >= self.path.len())
    }

    /// The current step, and the seconds into it.
    pub(crate) fn place(&self) -> (usize, f32) {
        (self.index, self.elapsed)
    }

    /// The play, as a play of library track `key`.
    pub(crate) fn of_track(self, key: String) -> Self {
        Self {
            track: Some(key),
            ..self
        }
    }

    /// The time since the start, in seconds.
    pub(crate) fn at(&self) -> f32 {
        let steps = &self.path.steps;
        let before: f32 = steps[..self.index.min(steps.len())]
            .iter()
            .map(|s| s[6])
            .sum();
        before + self.elapsed
    }
}

/// The offset of each key, from the start (key 0) to the end.
pub(crate) fn key_offsets(steps: &[Step]) -> Vec<Offset> {
    let mut at = Offset::ZERO;
    std::iter::once(at)
        .chain(steps.iter().map(|s| {
            at = at.then(s);
            at
        }))
        .collect()
}

/// A path. Its legs are measured and paced when first used, since doing so for
/// all of a long path takes too much fuel in one dispatch.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Path {
    steps: Vec<Step>,
    closed: bool,
    ramps: Vec<Ramp>,
    legs: Vec<Leg>,
}

impl Path {
    pub(crate) fn new(steps: &[Step], closed: bool) -> Self {
        Self {
            steps: steps.to_vec(),
            closed,
            ramps: ramps(steps, closed),
            legs: shapes(steps, closed),
        }
    }

    fn len(&self) -> usize {
        self.legs.len()
    }

    /// Leg `i`, measured and paced.
    pub(crate) fn leg(&mut self, i: usize) -> Option<&Leg> {
        if !self.legs.get(i)?.paced {
            let ends = [i, i + 1].map(|key| joined(&self.steps, key, self.closed));
            let beside = ends.into_iter().flatten().flat_map(|(a, b)| [a, b]);
            for at in beside.chain([i]) {
                self.legs[at].measure();
            }
            let keys = key_paces(&self.legs, &self.steps, self.closed, i);
            self.legs[i].set_pace(&self.ramps[i], keys);
            self.legs[i].paced = true;
        }
        self.legs.get(i)
    }

    /// The offset of the end.
    fn end(&self) -> Offset {
        self.legs.last().map_or(Offset::ZERO, |l| l.to)
    }
}

/// The paces of leg `i` at its two keys on an even ramp, relative to its
/// average. At a joined key it is the average of both legs, at most twice the
/// slower one, so that the speed changes gradually. It is 1 if the key isn't
/// joined or if one leg moves and the other only turns.
fn key_paces(legs: &[Leg], steps: &[Step], closed: bool, i: usize) -> [f32; 2] {
    [i, i + 1].map(|key| {
        let pair = joined(steps, key, closed)
            .and_then(|(a, b)| Some((&legs[a], legs[a].reach()?, &legs[b], legs[b].reach()?)));
        match pair {
            Some((a, (kind, far), b, (other, on))) if kind == other => {
                let (slow, fast) = (far / a.seconds, on / b.seconds);
                let mean = (far + on) / (a.seconds + b.seconds);
                let own = legs[i].reach().map_or(mean, |(_, r)| r / legs[i].seconds);
                mean.min(2.0 * slow.min(fast)) / own
            }
            _ => 1.0,
        }
    })
}

/// The legs of `steps`, without their length and pace. A handle is a third of
/// the chord long and turns from the chord (length 0) to the key's automatic
/// direction (length 1). A jump is drawn straight.
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
                arc: [0.0; ARC_SAMPLES + 1],
                measured: false,
                pace: [1.0; PACE_SAMPLES + 1],
                run: std::array::from_fn(|j| j as f32 / PACE_SAMPLES as f32),
                paced: false,
            }
        })
        .collect()
}

/// The rotation of pitch, yaw and roll in degrees, in the order VRChat's `Pose`
/// uses.
pub(crate) fn euler(pitch: f32, yaw: f32, roll: f32) -> Quat {
    Quat::from_euler(
        EulerRot::ZXYEx,
        roll.to_radians(),
        pitch.to_radians(),
        yaw.to_radians(),
    )
}

/// The pitch, yaw and roll of a rotation, in degrees.
pub(crate) fn angles(q: Quat) -> [f32; 3] {
    let (roll, pitch, yaw) = q.to_euler(EulerRot::ZXYEx);
    [pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees()]
}

/// The pose of a position and a rotation.
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

/// Whether two poses are the same within the rounding of VRChat's reports (1 cm
/// and about half a degree).
pub(crate) fn same_place(a: &[f32; 6], b: &[f32; 6]) -> bool {
    let (ra, rb) = (rotation(a), rotation(b));
    position(a).distance(position(b)) < 0.01
        && [Vec3::Y, Vec3::Z]
            .into_iter()
            .all(|v| (ra * v).distance(rb * v) < 0.01)
}

/// The rotation of the pose's yaw alone.
pub(crate) fn facing(pose: &[f32; 6]) -> Quat {
    euler(0.0, pose[4], 0.0)
}

/// `degrees` in -180..180.
fn wrapped(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

/// `to` with each angle shifted by full turns to be within half a turn of
/// `from`, so that a turn past 180° doesn't jump back by a full turn.
pub(crate) fn continuing(from: &[f32; 6], mut to: [f32; 6]) -> [f32; 6] {
    for k in 3..6 {
        to[k] = from[k] + wrapped(to[k] - from[k]);
    }
    to
}

/// `pose` with its angles in -180..180.
pub(crate) fn in_range(mut pose: [f32; 6]) -> [f32; 6] {
    for angle in &mut pose[3..] {
        *angle = wrapped(*angle);
    }
    pose
}

/// The step from the end of `steps`, played from `start`, to the world pose
/// `to`. Each angle turns the shorter way.
pub(crate) fn step_to(start: &[f32; 6], steps: &[Step], to: &[f32; 6], seconds: f32) -> Step {
    let end = key_offsets(steps)[steps.len()];
    let pos = facing(start).inverse() * (position(to) - position(start)) - end.pos;
    // Both angles come from rotations, so equal orientations give equal angles
    // whichever form VRChat reported them in.
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

/// The world pose `elapsed` seconds into step `index`, played from `start`.
pub(crate) fn pose_at(
    start: &[f32; 6],
    steps: &[Step],
    closed: bool,
    index: usize,
    elapsed: f32,
) -> [f32; 6] {
    let mut path = Path::new(steps, closed);
    let o = match path.leg(index) {
        Some(leg) if leg.seconds > 0.0 => leg.at((elapsed / leg.seconds).min(1.0)),
        Some(leg) => leg.to,
        None => return key_pose(start, steps, index),
    };
    let at = position(start) + facing(start) * o.pos;
    let turn = Vec3::new(start[3], start[4], start[5]) + o.turn;
    [at.x, at.y, at.z, turn.x, turn.y, turn.z]
}

/// The step at time `seconds`, and the seconds into it, limited to the path.
pub(crate) fn place_at(steps: &[Step], seconds: f32) -> (usize, f32) {
    let times = key_times(steps);
    let i = (0..steps.len())
        .rfind(|i| times[*i] <= seconds)
        .unwrap_or(0);
    let into = (seconds - times[i]).clamp(0.0, steps.get(i).map_or(0.0, |s| s[6]));
    (i, into)
}

/// The world pose of key `key` (0 is the start), played from `start`.
pub(crate) fn key_pose(start: &[f32; 6], steps: &[Step], key: usize) -> [f32; 6] {
    let o = key_offsets(steps)[key.min(steps.len())];
    let at = position(start) + facing(start) * o.pos;
    let turn = Vec3::new(start[3], start[4], start[5]) + o.turn;
    [at.x, at.y, at.z, turn.x, turn.y, turn.z]
}

/// Adds `sign * change` to the step's move and turn.
fn shift(step: &mut Step, change: &[f32; 6], sign: f32) {
    for (v, c) in step.iter_mut().zip(change) {
        *v += sign * c;
    }
}

/// Moves key `key` (1 or more) to the world pose `to`, leaving the other keys
/// in place.
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

/// Moves key `index` (1 or more) to `at` seconds, limited to the times of its
/// neighbors, which stay in place.
pub(crate) fn retime_key(steps: &mut [Step], index: usize, at: f32) {
    let times = key_times(steps);
    let Some(i) = index.checked_sub(1).filter(|i| *i < steps.len()) else {
        return;
    };
    let (before, after) = (times[i], times.get(index + 1).copied());
    let at = at.max(before).min(after.unwrap_or(f32::INFINITY));
    let cap = STEP_SECONDS as f32;
    steps[i][6] = (at - before).min(cap);
    if let (Some(after), Some(next)) = (after, steps.get_mut(index)) {
        next[6] = (after - at).min(cap);
    }
}

/// Inserts a key at the world pose `to` after key `key`, leaving the other keys
/// in place and on time: the new step takes half the duration of the next one,
/// or `seconds` at the end.
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

/// Rewrites `steps`, played from `from`, to be played from `to` with the keys
/// after the start in the same place.
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

/// A step with its values limited to their ranges. Its optics are limited when
/// they are sent.
pub(crate) fn clamp_step(mut s: Step) -> Step {
    for (k, v) in s[..WIRE].iter_mut().enumerate() {
        let (low, high) = match k {
            0..=2 => (-STEP_REACH, STEP_REACH),
            3..=5 => (-STEP_TURN, STEP_TURN),
            6 => (0.0, STEP_SECONDS),
            OUT_LEN | IN_LEN => (0.0, HANDLE_MAX),
            _ => RAMP_RANGE,
        };
        *v = match v.is_finite() {
            true => v.clamp(low as f32, high as f32),
            false => 0.0,
        };
    }
    s
}

/// The directions the camera can be flown in.
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
    /// The toggle that makes VRChat level this rotation, if any.
    pub(crate) fn leveled_by(self) -> Option<&'static str> {
        match self {
            Fly::PitchUp | Fly::PitchDown => Some("auto_level_pitch"),
            Fly::RollLeft | Fly::RollRight => Some("auto_level_roll"),
            _ => None,
        }
    }

    pub(crate) const PADS: [Pad; 12] = [
        Pad::new(
            Fly::Forward,
            "fly:forward",
            "forward",
            "Forward",
            Key::Char('w'),
        ),
        Pad::new(Fly::Back, "fly:back", "back", "Back", Key::Char('s')),
        Pad::new(Fly::Left, "fly:left", "left", "Left", Key::Char('a')),
        Pad::new(Fly::Right, "fly:right", "right", "Right", Key::Char('d')),
        Pad::new(Fly::Rise, "fly:rise", "rise", "Up", Key::Space),
        Pad::new(Fly::Sink, "fly:sink", "sink", "Down", Key::Shift),
        Pad::new(Fly::PitchUp, "fly:pitch_up", "tilt_up", "Tilt up", Key::Up),
        Pad::new(
            Fly::PitchDown,
            "fly:pitch_down",
            "tilt_down",
            "Tilt down",
            Key::Down,
        ),
        Pad::new(
            Fly::YawLeft,
            "fly:yaw_left",
            "pan_left",
            "Turn left",
            Key::Left,
        ),
        Pad::new(
            Fly::YawRight,
            "fly:yaw_right",
            "pan_right",
            "Turn right",
            Key::Right,
        ),
        Pad::new(
            Fly::RollLeft,
            "fly:roll_left",
            "roll_left",
            "Roll left",
            Key::Char('q'),
        ),
        Pad::new(
            Fly::RollRight,
            "fly:roll_right",
            "roll_right",
            "Roll right",
            Key::Char('e'),
        ),
    ];

    /// The pad for this direction.
    pub(crate) fn pad(self) -> &'static Pad {
        Self::PADS
            .iter()
            .find(|p| p.fly == self)
            .expect("every way has a pad")
    }

    /// The direction of the pad with this id.
    pub(crate) fn of_id(id: &str) -> Option<Fly> {
        Self::PADS.iter().find(|p| p.id == id).map(|p| p.fly)
    }

    /// The move (right, up, forward) and the turn (pitch, yaw, roll) per
    /// second, at speed 1. Moves are relative to the camera with local
    /// navigation, and to its heading otherwise. Yaw turns around the world's
    /// vertical axis.
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

/// A button that flies the camera while it is held.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pad {
    pub(crate) fly: Fly,
    pub(crate) id: &'static str,
    pub(crate) icon: &'static str,
    pub(crate) tip: &'static str,
    pub(crate) key: Key,
}

impl Pad {
    const fn new(
        fly: Fly,
        id: &'static str,
        icon: &'static str,
        tip: &'static str,
        key: Key,
    ) -> Self {
        Self {
            fly,
            id,
            icon,
            tip,
            key,
        }
    }
}

/// The flying speeds in metres and degrees per second: minimum, maximum,
/// default.
pub(crate) const MOVE_SPEED: (f32, f32, f32) = (0.01, 100.0, 2.0);
pub(crate) const TURN_SPEED: (f32, f32, f32) = (0.1, 360.0, 45.0);

/// The factor one line of the wheel changes the move speed by.
pub(crate) const SPEED_PER_LINE: f32 = 1.25;

/// The time of each key, starting at 0.
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
    /// Starts the play `make` builds from the camera's pose. A play without
    /// steps is answered at once. Without a camera in VRChat, it moves the
    /// stand-in.
    pub(crate) fn start(
        &mut self,
        call: Option<u64>,
        make: impl FnOnce(&[f32; 6]) -> Play,
        out: &mut Vec<Command>,
    ) {
        let play = make(&self.pose);
        if play.path.len() == 0 {
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
            // The first tick sends the pose.
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

    /// Marks the clock as stopped. The stand-in's angles return to -180..180;
    /// those of VRChat's camera do with its next report.
    fn rest(&mut self) {
        self.ticking = false;
        self.last_tick = None;
        if !self.linked {
            self.pose = in_range(self.pose);
        }
    }

    /// Starts or stops flying in a direction.
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

    /// One tick: advances every play, held direction and look, and sends the
    /// resulting pose. The clock runs while any of them is active.
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

    /// Applies `dt` seconds of every play, held direction and look to the pose.
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
                // Level navigation keeps the height: moves follow the heading.
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

    /// Ends every play and answers their calls.
    pub(crate) fn end_plays(&mut self, out: &mut Vec<Command>) {
        self.end_plays_where(|_| true, out);
    }

    /// Ends the plays that match `hit` and answers their calls. A track started
    /// by the user can be resumed from here.
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
