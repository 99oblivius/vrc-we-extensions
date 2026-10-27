//! A starting point for an extension: a counter scripts read as
//! `@TEMPLATE.count` and add to with `@TEMPLATE.add(n)`, kept in the
//! extension's store across runs, with a page that shows and resets it and
//! an action that adds to it.
//!
//! Build: `cargo build --release --target wasm32-unknown-unknown -p template`;
//! install the `.wasm` in the client from Settings, Extensions.

use vrc_we_sdk::{
    Action, Command, Condition, Does, Event, Extension, Field, Manifest, Method, Namespace, Node,
    Page, Param, Range, Shape, Signal, UiEvent, Value,
};

vrc_we_sdk::export!(Template);

const PAGE: &str = "template";
const STORE_KEY: &str = "count";

#[derive(Default)]
pub struct Template {
    count: f64,
}

impl Template {
    /// The count as scripts and the page see it, and kept.
    fn publish(&self) -> Vec<Command> {
        vec![
            Command::Field {
                name: "count".into(),
                value: Value::Number(self.count),
            },
            Command::Page {
                page: PAGE.into(),
                view: Node::Column(vec![
                    Node::Heading("Template".into()),
                    Node::Text(format!("Count: {}", self.count)),
                    Node::Button {
                        id: "reset".into(),
                        label: "Reset".into(),
                        icon: None,
                        wide: false,
                    },
                ]),
            },
            Command::Store {
                key: STORE_KEY.into(),
                value: self.count.to_string(),
            },
        ]
    }
}

impl Extension for Template {
    fn manifest(&self) -> Manifest {
        Manifest {
            name: "template".into(),
            author: "vrc-we".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            namespace: Some(Namespace {
                name: "TEMPLATE".into(),
                fields: vec![Field {
                    name: "count".into(),
                    shape: Shape::Number(None),
                    writable: false,
                    shared: true,
                }],
                methods: vec![Method {
                    name: "add".into(),
                    params: vec![Param {
                        name: "n".into(),
                        shape: Shape::Number(Some(Range::new(-1000.0, 1000.0))),
                        options: None,
                        optional: false,
                    }],
                    returns: Shape::Number(None),
                    coroutine: false,
                    moves: Vec::new(),
                    contacts: false,
                }],
                signals: vec![Signal {
                    name: "reset".into(),
                    label: "Count reset".into(),
                    payload: Shape::Number(None),
                }],
            }),
            osc_read: Vec::new(),
            osc_write: Vec::new(),
            pages: vec![Page {
                id: PAGE.into(),
                title: "Template".into(),
            }],
            actions: vec![Action {
                label: "Add to the count".into(),
                does: Does::Call("add".into()),
                in_macros: true,
            }],
            conditions: vec![Condition {
                label: "Count".into(),
                field: "count".into(),
            }],
            secrets: Vec::new(),
            notifies: false,
            network: Vec::new(),
            log_prefixes: Vec::new(),
            contacts: false,
            icons: Vec::new(),
        }
    }

    fn handle(&mut self, event: Event) -> Vec<Command> {
        match event {
            Event::Start => vec![Command::Load {
                key: STORE_KEY.into(),
            }],
            Event::Loaded { value, .. } => {
                self.count = value.and_then(|v| v.parse().ok()).unwrap_or(0.0);
                self.publish()
            }
            Event::Call {
                id, method, args, ..
            } if method == "add" => {
                if let Some(Value::Number(n)) = args.first() {
                    self.count += n;
                }
                let mut out = self.publish();
                out.push(Command::Reply {
                    id,
                    value: Value::Number(self.count),
                });
                out
            }
            Event::Ui {
                event: UiEvent::Pressed { id },
                ..
            } if id == "reset" => {
                let mut out = vec![Command::Signal {
                    name: "reset".into(),
                    value: Value::Number(self.count),
                }];
                self.count = 0.0;
                out.extend(self.publish());
                out
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_manifest_is_one_the_client_accepts() {
        assert_eq!(Template::default().manifest().validate(), Ok(()));
    }

    #[test]
    fn it_counts_from_its_store_and_resets() {
        let mut t = Template::default();
        assert_eq!(
            t.handle(Event::Start),
            [Command::Load {
                key: "count".into()
            }]
        );
        t.handle(Event::Loaded {
            key: "count".into(),
            value: Some("3".into()),
        });
        let out = t.handle(Event::Call {
            id: 1,
            method: "add".into(),
            args: vec![Value::Number(2.0)],
            from: None,
        });
        assert!(out.contains(&Command::Reply {
            id: 1,
            value: Value::Number(5.0)
        }));
        let out = t.handle(Event::Ui {
            page: PAGE.into(),
            event: UiEvent::Pressed { id: "reset".into() },
        });
        assert_eq!(t.count, 0.0);
        assert!(
            out.contains(&Command::Signal {
                name: "reset".into(),
                value: Value::Number(5.0)
            }),
            "a reset is signalled with the count it ended"
        );
    }
}
