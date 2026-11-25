//! Views: the tree of nodes a page, window or modal shows, and the events its
//! widgets send back.

use crate::*;

use serde::{Deserialize, Serialize};

/// The tone of a status. The extension picks the meaning and the client picks
/// the color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tone {
    /// Working.
    Good,
    /// In progress, or blocked by something that may clear on its own.
    Waiting,
    /// Failed, and needs someone to act.
    Bad,
    /// Not in use.
    Idle,
}

/// A one-line status: an icon, a dot in its tone, and a label. [`Node::Status`]
/// shows it in a view, [`Command::Status`] in the client's status bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub tone: Tone,
    /// The text, which must be a label ([`is_label`]).
    pub label: String,
    /// A packaged icon, drawn first. In the status bar, a status without an
    /// icon is prefixed with the extension's name.
    #[serde(default)]
    pub icon: Option<String>,
    /// The page that pressing the status bar line opens; the manifest's first
    /// page if `None`. Unused in a view.
    #[serde(default)]
    pub page: Option<String>,
}

/// A view: a tree of widgets that the client renders, themes and makes
/// navigable by keyboard.
///
/// Each part is a struct with a `new` function and chainable setters
/// (`Button::new("go", "Go").primary()`), and converts into a `Node`. Fields
/// added in later versions have defaults, so existing modules keep working.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    Flow(Flow),
    Grid(Grid),
    /// Children stacked on top of each other, the first at the bottom. A press
    /// on an upper child doesn't reach the ones below it.
    Layers(Vec<Node>),
    Frame(Frame),
    Text(Text),
    Glyph(Glyph),
    /// A status line, in the client's status colors.
    Status(Status),
    /// A thin rule between groups.
    Divider,
    Button(Button),
    Input(Input),
    Number(Number),
    Slider(Slider),
    Toggle(Toggle),
    Choice(Choice),
    Plot(Plot),
    Scene(Scene),
}

