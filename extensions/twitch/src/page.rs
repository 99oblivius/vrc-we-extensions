//! The module's page and its status bar line: whether the account is signed
//! in, whether Twitch is delivering what is wanted, the last event and the
//! last error. It reads a snapshot of the module, never the machine itself.

use vrc_we_sdk::{Node, Status, Tone};

use crate::protocol::{Fact, Group};
use crate::Login;

/// The icon the module packages: Twitch's logo.
pub(crate) const LOGO: &str = "twitch";

/// What the socket and its subscriptions come to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Events {
    /// Nothing reads Twitch, so no socket is open.
    Idle,
    /// Something is wanted and no socket is welcomed yet.
    Connecting,
    /// Something wanted isn't active, or none of it can be.
    Waiting,
    /// Welcomed, and everything wanted is active.
    Connected,
}

/// What the page shows, gathered where the module publishes it.
pub(crate) struct Snapshot<'a> {
    pub login: Login,
    pub events: Events,
    /// The account the token was last known to be.
    pub who: Option<&'a str>,
    /// The address Twitch waits for an approval at, while one is waiting.
    pub device: Option<&'a str>,
    /// A device code has been asked for.
    pub device_asked: bool,
    /// The groups Twitch has confirmed in full.
    pub listening: Vec<Group>,
    /// The groups Twitch refused, and what it said.
    pub refused: Vec<(Group, &'a str)>,
    /// Of what is wanted, how many are active.
    pub active: usize,
    pub total: usize,
    /// Twitch was busy: a create goes again after a wait.
    pub busy: bool,
    /// A socket opens again after a wait.
    pub retry_armed: bool,
    /// Groups the token's scopes cannot subscribe, and the scope each needs.
    pub missing: &'a [(Group, &'static str)],
    pub last: Option<&'a Fact>,
    pub error: Option<&'a str>,
}

/// Whether the person is signed in.
fn login_line(s: &Snapshot) -> (Tone, String) {
    match s.login {
        Login::In => (
            Tone::Good,
            format!("Signed in as {}", s.who.unwrap_or_default()),
        ),
        _ if s.device.is_some() || s.device_asked => {
            (Tone::Waiting, "Waiting for your approval".into())
        }
        Login::Checking => (Tone::Waiting, "Checking the sign-in".into()),
        Login::Out => (Tone::Idle, "Not signed in".into()),
        Login::Expired => (Tone::Bad, "Sign-in expired".into()),
    }
}

/// Whether Twitch is delivering what is wanted.
fn events_line(events: Events) -> (Tone, &'static str) {
    match events {
        Events::Idle => (Tone::Idle, "Nothing to listen for"),
        Events::Connecting => (Tone::Waiting, "Connecting"),
        Events::Waiting => (Tone::Waiting, "Waiting"),
        Events::Connected => (Tone::Good, "Connected"),
    }
}

/// Its line in the client's status bar: the sign-in until there is one, then
/// the events.
pub(crate) fn bar(s: &Snapshot) -> Status {
    let (tone, label) = match (s.login, s.events) {
        (Login::In, Events::Idle) => (Tone::Idle, "Signed in".to_owned()),
        (Login::In, events) => {
            let (tone, label) = events_line(events);
            (tone, label.to_owned())
        }
        _ => login_line(s),
    };
    Status {
        tone,
        label,
        icon: Some(LOGO.into()),
    }
}

