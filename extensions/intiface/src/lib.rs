//! Device control through Intiface: `@INTIFACE.vibrate` drives the devices a
//! local Intiface server has connected, from mappings and scripts, and from a
//! contact's when this person consents.
//!
//! It speaks the Buttplug protocol as plain JSON over a socket the client holds,
//! so it has no Buttplug client and no IO of its own. Every event is one step,
//! then one reconcile of what is wanted against what exists, then one publish of
//! what changed.
//!
//! A vibration has no duration in the protocol: it runs until something stops
//! it. So stopping is layered. Every drive has a length, ended by a timer here.
//! A cancelled flow, `stop`, and a device leaving each end it. And when this
//! module stops, faults, or the client exits or dies, the socket closes and the
//! server stops every device: measured against Intiface Engine 5.0.5, a stop
//! arrives within a millisecond of a close or a killed process.
//!
//! What a hung client keeps running is the one gap left: its socket stays open.
//! Only `MaxPingTime` closes that, which Intiface leaves off by default. When a
//! server sets one, this module pings well inside it.
//!
//! Build: `cargo wasm -p intiface`.

mod manifest;
mod page;
mod protocol;

use std::collections::HashMap;

use vrc_we_sdk::{
    Command, Data, Endpoint, Event, Extension, Manifest, Node, Status, UiEvent, Value,
};

use page::{Snapshot, State};
use protocol::{Device, Message};

vrc_we_sdk::export!(Intiface);

const PAGE: &str = "intiface";
const K_LIMIT: &str = "limit";

/// The longest one call may run, in seconds. A longer effect is a script asking
/// again, which keeps a stalled script from holding a device.
const SECONDS_MAX: f64 = 300.0;
/// Drives running at once. Each holds a timer, of which the host allows 64.
const DRIVES_MAX: usize = 16;
/// The shortest and the longest wait before looking for the server again.
const RETRY_MS: u32 = 5_000;
const RETRY_MAX_MS: u32 = 60_000;
/// How long a server that accepted the socket has to answer.
const GREETING_MS: u32 = 5_000;

/// Where the socket has got to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Link {
    #[default]
    Closed,
    Opening(u64),
    Greeting(u64),
    Ready(u64),
}

impl Link {
    fn socket(self) -> Option<u64> {
        match self {
            Self::Closed => None,
            Self::Opening(id) | Self::Greeting(id) | Self::Ready(id) => Some(id),
        }
    }
}

/// One output a drive holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Target {
    device: u32,
    feature: u32,
}

/// A running call: it answers when it ends, and owns the outputs it holds, each
/// of which only one drive can hold.
#[derive(Debug, Clone, PartialEq)]
struct Drive {
    call: u64,
    targets: Vec<Target>,
    level: f64,
}

fn timer_of(call: u64) -> String {
    format!("drive-{call}")
}

#[derive(Default)]
pub struct Intiface {
    started: bool,
    link: Link,
    /// Why Intiface refused this client. It holds until Reconnect, since asking
    /// again would meet the same answer.
    refused: Option<String>,
    /// A server has answered since the last Reconnect, so a lost one is news.
    answered: bool,
    devices: Vec<Device>,
    drives: Vec<Drive>,
    /// The strongest this person allows, on every path: none until it has been
    /// read, and nothing runs before then.
    limit: Option<f64>,
    scanning: bool,
    backoff_ms: u32,
    retry_armed: bool,
    next_id: u64,
    max_ping_ms: u32,
    error: Option<String>,
    published: HashMap<&'static str, Value>,
    shown_page: Option<Node>,
    shown_status: Option<Status>,
}

impl Intiface {
    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn send(&self, out: &mut Vec<Command>, text: String) {
        if let Some(id) = self.link.socket() {
            out.push(Command::Send {
                id,
                data: Data::Text(text),
            });
        }
    }

