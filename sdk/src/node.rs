//! What a view is made of: the node tree a page, window or modal draws, the
//! events its widgets send back, and the slider and timeline values it shows.

use crate::*;

use serde::{Deserialize, Serialize};

/// A view: a tree of client widgets the client renders, themes and
/// focus-walks. The extension describes; it never draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    Column(Vec<Node>),
    Row(Vec<Node>),
    Heading(String),
    Text(String),
    /// A number field; an edit commits [`UiEvent::Number`] on Enter.
    Number {
        id: String,
        label: String,
        value: Option<f64>,
    },
    /// [`UiEvent::Pressed`]. `icon` (one of [`Manifest::icons`]) goes
    /// before the label; a `wide` one fills the width, taller and in the
    /// accent colour, for a view's main action.
    Button {
        id: String,
        label: String,
        #[serde(default)]
        icon: Option<String>,
        #[serde(default)]
        wide: bool,
    },
    /// One of `options`; [`UiEvent::Chosen`] carries its index.
    Choice {
        id: String,
        options: Vec<String>,
        selected: Option<usize>,
    },
    /// A path in 3D the person turns by dragging: `path`'s points joined
    /// in order, the points `keys` names (indices into `path`) marked as
    /// keys, `selected` among them, and `camera` a position with the
    /// directions it faces and its top faces (`[at, forward, up]`, drawn at
    /// the scene's scale). A press near a key is [`UiEvent::Chosen`] with its place
    /// in `keys`. The view fits the keys (the path when it has none); the
    /// wheel zooms, a double-click fits again, and a camera outside the
    /// view is marked at its edge. A drag with the right button is
    /// [`UiEvent::Look`] at most once a frame, the cursor hidden while it
    /// lasts. `overlay` sits over its top-left corner: a press on it never
    /// reaches the scene. A `closed` path ends on its first key, so its last
    /// key draws no number of its own. `hint` follows the view's own mouse
    /// controls in its bottom-right corner, always shown: the extension's
    /// keys, say. [`Command::Mark`] moves `camera` without the view.
    Scene3d {
        id: String,
        path: Vec<[f32; 3]>,
        keys: Vec<usize>,
        selected: Option<usize>,
        camera: Option<[[f32; 3]; 3]>,
        #[serde(default)]
        overlay: Option<Box<Node>>,
        #[serde(default)]
        closed: bool,
        #[serde(default)]
        hint: Option<String>,
    },
    /// One of the icons its manifest packages ([`Manifest::icons`]), drawn
    /// in the theme's text colour; a name it doesn't package draws nothing.
    Icon {
        icon: String,
    },
    /// A button showing icon `icon`, with `tip` on hover;
    /// [`UiEvent::Pressed`].
    IconButton {
        id: String,
        icon: String,
        tip: String,
    },
    /// A button held down: [`UiEvent::Held`] with `down` when pressed, and
    /// without it when let go. The client lets it go on any mouse release,
    /// when its view goes or no longer holds it, and when the window loses
    /// focus, so a hold never outlasts the person's. In a modal or a
    /// window, `key` holds it too while that key is down, unless a text
    /// field there has focus; several may be held at once. While any is
    /// held, the wheel is [`UiEvent::Wheel`] instead of scrolling or
    /// zooming. A `disabled` one is drawn dimmed and neither the mouse
    /// nor its key holds it; a key let go still lets it go.
    HoldButton {
        id: String,
        icon: String,
        tip: String,
        #[serde(default)]
        key: Option<Key>,
        #[serde(default)]
        disabled: bool,
    },
    /// A grid of buttons choosing one of `options`, `columns` wide (1 to
    /// [`COLUMNS_MAX`]); [`UiEvent::Chosen`] carries its index.
    Buttons {
        id: String,
        options: Vec<String>,
        selected: Option<usize>,
        columns: u8,
    },
    /// `main` filling the space beside `side`, a panel of fixed width that
    /// scrolls.
    Split {
        main: Box<Node>,
        side: Box<Node>,
    },
    /// Keys on a ruler of `length` seconds, each at its time, in order. A
    /// press on one is [`UiEvent::Chosen`]; dragging one is
    /// [`UiEvent::Moved`] with the time it is let go at, which stays
    /// between the keys either side of it. The first key doesn't move; the
    /// last may be dragged past the ruler's end, which grows to follow it.
    /// `playhead` marks where a play is. The wheel zooms the ruler about
    /// the cursor, Shift and the wheel or a middle-drag pan it, and a
    /// double-click fits it again; `lane` shares its time axis.
    /// [`Command::Mark`] moves `playhead` without the view.
    Timeline {
        id: String,
        keys: Vec<f32>,
        length: f32,
        selected: Option<usize>,
        playhead: Option<f32>,
        #[serde(default)]
        lane: Option<Lane>,
    },
    /// A value between `min` and `max` dragged along a bar, spread evenly
    /// or, with `log`, by ratio (then `min` is above 0) and held to
    /// [`SLIDER_FIGURES`] significant figures ([`significant`]); letting
    /// go commits [`UiEvent::Number`], and with `live` every move does.
    Slider {
        id: String,
        label: String,
        min: f64,
        max: f64,
        value: Option<f64>,
        log: bool,
        #[serde(default)]
        live: bool,
    },
    /// A text field: Enter commits [`UiEvent::Text`], or with `live` every
    /// change does (a search). A `secret` one is drawn masked, for a token.
    TextInput {
        id: String,
        label: String,
        value: String,
        live: bool,
        #[serde(default)]
        secret: bool,
    },
    /// On or off; a press commits [`UiEvent::Toggled`] with the other
    /// state (on, while it is unknown).
    Toggle {
        id: String,
        label: String,
        value: Option<bool>,
    },
    /// Rows to pick one of; [`UiEvent::Chosen`] carries its index.
    List {
        id: String,
        items: Vec<String>,
        selected: Option<usize>,
    },
    /// A button that puts `text` on the clipboard. The client writes it on
    /// the person's press; the extension hears nothing.
    Copy {
        label: String,
        text: String,
    },
    /// A thin rule between groups.
    Divider,
}