/// Whether a field has its default value and can be left out when serialized.
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// Generates chainable setters.
macro_rules! setters {
    ($($(#[$doc:meta])* $kind:ident $name:ident $(: $ty:ty)?;)*) => {
        $(setters!(@$kind $(#[$doc])* $name $(: $ty)?);)*
    };
    (@text $(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        pub fn $name(mut self, value: impl Into<String>) -> Self {
            self.$name = value.into();
            self
        }
    };
    (@named $(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        pub fn $name(mut self, value: impl Into<String>) -> Self {
            self.$name = Some(value.into());
            self
        }
    };
    (@some $(#[$doc:meta])* $name:ident : $ty:ty) => {
        $(#[$doc])*
        pub fn $name(mut self, value: $ty) -> Self {
            self.$name = Some(value);
            self
        }
    };
    (@put $(#[$doc:meta])* $name:ident : $ty:ty) => {
        $(#[$doc])*
        pub fn $name(mut self, value: $ty) -> Self {
            self.$name = value;
            self
        }
    };
}

/// Implements `From` for each part, into the variant of the same name.
macro_rules! variants {
    ($into:ident: $($part:ident),*) => {
        $(impl From<$part> for $into {
            fn from(part: $part) -> $into {
                $into::$part(part)
            }
        })*
    };
}

variants!(
    Node: Flow, Grid, Frame, Text, Glyph, Status, Button, Input, Number, Slider, Toggle, Choice,
    Plot, Scene
);

/// A horizontal [`Flow`] of the given nodes: `row![a, b]`.
#[macro_export]
macro_rules! row {
    ($($child:expr),* $(,)?) => {
        $crate::Flow::row(vec![$($crate::Node::from($child)),*])
    };
}

/// A vertical [`Flow`] of the given nodes: `col![a, b]`.
#[macro_export]
macro_rules! col {
    ($($child:expr),* $(,)?) => {
        $crate::Flow::col(vec![$($crate::Node::from($child)),*])
    };
}

/// How much space a part takes along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Size {
    /// The part's default.
    #[default]
    Auto,
    /// As much as its content needs.
    Shrink,
    /// All the remaining space.
    Fill,
    /// A share of the remaining space, relative to the shares of its siblings.
    Share(u16),
    /// A fixed number of pixels.
    Px(f32),
}

/// Alignment within the available space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Align {
    #[default]
    Start,
    Middle,
    End,
}

/// The role of a color. The client's theme maps it to an actual color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Ink {
    #[default]
    Plain,
    Secondary,
    Muted,
    Faint,
    Accent,
    /// Something selected or in progress.
    Marked,
    Good,
    Bad,
}

/// The default gap between the children of a flow, in pixels.
pub const GAP: f32 = 8.0;

/// Children laid out in a row, or in a column if `down`. `gap` is the space
/// between them in pixels, and `align` positions them on the other axis. With
/// `wrap`, a row that runs out of width continues on the next line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Flow {
    pub children: Vec<Node>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub down: bool,
    #[serde(default = "gap")]
    pub gap: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub align: Align,
    #[serde(default, skip_serializing_if = "is_default")]
    pub wrap: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub width: Size,
    #[serde(default, skip_serializing_if = "is_default")]
    pub height: Size,
}

fn gap() -> f32 {
    GAP
}

impl Flow {
    pub fn row(children: Vec<Node>) -> Self {
        Self {
            children,
            down: false,
            gap: GAP,
            align: Align::Start,
            wrap: false,
            width: Size::Auto,
            height: Size::Auto,
        }
    }

    pub fn col(children: Vec<Node>) -> Self {
        Self {
            down: true,
            ..Self::row(children)
        }
    }

    pub fn push(mut self, child: impl Into<Node>) -> Self {
        self.children.push(child.into());
        self
    }

    pub fn extend<N: Into<Node>>(mut self, children: impl IntoIterator<Item = N>) -> Self {
        self.children.extend(children.into_iter().map(Into::into));
        self
    }

    setters! {
        put gap: f32;
        put align: Align;
        put wrap: bool;
        put width: Size;
        put height: Size;
    }
}

/// Children in rows of `columns` equal cells (1 to [`COLUMNS_MAX`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Grid {
    pub children: Vec<Node>,
    pub columns: u8,
    #[serde(default = "gap")]
    pub gap: f32,
}

impl Grid {
    pub fn new(columns: u8, children: Vec<Node>) -> Self {
        Self {
            children,
            columns,
            gap: GAP / 2.0,
        }
    }

    setters! {
        put gap: f32;
    }
}

/// A frame's background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Surface {
    #[default]
    None,
    /// Recessed, like a side panel.
    Inset,
    /// Elevated, like a card.
    Raised,
}

/// The directions a frame scrolls in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Scroll {
    #[default]
    None,
    Down,
    Across,
    Both,
}

/// A container for one node, with its own size, padding, background, scrolling
/// and alignment (horizontal, vertical). Each entry in `keys` binds a key to an
/// id, which it holds like the key of an [`Act::Hold`] button.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Frame {
    pub child: Box<Node>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub width: Size,
    #[serde(default, skip_serializing_if = "is_default")]
    pub height: Size,
    #[serde(default, skip_serializing_if = "is_default")]
    pub max_width: Option<f32>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub max_height: Option<f32>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub padding: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub surface: Surface,
    #[serde(default, skip_serializing_if = "is_default")]
    pub scroll: Scroll,
    #[serde(default, skip_serializing_if = "is_default")]
    pub align: (Align, Align),
    #[serde(default, skip_serializing_if = "is_default")]
    pub keys: Vec<(Key, String)>,
}

impl Frame {
    pub fn new(child: impl Into<Node>) -> Self {
        Self {
            child: Box::new(child.into()),
            width: Size::Auto,
            height: Size::Auto,
            max_width: None,
            max_height: None,
            padding: 0.0,
            surface: Surface::None,
            scroll: Scroll::None,
            align: (Align::Start, Align::Start),
            keys: Vec::new(),
        }
    }

    setters! {
        put width: Size;
        put height: Size;
        some max_width: f32;
        some max_height: f32;
        put padding: f32;
        put surface: Surface;
        put scroll: Scroll;
        put align: (Align, Align);
        put keys: Vec<(Key, String)>;
    }
}

/// Text sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextSize {
    Small,
    #[default]
    Body,
    Heading,
    Title,
}

/// Text, in the client's monospace font if `mono`. With `clip` it stays on one
/// line, cut off where it runs out of space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Text {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub size: TextSize,
    #[serde(default, skip_serializing_if = "is_default")]
    pub ink: Ink,
    #[serde(default, skip_serializing_if = "is_default")]
    pub mono: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub clip: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub width: Size,
    #[serde(default, skip_serializing_if = "is_default")]
    pub align: Align,
}

impl Text {
    /// Body text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: TextSize::Body,
            ink: Ink::Secondary,
            mono: false,
            clip: false,
            width: Size::Auto,
            align: Align::Start,
        }
    }

    /// The heading of a group.
    pub fn heading(text: impl Into<String>) -> Self {
        Self::new(text).size(TextSize::Heading).ink(Ink::Plain)
    }

    setters! {
        put size: TextSize;
        put ink: Ink;
        put mono: bool;
        put clip: bool;
        put width: Size;
        put align: Align;
    }
}

impl From<&str> for Node {
    fn from(text: &str) -> Node {
        Text::new(text).into()
    }
}

impl From<String> for Node {
    fn from(text: String) -> Node {
        Text::new(text).into()
    }
}

/// A packaged icon ([`Manifest::icons`]), `px` pixels high. An unknown name
/// draws nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Glyph {
    pub icon: String,
    #[serde(default = "icon_px")]
    pub px: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub ink: Ink,
}

fn icon_px() -> f32 {
    18.0
}

impl Glyph {
    pub fn new(icon: impl Into<String>) -> Self {
        Self {
            icon: icon.into(),
            px: icon_px(),
            ink: Ink::Plain,
        }
    }