    /// What the socket and the devices come to.
    fn state(&self) -> State {
        match self.link {
            _ if self.refused.is_some() => State::Refused,
            Link::Ready(_) if self.devices.is_empty() => State::Waiting,
            Link::Ready(_) => State::Connected,
            Link::Closed if self.retry_armed && !self.answered => State::NotRunning,
            _ => State::Connecting,
        }
    }

    /// Opens the socket whenever there should be one: the module's whole job
    /// needs it, and a loopback dial costs nothing while Intiface is off.
    fn reconcile(&mut self, out: &mut Vec<Command>) {
        if !self.started || self.link != Link::Closed || self.retry_armed || self.refused.is_some()
        {
            return;
        }
        let id = self.id();
        self.link = Link::Opening(id);
        out.push(Command::Open {
            id,
            to: Endpoint::Ws {
                url: protocol::URL.to_owned(),
            },
        });
    }

    /// The socket is gone, and the server stopped every device with it: each
    /// drive ends, and another socket opens after a wait that grows until a
    /// server answers.
    fn lost(&mut self, out: &mut Vec<Command>) {
        self.link = Link::Closed;
        self.devices.clear();
        self.scanning = false;
        self.max_ping_ms = 0;
        cancel(out, "ping");
        cancel(out, "greeting");
        for drive in std::mem::take(&mut self.drives) {
            cancel(out, &timer_of(drive.call));
            reply(out, drive.call, false);
        }
        if self.refused.is_some() {
            return;
        }
        let wait = self.backoff_ms.max(RETRY_MS);
        self.backoff_ms = wait.saturating_mul(2).min(RETRY_MAX_MS);
        self.retry_armed = true;
        out.push(Command::Timer {
            id: "retry".into(),
            after_ms: wait,
        });
    }

    fn close(&mut self, out: &mut Vec<Command>) {
        if let Some(id) = self.link.socket() {
            out.push(Command::Close { id });
        }
        self.lost(out);
    }

    /// Holds `targets` at `level`, as this person's limit allows.
    fn hold(&mut self, out: &mut Vec<Command>, targets: &[Target], level: f64) {
        for target in targets {
            let Some(output) = self
                .devices
                .iter()
                .find(|d| d.index == target.device)
                .and_then(|d| d.outputs.iter().find(|o| o.feature == target.feature))
            else {
                continue;
            };
            let value = output.value(level);
            let id = self.id();
            self.send(
                out,
                protocol::vibrate(id, target.device, target.feature, value),
            );
        }
    }

    /// Ends drive `at`, setting what it held to nothing.
    fn end(&mut self, out: &mut Vec<Command>, at: usize) -> u64 {
        let drive = self.drives.remove(at);
        self.hold(out, &drive.targets, 0.0);
        cancel(out, &timer_of(drive.call));
        drive.call
    }

    /// Every output a call with this `device` name reaches: all of them with
    /// none.
    fn targets(&self, device: Option<&str>) -> Vec<Target> {
        self.devices
            .iter()
            .filter(|d| device.is_none_or(|name| d.name == name))
            .flat_map(|d| {
                d.outputs.iter().map(|o| Target {
                    device: d.index,
                    feature: o.feature,
                })
            })
            .collect()
    }

    /// `vibrate`: it answers true when its time is up, and false when it
    /// couldn't run or something took it over.
    fn vibrate(&mut self, out: &mut Vec<Command>, call: u64, args: &[Value]) {
        let number = |i: usize| match args.get(i) {
            Some(Value::Number(n)) => *n,
            _ => 0.0,
        };
        let (intensity, seconds) = (number(0), number(1));
        let device = match args.get(2) {
            Some(Value::Text(name)) if !name.is_empty() => Some(name.as_str()),
            _ => None,
        };
        let targets = self.targets(device);
        let Some(limit) = self.limit.filter(|_| !targets.is_empty()) else {
            reply(out, call, false);
            return;
        };
        if self.drives.len() >= DRIVES_MAX {
            self.error = Some("Too many drives at once.".into());
            reply(out, call, false);
            return;
        }
        if seconds <= 0.0 {
            reply(out, call, true);
            return;
        }
        // An output has one holder: whoever asked last.
        for drive in &mut self.drives {
            drive.targets.retain(|t| !targets.contains(t));
        }
        let mut at = 0;
        while at < self.drives.len() {
            if self.drives[at].targets.is_empty() {
                let drive = self.drives.remove(at);
                cancel(out, &timer_of(drive.call));
                reply(out, drive.call, false);
            } else {
                at += 1;
            }
        }
        let level = intensity.min(limit);
        self.hold(out, &targets, level);
        out.push(Command::Timer {
            id: timer_of(call),
            after_ms: (seconds * 1000.0) as u32,
        });
        self.drives.push(Drive {
            call,
            targets,
            level,
        });
    }