/// What [`Command::Mark`] moves in a view already shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Mark {
    /// A [`Node::Scene3d`]'s `camera`.
    Camera(Option<[[f32; 3]; 3]>),
    /// A [`Node::Timeline`]'s `playhead`.
    Playhead(Option<f32>),
}

impl Mark {
    /// Whether its numbers are all finite.
    pub fn finite(&self) -> bool {
        match self {
            Mark::Camera(camera) => camera.iter().flatten().flatten().all(|c| c.is_finite()),
            Mark::Playhead(at) => at.is_none_or(f32::is_finite),
        }
    }
}

/// The widest a [`Node::Buttons`] grid is.
pub const COLUMNS_MAX: u8 = 8;

/// The most nodes a view holds; a list's rows, a choice's options and a
/// scene's or timeline's points each count as one.
pub const VIEW_NODES_MAX: usize = 4096;

/// The deepest a view nests.
pub const VIEW_DEPTH_MAX: usize = 32;

/// The most text a view holds besides its copy texts: every id, label,
/// row and value together, each at most [`TEXT_MAX`].
pub const VIEW_TEXT_MAX: usize = 64 << 10;

/// The most text a view's [`Node::Copy`] buttons hold together.
pub const VIEW_COPY_MAX: usize = 256 << 10;

/// A key a [`Node::HoldButton`] is held by: a letter or a digit
/// (`Char`, lower case), Space, Shift or an arrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Key {
    Char(char),
    Space,
    Shift,
    Up,
    Down,
    Left,
    Right,
}

impl Key {
    /// Whether a view may bind it: a `Char` only for a–z or 0–9.
    pub fn bindable(self) -> bool {
        match self {
            Key::Char(c) => c.is_ascii_lowercase() || c.is_ascii_digit(),
            _ => true,
        }
    }
}

/// The significant figures a [`Node::Slider`] spread by ratio holds its
/// value to: `0.013`, `2.5`, `38`.
pub const SLIDER_FIGURES: u32 = 2;

/// `value` rounded to `figures` significant figures; 0 and what isn't
/// finite as they are.
pub fn significant(value: f64, figures: u32) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    let scale = 10f64.powi(figures as i32 - 1 - value.abs().log10().floor() as i32);
    (value * scale).round() / scale
}

/// The client's tick: how often timers can fire, a second.
pub const TICK_HZ: u32 = 60;

/// What a view may still hold while it is checked.
struct ViewBudget {
    nodes: usize,
    text: usize,
    copy: usize,
}

/// Takes `n` from `left`; false when it hasn't that much.
fn take(left: &mut usize, n: usize) -> bool {
    left.checked_sub(n).map(|rest| *left = rest).is_some()
}

