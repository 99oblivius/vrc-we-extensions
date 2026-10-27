//! What a module answers with: the effects the client carries out, and the
//! values a rule or a script reads from it.

use crate::*;

use serde::{Deserialize, Serialize};

/// What an extension asks of the client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Send a message to one of its write prefixes; any other is dropped.
    Osc(OscMessage),
    /// A field now holds `value`: scripts read it and `on … changes` fires.
    /// A field never set reads `none`.
    Field {
        name: String,
        value: Value,
    },
    /// Signal `name` happened, carrying `value`: its triggers fire and
    /// scripts' `on @NS.name` handlers run, each time. One undeclared or
    /// whose payload doesn't fit is dropped.
    Signal {
        name: String,
        value: Value,
    },
    /// An HTTP(S) request to a host a grant reaches, answered by
    /// [`Event::Http`] under `id`. Redirects aren't followed: a 3xx comes
    /// back as it is.
    Http {
        id: u64,
        method: String,
        url: String,
        headers: Vec<(String, Header)>,
        body: Option<Data>,
    },
    /// Opens connection `id` to a host a grant reaches: [`Event::Opened`],
    /// or [`Event::Closed`] with why not.
    Open {
        id: u64,
        to: Endpoint,
    },
    /// Sends `data` on open connection `id`.
    Send {
        id: u64,
        data: Data,
    },
    /// Closes connection `id`, or stops listener `id`.
    Close {
        id: u64,
    },
    /// Listens on `port` as listener `id`, if a listen grant covers it:
    /// [`Event::Opened`] once listening, then [`Event::Accepted`] (TCP) or
    /// [`Event::Datagram`] (UDP).
    Listen {
        id: u64,
        proto: Proto,
        port: u16,
    },
    /// Sends a datagram from UDP listener `id` to `to` (`address:port`), an
    /// address that sent it one.
    SendTo {
        id: u64,
        to: String,
        data: Data,
    },
    /// The result of call `id`. Every call gets exactly one, in the
    /// `handle` that received it or a later one; a cancelled one needs
    /// none.
    Reply {
        id: u64,
        value: Value,
    },
    /// Page `page`'s view is now `view`; a page never given one shows
    /// nothing.
    Page {
        page: String,
        view: Node,
    },
    /// [`Event::Timer`] `id` in `after_ms`, replacing a timer of that id.
    /// Timers fire on the client's tick, [`TICK_HZ`] times a second: on the
    /// first tick at or after they are due, so 0 is the next tick.
    Timer {
        id: String,
        after_ms: u32,
    },
    CancelTimer {
        id: String,
    },
    /// Keeps `value` under `key` in its own store, across runs; `key` is an
    /// identifier, `value` at most [`STORE_VALUE_MAX`] bytes.
    Store {
        key: String,
        value: String,
    },
    /// Asks for [`Event::Loaded`] with what is stored under `key`.
    Load {
        key: String,
    },
    /// Removes what is stored under `key`.
    Unstore {
        key: String,
    },
    /// Keeps `value` in the OS keyring under one of its declared secret
    /// slots ([`Manifest::secrets`]), at most [`SECRET_MAX`] bytes.
    StoreSecret {
        slot: String,
        value: String,
    },
    /// Asks for [`Event::SecretLoaded`] with what slot `slot` holds.
    LoadSecret {
        slot: String,
    },
    /// Removes what slot `slot` holds.
    UnstoreSecret {
        slot: String,
    },
    /// List `list` now holds `items`, which actions pick from
    /// ([`Param::options`]); an item that doesn't fit ([`Item::fits`]) is
    /// dropped.
    Options {
        list: String,
        items: Vec<Item>,
    },
    /// The data of the item [`Event::Resolve`] `id` asked for, none when
    /// it is gone; data that doesn't fit [`Namespace::check_item`] reads
    /// none.
    Resolved {
        id: u64,
        data: Value,
    },
    /// The item [`Event::Import`] `id` kept its data as, none when it
    /// refused it.
    Imported {
        id: u64,
        item: Option<Item>,
    },
    /// Opens window `id` showing `view`, or shows `view` in it when open.
    Window {
        id: String,
        title: String,
        view: Node,
    },
    CloseWindow {
        id: String,
    },
    /// Opens modal `id` over the main window, large, showing `view` under
    /// `title`, or shows `view` in it when open; at most [`MODALS_MAX`] at
    /// a time.
    Modal {
        id: String,
        title: String,
        view: Node,
    },
    CloseModal {
        id: String,
    },
    /// Moves what node `id` of view `view` (one of its pages, windows or
    /// modals, by id) marks, without sending the view again: what changes
    /// at a tick's rate, while the rest stays. A view not shown, an id it
    /// doesn't hold, a node of another kind or a number not finite changes
    /// nothing.
    Mark {
        view: String,
        id: String,
        mark: Mark,
    },
    /// Calls `method`, one of its contact methods, on `contact`'s copy of
    /// this extension (the same name and author), if its manifest knows
    /// contacts; answered by [`Event::Fired`].
    Fire {
        id: u64,
        contact: String,
        method: String,
        args: Vec<Value>,
    },
    /// Shows a notification for `seconds` (at most
    /// [`NOTIFY_SECONDS_MAX`]), if the manifest says it notifies. The
    /// title is a label; the body at most [`NOTIFY_BODY_MAX`] bytes.
    Notify {
        title: String,
        body: String,
        seconds: u32,
    },
}

