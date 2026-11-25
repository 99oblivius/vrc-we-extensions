//! The parts the views are built from. They depend only on their arguments.

use vrc_we_sdk::{
    Axis, Button, Curve, Drag, Flow, Frame, Ink, Input, Line, Marks, Node, Number, Piece, Plot,
    Point, Scroll, Size, Slider, Stroke, Surface, Text,
};

use crate::motion::{
    height, key_ramp, key_times, ramps, Step, HANDLE_MAX, IN_LEN, OUT_LEN, PATH_MAX,
};

/// The duration of a flight to a position, in seconds.
pub(crate) const GO_SECONDS: f32 = 2.0;

/// The duration of a new key's step, in seconds.
pub(crate) const KEY_SECONDS: f32 = 2.0;

/// The most points used to draw a track in the 3D view, and the most per leg.
pub(crate) const PATH_POINTS: usize = 1024;
pub(crate) const LEG_POINTS: usize = 16;

/// The most steps for which the timeline shows ramps. Building the view of a
/// longer track takes too much fuel.
pub(crate) const RAMP_STEPS_MAX: usize = 256;

pub(crate) const RAMP_TOO_LONG: &str =
    "Ramps are edited on tracks of up to 256 keys; a longer track's still play.";

/// The labels of a step's move and turn, in order.
pub(crate) const STEP_LABELS: [&str; 6] = ["Right", "Up", "Forward", "Pitch", "Yaw", "Roll"];

/// The keyboard controls, shown below the 3D view's mouse controls.
pub(crate) const HINT: &str = "W A S D: move · Space, Shift: up, down\n\
                               Arrows: turn · Q, E: roll\n\
                               Wheel while moving or looking: speed";

/// Shown while there is no camera in VRChat to move.
pub(crate) const STAND_IN: &str = "Until then, the pads, keys and plays move a stand-in here.";

/// Shown when a stored value can't be read. Nothing is saved over it until the
/// user starts a new library.
pub(crate) const UNREADABLE: &str =
    "This client couldn't read a stored camera library, so it shows and saves nothing over it.";
pub(crate) const NEW_LIBRARY: &str = "Start a new library";

/// The padding of a panel.
pub(crate) const PANEL_PAD: f32 = 12.0;

/// The height of the timeline with ramps and without.
const TIMELINE_PX: f32 = 152.0;
const PLAIN_PX: f32 = 64.0;

/// The labeled ramp heights.
const RAMP_MARKS: [(f32, &str); 3] = [(0.0, "stop"), (0.5, "even"), (1.0, "fastest")];

pub(crate) fn button(id: &str, label: &str, icon: &str) -> Button {
    Button::new(id, label).icon(icon)
}

pub(crate) fn number_node(id: &str, label: &str, value: Option<f32>) -> Node {
    Number::new(id, label, value.map(f64::from)).into()
}

pub(crate) fn speed_slider(
    id: &str,
    label: &str,
    (min, max, _): (f32, f32, f32),
    value: f32,
) -> Node {
    Slider::new(
        id,
        label,
        (f64::from(min), f64::from(max)),
        Some(f64::from(value)),
    )
    .log(true)
    .into()
}

