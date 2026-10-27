//! What a module declares: its identity, the namespace it provides, the
//! pages, actions and conditions a mapping offers, and what it may touch.

use crate::*;

use serde::{Deserialize, Serialize};

/// What an extension declares. The client approves it at install and holds
/// the extension to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Unique within its repo: the identity is the repo and this name.
    pub name: String,
    /// Who made it: shown, and named where it is missing.
    pub author: String,
    pub version: String,
    /// The Wex namespace it provides.
    pub namespace: Option<Namespace>,
    /// OSC address prefixes it receives (`/usercamera/`).
    pub osc_read: Vec<String>,
    /// OSC address prefixes it may send to.
    pub osc_write: Vec<String>,
    /// Its pages in the client's right panel.
    pub pages: Vec<Page>,
    /// The actions the client's action editor offers for it.
    pub actions: Vec<Action>,
    /// The conditions the client's trigger editor offers for it.
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// The secrets it keeps in the OS keyring, by slot name
    /// ([`Command::StoreSecret`]).
    #[serde(default)]
    pub secrets: Vec<String>,
    /// It shows notifications ([`Command::Notify`]).
    #[serde(default)]
    pub notifies: bool,
    /// Where it may connect and listen ([`Command::Http`],
    /// [`Command::Open`]); nothing else is reachable.
    #[serde(default)]
    pub network: Vec<NetGrant>,
    /// The VRChat log lines it receives ([`Event::Log`]): those whose
    /// message (after the time and level) starts with one of these. The
    /// log names players.
    #[serde(default)]
    pub log_prefixes: Vec<String>,
    /// The icons its views may show ([`Node::Icon`]), at most
    /// [`ICONS_MAX`], each SVG at most [`ICON_MAX`] bytes and self-contained
    /// ([`icon_is_self_contained`]).
    #[serde(default)]
    pub icons: Vec<Icon>,
    /// It knows the contacts the person controls in a live session
    /// ([`Event::Contacts`]) and fires its own contact methods
    /// ([`Method::contacts`]) on their copy of it ([`Command::Fire`]).
    #[serde(default)]
    pub contacts: bool,
}

/// An icon a manifest packages: a name its views use, and its SVG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Icon {
    pub name: String,
    pub svg: String,
}

/// The most icons a manifest packages.
pub const ICONS_MAX: usize = 64;

/// The largest icon's SVG.
pub const ICON_MAX: usize = 8 << 10;

/// Whether `svg` draws from itself alone: no image, no reference outside
/// it (an `href` or a `url(` to anything but `#…`), no entity, import,
/// script or foreign object. SVG renderers load what an image names from
/// disk, and a device path never ends.
pub fn icon_is_self_contained(svg: &str) -> bool {
    let s = svg.to_ascii_lowercase();
    let banned = [
        "<image",
        "feimage",
        "<!entity",
        "<!doctype",
        "@import",
        "<script",
        "<foreignobject",
    ];
    // What follows `at`, past spaces, `=`, `(` and quotes.
    let local = |at: usize| {
        s[at..]
            .trim_start_matches(|c: char| c.is_ascii_whitespace() || "=(\"'".contains(c))
            .starts_with('#')
    };
    !banned.iter().any(|b| s.contains(b))
        && s.match_indices("href").all(|(i, _)| local(i + 4))
        && s.match_indices("url(").all(|(i, _)| local(i + 4))
}

/// A contact as an extension knows it: an id that is its own (another
/// extension's id for the same contact differs), and the person's label for
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub label: String,
}

/// An action a mapping can fire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    /// What the action editor lists it as (`Play track`).
    pub label: String,
    pub does: Does,
    /// It is offered as a macro step too; false keeps it out of macros.
    #[serde(default = "offered")]
    pub in_macros: bool,
}

fn offered() -> bool {
    true
}

/// A condition a mapping can trigger on: one of its fields as the client
/// last received it, compared in the client (never a call into the
/// extension). Only what it declares is offered: not every field is
/// meaningful to compare.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// What the trigger editor lists it as (`Camera mode`).
    pub label: String,
    /// A field of a [`Shape::Bool`] or [`Shape::Number`]: what the client
    /// compares.
    pub field: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Does {
    /// Calls the method; the form takes its parameters.
    Call(String),
    /// Sets one of its writable fields, picked in the form.
    Set,
}

/// A page in the client's right panel; its view comes by
/// [`Command::Page`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    /// Unique among its pages.
    pub id: String,
    pub title: String,
}

/// A Wex namespace: `@NAME.field` and `@NAME.method(args)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Namespace {
    /// `CAMERA` for `@CAMERA`.
    pub name: String,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
    /// What it signals ([`Command::Signal`]). Each is offered as a trigger
    /// and handled by scripts (`on @NS.signal { }`).
    #[serde(default)]
    pub signals: Vec<Signal>,
}

/// A signal an extension sends: something that happened, not a state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    /// Shares the namespace's name set with its fields and methods.
    pub name: String,
    /// What the trigger editor lists it as (`Track ended`).
    pub label: String,
    /// What each signal carries; [`Shape::None`] for nothing.
    pub payload: Shape,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    /// What it holds; a write and a published value must fit it.
    pub shape: Shape,
    /// Scripts and pages may set it ([`Event::Write`]).
    pub writable: bool,
    /// It may cross to another client: a controller reading it, a script
    /// its author sent reading or writing it there. Unshared, it never
    /// leaves the client.
    #[serde(default)]
    pub shared: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Method {
    pub name: String,
    pub params: Vec<Param>,
    /// What its reply holds; a reply that doesn't fit reads none.
    pub returns: Shape,
    /// Its effect lasts: the reply comes when the effect ends, with no
    /// time limit, and [`Event::Cancel`] comes instead when the caller
    /// stops waiting. Any other call is answered within two seconds or
    /// reads none.
    pub coroutine: bool,
    /// The writable fields a call may change. Snap-back restores them as it
    /// restores a script's direct writes.
    pub moves: Vec<String>,
    /// A contact controlling this client may call it: their own scripts
    /// (`@NS.|label|.method`) and their copy of this extension
    /// ([`Command::Fire`]). Any other method is this client's alone.
    #[serde(default)]
    pub contacts: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub shape: Shape,
    /// The list ([`Command::Options`]) an action's form picks it from. The
    /// action keeps the item's key; each time it fires, and when it travels
    /// with a mapping, the client asks for the item's data
    /// ([`Event::Resolve`]), which this parameter then receives.
    pub options: Option<String>,
    /// The person may leave it empty: the form says so, and the method
    /// receives `Value::Text("")` (or `none`), which a module reads as "not
    /// given". For an argument the API itself treats as optional.
    #[serde(default)]
    pub optional: bool,
}

/// The most fields a namespace declares.
pub const FIELDS_MAX: usize = 256;

/// The most methods a namespace declares.
pub const METHODS_MAX: usize = 256;

/// The most signals a namespace declares.
pub const SIGNALS_MAX: usize = 64;

/// The most parameters a method declares.
pub const PARAMS_MAX: usize = 16;

impl Manifest {
    /// Whether it receives a log line whose message is `message`.
    pub fn reads_log(&self, message: &str) -> bool {
        self.log_prefixes
            .iter()
            .any(|p| message.starts_with(p.as_str()))
    }
}
