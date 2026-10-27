//! The pieces a view is built from: buttons, fields and sliders, the pace
//! lane, the key panel. Nodes and the numbers they show, never state.

use vrc_we_sdk::{Lane, LaneCurve, LaneDot, Node};

use crate::motion::{
    dot_place, key_times, pace, Step, DOT_AT, DOT_PACE, HANDLE_MAX, IN_LEN, OUT_LEN, PACE_RANGE,
    PATH_MAX,
};

/// How long a press on a position takes to fly there.
pub(crate) const GO_SECONDS: f32 = 2.0;

/// The seconds an added key takes from the one before.
pub(crate) const KEY_SECONDS: f32 = 2.0;

/// The most points the 3D view draws a track's curve with, and the most
/// one leg takes: a long track draws each leg with fewer.
pub(crate) const PATH_POINTS: usize = 1024;
pub(crate) const LEG_POINTS: usize = 16;

/// The longest track the timeline shows a pace lane for, the most points
/// each of its curves takes, and the most one step takes: a longer track
/// would build a view past a dispatch's fuel.
pub(crate) const LANE_STEPS_MAX: usize = 256;
pub(crate) const LANE_POINTS: usize = 256;
pub(crate) const LANE_STEP_POINTS: usize = 24;

pub(crate) const LANE_TOO_LONG: &str =
    "Pace is edited on tracks of up to 256 keys; its pace still plays.";

/// A step's move and turn as the editor names them, in a step's order.
pub(crate) const STEP_LABELS: [&str; 6] = ["Right", "Up", "Forward", "Pitch", "Yaw", "Roll"];

/// The pads' keys, after the 3D view's own mouse controls.
pub(crate) const HINT: &str =
    "W A S D move · Space, Shift rise and sink · arrows turn · Q, E roll · \
                    wheel while one is held: speed";

/// What the pads move until VRChat reports where its camera is.
pub(crate) const STAND_IN: &str = "Until then, the pads, keys and plays move a stand-in here.";

/// What the page and the editor say when a stored library value wouldn't
/// read: nothing shows, nothing is saved over it, and one press replaces it.
pub(crate) const UNREADABLE: &str =
    "This client couldn't read a stored camera library, so it shows and saves nothing over it.";
pub(crate) const NEW_LIBRARY: &str = "Start a new library";

pub(crate) fn button(id: &str, label: &str, icon: Option<&str>, wide: bool) -> Node {
    Node::Button {
        id: id.to_owned(),
        label: label.to_owned(),
        icon: icon.map(str::to_owned),
        wide,
    }
}

pub(crate) fn icon_button(id: &str, icon: &str, tip: &str) -> Node {
    Node::IconButton {
        id: id.to_owned(),
        icon: icon.to_owned(),
        tip: tip.to_owned(),
    }
}

pub(crate) fn number_node(id: &str, label: &str, value: Option<f32>) -> Node {
    Node::Number {
        id: id.to_owned(),
        label: label.to_owned(),
        value: value.map(f64::from),
    }
}

pub(crate) fn speed_slider(
    id: &str,
    label: &str,
    (min, max, _): (f32, f32, f32),
    value: f32,
) -> Node {
    Node::Slider {
        id: id.to_owned(),
        label: label.to_owned(),
        min: f64::from(min),
        max: f64::from(max),
        value: Some(f64::from(value)),
        log: true,
        live: false,
    }
}

/// The monotone cubic (Fritsch–Carlson) through `xs` (rising) and `ys`:
/// no overshoot between points, each extreme level.
pub(crate) fn monotone(xs: Vec<f32>, ys: Vec<f32>) -> impl Fn(f32) -> f32 {
    let n = xs.len();
    let h: Vec<f32> = xs.windows(2).map(|w| w[1] - w[0]).collect();
    let d: Vec<f32> = (0..n - 1).map(|i| (ys[i + 1] - ys[i]) / h[i]).collect();
    let mut m = vec![0.0; n];
    for i in 1..n - 1 {
        if d[i - 1] * d[i] > 0.0 {
            let (w1, w2) = (2.0 * h[i] + h[i - 1], h[i] + 2.0 * h[i - 1]);
            m[i] = (w1 + w2) / (w1 / d[i - 1] + w2 / d[i]);
        }
    }
    if n > 1 {
        m[0] = d[0] * 0.5;
        m[n - 1] = d[n - 2] * 0.5;
    }
    move |t| {
        let i = (0..n.saturating_sub(2))
            .find(|i| t <= xs[i + 1])
            .unwrap_or(n.saturating_sub(2));
        if n < 2 {
            return ys.first().copied().unwrap_or(1.0);
        }
        let s = (t - xs[i]) / h[i];
        let (s2, s3) = (s * s, s * s * s);
        (2.0 * s3 - 3.0 * s2 + 1.0) * ys[i]
            + (s3 - 2.0 * s2 + s) * h[i] * m[i]
            + (-2.0 * s3 + 3.0 * s2) * ys[i + 1]
            + (s3 - s2) * h[i] * m[i + 1]
    }
}

/// The steps of `steps` that take time, by index: the pace lane's dots, in
/// order.
pub(crate) fn timed(steps: &[Step]) -> Vec<usize> {
    (0..steps.len()).filter(|i| steps[*i][6] > 0.0).collect()
}

