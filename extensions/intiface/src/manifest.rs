//! What the module declares: `@INTIFACE`, its two actions, its page, and the
//! one local port it may reach.

use vrc_we_sdk::{
    cargo_authors, Action, Condition, Does, Field, Manifest, Method, Namespace, NetGrant, NetTo,
    NetWhat, Page, Param, Ports, Proto, Range, Shape,
};

use crate::{PAGE, SECONDS_MAX};

fn field(name: &str, shape: Shape) -> Field {
    Field {
        name: name.into(),
        shape,
        writable: false,
        shared: true,
    }
}

fn param(name: &str, shape: Shape, optional: bool) -> Param {
    Param {
        name: name.into(),
        shape,
        options: None,
        optional,
    }
}

pub(crate) fn build() -> Manifest {
    let unit = Shape::Number(Some(Range::new(0.0, 1.0)));
    let vibrate = Method {
        name: "vibrate".into(),
        params: vec![
            param("intensity", unit.clone(), false),
            param(
                "seconds",
                Shape::Number(Some(Range::new(0.0, SECONDS_MAX))),
                false,
            ),
            param("device", Shape::Text { max: 256 }, true),
        ],
        returns: Shape::Bool,
        coroutine: true,
        moves: Vec::new(),
        contacts: true,
    };
    let stop = Method {
        name: "stop".into(),
        params: Vec::new(),
        returns: Shape::Bool,
        coroutine: false,
        moves: Vec::new(),
        contacts: true,
    };
    Manifest {
        name: env!("CARGO_PKG_NAME").into(),
        author: cargo_authors(env!("CARGO_PKG_AUTHORS")),
        version: env!("CARGO_PKG_VERSION").into(),
        namespace: Some(Namespace {
            name: "INTIFACE".into(),
            fields: vec![
                field("connected", Shape::Bool),
                field("devices", Shape::Number(None)),
                field("level", unit),
            ],
            methods: vec![vibrate, stop],
            signals: Vec::new(),
        }),
        osc_read: Vec::new(),
        osc_write: Vec::new(),
        pages: vec![Page {
            id: PAGE.into(),
            title: "Intiface".into(),
        }],
        actions: vec![
            Action {
                label: "Vibrate".into(),
                does: Does::Call("vibrate".into()),
                in_macros: true,
            },
            Action {
                label: "Stop all devices".into(),
                does: Does::Call("stop".into()),
                in_macros: true,
            },
        ],
        conditions: vec![
            Condition {
                label: "Connected".into(),
                field: "connected".into(),
            },
            Condition {
                label: "Connected devices".into(),
                field: "devices".into(),
            },
        ],
        secrets: Vec::new(),
        notifies: false,
        network: vec![NetGrant {
            what: NetWhat::Connect,
            proto: Proto::Tcp,
            to: NetTo::Loopback,
            ports: Ports::one(12345),
        }],
        log_prefixes: Vec::new(),
        contacts: false,
        icons: Vec::new(),
        terms: None,
        status: true,
        game: false,
        players: false,
        description: env!("CARGO_PKG_DESCRIPTION").into(),
        documentation: env!("CARGO_PKG_HOMEPAGE").into(),
        license: env!("CARGO_PKG_LICENSE").into(),
        docs: String::new(),
    }
}