impl Node {
    /// Whether the client draws it: within [`VIEW_NODES_MAX`],
    /// [`VIEW_DEPTH_MAX`], [`VIEW_TEXT_MAX`] and [`VIEW_COPY_MAX`], its
    /// numbers finite, a slider's range ordered (above 0 when `log`), and
    /// its keys bindable.
    pub fn fits(&self) -> bool {
        let mut budget = ViewBudget {
            nodes: VIEW_NODES_MAX,
            text: VIEW_TEXT_MAX,
            copy: VIEW_COPY_MAX,
        };
        self.within(1, &mut budget)
    }

    fn within(&self, depth: usize, budget: &mut ViewBudget) -> bool {
        if depth > VIEW_DEPTH_MAX {
            return false;
        }
        let rows = match self {
            Node::List { items, .. } => items.len(),
            Node::Choice { options, .. } | Node::Buttons { options, .. } => options.len(),
            Node::Scene3d { path, keys, .. } => path.len() + keys.len() + 1,
            Node::Timeline { keys, lane, .. } => keys.len() + lane.as_ref().map_or(0, Lane::nodes),
            _ => 0,
        };
        let finite = match self {
            Node::Scene3d { path, camera, .. } => path
                .iter()
                .chain(camera.iter().flatten())
                .flatten()
                .all(|c| c.is_finite()),
            Node::Timeline {
                keys,
                length,
                playhead,
                lane,
                ..
            } => {
                keys.iter()
                    .chain([length])
                    .chain(playhead)
                    .all(|t| t.is_finite())
                    && lane.as_ref().is_none_or(Lane::finite)
            }
            Node::Slider {
                min,
                max,
                value,
                log,
                ..
            } => {
                [min, max].into_iter().chain(value).all(|v| v.is_finite())
                    && min < max
                    && (!log || *min > 0.0)
            }
            Node::HoldButton { key, .. } => key.is_none_or(Key::bindable),
            _ => true,
        };
        let texts = self.texts();
        let copy = match self {
            Node::Copy { text, .. } => text.len(),
            _ => 0,
        };
        let fits = finite
            && texts.iter().all(|t| t.len() <= TEXT_MAX)
            && take(&mut budget.nodes, 1 + rows)
            && take(&mut budget.text, texts.iter().map(|t| t.len()).sum())
            && take(&mut budget.copy, copy);
        fits && match self {
            Node::Column(children) | Node::Row(children) => {
                children.iter().all(|c| c.within(depth + 1, budget))
            }
            Node::Split { main, side } => {
                main.within(depth + 1, budget) && side.within(depth + 1, budget)
            }
            Node::Scene3d {
                overlay: Some(overlay),
                ..
            } => overlay.within(depth + 1, budget),
            _ => true,
        }
    }

    /// Sets what `mark` moves on the node `id` inside it, of the mark's
    /// kind; true when one changed.
    pub fn mark(&mut self, id: &str, mark: &Mark) -> bool {
        match (self, mark) {
            (Node::Column(children) | Node::Row(children), _) => {
                children.iter_mut().any(|c| c.mark(id, mark))
            }
            (Node::Split { main, side }, _) => main.mark(id, mark) || side.mark(id, mark),
            (Node::Scene3d { id: at, camera, .. }, Mark::Camera(to)) if at == id => {
                std::mem::replace(camera, *to) != *to
            }
            (
                Node::Timeline {
                    id: at, playhead, ..
                },
                Mark::Playhead(to),
            ) if at == id => std::mem::replace(playhead, *to) != *to,
            _ => false,
        }
    }

    /// Its own strings, its children's and its copy text aside.
    fn texts(&self) -> Vec<&str> {
        match self {
            Node::Column(_) | Node::Row(_) | Node::Split { .. } | Node::Divider => Vec::new(),
            Node::Scene3d { id, hint, .. } => std::iter::once(id)
                .chain(hint)
                .map(String::as_str)
                .collect(),
            Node::Timeline { id, lane, .. } => std::iter::once(id)
                .chain(lane.as_ref().map(|l| &l.label))
                .map(String::as_str)
                .collect(),
            Node::Icon { icon } => vec![icon],
            Node::IconButton { id, icon, tip } | Node::HoldButton { id, icon, tip, .. } => {
                vec![id, icon, tip]
            }
            Node::Heading(t) | Node::Text(t) => vec![t],
            Node::Button {
                id, label, icon, ..
            } => [id, label]
                .into_iter()
                .chain(icon)
                .map(String::as_str)
                .collect(),
            Node::Number { id, label, .. }
            | Node::Toggle { id, label, .. }
            | Node::Slider { id, label, .. } => vec![id, label],
            Node::TextInput {
                id, label, value, ..
            } => vec![id, label, value],
            Node::Choice {
                id, options: rows, ..
            }
            | Node::Buttons {
                id, options: rows, ..
            }
            | Node::List {
                id, items: rows, ..
            } => std::iter::once(id)
                .chain(rows)
                .map(String::as_str)
                .collect(),
            Node::Copy { label, .. } => vec![label],
        }
    }
}

