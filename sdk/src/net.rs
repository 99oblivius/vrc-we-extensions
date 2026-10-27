//! Where an extension may reach: its grants, the endpoints a command names,
//! the bytes a body carries, and the headers a secret is placed in.

use serde::{Deserialize, Serialize};

/// Where an extension may connect or listen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetGrant {
    pub what: NetWhat,
    pub proto: Proto,
    pub to: NetTo,
    pub ports: Ports,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetWhat {
    Connect,
    Listen,
}

/// HTTP(S) and WebSocket connections are TCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Proto {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetTo {
    /// This computer: 127.0.0.0/8, ::1 and `localhost`.
    Loopback,
    /// The person's network: an address mask (`192.168.1.0/24`) or an
    /// mDNS name (`printer.local`, `*.local`).
    Lan(String),
    /// An internet name, or every name under one (`*.example.com`). Its
    /// addresses must be public, unless `lan`: a name that resolves to the
    /// person's own network (`*.lovense.club`).
    Domain { name: String, lan: bool },
    /// Anywhere.
    Any,
}

/// A port, or a range of them, inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ports {
    pub from: u16,
    pub to: u16,
}

impl Ports {
    pub fn one(port: u16) -> Self {
        Self {
            from: port,
            to: port,
        }
    }

    pub fn contains(&self, port: u16) -> bool {
        (self.from..=self.to).contains(&port)
    }
}

/// The most network grants a manifest declares.
pub const NET_GRANTS_MAX: usize = 16;

impl NetGrant {
    /// Whether it reaches far enough to warn about: anywhere, a mask
    /// wider than a /16 (IPv4) or a /48 (IPv6), every port, or listening
    /// beyond this computer.
    pub fn broad(&self) -> bool {
        let wide = match &self.to {
            NetTo::Any => true,
            NetTo::Lan(mask) => lan_mask(mask).is_some_and(|(ip, bits)| match ip {
                std::net::IpAddr::V4(_) => bits < 16,
                std::net::IpAddr::V6(_) => bits < 48,
            }),
            NetTo::Loopback | NetTo::Domain { .. } => false,
        };
        let every_port = self.ports.from <= 1 && self.ports.to == u16::MAX;
        let listens_out = self.what == NetWhat::Listen && self.to != NetTo::Loopback;
        wide || every_port || listens_out
    }

    pub(crate) fn sound(&self) -> bool {
        let to = match &self.to {
            NetTo::Loopback | NetTo::Any => true,
            NetTo::Lan(lan) => lan_mask(lan).is_some() || is_mdns_name(lan),
            NetTo::Domain { name, .. } => {
                self.what == NetWhat::Connect && is_domain(name.strip_prefix("*.").unwrap_or(name))
            }
        };
        to && self.ports.from >= 1 && self.ports.from <= self.ports.to
    }
}

/// `addr/bits` as an address mask: the network address and its prefix
/// length.
pub fn lan_mask(mask: &str) -> Option<(std::net::IpAddr, u8)> {
    let (addr, bits) = mask.split_once('/')?;
    let addr: std::net::IpAddr = addr.parse().ok()?;
    let bits: u8 = bits.parse().ok()?;
    let max = if addr.is_ipv4() { 32 } else { 128 };
    (bits <= max).then_some((addr, bits))
}

/// An mDNS name: `name.local`, or `*.local` for any.
pub fn is_mdns_name(name: &str) -> bool {
    name.strip_suffix(".local")
        .is_some_and(|rest| rest == "*" || (is_domain(rest) && !rest.contains('*')))
}

/// A DNS name of letters, digits and hyphens: at least one dot, at most
/// 253 bytes, each label 1 to 63 bytes, not an address, not `.local`.
fn is_domain(name: &str) -> bool {
    name.len() <= 253
        && name.parse::<std::net::IpAddr>().is_err()
        && !name.ends_with(".local")
        && name.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        && (name.contains('.') || name == "localhost")
}

/// A connection's far end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Endpoint {
    /// A WebSocket: `ws://` or `wss://`.
    Ws { url: String },
    /// A TCP stream; what it receives comes as bytes, in chunks of at most
    /// [`NET_CHUNK_MAX`].
    Tcp { host: String, port: u16 },
    /// UDP datagrams to and from one address.
    Udp { host: String, port: u16 },
}

/// The most bytes one [`Event::Received`] of a TCP stream carries, and the
/// largest UDP datagram.
pub const NET_CHUNK_MAX: usize = 64 * 1024;

/// The bit set in the ids the client gives accepted connections
/// ([`Event::Accepted`]); an extension's own ids leave it clear.
pub const ACCEPTED_ID: u64 = 1 << 63;

/// What crosses a connection or an HTTP body: text, or bytes (base64 in
/// the boundary's JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Data {
    Text(String),
    Bytes(#[serde(with = "base64_bytes")] Vec<u8>),
}

impl Data {
    pub fn len(&self) -> usize {
        match self {
            Data::Text(t) => t.len(),
            Data::Bytes(b) => b.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

mod base64_bytes {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(serde::de::Error::custom)
    }
}

/// A request header's value: text, or a secret slot the client fills in
/// from the keyring; the extension never holds the secret.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Header {
    Text(String),
    Secret(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Data,
}

/// The longest HTTP body either way, and the longest message a connection
/// carries either way, in bytes.
pub const NET_BODY_MAX: usize = 1024 * 1024;

/// The most HTTP requests in flight and connections open, per extension.
pub const NET_OPEN_MAX: usize = 16;