/// The longest a notification stays up, in seconds.
pub const NOTIFY_SECONDS_MAX: u32 = 300;

/// The longest notification body, in bytes.
pub const NOTIFY_BODY_MAX: usize = 1024;

/// The largest value [`Command::Store`] keeps.
pub const STORE_VALUE_MAX: usize = 1 << 20;

/// The most keys an extension's store holds.
pub const STORE_KEYS_MAX: usize = 1024;

/// The most bytes an extension's store holds, all values together.
pub const STORE_MAX: u64 = 64 << 20;

/// The most [`Command::Load`]s waiting for their answer; one past it is
/// dropped.
pub const LOADS_MAX: usize = 16;

/// The most timers an extension has set at once.
pub const TIMERS_MAX: usize = 64;

/// The most pages a manifest declares.
pub const PAGES_MAX: usize = 8;

/// The most windows an extension has open outside the main window.
pub const WINDOWS_MAX: usize = 1;

/// The most modals an extension has open at a time.
pub const MODALS_MAX: usize = 5;

/// One item of a list actions pick from: the key an action keeps, and the
/// label people pick it by. Its data stays with the extension until an
/// action fires or travels ([`Event::Resolve`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub key: String,
    pub label: String,
}

impl Item {
    /// A key as a store key is ([`is_store_key`]), a label people read of
    /// at most [`OPTION_LABEL_MAX`] bytes.
    pub fn fits(&self) -> bool {
        is_store_key(&self.key) && is_option_label(&self.label)
    }
}

/// A label people read of at most [`OPTION_LABEL_MAX`] bytes: what an item
/// is named.
pub fn is_option_label(label: &str) -> bool {
    is_text_label(label, OPTION_LABEL_MAX)
}

/// `base` if `taken` says it is free, else the first free `base (n)` from
/// 2, each cut to fit [`OPTION_LABEL_MAX`] bytes: how an import names an
/// item whose label is in use.
pub fn unique_label(base: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..)
        .map(|n| match n {
            1 => cut_to(base, OPTION_LABEL_MAX).to_owned(),
            n => {
                let suffix = format!(" ({n})");
                format!("{}{suffix}", cut_to(base, OPTION_LABEL_MAX - suffix.len()))
            }
        })
        .find(|label| !taken(label))
        .expect("some label is free")
}

/// The most items in a list ([`Command::Options`]).
pub const OPTIONS_MAX: usize = 1024;

/// The largest secret [`Command::StoreSecret`] keeps.
pub const SECRET_MAX: usize = 4096;

/// The most secret slots a manifest declares.
pub const SECRETS_MAX: usize = 16;

/// The most calls waiting on an extension at once; a call past it reads
/// none at once.
pub const CALLS_MAX: usize = 256;

/// The most of [`CALLS_MAX`] one contact's calls hold, so contacts never
/// crowd out the person's own.
pub const CONTACT_CALLS_MAX: usize = 64;

/// Whether a store key is one the client keeps: an identifier of at most
/// 64 bytes, never a name Windows reserves for a device.
pub fn is_store_key(key: &str) -> bool {
    key.len() <= 64 && is_ident(key) && !is_device_name(key)
}