    setters! {
        put px: f32;
        put ink: Ink;
    }
}

/// A button's style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ButtonKind {
    #[default]
    Secondary,
    /// The main action of the view.
    Primary,
    /// A destructive action.
    Danger,
    /// No background until hovered.
    Ghost,
    /// A list row: as wide as the list, with its label at the start.
    Row,
}

/// What a button does when pressed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum Act {
    /// Sends [`UiEvent::Pressed`].
    #[default]
    Press,
    /// Sends [`UiEvent::Chosen`] with this index, as one of several buttons
    /// that share an id.
    Choose(usize),
    /// Can be held down: sends [`UiEvent::Held`] with `down` set when pressed
    /// and unset when released.
    ///
    /// The client releases it on any mouse release, when its view closes or no
    /// longer has it, and when the window loses focus. In a modal or a window
    /// the key also holds it, unless a text field there has focus. Several
    /// buttons can be held at once, and while any is held the mouse wheel sends
    /// [`UiEvent::Wheel`] instead of scrolling or zooming. A disabled button
    /// can't be held, but releasing its key still releases it.
    Hold(Option<Key>),
    /// Deletes something: sends [`UiEvent::Pressed`] on the second of two
    /// presses. The first arms the button for [`DELETE_SECONDS`], shown in the
    /// danger style; any other use of a view disarms it. The client draws its
    /// own icon, in place of the button's.
    Delete,
    /// Copies the text to the clipboard. The extension receives no event.
    Copy(String),
    /// Opens the address in the browser, if it is `https`. The extension
    /// receives no event.
    Open(String),
}

/// A button with a label, an icon or both, and an optional tooltip (`tip`).
/// `selected` marks it as the current choice, and `large` makes it taller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Button {
    pub id: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub label: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub tip: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: ButtonKind,
    #[serde(default, skip_serializing_if = "is_default")]
    pub act: Act,
    #[serde(default, skip_serializing_if = "is_default")]
    pub selected: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub large: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub width: Size,
}

impl Button {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            tip: None,
            kind: ButtonKind::Secondary,
            act: Act::Press,
            selected: false,
            disabled: false,
            large: false,
            width: Size::Auto,
        }
    }

    /// A button with only an icon. `tip` is its tooltip.
    pub fn icon_only(
        id: impl Into<String>,
        icon: impl Into<String>,
        tip: impl Into<String>,
    ) -> Self {
        Self::new(id, "").icon(icon).tip(tip)
    }

    /// An icon button that can be held with the mouse and, in a modal or a
    /// window, with `key`.
    pub fn hold(
        id: impl Into<String>,
        icon: impl Into<String>,
        tip: impl Into<String>,
        key: Option<Key>,
    ) -> Self {
        Self::icon_only(id, icon, tip).act(Act::Hold(key))
    }

    /// An [`Act::Delete`] button.
    pub fn delete(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label).act(Act::Delete)
    }

    pub fn copy(label: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new("", label).act(Act::Copy(text.into()))
    }

    pub fn link(label: impl Into<String>, url: impl Into<String>) -> Self {
        Self::new("", label)
            .kind(ButtonKind::Ghost)
            .act(Act::Open(url.into()))
    }

    pub fn primary(self) -> Self {
        self.kind(ButtonKind::Primary)
    }

    /// Fills the available width.
    pub fn wide(self) -> Self {
        self.width(Size::Fill)
    }

    /// One button per option, each a label and an icon name (empty for none).
    /// Pressing one sends [`UiEvent::Chosen`] with its index.
    pub fn each<'a>(
        id: &str,
        options: impl IntoIterator<Item = (&'a str, &'a str)>,
        selected: Option<usize>,
    ) -> Vec<Button> {
        options
            .into_iter()
            .enumerate()
            .map(|(i, (label, icon))| Button {
                icon: (!icon.is_empty()).then(|| icon.to_owned()),
                ..Button::new(id, label)
                    .act(Act::Choose(i))
                    .selected(selected == Some(i))
                    .wide()
            })
            .collect()
    }

    /// [`Button::each`], in a grid of `columns` columns.
    pub fn grid<'a>(
        id: &str,
        options: impl IntoIterator<Item = (&'a str, &'a str)>,
        selected: Option<usize>,
        columns: u8,
    ) -> Grid {
        let buttons = Self::each(id, options, selected);
        Grid::new(columns, buttons.into_iter().map(Node::from).collect())
    }

    /// [`Button::each`], as list rows.
    pub fn list<'a>(
        id: &str,
        options: impl IntoIterator<Item = &'a str>,
        selected: Option<usize>,
    ) -> Node {
        let rows = Self::each(id, options.into_iter().map(|o| (o, "")), selected);
        Flow::col(Vec::new())
            .extend(rows.into_iter().map(|b| b.kind(ButtonKind::Row)))
            .gap(GAP / 2.0)
            .into()
    }

    setters! {
        text label;
        named icon;
        named tip;
        put kind: ButtonKind;
        put act: Act;
        put selected: bool;
        put disabled: bool;
        put large: bool;
        put width: Size;
    }
}