    /// Stops everything, on the server's own word: what isn't a drive of this
    /// module stops too.
    fn stop(&mut self, out: &mut Vec<Command>) {
        let id = self.id();
        self.send(out, protocol::stop_all(id));
        for drive in std::mem::take(&mut self.drives) {
            cancel(out, &timer_of(drive.call));
            reply(out, drive.call, false);
        }
    }

    /// A lower limit reaches what is already running.
    fn set_limit(&mut self, out: &mut Vec<Command>, limit: f64) {
        let limit = limit.clamp(0.0, 1.0);
        self.limit = Some(limit);
        out.push(Command::Store {
            key: K_LIMIT.into(),
            value: limit.to_string(),
        });
        for drive in self.drives.clone() {
            if drive.level > limit {
                self.hold(out, &drive.targets, limit);
            }
        }
        for drive in &mut self.drives {
            drive.level = drive.level.min(limit);
        }
    }

    /// The whole list replaces the last: an index that left may come back as
    /// another device, so a drive holds only what is still the same output.
    fn devices_changed(&mut self, out: &mut Vec<Command>, devices: Vec<Device>) {
        let kept = |t: &Target, old: &[Device], new: &[Device]| {
            let name = |list: &[Device]| {
                list.iter()
                    .find(|d| d.index == t.device)
                    .map(|d| d.name.clone())
            };
            name(old).is_some() && name(old) == name(new)
        };
        for drive in &mut self.drives {
            drive.targets.retain(|t| kept(t, &self.devices, &devices));
        }
        self.devices = devices;
        let mut at = 0;
        while at < self.drives.len() {
            if self.drives[at].targets.is_empty() {
                let drive = self.drives.remove(at);
                cancel(out, &timer_of(drive.call));
                reply(out, drive.call, false);
            } else {
                at += 1;
            }
        }
    }

    fn frame(&mut self, out: &mut Vec<Command>, text: &str) {
        let messages = match protocol::parse(text) {
            Ok(messages) => messages,
            Err(e) => {
                self.error = Some(format!("bad frame: {e}"));
                return;
            }
        };
        for message in messages {
            match message {
                Message::ServerInfo { max_ping_ms } => {
                    if let Link::Greeting(id) = self.link {
                        self.link = Link::Ready(id);
                    }
                    cancel(out, "greeting");
                    self.answered = true;
                    self.backoff_ms = 0;
                    self.error = None;
                    self.max_ping_ms = max_ping_ms;
                    self.arm_ping(out);
                    let id = self.id();
                    self.send(out, protocol::request_devices(id));
                }
                Message::Devices(devices) => self.devices_changed(out, devices),
                Message::ScanningFinished => self.scanning = false,
                // Refused while greeting is about the client itself, usually
                // a server older than the spec it asked for.
                Message::Error(why) if matches!(self.link, Link::Greeting(_)) => {
                    self.refused = Some(why.clone());
                    self.error = Some(why);
                    self.close(out);
                }
                Message::Error(why) => self.error = Some(why),
                Message::Other => {}
            }
        }
    }

    /// Pings at a third of the server's limit, when it has one, so one late
    /// tick doesn't stop a device.
    fn arm_ping(&self, out: &mut Vec<Command>) {
        if self.max_ping_ms > 0 {
            out.push(Command::Timer {
                id: "ping".into(),
                after_ms: (self.max_ping_ms / 3).max(1),
            });
        }
    }