/// The pace lane over `steps`: each timed step's dot, the curve drawn
/// through 1 at every key and through each dot, and the pace played (two
/// pieces a step, from 1 at its keys to its dot), faint; none for no
/// timed step or past [`LANE_STEPS_MAX`] steps.
pub(crate) fn pace_lane(steps: &[Step]) -> Option<Lane> {
    let timed = timed(steps);
    if timed.is_empty() || steps.len() > LANE_STEPS_MAX {
        return None;
    }
    let times = key_times(steps);
    let per = (LANE_POINTS / timed.len()).clamp(1, LANE_STEP_POINTS);
    let (mut xs, mut ys, mut dots) = (vec![times[timed[0]]], vec![1.0], Vec::new());
    for i in &timed {
        let (from, to) = (times[*i], times[i + 1]);
        let at = from + dot_place(steps[*i][DOT_AT]) * (to - from);
        dots.push(LaneDot {
            at: [at, steps[*i][DOT_PACE]],
            from,
            to,
        });
        if from > *xs.last().expect("a first point") {
            xs.push(from);
            ys.push(1.0);
        }
        xs.extend([at, to]);
        ys.extend([steps[*i][DOT_PACE], 1.0]);
    }
    let drawn = monotone(xs, ys);
    let (mut curve, mut played) = (Vec::new(), Vec::new());
    for i in &timed {
        let (s, from, to) = (steps[*i], times[*i], times[i + 1]);
        let mut us: Vec<f32> = (0..per).map(|j| j as f32 / per as f32).collect();
        us.push(dot_place(s[DOT_AT]));
        us.sort_by(f32::total_cmp);
        for u in us {
            let t = from + u * (to - from);
            curve.push([t, drawn(t)]);
            played.push([t, pace(s[DOT_AT], s[DOT_PACE], u)]);
        }
    }
    let end = times[timed[timed.len() - 1] + 1];
    curve.push([end, 1.0]);
    played.push([end, 1.0]);
    Some(Lane {
        min: 0.0,
        max: PACE_RANGE.1 as f32,
        base: 1.0,
        label: "1×".to_owned(),
        curves: vec![
            LaneCurve {
                points: curve,
                faint: false,
            },
            LaneCurve {
                points: played,
                faint: true,
            },
        ],
        dots,
    })
}

/// The steps holding key `key`'s handle into it and out of it on `n`
/// steps: the step before and the step after; none for the start's way in
/// or the last key's way out, unless `closed`, where the start and the end
/// are one key, in by the last step and out by the first.
pub(crate) fn side_steps(n: usize, key: usize, closed: bool) -> (Option<usize>, Option<usize>) {
    match key {
        _ if n == 0 || key > n => (None, None),
        k if closed && (k == 0 || k == n) => (Some(n - 1), Some(0)),
        k => (k.checked_sub(1), (k < n).then_some(k)),
    }
}

/// Key `key`'s handle lengths on `steps`: into it and out of it.
pub(crate) fn key_sides(steps: &[Step], key: usize, closed: bool) -> (Option<f32>, Option<f32>) {
    let (into, out) = side_steps(steps.len(), key, closed);
    (
        into.map(|i| steps[i][IN_LEN]),
        out.map(|i| steps[i][OUT_LEN]),
    )
}

/// Sets key `key`'s handle lengths on `steps`, the sides given.
pub(crate) fn set_key_sides(
    steps: &mut [Step],
    key: usize,
    closed: bool,
    into: Option<f32>,
    out: Option<f32>,
) {
    let (into_at, out_at) = side_steps(steps.len(), key, closed);
    let clamp = |v: f32| v.clamp(0.0, HANDLE_MAX as f32);
    if let (Some(v), Some(i)) = (into, into_at) {
        steps[i][IN_LEN] = clamp(v);
    }
    if let (Some(v), Some(i)) = (out, out_at) {
        steps[i][OUT_LEN] = clamp(v);
    }
}

/// The chosen key's shape, over the 3D view: Linear or Smooth (each sets
/// both its handles), and each handle's length, live as it is dragged;
/// none without a key that has a side.
pub(crate) fn key_panel(steps: &[Step], key: Option<usize>, closed: bool) -> Option<Node> {
    let (into, out) = key_sides(steps, key?, closed);
    if into.is_none() && out.is_none() {
        return None;
    }
    let linear = into.unwrap_or(0.0) == 0.0 && out.unwrap_or(0.0) == 0.0;
    let slider = |id: &str, label: &str, v: f32| Node::Slider {
        id: id.to_owned(),
        label: label.to_owned(),
        min: 0.0,
        max: HANDLE_MAX,
        value: Some(f64::from(v)),
        log: false,
        live: true,
    };
    let mut rows = vec![Node::Buttons {
        id: "key_shape".to_owned(),
        options: vec!["Linear".to_owned(), "Smooth".to_owned()],
        selected: Some(usize::from(!linear)),
        columns: 2,
    }];
    rows.extend(into.map(|v| slider("in_len", "In length", v)));
    rows.extend(out.map(|v| slider("out_len", "Out length", v)));
    Some(Node::Column(rows))
}

pub(crate) fn track_full() -> String {
    format!("A track holds at most {PATH_MAX} steps.")
}

pub(crate) fn text(t: impl Into<String>) -> Node {
    Node::Text(t.into())
}

pub(crate) fn heading(t: &str) -> Node {
    Node::Heading(t.to_owned())
}

pub(crate) fn name_field(id: &str, label: &str, value: &str, live: bool) -> Node {
    Node::TextInput {
        id: id.to_owned(),
        label: label.to_owned(),
        value: value.to_owned(),
        live,
        secret: false,
    }
}