/// A text field. Enter sends [`UiEvent::Text`]; with `live`, so does every
/// change. A `secret` field masks its text, and `hint` shows while it is empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Input {
    pub id: String,
    pub label: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub hint: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub live: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub secret: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub width: Size,
}

impl Input {
    pub fn new(id: impl Into<String>, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value: value.into(),
            hint: String::new(),
            live: false,
            secret: false,
            width: Size::Auto,
        }
    }

    setters! {
        text hint;
        put live: bool;
        put secret: bool;
        put width: Size;
    }
}

/// A number field. Enter sends [`UiEvent::Number`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Number {
    pub id: String,
    pub label: String,
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub width: Size,
}

impl Number {
    pub fn new(id: impl Into<String>, label: impl Into<String>, value: Option<f64>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value,
            width: Size::Auto,
        }
    }

    setters! {
        put width: Size;
    }
}

/// A slider between `min` and `max`. Its scale is linear, or logarithmic with
/// `log` (`min` must then be above 0), and its values are rounded to
/// [`SLIDER_FIGURES`] significant figures ([`significant`]). Releasing it sends
/// [`UiEvent::Number`]; with `live`, so does every movement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Slider {
    pub id: String,
    pub label: String,
    pub min: f64,
    pub max: f64,
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub log: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub live: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub width: Size,
}

impl Slider {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        (min, max): (f64, f64),
        value: Option<f64>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            min,
            max,
            value,
            log: false,
            live: false,
            width: Size::Auto,
        }
    }

    setters! {
        put log: bool;
        put live: bool;
        put width: Size;
    }
}

/// A switch. Pressing it sends [`UiEvent::Toggled`] with the opposite state, or
/// with `true` if the state is unknown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Toggle {
    pub id: String,
    pub label: String,
    pub value: Option<bool>,
}

impl Toggle {
    pub fn new(id: impl Into<String>, label: impl Into<String>, value: Option<bool>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value,
        }
    }
}

/// A drop-down of `options`. Choosing one sends [`UiEvent::Chosen`] with its
/// index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Choice {
    pub id: String,
    pub options: Vec<String>,
    pub selected: Option<usize>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub width: Size,
}

impl Choice {
    pub fn new(id: impl Into<String>, options: Vec<String>, selected: Option<usize>) -> Self {
        Self {
            id: id.into(),
            options,
            selected,
            width: Size::Auto,
        }
    }

    setters! {
        put width: Size;
    }
}

/// The marks along a plot's axis, which dragged points snap to.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum Marks {
    #[default]
    None,
    /// Round values at a readable density for the current zoom, labeled with
    /// this unit.
    Round(String),
    /// Labeled values.
    At(Vec<(f32, String)>),
}

/// A plot's axis, from one value to another.
///
/// With `zoom`, the mouse wheel zooms around the cursor, Shift and the wheel or
/// a middle-button drag pans, and a double-click away from the points resets
/// the view. With `grows`, dragging a point past the end extends the axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Axis {
    pub from: f32,
    pub to: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub marks: Marks,
    #[serde(default, skip_serializing_if = "is_default")]
    pub zoom: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub grows: bool,
}

impl Axis {
    pub fn new(from: f32, to: f32) -> Self {
        Self {
            from,
            to,
            marks: Marks::None,
            zoom: false,
            grows: false,
        }
    }

    setters! {
        put marks: Marks;
        put zoom: bool;
        put grows: bool;
    }
}

/// A line style.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Stroke {
    pub ink: Ink,
    pub width: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub dashed: bool,
}

impl Stroke {
    pub fn new(ink: Ink, width: f32) -> Self {
        Self {
            ink,
            width,
            dashed: false,
        }
    }

    setters! {
        put dashed: bool;
    }
}

/// A segment of a curve, starting where the previous one ended.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Piece {
    Line([f32; 2]),
    /// A cubic Bézier curve: two control points, then the end point.
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    /// Moves to a point without drawing.
    Move([f32; 2]),
}

impl Piece {
    fn coordinates(&self) -> impl Iterator<Item = f32> {
        let points = match *self {
            Piece::Line(to) | Piece::Move(to) => [to; 3],
            Piece::Cubic(a, b, to) => [a, b, to],
        };
        points.into_iter().flatten()
    }
}

/// The shape of a plot's point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PointShape {
    #[default]
    Diamond,
    Circle,
    Square,
}

/// How a plot's point can be dragged. `x` and `y` are the range it can move in
/// along each axis, without an upper limit if the second value is `None`; it
/// can't move along an axis without a range.
///
/// A dragged point snaps to the axes' marks and to its starting position,
/// unless Alt is held. Releasing it sends [`UiEvent::Dragged`]; with `live`, so
/// does every movement.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Drag {
    #[serde(default, skip_serializing_if = "is_default")]
    pub x: Option<(f32, Option<f32>)>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub y: Option<(f32, Option<f32>)>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub live: bool,
}

