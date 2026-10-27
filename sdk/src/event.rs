//! What the client sends a module, and nothing else.

use crate::*;

use serde::{Deserialize, Serialize};

/// What the client tells an extension. More may come in a later
/// boundary version: match with a wildcard arm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    /// Its first event, once loaded: the time to publish its fields and
    /// views.
    Start,
    /// Someone used one of its pages or windows (`page` is the window's
    /// id for a window).
    Ui { page: String, event: UiEvent },
    /// Timer `id` ([`Command::Timer`]) is due; `at_ms` is when it fired
    /// on the client's steady clock, in milliseconds: only differences
    /// between two mean anything.
    Timer {
        id: String,
        #[serde(default)]
        at_ms: u64,
    },
    /// What [`Command::Load`] found under `key`; none when nothing was
    /// stored.
    Loaded { key: String, value: Option<String> },
    /// What [`Command::LoadSecret`] found in slot `slot`; none when nothing
    /// was stored or the client has no keyring to read.
    SecretLoaded { slot: String, value: Option<String> },
    /// The person closed window `id`.
    WindowClosed { id: String },
    /// The person closed modal `id`.
    ModalClosed { id: String },
    /// A message under one of its read prefixes.
    Osc(OscMessage),
    /// What the client's enabled rules and their scripts read of it, by the
    /// names in its own namespace: sent once at start, then whenever the set
    /// changes. A module subscribes to what is actually used rather than to
    /// everything it could do.
    Watched {
        #[serde(default)]
        fields: Vec<String>,
        #[serde(default)]
        signals: Vec<String>,
    },
    /// A VRChat log line's message (after its time and level) that starts
    /// with one of its `log_prefixes`, as the game writes it, cut at
    /// [`LOG_LINE_MAX`]. Only lines written while the client runs.
    Log { line: String },
    /// Request `id` answered, or why it failed.
    Http {
        id: u64,
        response: Result<HttpResponse, String>,
    },
    /// Connection `id` is open: [`Command::Send`] reaches it.
    Opened { id: u64 },
    /// Connection `id` received `data`.
    Received { id: u64, data: Data },
    /// Connection `id` closed: by the other end, on [`Command::Close`], or
    /// failing (`error`), opening included. For a listener, it stopped.
    Closed { id: u64, error: Option<String> },
    /// TCP listener `listener` accepted a connection from `from`
    /// (`address:port`), open as `id` (with [`ACCEPTED_ID`] set).
    Accepted {
        listener: u64,
        id: u64,
        from: String,
    },
    /// UDP listener `id` received a datagram from `from` (`address:port`),
    /// which [`Command::SendTo`] may answer.
    Datagram { id: u64, from: String, data: Data },
    /// A script or a page sets a field; `value` passed
    /// [`Namespace::check_write`].
    Write { field: String, value: Value },
    /// A script calls a method; `args` passed [`Namespace::check_call`].
    /// `from` is the contact who called it, when [`Manifest::contacts`]
    /// lets it know contacts.
    Call {
        id: u64,
        method: String,
        args: Vec<Value>,
        #[serde(default)]
        from: Option<Contact>,
    },
    /// The contacts it may fire on now ([`Manifest::contacts`]): those the
    /// person controls in a live session. Sent when it starts and whenever
    /// they change.
    Contacts { list: Vec<Contact> },
    /// [`Command::Fire`] `id` answered: the method's reply, none when the
    /// contact refused it, lacks this extension, or the link went down.
    Fired { id: u64, value: Value },
    /// The caller of coroutine call `id` stopped waiting (its flow ended):
    /// end its effect. A reply to it now is dropped.
    Cancel { id: u64 },
    /// An action picked `key` from list `list` and fires or travels:
    /// answer with the item's data ([`Command::Resolved`]), none once it is
    /// gone.
    Resolve { id: u64, list: String, key: String },
    /// A mapping arrived with an item another client's copy of this
    /// extension resolved: keep `data` (it passed [`Namespace::check_item`])
    /// in list `list`, reusing an item that holds the same, else as a new
    /// one named `label` or the first free `label (n)` ([`unique_label`]),
    /// and answer with that item ([`Command::Imported`]).
    Import {
        id: u64,
        list: String,
        label: String,
        data: Value,
    },
}