    fn step(&mut self, event: Event, out: &mut Vec<Command>) {
        match event {
            Event::Start => {
                self.started = true;
                out.push(Command::Load {
                    key: K_LIMIT.into(),
                });
            }
            Event::Loaded { key, value } if key == K_LIMIT => {
                let stored = value.and_then(|text| text.parse::<f64>().ok());
                self.limit = Some(stored.unwrap_or(1.0).clamp(0.0, 1.0));
            }
            Event::Opened { id } if self.link == Link::Opening(id) => {
                self.link = Link::Greeting(id);
                let request = self.id();
                self.send(out, protocol::request_server_info(request));
                out.push(Command::Timer {
                    id: "greeting".into(),
                    after_ms: GREETING_MS,
                });
            }
            Event::Received { id, data } if self.link.socket() == Some(id) => {
                let text = match data {
                    Data::Text(text) => text,
                    Data::Bytes(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                };
                self.frame(out, &text);
            }
            Event::Closed { id, error } if self.link.socket() == Some(id) => {
                // A server that was never there isn't an error worth showing.
                if self.answered {
                    self.error = error;
                }
                self.lost(out);
            }
            Event::Timer { id, .. } => match id.as_str() {
                "retry" => self.retry_armed = false,
                "greeting" => self.close(out),
                "ping" => {
                    if matches!(self.link, Link::Ready(_)) {
                        let request = self.id();
                        self.send(out, protocol::ping(request));
                        self.arm_ping(out);
                    }
                }
                timer => {
                    let ended = self.drives.iter().position(|d| timer_of(d.call) == timer);
                    if let Some(at) = ended {
                        let call = self.end(out, at);
                        reply(out, call, true);
                    }
                }
            },
            Event::Call {
                id, method, args, ..
            } => match method.as_str() {
                "vibrate" => self.vibrate(out, id, &args),
                "stop" => {
                    self.stop(out);
                    reply(out, id, true);
                }
                _ => reply(out, id, false),
            },
            // The caller stopped waiting: its effect ends, and a reply would
            // be dropped.
            Event::Cancel { id } => {
                if let Some(at) = self.drives.iter().position(|d| d.call == id) {
                    self.end(out, at);
                }
            }
            Event::Ui { event, .. } => self.ui(out, event),
            _ => {}
        }
    }

    fn ui(&mut self, out: &mut Vec<Command>, event: UiEvent) {
        match event {
            UiEvent::Number { id, value } if id == "limit" => self.set_limit(out, value),
            UiEvent::Pressed { id } => match id.as_str() {
                "stop" => self.stop(out),
                "scan" => {
                    let request = self.id();
                    self.send(out, protocol::start_scanning(request));
                    self.scanning = true;
                }
                "scan_stop" => {
                    let request = self.id();
                    self.send(out, protocol::stop_scanning(request));
                    self.scanning = false;
                }
                // Asks again: what was refused, and the socket, at once.
                "reconnect" => {
                    self.refused = None;
                    self.answered = false;
                    self.error = None;
                    self.close(out);
                    self.backoff_ms = 0;
                    self.retry_armed = false;
                    cancel(out, "retry");
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// Publishes what changed since the last publish.
    fn publish(&mut self, out: &mut Vec<Command>) {
        let state = self.state();
        let level = self.drives.iter().map(|d| d.level).fold(0.0, f64::max);
        let fields = [
            ("connected", Value::Bool(state == State::Connected)),
            ("devices", Value::Number(self.devices.len() as f64)),
            ("level", Value::Number(level)),
        ];
        for (name, value) in fields {
            if self.published.get(name) != Some(&value) {
                self.published.insert(name, value.clone());
                out.push(Command::Field {
                    name: name.to_owned(),
                    value,
                });
            }
        }
        let view = page::view(&Snapshot {
            state,
            devices: &self.devices,
            scanning: self.scanning,
            limit: self.limit,
            running: !self.drives.is_empty(),
            error: self.error.as_deref(),
        });
        if self.shown_page.as_ref() != Some(&view) {
            self.shown_page = Some(view.clone());
            out.push(Command::Page {
                page: PAGE.into(),
                view,
            });
        }
        let status = page::line(state);
        if self.shown_status.as_ref() != Some(&status) {
            self.shown_status = Some(status.clone());
            out.push(Command::Status(Some(status)));
        }
    }
}

fn cancel(out: &mut Vec<Command>, id: &str) {
    out.push(Command::CancelTimer { id: id.to_owned() });
}

fn reply(out: &mut Vec<Command>, id: u64, value: bool) {
    out.push(Command::Reply {
        id,
        value: Value::Bool(value),
    });
}

impl Extension for Intiface {
    fn manifest(&self) -> Manifest {
        manifest::build()
    }

    fn handle(&mut self, event: Event) -> Vec<Command> {
        let mut out = Vec::new();
        self.step(event, &mut out);
        self.reconcile(&mut out);
        self.publish(&mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vrc_we_sdk::Tone;

    fn sent(out: &[Command]) -> Vec<String> {
        out.iter()
            .filter_map(|c| match c {
                Command::Send {
                    data: Data::Text(text),
                    ..
                } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The values `out` holds outputs at, in order.
    fn values(out: &[Command]) -> Vec<i64> {
        sent(out)
            .iter()
            .filter_map(|text| {
                let frame: serde_json::Value = serde_json::from_str(text).ok()?;
                frame[0]["OutputCmd"]["Command"]["Vibrate"]["Value"].as_i64()
            })
            .collect()
    }

    fn replied(out: &[Command], call: u64) -> Option<bool> {
        out.iter().find_map(|c| match c {
            Command::Reply {
                id,
                value: Value::Bool(answer),
            } if *id == call => Some(*answer),
            _ => None,
        })
    }

    fn opened(out: &[Command]) -> Option<u64> {
        out.iter().find_map(|c| match c {
            Command::Open { id, .. } => Some(*id),
            _ => None,
        })
    }

    fn line(out: &[Command]) -> Option<(Tone, String)> {
        out.iter().rev().find_map(|c| match c {
            Command::Status(Some(s)) => Some((s.tone, s.label.clone())),
            _ => None,
        })
    }

    fn timer(out: &[Command], name: &str) -> Option<u32> {
        out.iter().find_map(|c| match c {
            Command::Timer { id, after_ms } if id == name => Some(*after_ms),
            _ => None,
        })
    }

    fn fire(t: &mut Intiface, name: &str) -> Vec<Command> {
        t.handle(Event::Timer {
            id: name.into(),
            at_ms: 0,
        })
    }

    fn text(socket: u64, body: &str) -> Event {
        Event::Received {
            id: socket,
            data: Data::Text(body.into()),
        }
    }

    fn device_list(devices: &[(u32, &str)]) -> String {
        let devices: serde_json::Map<String, serde_json::Value> = devices
            .iter()
            .map(|(index, name)| {
                (
                    index.to_string(),
                    serde_json::json!({
                        "DeviceIndex": index,
                        "DeviceName": name,
                        "DeviceMessageTimingGap": 100,
                        "DeviceFeatures": {
                            "0": {"FeatureIndex": 0, "FeatureDescription": "",
                                  "Output": {"Vibrate": {"Value": [0, 20]}}},
                        },
                    }),
                )
            })
            .collect();
        serde_json::json!([{"DeviceList": {"Id": 0, "Devices": devices}}]).to_string()
    }

    /// Started, its limit read, the socket answered, `devices` connected.
    fn connected(devices: &[(u32, &str)]) -> (Intiface, u64) {
        let mut t = Intiface::default();
        let out = t.handle(Event::Start);
        let socket = opened(&out).expect("it looks for the server at once");
        t.handle(Event::Loaded {
            key: K_LIMIT.into(),
            value: None,
        });
        let out = t.handle(Event::Opened { id: socket });
        assert!(sent(&out)[0].contains("\"ProtocolVersionMajor\":4"));
        t.handle(text(
            socket,
            r#"[{"ServerInfo":{"Id":1,"MaxPingTime":0,"ProtocolVersionMajor":4,"ProtocolVersionMinor":0,"ServerName":"x"}}]"#,
        ));
        t.handle(text(socket, &device_list(devices)));
        (t, socket)
    }

    fn vibrate(t: &mut Intiface, call: u64, intensity: f64, seconds: f64) -> Vec<Command> {
        t.handle(Event::Call {
            id: call,
            method: "vibrate".into(),
            args: vec![
                Value::Number(intensity),
                Value::Number(seconds),
                Value::None,
            ],
            from: None,
        })
    }

    #[test]
    fn its_manifest_is_one_the_client_accepts() {
        let manifest = Intiface::default().manifest();
        assert_eq!(manifest.validate(), Ok(()));
        // The script editor's hover says what each one is for.
        let ns = manifest.namespace.expect("a namespace");
        let bare: Vec<&str> = ns
            .fields
            .iter()
            .map(|f| (&f.name, &f.summary))
            .chain(ns.methods.iter().map(|m| (&m.name, &m.summary)))
            .filter(|(_, summary)| summary.is_empty())
            .map(|(name, _)| name.as_str())
            .collect();
        assert!(bare.is_empty(), "no summary: {bare:?}");
    }

    /// A drive has a length, ended here: the protocol would let it run for ever.
    #[test]
    fn a_drive_holds_for_its_time_then_stops_and_answers() {
        let (mut t, _) = connected(&[(0, "Hush")]);
        let out = vibrate(&mut t, 7, 0.5, 3.0);
        assert_eq!(values(&out), [10], "half of the device's twenty steps");
        assert_eq!(timer(&out, "drive-7"), Some(3000));
        assert_eq!(replied(&out, 7), None, "a coroutine answers when it ends");

        let out = fire(&mut t, "drive-7");
        assert_eq!(values(&out), [0]);
        assert_eq!(replied(&out, 7), Some(true));
    }

    #[test]
    fn nothing_runs_before_the_limit_is_read_or_without_a_device() {
        let mut t = Intiface::default();
        let out = t.handle(Event::Start);
        let socket = opened(&out).unwrap();
        t.handle(Event::Opened { id: socket });
        t.handle(text(socket, r#"[{"ServerInfo":{"Id":1,"MaxPingTime":0}}]"#));
        t.handle(text(socket, &device_list(&[(0, "Hush")])));
        let out = vibrate(&mut t, 1, 1.0, 1.0);
        assert_eq!(values(&out), [0; 0], "the limit isn't known yet");
        assert_eq!(replied(&out, 1), Some(false));

        let (mut none, _) = connected(&[]);
        let out = vibrate(&mut none, 2, 1.0, 1.0);
        assert_eq!(replied(&out, 2), Some(false));
    }

    /// A person's limit holds whoever asks, and lowering it reaches what runs.
    #[test]
    fn the_persons_limit_holds_and_a_lower_one_reaches_what_runs() {
        let (mut t, _) = connected(&[(0, "Hush")]);
        let out = t.handle(Event::Ui {
            page: PAGE.into(),
            event: UiEvent::Number {
                id: "limit".into(),
                value: 0.5,
            },
        });
        assert!(out.iter().any(|c| matches!(c, Command::Store { key, value }
            if key == K_LIMIT && value == "0.5")));
        let out = vibrate(&mut t, 1, 1.0, 10.0);
        assert_eq!(values(&out), [10], "a full ask, held to the limit");
        let out = t.handle(Event::Ui {
            page: PAGE.into(),
            event: UiEvent::Number {
                id: "limit".into(),
                value: 0.25,
            },
        });
        assert_eq!(values(&out), [5], "without waiting for the next ask");
    }

    #[test]
    fn a_stored_limit_is_read_back() {
        let mut t = Intiface::default();
        t.handle(Event::Start);
        t.handle(Event::Loaded {
            key: K_LIMIT.into(),
            value: Some("0.3".into()),
        });
        assert_eq!(t.limit, Some(0.3));
    }

    #[test]
    fn a_cancelled_flow_stops_its_output_and_doesnt_answer() {
        let (mut t, _) = connected(&[(0, "Hush")]);
        vibrate(&mut t, 7, 0.5, 30.0);
        let out = t.handle(Event::Cancel { id: 7 });
        assert_eq!(values(&out), [0]);
        assert_eq!(replied(&out, 7), None);
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::CancelTimer { id } if id == "drive-7")));
    }

    #[test]
    fn stop_stops_everything_on_the_servers_word_and_answers_every_call() {
        let (mut t, _) = connected(&[(0, "Hush"), (1, "Lush")]);
        vibrate(&mut t, 1, 1.0, 30.0);
        let out = t.handle(Event::Call {
            id: 2,
            method: "stop".into(),
            args: Vec::new(),
            from: None,
        });
        assert!(sent(&out)[0].contains("StopCmd"));
        assert_eq!(replied(&out, 1), Some(false), "it didn't run its time");
        assert_eq!(replied(&out, 2), Some(true));
        assert_eq!(
            fire(&mut t, "drive-1")
                .iter()
                .filter(|c| matches!(c, Command::Reply { .. }))
                .count(),
            0
        );
    }

    /// An output has one holder, so a timer left over can't stop what took over.
    #[test]
    fn whoever_asked_last_holds_an_output() {
        let (mut t, _) = connected(&[(0, "Hush")]);
        vibrate(&mut t, 1, 0.5, 30.0);
        let out = vibrate(&mut t, 2, 1.0, 30.0);
        assert_eq!(values(&out), [20]);
        assert_eq!(replied(&out, 1), Some(false));
        let out = fire(&mut t, "drive-1");
        assert_eq!(values(&out), [0; 0], "the old timer stops nothing now");
    }

    #[test]
    fn a_named_device_is_the_only_one_driven() {
        let (mut t, _) = connected(&[(0, "Hush"), (1, "Lush")]);
        let out = t.handle(Event::Call {
            id: 1,
            method: "vibrate".into(),
            args: vec![
                Value::Number(1.0),
                Value::Number(1.0),
                Value::Text("Lush".into()),
            ],
            from: None,
        });
        assert_eq!(sent(&out).len(), 1);
        assert!(sent(&out)[0].contains("\"DeviceIndex\":1"));
        let out = t.handle(Event::Call {
            id: 2,
            method: "vibrate".into(),
            args: vec![
                Value::Number(1.0),
                Value::Number(1.0),
                Value::Text("Absent".into()),
            ],
            from: None,
        });
        assert_eq!(replied(&out, 2), Some(false));
    }

    /// The server stops every device when the socket goes, so a drive ends with
    /// it, and looking again waits longer each time until a server answers.
    #[test]
    fn a_lost_socket_ends_every_drive_and_looks_again_with_a_growing_wait() {
        let (mut t, socket) = connected(&[(0, "Hush")]);
        vibrate(&mut t, 1, 1.0, 30.0);
        let out = t.handle(Event::Closed {
            id: socket,
            error: None,
        });
        assert_eq!(replied(&out, 1), Some(false));
        assert_eq!(timer(&out, "retry"), Some(RETRY_MS));
        assert_eq!(opened(&out), None);
        assert_eq!(
            line(&out),
            Some((Tone::Waiting, "Connecting".to_owned())),
            "one that was there is on its way back"
        );
        let out = fire(&mut t, "retry");
        let again = opened(&out).expect("it looks again");
        let out = t.handle(Event::Closed {
            id: again,
            error: Some("refused".into()),
        });
        assert_eq!(timer(&out, "retry"), Some(RETRY_MS * 2));
    }

    #[test]
    fn a_server_that_isnt_there_is_quiet_and_never_an_error() {
        let mut t = Intiface::default();
        let out = t.handle(Event::Start);
        let out = t.handle(Event::Closed {
            id: opened(&out).unwrap(),
            error: Some("connection refused".into()),
        });
        assert_eq!(
            line(&out),
            Some((Tone::Idle, "Intiface isn't running".to_owned()))
        );
        assert_eq!(t.error, None);
    }

    /// A list replaces the last, and an index a device left may be another's.
    #[test]
    fn a_device_leaving_ends_its_drive_and_its_index_coming_back_isnt_driven() {
        let (mut t, socket) = connected(&[(0, "Hush"), (1, "Lush")]);
        vibrate(&mut t, 1, 1.0, 30.0);
        let out = t.handle(text(socket, &device_list(&[(1, "Lush")])));
        assert_eq!(replied(&out, 1), None, "it still holds the other");
        let out = t.handle(text(socket, &device_list(&[(0, "Edge"), (1, "Other")])));
        assert_eq!(
            replied(&out, 1),
            Some(false),
            "neither is what it was holding"
        );
        assert_eq!(t.drives.len(), 0);
    }

    #[test]
    fn it_pings_inside_a_servers_limit_and_not_when_it_has_none() {
        let mut t = Intiface::default();
        let out = t.handle(Event::Start);
        let socket = opened(&out).unwrap();
        t.handle(Event::Opened { id: socket });
        let out = t.handle(text(
            socket,
            r#"[{"ServerInfo":{"Id":1,"MaxPingTime":3000}}]"#,
        ));
        assert_eq!(timer(&out, "ping"), Some(1000));
        let out = fire(&mut t, "ping");
        assert!(sent(&out)[0].contains("Ping"));
        assert_eq!(timer(&out, "ping"), Some(1000));

        let (t, _) = connected(&[]);
        assert_eq!(t.max_ping_ms, 0);
    }

    /// Refused at the greeting is about this client, so asking again would meet
    /// the same answer until the person acts.
    #[test]
    fn a_refused_greeting_holds_until_reconnect_and_never_loops() {
        let mut t = Intiface::default();
        let out = t.handle(Event::Start);
        let socket = opened(&out).unwrap();
        t.handle(Event::Opened { id: socket });
        let out = t.handle(text(
            socket,
            r#"[{"Error":{"Id":1,"ErrorCode":1,"ErrorMessage":"Spec too new"}}]"#,
        ));
        assert_eq!(
            line(&out),
            Some((Tone::Bad, "Intiface refused this client".to_owned()))
        );
        assert_eq!(timer(&out, "retry"), None);
        assert_eq!(opened(&fire(&mut t, "retry")), None);
        let out = t.handle(Event::Ui {
            page: PAGE.into(),
            event: UiEvent::Pressed {
                id: "reconnect".into(),
            },
        });
        assert!(opened(&out).is_some(), "the person asked again");
    }

    #[test]
    fn a_server_that_never_greets_is_closed_and_looked_for_again() {
        let mut t = Intiface::default();
        let out = t.handle(Event::Start);
        let socket = opened(&out).unwrap();
        let out = t.handle(Event::Opened { id: socket });
        assert_eq!(timer(&out, "greeting"), Some(GREETING_MS));
        let out = fire(&mut t, "greeting");
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::Close { id } if *id == socket)));
        assert_eq!(timer(&out, "retry"), Some(RETRY_MS));
    }

    #[test]
    fn a_frame_from_another_socket_changes_nothing() {
        let (mut t, socket) = connected(&[(0, "Hush")]);
        t.handle(text(socket + 99, &device_list(&[])));
        assert_eq!(t.devices.len(), 1);
    }

    #[test]
    fn connected_needs_a_device_and_says_so() {
        let (mut t, socket) = connected(&[]);
        assert_eq!(t.state(), State::Waiting);
        let out = t.handle(text(socket, &device_list(&[(0, "Hush")])));
        assert_eq!(line(&out), Some((Tone::Good, "Connected".to_owned())));
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::Field { name, value }
            if name == "connected" && *value == Value::Bool(true))));
    }
}
