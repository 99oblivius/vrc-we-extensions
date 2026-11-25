//! What the module declares: `@INTIFACE`, its two actions, its page, and the
//! one local port it may reach.

use vrc_we_sdk::{
    cargo_authors, Action, Condition, Does, Field, Icon, Manifest, Method, Namespace, NetGrant,
    NetTo, NetWhat, Page, Param, Ports, Proto, Range, Shape,
};

use crate::page::LOGO;
use crate::{PAGE, SECONDS_MAX};

/// The letters of Intiface's logo in one color, which the client draws in its
/// theme's.
const LOGO_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="106 291 500 500"><defs><mask id="gap" maskUnits="userSpaceOnUse" x="106" y="291" width="500" height="500"><path d="M106 291h500v500H106z" fill="#fff"/><path d="M201,443c28-14,57-22,75-22c36,0,51,23,51,49c0,22-9,55-18,86l-22,74c25-6,42-11,66-18c8,10,14,26,16,39c-31,22-78,40-107,40c-35,0-52-18-52-44c0-17,3-29,12-60c10-33,28-77,44-113c-16,4-29,8-43,12C215,474,205,456,201,443z" fill="#000" stroke="#000" stroke-width="50"/></mask></defs><path d="M201,443c28-14,57-22,75-22c36,0,51,23,51,49c0,22-9,55-18,86l-22,74c25-6,42-11,66-18c8,10,14,26,16,39c-31,22-78,40-107,40c-35,0-52-18-52-44c0-17,3-29,12-60c10-33,28-77,44-113c-16,4-29,8-43,12C215,474,205,456,201,443zM264,343c6-30,27-44,56-44c30,0,51,9,45,38c-6,29-27,45-56,45C280,382,258,372,264,343z"/><path mask="url(#gap)" d="M426,453c-5,18-9,40-11,61c25,0,53,0,77,1c-1,16-8,45-14,61c-28-1-49-1-73-3l-13,76c-15,83-58,138-171,153c-9-14-15-41-14-61c31-11,62-17,92-15c11-35,19-71,25-103l9-54c-22,1-39,3-67,2c1-11,7-45,10-55c20-1,42-1,67-1c7-73,50-133,155-143c13,14,20,40,18,65C490,447,462,453,426,453z"/></svg>"##;

fn field(name: &str, shape: Shape, summary: &str) -> Field {
    Field {
        name: name.into(),
        shape,
        writable: false,
        shared: true,
        summary: summary.into(),
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
        summary: "Vibrates the named device, or all, for seconds".into(),
    };
    let stop = Method {
        name: "stop".into(),
        params: Vec::new(),
        returns: Shape::Bool,
        coroutine: false,
        moves: Vec::new(),
        contacts: true,
        summary: "Stops every device".into(),
    };
    Manifest {
        name: env!("CARGO_PKG_NAME").into(),
        author: cargo_authors(env!("CARGO_PKG_AUTHORS")),
        version: env!("CARGO_PKG_VERSION").into(),
        namespace: Some(Namespace {
            name: "INTIFACE".into(),
            fields: vec![
                field("connected", Shape::Bool, "Whether the server is connected"),
                field(
                    "devices",
                    Shape::Number(None),
                    "How many devices the server has connected",
                ),
                field("level", unit, "The strongest vibration running, 0 to 1"),
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
        icons: vec![Icon {
            name: LOGO.into(),
            svg: LOGO_SVG.into(),
        }],
        terms: None,
        status: true,
        game: false,
        players: false,
        description: env!("CARGO_PKG_DESCRIPTION").into(),
        documentation: env!("CARGO_PKG_HOMEPAGE").into(),
        license: env!("CARGO_PKG_LICENSE").into(),
        docs: include_str!("docs.md").into(),
    }
}