impl Drag {
    pub fn along_x(mut self, from: f32, to: Option<f32>) -> Self {
        self.x = Some((from, to));
        self
    }

    pub fn along_y(mut self, from: f32, to: Option<f32>) -> Self {
        self.y = Some((from, to));
        self
    }

    setters! {
        put live: bool;
    }
}

/// An item in a plot, positioned in axis values. Items are drawn in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Drawn {
    Curve(Curve),
    Point(Point),
    Line(Line),
}

/// A curve that starts at `from` and follows `pieces`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Curve {
    pub from: [f32; 2],
    pub pieces: Vec<Piece>,
    pub stroke: Stroke,
}

impl Curve {
    pub fn new(from: [f32; 2], pieces: Vec<Piece>, stroke: Stroke) -> Self {
        Self {
            from,
            pieces,
            stroke,
        }
    }
}

/// A point. Pressing it sends [`UiEvent::Chosen`] with its index among the
/// plot's points. `stem` draws a line from it down to the x axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Point {
    pub at: [f32; 2],
    #[serde(default, skip_serializing_if = "is_default")]
    pub shape: PointShape,
    #[serde(default, skip_serializing_if = "is_default")]
    pub ink: Ink,
    #[serde(default, skip_serializing_if = "is_default")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub selected: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub stem: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub drag: Option<Drag>,
}

impl Point {
    pub fn new(at: [f32; 2]) -> Self {
        Self {
            at,
            shape: PointShape::Diamond,
            ink: Ink::Accent,
            label: None,
            selected: false,
            stem: false,
            drag: None,
        }
    }

    setters! {
        put shape: PointShape;
        put ink: Ink;
        named label;
        put selected: bool;
        put stem: bool;
        some drag: Drag;
    }
}

/// A line at a value of the x axis, or of the y axis with `across`. Hidden if
/// `at` is `None`. [`Command::Mark`] moves it without resending the view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Line {
    pub at: Option<f32>,
    pub stroke: Stroke,
    #[serde(default, skip_serializing_if = "is_default")]
    pub across: bool,
}

impl Line {
    pub fn new(at: Option<f32>, stroke: Stroke) -> Self {
        Self {
            at,
            stroke,
            across: false,
        }
    }

    setters! {
        put across: bool;
    }
}

/// Items plotted over two axes: x from left to right with its marks below, y
/// from bottom to top with its marks on the left.
///
/// With `scrub`, pressing away from the points and dragging from there send
/// [`UiEvent::Scrubbed`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Plot {
    pub id: String,
    pub x: Axis,
    pub y: Axis,
    pub items: Vec<Drawn>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub scrub: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub width: Size,
    #[serde(default, skip_serializing_if = "is_default")]
    pub height: Size,
}

impl Plot {
    pub fn new(id: impl Into<String>, x: Axis, y: Axis) -> Self {
        Self {
            id: id.into(),
            x,
            y,
            items: Vec::new(),
            scrub: false,
            width: Size::Auto,
            height: Size::Auto,
        }
    }

    pub fn push(mut self, item: impl Into<Drawn>) -> Self {
        self.items.push(item.into());
        self
    }

    pub fn extend<I: Into<Drawn>>(mut self, items: impl IntoIterator<Item = I>) -> Self {
        self.items.extend(items.into_iter().map(Into::into));
        self
    }

    /// The plot's points in order, as indexed by [`UiEvent::Chosen`] and
    /// [`UiEvent::Dragged`].
    pub fn points(&self) -> impl Iterator<Item = &Point> {
        self.items.iter().filter_map(|i| match i {
            Drawn::Point(p) => Some(p),
            _ => None,
        })
    }

    setters! {
        put scrub: bool;
        put width: Size;
        put height: Size;
    }

    fn nodes(&self) -> usize {
        self.items
            .iter()
            .map(|i| match i {
                Drawn::Curve(c) => 1 + c.pieces.len(),
                Drawn::Point(_) | Drawn::Line(_) => 1,
            })
            .sum()
    }

    fn finite(&self) -> bool {
        let axis = |a: &Axis| {
            a.from.is_finite()
                && a.to.is_finite()
                && a.from < a.to
                && match &a.marks {
                    Marks::At(places) => places.iter().all(|p| p.0.is_finite()),
                    _ => true,
                }
        };
        let way = |bounds: &Option<(f32, Option<f32>)>| {
            bounds.is_none_or(|(low, high)| {
                low.is_finite() && high.is_none_or(|h| h.is_finite() && low <= h)
            })
        };
        let stroke = |s: &Stroke| s.width.is_finite();
        axis(&self.x)
            && axis(&self.y)
            && self.items.iter().all(|i| match i {
                Drawn::Curve(c) => {
                    stroke(&c.stroke)
                        && c.from.iter().all(|v| v.is_finite())
                        && c.pieces
                            .iter()
                            .flat_map(Piece::coordinates)
                            .all(f32::is_finite)
                }
                Drawn::Point(p) => {
                    p.at.iter().all(|v| v.is_finite())
                        && p.drag.is_none_or(|d| way(&d.x) && way(&d.y))
                }
                Drawn::Line(l) => stroke(&l.stroke) && l.at.is_none_or(f32::is_finite),
            })
    }
}