/// The page as `s` describes it.
pub(crate) fn view(s: &Snapshot) -> Node {
    let (tone, label) = login_line(s);
    let mut rows = vec![
        Node::Heading("Twitch".into()),
        Node::Status(Status {
            tone,
            label,
            icon: Some(LOGO.into()),
        }),
    ];
    if s.login == Login::In {
        let (tone, label) = events_line(s.events);
        rows.push(Node::Status(Status {
            tone,
            label: label.into(),
            icon: None,
        }));
        rows.extend(detail(s).into_iter().map(Node::Text));
    }
    if let Some(link) = s.device {
        rows.push(Node::Text(
            "Approve this client on Twitch; the page is already waiting. Nothing is typed here."
                .into(),
        ));
        rows.push(Node::Link {
            label: "Authorize on Twitch".into(),
            url: link.to_owned(),
        });
        rows.push(button("cancel", "Cancel"));
    } else {
        rows.extend(actions(s));
    }
    if let Some(fact) = s.last {
        rows.push(Node::Divider);
        rows.push(Node::Text(format!(
            "Last: {} by {}{}",
            fact.kind,
            fact.user,
            if fact.message.is_empty() {
                String::new()
            } else {
                format!(": {}", fact.message)
            }
        )));
    }
    if let Some(error) = s.error {
        rows.push(Node::Text(format!("Last error: {error}")));
    }
    Node::Column(rows)
}

/// What the events line comes down to: only the cases that say something the
/// line doesn't.
fn detail(s: &Snapshot) -> Vec<String> {
    let mut rows = Vec::new();
    match s.events {
        Events::Idle => rows.push(
            "A mapping or script that reads a Twitch field or handles one of its signals subscribes to it."
                .to_owned(),
        ),
        Events::Connecting if s.retry_armed => rows.push("Trying again shortly.".to_owned()),
        Events::Connecting => {}
        Events::Waiting | Events::Connected => {
            if !s.listening.is_empty() {
                rows.push(format!("Listening for {}", labels(&s.listening)));
            }
            if s.events == Events::Waiting && s.active + s.refused.len() < s.total {
                rows.push(format!("Subscribing, {} of {}.", s.active, s.total));
            }
        }
    }
    if s.busy {
        rows.push("Twitch is busy, trying again.".to_owned());
    }
    if !s.missing.is_empty() {
        rows.push(format!(
            "Not listening, since the token lacks {}",
            s.missing
                .iter()
                .map(|(group, scope)| format!("{} ({scope})", label_of(*group)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for (group, why) in &s.refused {
        rows.push(format!("Twitch refused {}: {why}", label_of(*group)));
    }
    rows
}

/// What can be pressed while no approval is waiting. A sign-in that is held
/// but can't be used keeps Reconnect and Forget next to Connect: a keyring
/// that was locked at launch reads as signed out, and that loses nothing.
fn actions(s: &Snapshot) -> Vec<Node> {
    let mut rows = Vec::new();
    let held = s.who.is_some();
    match s.login {
        Login::In => {
            if !s.missing.is_empty() {
                rows.push(button("connect", "Grant them on Twitch"));
            }
            rows.push(button("reconnect", "Reconnect"));
            rows.push(button("forget", "Forget the token"));
        }
        Login::Checking => {}
        Login::Out | Login::Expired if s.device_asked => {}
        Login::Out | Login::Expired => {
            rows.push(Node::Text(
                "Twitch asks you to approve this client once, in your browser. Nothing is typed here."
                    .into(),
            ));
            rows.push(button("connect", "Connect with Twitch"));
            if held {
                rows.push(button("reconnect", "Reconnect"));
                rows.push(button("forget", "Forget the token"));
            }
        }
    }
    rows
}

fn button(id: &str, label: &str) -> Node {
    Node::Button {
        id: id.into(),
        label: label.into(),
        icon: None,
        wide: false,
    }
}

fn labels(groups: &[Group]) -> String {
    groups
        .iter()
        .map(|g| label_of(*g))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The page's label for a group.
fn label_of(group: Group) -> &'static str {
    match group {
        Group::Redeems => "Channel point redemptions",
        Group::Chat => "Chat messages",
        Group::Subs => "Subscriptions and gifts",
        Group::Cheer => "Cheers",
        Group::Raid => "Raids",
        Group::Hype => "Hype train",
        Group::Follow => "Follows",
        Group::Live => "Going live and offline",
        Group::Poll => "Polls",
    }
}
