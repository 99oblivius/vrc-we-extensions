//! The module's page and its status bar line, from a snapshot of the module.

use vrc_we_sdk::{Button, Flow, Node, Slider, Status, Tone};

use crate::protocol::Device;

/// The packaged icon: Intiface's logo.
pub(crate) const LOGO: &str = "intiface";

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

/// The line in the status bar, with the logo.
pub(crate) fn bar(state: State) -> Status {
    Status {
        icon: Some(LOGO.into()),
        ..line(state)
    }
}

/// The line at the top of the page.
fn line(state: State) -> Status {
    let (tone, label) = match state {
        State::Connecting => (Tone::Waiting, "Connecting"),
        State::NotRunning => (Tone::Idle, "Not running"),
        State::Waiting => (Tone::Waiting, "Waiting for a device"),
        State::Connected => (Tone::Good, "Connected"),
        State::Refused => (Tone::Bad, "Refused this client"),
    };
    Status {
        tone,
        label: label.into(),
        icon: None,
        page: None,
    }
}

pub(crate) fn view(s: &Snapshot) -> Node {
    let mut rows: Vec<Node> = vec![line(s.state).into()];
    match s.state {
        State::NotRunning => {
            rows.push("Start Intiface Central and its server, at any time.".into())
        }
        State::Waiting => rows.push("Turn a device on, or scan for one.".into()),
        _ => {}
    }
    rows.extend(s.devices.iter().map(|d| Node::from(d.name.as_str())));
    if let Some(limit) = s.limit {
        rows.push(Slider::new("limit", "Strongest allowed", (0.0, 1.0), Some(limit)).into());
    }
    if s.running {
        rows.push(Button::new("stop", "Stop all devices").into());
    }
    if matches!(s.state, State::Waiting | State::Connected) {
        let (id, label) = match s.scanning {
            true => ("scan_stop", "Stop scanning"),
            false => ("scan", "Scan for devices"),
        };
        rows.push(Button::new(id, label).into());
    }
    rows.push(Button::new("reconnect", "Reconnect").into());
    if let Some(error) = s.error {
        rows.push(format!("Last error: {error}").into());
    }
    Flow::col(rows).into()
}