/// The timeline: each key at its time and the height of its ramp, the curve
/// through them, and the playhead. A key can be dragged in time between its
/// neighbors and in height; the start only in height. Past [`RAMP_STEPS_MAX`]
/// steps it shows no ramps.
pub(crate) fn timeline(
    steps: &[Step],
    keys: usize,
    closed: bool,
    selected: Option<usize>,
    playhead: Option<f32>,
) -> Plot {
    let times = match keys {
        0 => Vec::new(),
        _ => key_times(steps),
    };
    let ramped = steps.len() <= RAMP_STEPS_MAX;
    let at: Vec<[f32; 2]> = times
        .iter()
        .enumerate()
        .map(|(key, time)| match ramped {
            true => [*time, height(key_ramp(steps, key))],
            false => [*time, 0.0],
        })
        .collect();
    let end = times.last().copied().unwrap_or(0.0).max(1.0);
    let x = Axis::new(0.0, end)
        .marks(Marks::Round("s".to_owned()))
        .zoom(true)
        .grows(true);
    let names = RAMP_MARKS.iter().map(|(h, n)| (*h, (*n).to_owned()));
    let y = Axis::new(0.0, 1.0).marks(match ramped {
        true => Marks::At(names.collect()),
        false => Marks::None,
    });
    let mut plot = Plot::new("timeline", x, y)
        .scrub(true)
        .height(Size::Px(if ramped { TIMELINE_PX } else { PLAIN_PX }));
    if let Some(from) = at.first().filter(|_| ramped) {
        let pieces = ramps(steps, closed)
            .iter()
            .zip(at.windows(2))
            .map(|(ramp, ends)| {
                let third = (ends[1][0] - ends[0][0]) / 3.0;
                let [out, into] = ramp.slopes;
                match third > 0.0 {
                    true => Piece::Cubic(
                        [ends[0][0] + third, ends[0][1] + out * third],
                        [ends[1][0] - third, ends[1][1] - into * third],
                        ends[1],
                    ),
                    false => Piece::Move(ends[1]),
                }
            })
            .collect();
        plot = plot.push(Curve::new(*from, pieces, Stroke::new(Ink::Accent, 2.0)));
    }
    let drag = |key: usize| {
        let drag = match ramped {
            true => Drag::default().along_y(0.0, Some(1.0)).live(true),
            false => Drag::default(),
        };
        match key.checked_sub(1) {
            Some(before) => drag.along_x(times[before], times.get(key + 1).copied()),
            None => drag,
        }
    };
    plot.extend(at.iter().enumerate().map(|(key, at)| {
        let mut point = Point::new(*at)
            .selected(selected == Some(key))
            .stem(ramped)
            .drag(drag(key));
        if ramped {
            point = point.label((key + 1).to_string());
        }
        point
    }))
    .push(Line::new(playhead, Stroke::new(Ink::Marked, 2.0)))
}

/// The steps that hold the handles of key `key`, into it and out of it, in a
/// path of `n` steps. The start and end of a `closed` path are the same key.
pub(crate) fn side_steps(n: usize, key: usize, closed: bool) -> (Option<usize>, Option<usize>) {
    match key {
        _ if n == 0 || key > n => (None, None),
        k if closed && (k == 0 || k == n) => (Some(n - 1), Some(0)),
        k => (k.checked_sub(1), (k < n).then_some(k)),
    }
}

/// The handle lengths of key `key`, into it and out of it.
pub(crate) fn key_sides(steps: &[Step], key: usize, closed: bool) -> (Option<f32>, Option<f32>) {
    let (into, out) = side_steps(steps.len(), key, closed);
    (
        into.map(|i| steps[i][IN_LEN]),
        out.map(|i| steps[i][OUT_LEN]),
    )
}

/// Sets the given handle lengths of key `key`.
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

/// The path controls of key `key`: straight or smooth, and the length of each
/// handle.
pub(crate) fn path_rows(steps: &[Step], key: usize, closed: bool) -> Vec<Node> {
    let (into, out) = key_sides(steps, key, closed);
    if into.is_none() && out.is_none() {
        return Vec::new();
    }
    let linear = into.unwrap_or(0.0) == 0.0 && out.unwrap_or(0.0) == 0.0;
    let slider = |id: &str, label: &str, v: f32| -> Node {
        Slider::new(id, label, (0.0, HANDLE_MAX), Some(f64::from(v)))
            .live(true)
            .into()
    };
    let shapes = [("Straight", "straight"), ("Smooth", "smooth")];
    let mut rows: Vec<Node> = vec![
        Text::heading("Path through this key").into(),
        Button::grid("key_shape", shapes, Some(usize::from(!linear)), 2).into(),
    ];
    rows.extend(into.map(|v| slider("in_len", "Curve arriving", v)));
    rows.extend(out.map(|v| slider("out_len", "Curve leaving", v)));
    rows
}

pub(crate) fn track_full() -> String {
    format!("A track holds at most {PATH_MAX} steps.")
}

pub(crate) fn name_field(id: &str, label: &str, value: &str) -> Input {
    Input::new(id, label, value).width(Size::Fill)
}

/// A column of rows.
pub(crate) fn column(rows: Vec<Node>) -> Flow {
    Flow::col(rows)
}

/// A side panel of the editor.
pub(crate) fn panel(parts: Flow, width: f32) -> Frame {
    Frame::new(parts.gap(0.0).height(Size::Fill))
        .surface(Surface::Inset)
        .width(Size::Px(width))
        .height(Size::Fill)
}

/// The scrolling part of a panel, which takes the remaining height.
pub(crate) fn scrolled(rows: Vec<Node>) -> Frame {
    Frame::new(column(rows))
        .scroll(Scroll::Down)
        .padding(PANEL_PAD)
        .height(Size::Fill)
}

/// A part of a panel that doesn't scroll.
pub(crate) fn pinned(part: impl Into<Node>) -> Frame {
    Frame::new(part).padding(PANEL_PAD)
}