variants!(Drawn: Curve, Point, Line);

/// A line through points in 3D.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Path3 {
    pub points: Vec<[f32; 3]>,
    pub stroke: Stroke,
}

impl Path3 {
    pub fn new(points: Vec<[f32; 3]>, stroke: Stroke) -> Self {
        Self { points, stroke }
    }
}

/// A point in 3D. Pressing it sends [`UiEvent::Chosen`] with its index among
/// the scene's points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Point3 {
    pub at: [f32; 3],
    #[serde(default, skip_serializing_if = "is_default")]
    pub ink: Ink,
    #[serde(default, skip_serializing_if = "is_default")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub selected: bool,
}

impl Point3 {
    pub fn new(at: [f32; 3]) -> Self {
        Self {
            at,
            ink: Ink::Accent,
            label: None,
            selected: false,
        }
    }

    setters! {
        put ink: Ink;
        named label;
        put selected: bool;
    }
}

/// A camera's position, forward direction and up direction.
pub type Pose3 = [[f32; 3]; 3];

/// A 3D scene of paths, points and cameras.
///
/// Dragging orbits, a middle-button drag pans, the mouse wheel zooms, and a
/// double-click resets the view. The view is fitted to the points, or to the
/// paths if there are none, or else to the cameras. A camera outside the view
/// is marked at its edge. A right-button drag hides the cursor and sends
/// [`UiEvent::Look`], at most once per frame; during it, the wheel sends
/// [`UiEvent::Wheel`] instead of zooming.
///
/// `empty` is shown when there is nothing to draw. `hint` is shown in the
/// bottom right corner, below the list of mouse controls. Its lines don't
/// wrap. [`Command::Mark`]
/// moves a camera without resending the view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Scene {
    pub id: String,
    #[serde(default)]
    pub paths: Vec<Path3>,
    #[serde(default)]
    pub points: Vec<Point3>,
    #[serde(default)]
    pub cameras: Vec<Pose3>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub empty: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub hint: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub width: Size,
    #[serde(default, skip_serializing_if = "is_default")]
    pub height: Size,
}

impl Scene {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            paths: Vec::new(),
            points: Vec::new(),
            cameras: Vec::new(),
            empty: String::new(),
            hint: String::new(),
            width: Size::Auto,
            height: Size::Auto,
        }
    }

    setters! {
        put paths: Vec<Path3>;
        put points: Vec<Point3>;
        put cameras: Vec<Pose3>;
        text empty;
        text hint;
        put width: Size;
        put height: Size;
    }

    fn nodes(&self) -> usize {
        let path: usize = self.paths.iter().map(|p| 1 + p.points.len()).sum();
        path + self.points.len() + self.cameras.len()
    }

    fn finite(&self) -> bool {
        let points = self.paths.iter().flat_map(|p| &p.points);
        points
            .chain(self.points.iter().map(|p| &p.at))
            .chain(self.cameras.iter().flatten())
            .flatten()
            .all(|v| v.is_finite())
            && self.paths.iter().all(|p| p.stroke.width.is_finite())
    }
}

/// A change [`Command::Mark`] makes to a view that is already shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Mark {
    /// Moves the [`Scene`] camera at this index.
    Camera(usize, Pose3),
    /// Moves the [`Plot`] line at this index among the plot's lines.
    Line(usize, Option<f32>),
}

impl Mark {
    /// Whether all its numbers are finite.
    pub fn finite(&self) -> bool {
        match self {
            Mark::Camera(_, pose) => pose.iter().flatten().all(|c| c.is_finite()),
            Mark::Line(_, at) => at.is_none_or(f32::is_finite),
        }
    }
}

/// The most columns in a [`Grid`].
pub const COLUMNS_MAX: u8 = 8;

/// The most nodes in a view. A choice's options, a frame's keys, and the points
/// and pieces of a scene or a plot each count as one.
pub const VIEW_NODES_MAX: usize = 4096;

/// The deepest a view can nest.
pub const VIEW_DEPTH_MAX: usize = 32;

/// The most text in a view, in bytes, without its copy texts. Each string is at
/// most [`TEXT_MAX`].
pub const VIEW_TEXT_MAX: usize = 64 << 10;

/// The most text in a view's [`Act::Copy`] buttons, in bytes.
pub const VIEW_COPY_MAX: usize = 256 << 10;

/// The largest size, gap or padding, in pixels.
pub const PX_MAX: f32 = 4096.0;