/// A band above a [`Node::Timeline`]'s ruler on its time axis, values
/// from `min` (bottom) to `max` (top): a line at `base` named `label`, its
/// `curves` as (seconds, value) points, and `dots` the person drags
/// anywhere in it, each kept within its own time span and the band's
/// values. A drag is [`UiEvent::Dragged`] as it goes and once more, `done`,
/// when let go.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lane {
    pub min: f32,
    pub max: f32,
    pub base: f32,
    pub label: String,
    pub curves: Vec<LaneCurve>,
    pub dots: Vec<LaneDot>,
}

/// A curve on a [`Lane`]; a `faint` one is drawn dashed and muted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaneCurve {
    pub points: Vec<[f32; 2]>,
    pub faint: bool,
}

/// A dot on a [`Lane`] at (seconds, value), dragged within `from`..`to`
/// seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaneDot {
    pub at: [f32; 2],
    pub from: f32,
    pub to: f32,
}

impl Lane {
    /// Its points and dots, each a node of a view's budget.
    fn nodes(&self) -> usize {
        self.curves.iter().map(|c| c.points.len()).sum::<usize>() + self.dots.len()
    }

    fn finite(&self) -> bool {
        let dots = self
            .dots
            .iter()
            .flat_map(|d| d.at.into_iter().chain([d.from, d.to]));
        [self.min, self.max, self.base]
            .into_iter()
            .chain(
                self.curves
                    .iter()
                    .flat_map(|c| c.points.iter().flatten().copied()),
            )
            .chain(dots)
            .all(f32::is_finite)
            && self.min < self.max
            && self.dots.iter().all(|d| d.from <= d.to)
    }
}

/// What a person did on a view. More may come in a later boundary
/// version: match with a wildcard arm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum UiEvent {
    Pressed {
        id: String,
    },
    Number {
        id: String,
        value: f64,
    },
    Chosen {
        id: String,
        index: usize,
    },
    Text {
        id: String,
        value: String,
    },
    Toggled {
        id: String,
        value: bool,
    },
    /// A [`Node::HoldButton`] went down, or was let go.
    Held {
        id: String,
        down: bool,
    },
    /// A [`Node::Timeline`]'s key `index` was dragged to `value` seconds.
    Moved {
        id: String,
        index: usize,
        value: f64,
    },
    /// A [`Node::Timeline`]'s ruler pressed or dragged away from its keys:
    /// the seconds under the cursor, as it goes.
    Scrubbed {
        id: String,
        at: f64,
    },
    /// A [`Lane`]'s dot `index` is dragged to `at` seconds and `value`;
    /// `done` once it is let go.
    Dragged {
        id: String,
        index: usize,
        at: f64,
        value: f64,
        done: bool,
    },
    /// The wheel turned `lines` (away from the person is positive) while
    /// a [`Node::HoldButton`] of the view was held.
    Wheel {
        lines: f64,
    },
    /// A [`Node::Scene3d`] dragged with the right button: the pixels the
    /// cursor moved since the last, right and down positive.
    Look {
        id: String,
        x: f64,
        y: f64,
    },
}

impl UiEvent {
    /// Whether the client delivers it: its text within [`TEXT_MAX`], its
    /// number finite.
    pub fn fits(&self) -> bool {
        match self {
            UiEvent::Text { value, .. } => value.len() <= TEXT_MAX,
            UiEvent::Number { value, .. } | UiEvent::Moved { value, .. } => value.is_finite(),
            UiEvent::Wheel { lines } => lines.is_finite(),
            UiEvent::Look { x, y, .. } => x.is_finite() && y.is_finite(),
            UiEvent::Dragged { at, value, .. } => at.is_finite() && value.is_finite(),
            UiEvent::Scrubbed { at, .. } => at.is_finite(),
            UiEvent::Pressed { .. }
            | UiEvent::Chosen { .. }
            | UiEvent::Toggled { .. }
            | UiEvent::Held { .. } => true,
        }
    }
}
