//! The module's page: the token to paste, what is being listened for, the last
//! event and the last error. It reads a snapshot of the module, never the
//! machine itself.

use vrc_we_sdk::Node;

use crate::protocol::{Fact, Group};
use crate::Phase;

/// What the page shows, gathered where the module publishes it.
pub(crate) struct Status<'a> {
    pub phase: &'a Phase,
    /// A token is held and checked: the fields make way for the buttons.
    pub logged_in: bool,
    /// The account the token belongs to, empty before the check.
    pub who: &'a str,
    pub token_field: &'a str,
    pub refresh_field: &'a str,
    /// The groups the person's rules and scripts read.
    pub groups: &'a [Group],
    /// Groups the token's scopes cannot subscribe, and the scope each needs.
    pub missing: &'a [(Group, &'static str)],
    pub last: Option<&'a Fact>,
    pub error: Option<&'a str>,
}

/// The page as `Status` describes it.
pub(crate) fn view(status: &Status) -> Node {
    let (title, detail) = headline(status);
    let mut rows = vec![
        Node::Heading("Twitch".into()),
        Node::Text(title),
        Node::Text(detail),
    ];
    if status.logged_in {
        rows.push(button("reconnect", "Reconnect"));
        rows.push(button("forget", "Forget the token"));
    } else {
        rows.push(Node::TextInput {
            id: "token".into(),
            label: "User access token".into(),
            value: status.token_field.to_owned(),
            live: false,
            secret: true,
        });
        rows.push(Node::TextInput {
            id: "refresh".into(),
            label: "Refresh token, if you have one".into(),
            value: status.refresh_field.to_owned(),
            live: false,
            secret: true,
        });
        rows.push(button("connect", "Connect"));
    }
    rows.push(Node::Divider);
    rows.push(Node::Text(match status.groups.len() {
        0 => "Nothing is watching yet: a mapping or script that reads a Twitch field or handles one of its signals subscribes to it.".to_owned(),
        1 => format!("Listening for {}", label_of(status.groups[0])),
        _ => format!(
            "Listening for {}",
            status
                .groups
                .iter()
                .map(|g| label_of(*g))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }));
    if !status.missing.is_empty() {
        rows.push(Node::Text(format!(
            "Not listening, since the token lacks {}",
            status
                .missing
                .iter()
                .map(|(group, scope)| format!("{} ({scope})", label_of(*group)))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if let Some(fact) = status.last {
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
    if let Some(error) = status.error {
        rows.push(Node::Text(format!("Last error: {error}")));
    }
    Node::Column(rows)
}

/// The headline and the line under it.
fn headline(status: &Status) -> (String, String) {
    match status.phase {
        Phase::Off => (
            "Not connected".into(),
            "Paste your own user access token to read your channel.".into(),
        ),
        Phase::Connecting => ("Connecting".into(), format!("As {}.", status.who)),
        Phase::Live { .. } => ("Live".into(), format!("As {}, listening.", status.who)),
    }
}

fn button(id: &str, label: &str) -> Node {
    Node::Button {
        id: id.into(),
        label: label.into(),
        icon: None,
        wide: false,
    }
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