/// A key that can be bound in a view: a letter or a digit (`Char`, lower case),
/// Space, Shift or an arrow.
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
    /// Whether a view can bind it: a `Char` must be a–z or 0–9.
    pub fn bindable(self) -> bool {
        match self {
            Key::Char(c) => c.is_ascii_lowercase() || c.is_ascii_digit(),
            _ => true,
        }
    }
}

/// The significant figures a [`Slider`]'s value is rounded to.
pub const SLIDER_FIGURES: u32 = 2;

/// `value` rounded to `figures` significant figures. Zero and non-finite values
/// are returned as they are.
pub fn significant(value: f64, figures: u32) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    let scale = 10f64.powi(figures as i32 - 1 - value.abs().log10().floor() as i32);
    (value * scale).round() / scale
}

/// The client's tick rate per second: the most often a timer can fire.
pub const TICK_HZ: u32 = 60;

/// How long a pressed [`Act::Delete`] button waits for its second press.
pub const DELETE_SECONDS: u64 = 3;

/// What remains of a view's limits while it is being checked.
struct ViewBudget {
    nodes: usize,
    text: usize,
    copy: usize,
}

/// Subtracts `n` from `left`. False if `left` is too small.
fn take(left: &mut usize, n: usize) -> bool {
    left.checked_sub(n).map(|rest| *left = rest).is_some()
}

impl Node {
    /// Whether the client accepts the view: it is within [`VIEW_NODES_MAX`],
    /// [`VIEW_DEPTH_MAX`], [`VIEW_TEXT_MAX`] and [`VIEW_COPY_MAX`], its numbers
    /// are finite, its sizes are at most [`PX_MAX`], its ranges are ordered
    /// (and above 0 for a `log` slider), and its keys can be bound.
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
        let entries = match self {
            Node::Choice(c) => c.options.len(),
            Node::Plot(p) => p.nodes(),
            Node::Scene(s) => s.nodes(),
            Node::Frame(f) => f.keys.len(),
            _ => 0,
        };
        let copy = match self {
            Node::Button(Button {
                act: Act::Copy(text),
                ..
            }) => text.len(),
            _ => 0,
        };
        let texts = self.texts();
        self.sound()
            && texts.iter().all(|t| t.len() <= TEXT_MAX)
            && take(&mut budget.nodes, 1 + entries)
            && take(&mut budget.text, texts.iter().map(|t| t.len()).sum())
            && take(&mut budget.copy, copy)
            && self.children().iter().all(|c| c.within(depth + 1, budget))
    }

    /// Whether the node's own numbers, ranges and keys are valid.
    fn sound(&self) -> bool {
        let px = |v: f32| (0.0..=PX_MAX).contains(&v);
        let size = |s: Size| match s {
            Size::Px(v) => px(v),
            _ => true,
        };
        match self {
            Node::Flow(f) => px(f.gap) && size(f.width) && size(f.height),
            Node::Grid(g) => px(g.gap),
            Node::Frame(f) => {
                px(f.padding)
                    && size(f.width)
                    && size(f.height)
                    && f.max_width.is_none_or(px)
                    && f.max_height.is_none_or(px)
                    && f.keys.iter().all(|(key, _)| key.bindable())
            }
            Node::Text(t) => size(t.width),
            Node::Glyph(g) => px(g.px),
            Node::Status(status) => is_label(&status.label),
            Node::Button(b) => {
                size(b.width)
                    && match b.act {
                        Act::Hold(key) => key.is_none_or(Key::bindable),
                        _ => true,
                    }
            }
            Node::Input(i) => size(i.width),
            Node::Number(n) => size(n.width),
            Node::Slider(s) => {
                [s.min, s.max].iter().chain(&s.value).all(|v| v.is_finite())
                    && s.min < s.max
                    && (!s.log || s.min > 0.0)
                    && size(s.width)
            }
            Node::Choice(c) => size(c.width),
            Node::Plot(p) => p.finite() && size(p.width) && size(p.height),
            Node::Scene(s) => s.finite() && size(s.width) && size(s.height),
            Node::Layers(_) | Node::Divider | Node::Toggle(_) => true,
        }
    }

    /// The node's direct children, in order.
    pub fn children(&self) -> &[Node] {
        match self {
            Node::Flow(f) => &f.children,
            Node::Grid(g) => &g.children,
            Node::Layers(children) => children,
            Node::Frame(f) => std::slice::from_ref(&*f.child),
            _ => &[],
        }
    }

    fn children_mut(&mut self) -> &mut [Node] {
        match self {
            Node::Flow(f) => &mut f.children,
            Node::Grid(g) => &mut g.children,
            Node::Layers(children) => children,
            Node::Frame(f) => std::slice::from_mut(&mut *f.child),
            _ => &mut [],
        }
    }

    /// Calls `see` on the node and then on each of its descendants, parents
    /// first.
    pub fn each<'a>(&'a self, see: &mut impl FnMut(&'a Node)) {
        see(self);
        for child in self.children() {
            child.each(see);
        }
    }

    /// Everything that can be held in the view: its id, its key, and whether it
    /// is enabled.
    pub fn holds(&self) -> Vec<(&str, Option<Key>, bool)> {
        let mut holds = Vec::new();
        self.each(&mut |node| match node {
            Node::Button(b) => {
                if let Act::Hold(key) = b.act {
                    holds.push((b.id.as_str(), key, !b.disabled));
                }
            }
            Node::Frame(f) => {
                let keys = f.keys.iter();
                holds.extend(keys.map(|(key, id)| (id.as_str(), Some(*key), true)));
            }
            _ => {}
        });
        holds
    }

    /// Applies `mark` to the node with this id and of the mark's kind. True if
    /// it changed anything.
    pub fn mark(&mut self, id: &str, mark: &Mark) -> bool {
        match (&mut *self, mark) {
            (Node::Scene(s), Mark::Camera(at, pose)) if s.id == id => s
                .cameras
                .get_mut(*at)
                .is_some_and(|c| std::mem::replace(c, *pose) != *pose),
            (Node::Plot(p), Mark::Line(at, to)) if p.id == id => p
                .items
                .iter_mut()
                .filter_map(|i| match i {
                    Drawn::Line(l) => Some(l),
                    _ => None,
                })
                .nth(*at)
                .is_some_and(|l| std::mem::replace(&mut l.at, *to) != *to),
            _ => self.children_mut().iter_mut().any(|c| c.mark(id, mark)),
        }
    }

    /// The node's own strings, without its children's and its copy text.
    fn texts(&self) -> Vec<&str> {
        match self {
            Node::Flow(_) | Node::Grid(_) | Node::Layers(_) | Node::Divider => Vec::new(),
            Node::Frame(f) => f.keys.iter().map(|(_, id)| id.as_str()).collect(),
            Node::Text(t) => vec![&t.text],
            Node::Glyph(g) => vec![&g.icon],
            Node::Status(s) => std::iter::once(&s.label)
                .chain(&s.icon)
                .map(String::as_str)
                .collect(),
            Node::Button(b) => {
                let url = match &b.act {
                    Act::Open(url) => Some(url),
                    _ => None,
                };
                [&b.id, &b.label]
                    .into_iter()
                    .chain(&b.icon)
                    .chain(&b.tip)
                    .chain(url)
                    .map(String::as_str)
                    .collect()
            }
            Node::Input(i) => vec![&i.id, &i.label, &i.value, &i.hint],
            Node::Number(n) => vec![&n.id, &n.label],
            Node::Slider(s) => vec![&s.id, &s.label],
            Node::Toggle(t) => vec![&t.id, &t.label],
            Node::Choice(c) => std::iter::once(&c.id)
                .chain(&c.options)
                .map(String::as_str)
                .collect(),
            Node::Plot(p) => {
                let marks = [&p.x.marks, &p.y.marks].into_iter().flat_map(|m| match m {
                    Marks::None => Vec::new(),
                    Marks::Round(unit) => vec![unit.as_str()],
                    Marks::At(places) => places.iter().map(|(_, name)| name.as_str()).collect(),
                });
                std::iter::once(p.id.as_str())
                    .chain(marks)
                    .chain(p.points().filter_map(|p| p.label.as_deref()))
                    .collect()
            }
            Node::Scene(s) => [&s.id, &s.empty, &s.hint]
                .into_iter()
                .chain(s.points.iter().filter_map(|p| p.label.as_ref()))
                .map(String::as_str)
                .collect(),
        }
    }
}

