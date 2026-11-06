//! The Buttplug protocol, spec v4, as text: what the module sends to a local
//! Intiface server over the socket the client holds, and what it reads back.
//! It is plain JSON, so the module needs no Buttplug client and adds no IO.

use serde_json::{json, Value};

/// Where Intiface Central's server listens unless told otherwise. A person who
/// moved it remaps the port in the extension's Settings.
pub const URL: &str = "ws://127.0.0.1:12345";
const CLIENT: &str = "VRC WiredExpressions";

fn one(name: &str, body: Value) -> String {
    json!([{ name: body }]).to_string()
}

pub fn request_server_info(id: u64) -> String {
    one(
        "RequestServerInfo",
        json!({
            "Id": id,
            "ClientName": CLIENT,
            "ProtocolVersionMajor": 4,
            "ProtocolVersionMinor": 0,
        }),
    )
}

pub fn request_devices(id: u64) -> String {
    one("RequestDeviceList", json!({ "Id": id }))
}

pub fn start_scanning(id: u64) -> String {
    one("StartScanning", json!({ "Id": id }))
}

pub fn stop_scanning(id: u64) -> String {
    one("StopScanning", json!({ "Id": id }))
}

pub fn ping(id: u64) -> String {
    one("Ping", json!({ "Id": id }))
}

/// Holds output `feature` of `device` at `value`, until something says otherwise:
/// the spec gives a vibration no duration.
pub fn vibrate(id: u64, device: u32, feature: u32, value: i64) -> String {
    one(
        "OutputCmd",
        json!({
            "Id": id,
            "DeviceIndex": device,
            "FeatureIndex": feature,
            "Command": { "Vibrate": { "Value": value } },
        }),
    )
}

/// Stops every output of every device.
pub fn stop_all(id: u64) -> String {
    one(
        "StopCmd",
        json!({ "Id": id, "Inputs": true, "Outputs": true }),
    )
}

/// One output it can drive, with the range the device reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub feature: u32,
    pub max: i64,
}

impl Output {
    /// `level` (0 to 1) in the device's steps. Anything above nothing is at
    /// least one step, so a low level is never silently off.
    pub fn value(&self, level: f64) -> i64 {
        if level <= 0.0 {
            return 0;
        }
        ((level.min(1.0) * self.max as f64).round() as i64).clamp(1, self.max)
    }
}

/// A device the server has connected, with the outputs this module can drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Stable only while the device stays in the list: a server may reuse it.
    pub index: u32,
    pub name: String,
    pub outputs: Vec<Output>,
}

/// What a server said.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    ServerInfo {
        max_ping_ms: u32,
    },
    /// The whole list, sent on connect and on every change.
    Devices(Vec<Device>),
    ScanningFinished,
    Error(String),
    Other,
}

/// Reads one frame: an array of messages, each keyed by its name.
pub fn parse(text: &str) -> Result<Vec<Message>, String> {
    let frame: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let messages = frame.as_array().ok_or("a frame is an array")?;
    Ok(messages.iter().map(message).collect())
}

fn message(m: &Value) -> Message {
    if let Some(info) = m.get("ServerInfo") {
        return Message::ServerInfo {
            max_ping_ms: info
                .get("MaxPingTime")
                .and_then(Value::as_u64)
                .and_then(|ms| u32::try_from(ms).ok())
                .unwrap_or(0),
        };
    }
    if let Some(list) = m.get("DeviceList") {
        return Message::Devices(devices(list));
    }
    if m.get("ScanningFinished").is_some() {
        return Message::ScanningFinished;
    }
    if let Some(error) = m.get("Error") {
        return Message::Error(
            error
                .get("ErrorMessage")
                .and_then(Value::as_str)
                .unwrap_or("Intiface reported an error")
                .to_owned(),
        );
    }
    Message::Other
}

/// The devices of a `DeviceList`, by index, without those it has nothing to
/// drive on. Indexes arrive as keys, which sort as text.
fn devices(list: &Value) -> Vec<Device> {
    let mut out: Vec<Device> = list
        .get("Devices")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(index, device)| {
            let outputs = outputs(device);
            (!outputs.is_empty()).then(|| {
                let text = |key: &str| device.get(key).and_then(Value::as_str);
                Device {
                    index: index.parse().unwrap_or(u32::MAX),
                    name: text("DeviceDisplayName")
                        .or(text("DeviceName"))
                        .unwrap_or("Device")
                        .to_owned(),
                    outputs,
                }
            })
        })
        .collect();
    out.sort_by_key(|d| d.index);
    out
}

fn outputs(device: &Value) -> Vec<Output> {
    let mut out: Vec<Output> = device
        .get("DeviceFeatures")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(feature, f)| {
            let range = f.get("Output")?.get("Vibrate")?.get("Value")?;
            let max = range.get(1)?.as_i64().filter(|max| *max > 0)?;
            Some(Output {
                feature: feature.parse().ok()?,
                max,
            })
        })
        .collect();
    out.sort_by_key(|o| o.feature);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As Intiface Engine 5.0.5 answers for a Lovense Hush.
    const REAL_LIST: &str = r#"[{"DeviceList":{"Devices":{"0":{"DeviceFeatures":{"0":{"FeatureDescription":"","FeatureIndex":0,"Output":{"Vibrate":{"Value":[0,20]}}},"1":{"FeatureDescription":"battery Level","FeatureIndex":1,"Input":{"Battery":{"Command":["Read"],"Value":[[0,100]]}}}},"DeviceIndex":0,"DeviceMessageTimingGap":100,"DeviceName":"Lovense Hush"}},"Id":0}}]"#;

    #[test]
    fn a_real_device_list_reads_its_outputs_and_skips_its_battery() {
        let [Message::Devices(devices)] = &parse(REAL_LIST).unwrap()[..] else {
            panic!("one device list");
        };
        assert_eq!(
            devices,
            &[Device {
                index: 0,
                name: "Lovense Hush".into(),
                outputs: vec![Output {
                    feature: 0,
                    max: 20
                }],
            }]
        );
    }

    #[test]
    fn a_level_lands_within_the_devices_steps() {
        let output = Output {
            feature: 0,
            max: 20,
        };
        assert_eq!(output.value(0.0), 0);
        assert_eq!(output.value(0.5), 10);
        assert_eq!(output.value(1.0), 20);
        assert_eq!(output.value(0.001), 1, "a low level isn't silently off");
        assert_eq!(output.value(5.0), 20);
    }

    #[test]
    fn a_frame_that_isnt_an_array_is_an_error_and_an_unknown_message_is_nothing() {
        assert!(parse("{}").is_err());
        assert_eq!(parse(r#"[{"Ok":{"Id":1}}]"#).unwrap(), [Message::Other]);
    }
}
