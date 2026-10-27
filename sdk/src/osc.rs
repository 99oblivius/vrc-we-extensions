//! One OSC message, as an extension receives or sends it.

use crate::*;

use serde::{Deserialize, Serialize};

/// An OSC message, its arguments as VRChat sends them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OscMessage {
    pub addr: String,
    pub args: Vec<OscArg>,
}

/// The longest OSC address, in bytes.
pub const OSC_ADDR_MAX: usize = 256;

/// The largest OSC message the client sends for an extension, encoded.
pub const OSC_MESSAGE_MAX: usize = 4096;

/// The most prefixes in `osc_read`, and in `osc_write`.
pub const OSC_PREFIXES_MAX: usize = 16;

impl OscMessage {
    /// Its size encoded as OSC 1.0: the address, the type tags, then each
    /// argument, strings NUL-terminated and everything padded to 4 bytes.
    pub fn encoded_len(&self) -> usize {
        let string = |len: usize| (len + 4) & !3;
        string(self.addr.len())
            + string(1 + self.args.len())
            + self
                .args
                .iter()
                .map(|a| match a {
                    OscArg::Bool(_) => 0,
                    OscArg::Int(_) | OscArg::Float(_) => 4,
                    OscArg::Text(t) => string(t.len()),
                })
                .sum::<usize>()
    }

    /// Whether the client sends it: a literal address (no pattern
    /// characters), finite numbers, no NUL inside a text, and at most
    /// [`OSC_MESSAGE_MAX`] encoded.
    pub fn fits(&self) -> bool {
        is_osc_address(&self.addr)
            && self.args.iter().all(|a| match a {
                OscArg::Float(f) => f.is_finite(),
                OscArg::Text(t) => !t.contains('\0'),
                OscArg::Bool(_) | OscArg::Int(_) => true,
            })
            && self.encoded_len() <= OSC_MESSAGE_MAX
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OscArg {
    Bool(bool),
    Int(i32),
    Float(f32),
    Text(String),
}