/// What someone did in a view. Later versions may add variants, so match with a
/// wildcard arm.
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
    /// Something that can be held ([`Act::Hold`], [`Frame`]'s `keys`) was
    /// pressed or released.
    Held {
        id: String,
        down: bool,
    },
    /// A [`Plot`] was pressed or dragged away from its points. `at` is the x
    /// value under the cursor.
    Scrubbed {
        id: String,
        at: f64,
    },
    /// The point `index` of a [`Plot`] was dragged to `at` on the x axis and
    /// `value` on the y axis. `done` is set when it is released.
    Dragged {
        id: String,
        index: usize,
        at: f64,
        value: f64,
        done: bool,
    },
    /// The mouse wheel turned by `lines`, positive away from the user, while
    /// something in the view was held or a [`Scene`] was being dragged with the
    /// right button.
    Wheel {
        lines: f64,
    },
    /// A [`Scene`] was dragged with the right button, by this many pixels since
    /// the last event. Right and down are positive.
    Look {
        id: String,
        x: f64,
        y: f64,
    },
}

impl UiEvent {
    /// Whether the client delivers it: its text is within [`TEXT_MAX`] and its
    /// numbers are finite.
    pub fn fits(&self) -> bool {
        match self {
            UiEvent::Text { value, .. } => value.len() <= TEXT_MAX,
            UiEvent::Number { value, .. } => value.is_finite(),
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
