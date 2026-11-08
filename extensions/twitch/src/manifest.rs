//! What the module declares: the namespace `@TWITCH`, its fields, methods and
//! signals, the actions and conditions a mapping offers, and what it may reach.
//!
//! The surface is the contract the client enforces; `lib.rs` keeps the flow.

use vrc_we_sdk::{
    Action, Condition, Does, Field, Manifest, Method, Namespace, NetGrant, NetTo, NetWhat, Page,
    Param, Ports, Proto, Shape, Signal,
};

use crate::protocol;
use crate::protocol::GROUPS;
use crate::{PAGE, SECRET_AUTH, SECRET_REFRESH};

/// Twitch's Glitch, one colour, which the client draws in its theme's.
const LOGO_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M11.571 4.714h1.715v5.143H11.57zm4.715 0H18v5.143h-1.714zM6 0L1.714 4.286v15.428h5.143V24l4.286-4.286h3.428L22.286 12V0zm14.571 11.143l-3.428 3.428h-3.429l-3 3v-3H6.857V1.714h13.714Z"/></svg>"#;

pub(crate) fn build() -> Manifest {
    let text = |max: usize| Shape::Text { max };
    let number = Shape::Number(None);
    Manifest {
        name: "twitch".into(),
        author: vrc_we_sdk::cargo_authors(env!("CARGO_PKG_AUTHORS")),
        version: env!("CARGO_PKG_VERSION").into(),
        namespace: Some(Namespace {
            name: "TWITCH".into(),
            fields: vec![
                field("connected", Shape::Bool),
                field("user", text(64)),
                field("event", text(64)),
                field("user_name", text(128)),
                field("message", text(vrc_we_sdk::TEXT_MAX)),
                field("reward", text(256)),
                field("input", text(1_024)),
                field("error", text(1_024)),
                field("bits", number.clone()),
                field("tier", number.clone()),
                field("months", number.clone()),
                field("viewers", number.clone()),
                field("hype_level", number.clone()),
                field("hype_progress", number.clone()),
                field("hype_goal", number.clone()),
                field("live", Shape::Bool),
                field("poll_title", text(256)),
                field("poll_status", text(32)),
                field("poll_winner", text(256)),
                field("poll_votes", number),
            ],
            methods: vec![
                Method {
                    name: "chat".into(),
                    params: vec![Param {
                        name: "message".into(),
                        shape: text(protocol::CHAT_MAX),
                        options: None,
                        optional: false,
                    }],
                    returns: Shape::Bool,
                    coroutine: false,
                    moves: Vec::new(),
                    contacts: false,
                },
                Method {
                    name: "clip".into(),
                    params: vec![Param {
                        name: "title".into(),
                        shape: text(protocol::TITLE_MAX),
                        options: None,
                        // Twitch titles a clip itself; an empty one is fine.
                        optional: true,
                    }],
                    returns: Shape::Bool,
                    coroutine: false,
                    moves: Vec::new(),
                    contacts: false,
                },
                Method {
                    name: "marker".into(),
                    params: vec![Param {
                        name: "description".into(),
                        shape: text(protocol::TITLE_MAX),
                        options: None,
                        // A marker without a note is still a marker.
                        optional: true,
                    }],
                    returns: Shape::Bool,
                    coroutine: false,
                    moves: Vec::new(),
                    contacts: false,
                },
                Method {
                    name: "announce".into(),
                    params: vec![Param {
                        name: "message".into(),
                        shape: text(protocol::CHAT_MAX),
                        options: None,
                        optional: false,
                    }],
                    returns: Shape::Bool,
                    coroutine: false,
                    moves: Vec::new(),
                    contacts: false,
                },
                Method {
                    name: "title".into(),
                    params: vec![Param {
                        name: "title".into(),
                        shape: text(protocol::TITLE_MAX),
                        options: None,
                        optional: false,
                    }],
                    returns: Shape::Bool,
                    coroutine: false,
                    moves: Vec::new(),
                    contacts: false,
                },
                Method {
                    name: "commercial".into(),
                    params: vec![Param {
                        name: "seconds".into(),
                        shape: Shape::Number(None),
                        options: None,
                        // 60 unless the person asks for another length.
                        optional: true,
                    }],
                    returns: Shape::Bool,
                    coroutine: false,
                    moves: Vec::new(),
                    contacts: false,
                },
                Method {
                    name: "poll".into(),
                    params: {
                        let mut params = vec![
                            Param {
                                name: "title".into(),
                                shape: text(protocol::TITLE_MAX),
                                options: None,
                                optional: false,
                            },
                            Param {
                                name: "first".into(),
                                shape: text(protocol::TITLE_MAX),
                                options: None,
                                optional: false,
                            },
                            Param {
                                name: "second".into(),
                                shape: text(protocol::TITLE_MAX),
                                options: None,
                                optional: false,
                            },
                        ];
                        for name in ["third", "fourth", "fifth"] {
                            params.push(Param {
                                name: name.into(),
                                shape: text(protocol::TITLE_MAX),
                                options: None,
                                optional: true,
                            });
                        }
                        params.push(Param {
                            name: "seconds".into(),
                            shape: Shape::Number(None),
                            options: None,
                            optional: true,
                        });
                        params
                    },
                    returns: Shape::Bool,
                    coroutine: false,
                    moves: Vec::new(),
                    contacts: false,
                },
            ],
            // One signal per group: a mapping on `cheered` is a mapping
            // that needs cheers, and the client tells the module so.
            signals: GROUPS
                .into_iter()
                .flat_map(|g| {
                    g.signals().iter().map(move |s| Signal {
                        name: (*s).into(),
                        label: signal_label(s).into(),
                        payload: g.payload_shape(s),
                    })
                })
                .collect(),
        }),
        osc_read: Vec::new(),
        osc_write: Vec::new(),
        pages: vec![Page {
            id: PAGE.into(),
            title: "Twitch".into(),
        }],
        actions: vec![
            Action {
                label: "Send to Twitch chat".into(),
                does: Does::Call("chat".into()),
                in_macros: true,
            },
            Action {
                label: "Clip the last 30 seconds".into(),
                does: Does::Call("clip".into()),
                in_macros: true,
            },
            Action {
                label: "Mark this moment".into(),
                does: Does::Call("marker".into()),
                in_macros: true,
            },
            Action {
                label: "Announce in chat".into(),
                does: Does::Call("announce".into()),
                in_macros: true,
            },
            Action {
                label: "Set the stream title".into(),
                does: Does::Call("title".into()),
                in_macros: true,
            },
            Action {
                label: "Run a commercial".into(),
                does: Does::Call("commercial".into()),
                in_macros: true,
            },
            Action {
                label: "Start a poll".into(),
                does: Does::Call("poll".into()),
                in_macros: true,
            },
        ],
        // Conditions read a field the editor can hold to: a switch or a
        // number, never text.
        conditions: vec![
            Condition {
                label: "Connected".into(),
                field: "connected".into(),
            },
            Condition {
                label: "Bits".into(),
                field: "bits".into(),
            },
            Condition {
                label: "Hype level".into(),
                field: "hype_level".into(),
            },
            Condition {
                label: "Raid viewers".into(),
                field: "viewers".into(),
            },
            Condition {
                label: "Subscription tier".into(),
                field: "tier".into(),
            },
            Condition {
                label: "Months subscribed".into(),
                field: "months".into(),
            },
            Condition {
                label: "Poll votes".into(),
                field: "poll_votes".into(),
            },
            Condition {
                label: "The stream is live".into(),
                field: "live".into(),
            },
        ],
        secrets: vec![SECRET_AUTH.into(), SECRET_REFRESH.into()],
        notifies: false,
        network: vec![
            grant("id.twitch.tv"),
            grant("api.twitch.tv"),
            grant("eventsub.wss.twitch.tv"),
        ],
        log_prefixes: Vec::new(),
        contacts: false,
        icons: vec![vrc_we_sdk::Icon {
            name: crate::page::LOGO.into(),
            svg: LOGO_SVG.into(),
        }],
        terms: Some(vrc_we_sdk::Terms {
            label: "Twitch terms of service".into(),
            url: "https://www.twitch.tv/p/legal/terms-of-service/".into(),
            ..vrc_we_sdk::Terms::default()
        }),
        status: true,
        description: env!("CARGO_PKG_DESCRIPTION").into(),
        documentation: env!("CARGO_PKG_HOMEPAGE").into(),
        license: env!("CARGO_PKG_LICENSE").into(),
        docs: include_str!("docs.md").into(),
    }
}

fn field(name: &str, shape: Shape) -> Field {
    Field {
        name: name.into(),
        shape,
        writable: false,
        shared: false,
    }
}

fn grant(name: &str) -> NetGrant {
    NetGrant {
        what: NetWhat::Connect,
        proto: Proto::Tcp,
        to: NetTo::Domain {
            name: name.into(),
            lan: false,
        },
        ports: Ports::one(443),
    }
}

fn signal_label(signal: &str) -> &'static str {
    match signal {
        "redeemed" => "A channel point redemption",
        "chatted" => "A chat message",
        "subscribed" => "A subscription",
        "gifted" => "Gifted subs",
        "cheered" => "A cheer",
        "raided" => "A raid",
        "hype" => "The hype train",
        "followed" => "A follow",
        "went_live" => "The stream went live",
        "went_offline" => "The stream went offline",
        "polled" => "A poll",
        "poll_ended" => "A poll ended",
        _ => "A Twitch event",
    }
}
