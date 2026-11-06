//! The module's page and its status bar line, from a snapshot of the module.

use vrc_we_sdk::{Node, Status, Tone};

use crate::protocol::Device;

/// What the connection comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// A first try is on its way.
    Connecting,
    /// Nothing answers on its port, and it keeps looking quietly.
    NotRunning,
    /// Intiface answered and has nothing to drive yet.
    Waiting,
    /// Intiface answered and has a device.
    Connected,
    /// Intiface refused this client, which holds until Reconnect.
    Refused,
}

pub(crate) struct Snapshot<'a> {
    pub state: State,
    pub devices: &'a [Device],
    pub scanning: bool,
    /// The strongest this person allows, none until it has been read.
    pub limit: Option<f64>,
    pub running: bool,
    pub error: Option<&'a str>,
}

/// Its line in the status bar and at the head of its page.
pub(crate) fn line(state: State) -> Status {
    let (tone, label) = match state {
        State::Connecting => (Tone::Waiting, "Connecting"),
        State::NotRunning => (Tone::Idle, "Intiface isn't running"),
        State::Waiting => (Tone::Waiting, "Waiting for a device"),
        State::Connected => (Tone::Good, "Connected"),
        State::Refused => (Tone::Bad, "Intiface refused this client"),
    };
    Status {
        tone,
        label: label.into(),
        icon: None,
    }
}

pub(crate) fn view(s: &Snapshot) -> Node {
    let mut rows = vec![
        Node::Heading("Intiface".into()),
        Node::Status(line(s.state)),
    ];
    match s.state {
        State::NotRunning => rows.push(Node::Text(
            "Start Intiface Central and its server. This keeps looking for it.".into(),
        )),
        State::Waiting => rows.push(Node::Text(
            "Turn a device on. Scan looks for one if Intiface isn't already.".into(),
        )),
        _ => {}
    }
    rows.extend(s.devices.iter().map(|d| Node::Text(d.name.clone())));
    if let Some(limit) = s.limit {
        rows.push(Node::Slider {
            id: "limit".into(),
            label: "Strongest allowed".into(),
            min: 0.0,
            max: 1.0,
            value: Some(limit),
            log: false,
            live: false,
        });
    }
    if s.running {
        rows.push(button("stop", "Stop all devices"));
    }
    if matches!(s.state, State::Waiting | State::Connected) {
        let (id, label) = match s.scanning {
            true => ("scan_stop", "Stop scanning"),
            false => ("scan", "Scan for devices"),
        };
        rows.push(button(id, label));
    }
    rows.push(button("reconnect", "Reconnect"));
    if let Some(error) = s.error {
        rows.push(Node::Text(format!("Last error: {error}")));
    }
    Node::Column(rows)
}

fn button(id: &str, label: &str) -> Node {
    Node::Button {
        id: id.into(),
        label: label.into(),
        icon: None,
        wide: false,
    }
}
