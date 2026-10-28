//! Twitch for mappings: chat messages, channel point redemptions, subs, cheers,
//! raids, hype trains and follows arrive over one EventSub WebSocket, and a
//! mapping reacts to them. The page is setup and maintenance only: whether the
//! account is connected, what is being listened for, and what went wrong last.
//!
//! The token is the person's own user token, obtained through Twitch's device
//! authorization grant with a public client: the module asks for a code, the
//! page offers Twitch's own address with that code already in it, and the
//! person approves there. Nothing is typed into the client, and there is no
//! redirect and no client secret. The pair lives in the OS keyring;
//! `/oauth2/validate` names the account, the client and the scopes, and every
//! Helix call carries the application's client id.
//!
//! A public client's refresh token is one-time use and expires after 30 days
//! unused, so every renewal rotates both halves and a lapsed one sends the
//! person back through one approval. The flow end to end: the page's Connect
//! asks `/oauth2/device` for a code, the page shows the verification address,
//! the module polls `/oauth2/token` at the interval Twitch gave, and the
//! answer is stored and checked. The renewal timer is armed a minute before
//! the access token's life ends. The socket subscribes what the client says
//! its rules read. The action that met the 401 is repeated once: Twitch
//! refused it before doing anything, so repeating it is not doing it twice.
//!
//! Build: `cargo wasm -p twitch`.

mod manifest;
mod page;
mod protocol;

use std::collections::HashMap;

use vrc_we_sdk::{
    Command, Data, Endpoint, Event, Extension, Header, Manifest, Shape, UiEvent, Value,
};

use page::Status;
use protocol::{Frame, Group, Identity, Token, GROUPS};
use serde_json::Value as Json;

vrc_we_sdk::export!(Twitch);

const PAGE: &str = "twitch";
/// `"Bearer <access>"`, which both Helix and `/oauth2/validate` take.
const SECRET_AUTH: &str = "auth";
const SECRET_REFRESH: &str = "refresh";
const K_IDENTITY: &str = "identity";

/// What each send needs of the token; a refusal names it.
const SCOPE_CHAT: &str = "user:write:chat";
const SCOPE_CLIP: &str = "clips:edit";
const SCOPE_BROADCAST: &str = "channel:manage:broadcast";
const SCOPE_ANNOUNCE: &str = "moderator:manage:announcements";
const SCOPE_COMMERCIAL: &str = "channel:edit:commercial";
const SCOPE_POLLS: &str = "channel:manage:polls";

/// The Twitch application this build asks for a token. A client id is public
/// — Twitch says so, and it travels in every authorize URL — but it names one
/// application: a fork must register its own and replace this, since reusing
/// it would be a second application in Twitch's eyes.
const CLIENT_ID: &str = "awyzerbycsfre9z9rsox3q0wlpnn9r";

/// How long before a token's expiry it is renewed.
const REFRESH_LEAD_MS: u64 = 60_000;
/// How long to wait before reconnecting a socket that dropped.
const RETRY_MS: u32 = 5_000;
/// What a mapping's poll runs for, within Twitch's bounds.
const POLL_SECONDS: u64 = 300;
const POLL_MIN_SECONDS: u64 = 15;
const POLL_MAX_SECONDS: u64 = 1800;

/// What the client's rules and scripts read of this extension, by name.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Watched {
    fields: Vec<String>,
    signals: Vec<String>,
}

impl Group {
    /// What its signal carries: the value a mapping's payload match compares,
    /// so a number group matches with a numeric condition and a text one with
    /// equality, and the group itself already says which event it was.
    fn payload_shape(self, signal: &str) -> Shape {
        match (self, signal) {
            (Self::Redeems, _) => Shape::Text { max: 256 },
            (Self::Chat, _) => Shape::Text { max: 64 },
            (Self::Poll, "polled" | "poll_ended") => Shape::Text { max: 256 },
            (Self::Follow, _) | (Self::Live, _) => Shape::None,
            _ => Shape::Number(None),
        }
    }
}

/// The signal one event is sent under, and what it carries: the value a
/// mapping matches, since the signal's name already says what happened.
fn signal_of(fact: &protocol::Fact) -> Option<(&'static str, Value)> {
    let kind = fact.kind.as_str();
    Some(match kind {
        "channel.channel_points_custom_reward_redemption.add" => {
            ("redeemed", Value::Text(fact.reward.clone()))
        }
        "channel.chat.message" => (
            "chatted",
            Value::Text(fact.message.chars().take(64).collect()),
        ),
        "channel.subscribe" | "channel.subscription.message" => {
            ("subscribed", Value::Number(fact.tier))
        }
        "channel.subscription.gift" => ("gifted", Value::Number(fact.gifts)),
        "channel.cheer" => ("cheered", Value::Number(fact.bits)),
        "channel.raid" => ("raided", Value::Number(fact.viewers)),
        "channel.follow" => ("followed", Value::None),
        "channel.hype_train.begin" | "channel.hype_train.progress" | "channel.hype_train.end" => {
            ("hype", Value::Number(fact.level))
        }
        "stream.online" => ("went_live", Value::None),
        "stream.offline" => ("went_offline", Value::None),
        "channel.poll.begin" | "channel.poll.progress" => {
            ("polled", Value::Text(fact.title.clone()))
        }
        "channel.poll.end" => ("poll_ended", Value::Text(fact.winner.clone())),
        _ => return None,
    })
}

impl Watched {
    /// The groups these references need: a signal names its own group, and a
    /// field the groups that write it.
    fn groups(&self) -> Vec<Group> {
        let mut out: Vec<Group> = Vec::new();
        let want = |g: Group, out: &mut Vec<Group>| {
            if !out.contains(&g) {
                out.push(g);
            }
        };
        for signal in &self.signals {
            if let Some(g) = GROUPS
                .into_iter()
                .find(|g| g.signals().contains(&signal.as_str()))
            {
                want(g, &mut out);
            }
        }
        for field in &self.fields {
            match field.as_str() {
                // Written by every event.
                "event" | "user_name" => {
                    for g in GROUPS {
                        want(g, &mut out);
                    }
                }
                "message" => {
                    want(Group::Chat, &mut out);
                    want(Group::Cheer, &mut out);
                }
                "reward" | "input" => want(Group::Redeems, &mut out),
                "bits" => {
                    want(Group::Cheer, &mut out);
                    want(Group::Hype, &mut out);
                }
                "tier" | "months" => want(Group::Subs, &mut out),
                "viewers" => want(Group::Raid, &mut out),
                "hype_level" | "hype_progress" | "hype_goal" => want(Group::Hype, &mut out),
                "live" => want(Group::Live, &mut out),
                "poll_title" | "poll_status" | "poll_winner" | "poll_votes" => {
                    want(Group::Poll, &mut out)
                }
                // `connected`, `user` and `error` are its own state: no event.
                _ => {}
            }
        }
        out.sort_by_key(|g| GROUPS.iter().position(|x| x == g));
        out
    }
}

/// What the module is doing, as the page reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Phase {
    /// No token held.
    Off,
    /// A token is held and the socket is being opened.
    Connecting,
    /// The socket is up and its session subscribed.
    Live { session: String },
}

/// A request that is out, and what its answer is for.
#[derive(Debug, Clone, PartialEq)]
enum Pending {
    Validate,
    Refresh,
    /// Asking Twitch for a device code.
    Device,
    /// Asking whether the person has approved it yet.
    DeviceToken,
    Subscribe {
        group: Group,
    },
    Chat,
    Clip,
    Marker,
    Announce,
    Title,
    Commercial,
    Poll,
}

/// An action's request, kept so a renewal can repeat it once: a 401 means
/// Twitch did not do the work, so repeating it is not doing it twice.
#[derive(Debug, Clone, PartialEq)]
struct Retry {
    method: String,
    url: String,
    content: Option<(&'static str, String)>,
    pending: Pending,
}

#[derive(Default)]
pub struct Twitch {
    phase: Option<Phase>,
    identity: Option<Identity>,
    /// What the client's rules and scripts read of it: the whole of what it
    /// listens for.
    watched: Watched,
    /// The groups this session subscribed; it only ever grows, since Twitch
    /// retires a session's subscriptions when its socket closes.
    subscribed: Vec<Group>,
    last: Option<protocol::Fact>,
    error: Option<String>,
    pending: HashMap<u64, Pending>,
    next_id: u64,
    ws: Option<u64>,
    /// The socket a reconnect_url opened, closed once the new session is up.
    old_ws: Option<u64>,
    keepalive_ms: u64,
    /// The sign-in that is waiting on the person: Twitch's code, the address
    /// to approve at, and how often to ask.
    device: Option<protocol::DeviceCode>,
    /// Groups the token's scopes can't subscribe, and the scope each needs.
    missing: Vec<(Group, &'static str)>,
    /// The action request that is out, if one is.
    action: Option<Retry>,
    /// The action a renewal interrupted, to repeat once the new token is in.
    retry: Option<Retry>,
    /// The action out is that repeat: a second refusal is reported, not
    /// renewed again.
    retrying: bool,
}

impl Twitch {
    fn phase(&self) -> &Phase {
        self.phase.as_ref().unwrap_or(&Phase::Off)
    }

    /// The client the held token was issued to: every Helix call names it.
    fn client_id(&self) -> Option<&str> {
        self.identity.as_ref().map(|i| i.client_id.as_str())
    }

    /// Every scope this build may ever ask for, so a rule added later does not
    /// send the person through Twitch's consent again.
    fn scopes() -> String {
        let mut set: Vec<&'static str> = GROUPS.iter().filter_map(|g| g.scope()).collect();
        for scope in [
            SCOPE_CHAT,
            SCOPE_CLIP,
            SCOPE_BROADCAST,
            SCOPE_ANNOUNCE,
            SCOPE_COMMERCIAL,
            SCOPE_POLLS,
        ] {
            if !set.contains(&scope) {
                set.push(scope);
            }
        }
        set.join(" ")
    }

    /// Starts a device authorization: Twitch answers with a code this build
    /// polls and an address the person opens, code included. No redirect, no
    /// secret, nothing typed.
    fn start_device(&mut self, out: &mut Vec<Command>) {
        if self.device.is_some() {
            return;
        }
        self.next_id += 1;
        let id = self.next_id;
        out.push(Command::Http {
            id,
            method: "POST".into(),
            url: protocol::DEVICE_URL.into(),
            headers: vec![(
                "Content-Type".to_owned(),
                Header::Text("application/x-www-form-urlencoded".to_owned()),
            )],
            body: Some(Data::Text(protocol::device_body(
                CLIENT_ID,
                &Self::scopes(),
            ))),
        });
        self.pending.insert(id, Pending::Device);
        self.error = None;
    }

    /// Asks whether the person has approved the waiting code yet.
    fn poll_device(&mut self, out: &mut Vec<Command>) {
        let Some(code) = self.device.as_ref().map(|d| d.code.clone()) else {
            return;
        };
        self.next_id += 1;
        let id = self.next_id;
        out.push(Command::Http {
            id,
            method: "POST".into(),
            url: protocol::TOKEN_URL.into(),
            headers: vec![(
                "Content-Type".to_owned(),
                Header::Text("application/x-www-form-urlencoded".to_owned()),
            )],
            body: Some(Data::Text(protocol::device_token_body(
                CLIENT_ID,
                &Self::scopes(),
                &code,
            ))),
        });
        self.pending.insert(id, Pending::DeviceToken);
    }

    /// The sign-in was cancelled or expired: stop asking.
    fn cancel_device(&mut self, out: &mut Vec<Command>) {
        self.device = None;
        out.push(Command::CancelTimer {
            id: "device-poll".into(),
        });
        out.push(Command::CancelTimer {
            id: "device-expire".into(),
        });
    }

    /// Asks `method url` with the token's client id, the bearer when `auth`,
    /// and `body` in `content`.
    fn request(
        &mut self,
        out: &mut Vec<Command>,
        method: &str,
        url: &str,
        auth: bool,
        content: Option<(&'static str, String)>,
        pending: Pending,
    ) {
        self.next_id += 1;
        let id = self.next_id;
        let mut headers = Vec::new();
        if let Some(client) = self.client_id() {
            headers.push(("Client-Id".to_owned(), Header::Text(client.to_owned())));
        }
        if auth {
            headers.push((
                "Authorization".to_owned(),
                Header::Secret(SECRET_AUTH.to_owned()),
            ));
        }
        let body = content.clone().map(|(kind, body)| {
            headers.push(("Content-Type".to_owned(), Header::Text(kind.to_owned())));
            Data::Text(body)
        });
        if is_action(&pending) {
            self.action = Some(Retry {
                method: method.to_owned(),
                url: url.to_owned(),
                content: content.clone(),
                pending: pending.clone(),
            });
        }
        out.push(Command::Http {
            id,
            method: method.to_owned(),
            url: url.to_owned(),
            headers,
            body,
        });
        self.pending.insert(id, pending);
    }

    /// Asks what the held token is: the client it was issued to, the account,
    /// its scopes and its life. The one call that needs no client id.
    fn validate(&mut self, out: &mut Vec<Command>) {
        self.next_id += 1;
        let id = self.next_id;
        out.push(Command::Http {
            id,
            method: "GET".into(),
            url: protocol::VALIDATE_URL.into(),
            headers: vec![(
                "Authorization".to_owned(),
                Header::Secret(SECRET_AUTH.to_owned()),
            )],
            body: None,
        });
        self.pending.insert(id, Pending::Validate);
    }

    /// Opens the EventSub socket.
    fn connect(&mut self, out: &mut Vec<Command>) {
        self.next_id += 1;
        let id = self.next_id;
        self.ws = Some(id);
        self.phase = Some(Phase::Connecting);
        out.push(Command::Open {
            id,
            to: Endpoint::Ws {
                url: protocol::WS_URL.to_owned(),
            },
        });
    }

    /// Reads the token the last answer carried: keeps it, and gets the account
    /// it belongs to.
    fn accepted(&mut self, out: &mut Vec<Command>, token: Token) {
        self.store_token(out, &token);
        // A renewal answers with the token's own scopes: the held identity
        // follows them, so a group that came into scope starts listening.
        if let Some(identity) = self.identity.as_mut() {
            identity.scopes = token.scopes.clone();
        }
        out.push(Command::Timer {
            id: "refresh".into(),
            after_ms: token.expires_ms.saturating_sub(REFRESH_LEAD_MS) as u32,
        });
        // The action the renewal interrupted goes again, once, with the new
        // token: Twitch refused it before doing anything.
        if let Some(retry) = self.retry.take() {
            self.retrying = true;
            self.request(
                out,
                &retry.method,
                &retry.url,
                true,
                retry.content,
                retry.pending,
            );
        }
        // A renewal while live changes nothing the socket is holding: its
        // subscriptions were made with the token that is still valid.
        if matches!(self.phase(), Phase::Live { .. }) || self.identity.is_none() {
            return;
        }
        self.connect(out);
    }

    fn store_token(&self, out: &mut Vec<Command>, token: &Token) {
        out.push(Command::StoreSecret {
            slot: SECRET_AUTH.into(),
            value: format!("Bearer {}", token.access),
        });
        if let Some(refresh) = &token.refresh {
            out.push(Command::StoreSecret {
                slot: SECRET_REFRESH.into(),
                value: refresh.clone(),
            });
        }
    }

    /// Subscribes the groups the watched set needs and this session doesn't
    /// have. Twitch retires a session's subscriptions when its socket closes,
    /// so nothing is ever deleted here; the set only grows while connected.
    fn subscribe(&mut self, out: &mut Vec<Command>) {
        let Some(identity) = self.identity.clone() else {
            return;
        };
        let want: Vec<Group> = self
            .watched
            .groups()
            .into_iter()
            .filter(|g| !self.subscribed.contains(g))
            .collect();
        self.missing.clear();
        for group in want {
            if let Some(scope) = group.scope() {
                if !identity.scopes.iter().any(|s| s == scope) {
                    self.missing.push((group, scope));
                    continue;
                }
            }
            self.subscribed.push(group);
            for (kind, version, condition) in group.subscriptions(&identity.id) {
                let session = self.session();
                let body = serde_json::json!({
                    "type": kind,
                    "version": version.to_string(),
                    "condition": condition,
                    "transport": { "method": "websocket", "session_id": session }
                })
                .to_string();
                self.request(
                    out,
                    "POST",
                    protocol::SUBSCRIPTIONS_URL,
                    true,
                    Some(("application/json", body)),
                    Pending::Subscribe { group },
                );
            }
        }
    }

    fn session(&self) -> String {
        match self.phase() {
            Phase::Live { session } => session.clone(),
            _ => String::new(),
        }
    }

    /// Publishes what the module is, as fields and the page.
    fn publish(&self) -> Vec<Command> {
        let fact = self.last.clone().unwrap_or_default();
        let live = matches!(self.phase(), Phase::Live { .. });
        let mut fields = vec![
            ("connected", Value::Bool(live)),
            ("live", Value::Bool(fact.live)),
            (
                "user",
                Value::Text(
                    self.identity
                        .as_ref()
                        .map(|i| i.login.clone())
                        .unwrap_or_default(),
                ),
            ),
            ("event", Value::Text(fact.kind.clone())),
            ("user_name", Value::Text(fact.user.clone())),
            ("message", Value::Text(fact.message.clone())),
            ("reward", Value::Text(fact.reward.clone())),
            ("input", Value::Text(fact.input.clone())),
            ("poll_title", Value::Text(fact.title.clone())),
            ("poll_status", Value::Text(fact.status.clone())),
            ("poll_winner", Value::Text(fact.winner.clone())),
            ("poll_votes", Value::Number(fact.votes)),
            ("error", Value::Text(self.error.clone().unwrap_or_default())),
        ];
        for (name, value) in [
            ("bits", fact.bits),
            ("tier", fact.tier),
            ("months", fact.months),
            ("viewers", fact.viewers),
            ("hype_level", fact.level),
            ("hype_progress", fact.progress),
            ("hype_goal", fact.goal),
        ] {
            fields.push((name, Value::Number(value)));
        }
        let mut out: Vec<Command> = fields
            .into_iter()
            .map(|(name, value)| Command::Field {
                name: name.to_owned(),
                value,
            })
            .collect();
        let groups = self.watched.groups();
        let status = Status {
            phase: self.phase(),
            logged_in: self.identity.is_some(),
            who: self.identity.as_ref().map_or("", |i| i.login.as_str()),
            device: self.device.as_ref().map(|d| d.link.as_str()),
            groups: &groups,
            missing: &self.missing,
            last: self.last.as_ref(),
            error: self.error.as_deref(),
        };
        out.push(Command::Page {
            page: PAGE.into(),
            view: page::view(&status),
        });
        out
    }

    /// Answers an action that can't run now.
    fn refuse(&mut self, out: &mut Vec<Command>, id: u64, why: &str) {
        self.error = Some(why.to_owned());
        out.push(Command::Reply {
            id,
            value: Value::Bool(false),
        });
    }
}

/// Whether `pending` is an action's request: the ones a renewal may repeat.
fn is_action(pending: &Pending) -> bool {
    matches!(
        pending,
        Pending::Chat
            | Pending::Clip
            | Pending::Marker
            | Pending::Announce
            | Pending::Title
            | Pending::Commercial
            | Pending::Poll
    )
}

/// The trigger editor's label for one of its signals.
impl Extension for Twitch {
    fn manifest(&self) -> Manifest {
        crate::manifest::build()
    }

    fn handle(&mut self, event: Event) -> Vec<Command> {
        let mut out = Vec::new();
        match event {
            Event::Start => {
                // The identity first: a renewal needs the client id it holds,
                // and it is read before the token is checked.
                out.push(Command::Load {
                    key: K_IDENTITY.into(),
                });
            }
            Event::Loaded { key, value } => {
                if key == K_IDENTITY {
                    self.identity = value.and_then(|t| serde_json::from_str(&t).ok());
                    out.push(Command::LoadSecret {
                        slot: SECRET_AUTH.into(),
                    });
                }
                out.extend(self.publish());
            }
            Event::SecretLoaded { slot, value } if slot == SECRET_AUTH => match value {
                // A token is held: ask what it is, then open the socket.
                Some(_) => self.validate(&mut out),
                None => {
                    self.phase = Some(Phase::Off);
                    out.extend(self.publish());
                }
            },
            Event::SecretLoaded { slot, value } if slot == SECRET_REFRESH => {
                let (refresh, client) = match (value, self.client_id().map(str::to_owned)) {
                    (Some(refresh), Some(client)) => (refresh, client),
                    _ => {
                        self.expired(&mut out);
                        return out;
                    }
                };
                self.request(
                    &mut out,
                    "POST",
                    protocol::TOKEN_URL,
                    false,
                    Some((
                        "application/x-www-form-urlencoded",
                        protocol::refresh_body(&client, &refresh),
                    )),
                    Pending::Refresh,
                );
            }
            Event::Http { id, response } => {
                if let Some(pending) = self.pending.remove(&id) {
                    self.answered(&mut out, pending, response);
                }
            }
            Event::Opened { .. } => {}
            Event::Received { data, .. } => {
                let Data::Text(text) = data else {
                    return out;
                };
                match protocol::parse_frame(&text) {
                    Ok(frame) => out.extend(self.frame(frame)),
                    Err(e) => self.error = Some(format!("bad frame: {e}")),
                }
            }
            Event::Closed { id, error } if Some(id) == self.ws || Some(id) == self.old_ws => {
                self.ws = None;
                self.old_ws = None;
                if let Some(why) = error {
                    self.error = Some(why);
                }
                if !matches!(self.phase(), Phase::Off) {
                    self.phase = Some(Phase::Connecting);
                    out.push(Command::Timer {
                        id: "retry".into(),
                        after_ms: RETRY_MS,
                    });
                }
            }
            Event::Closed { .. } => {}
            Event::Timer { id, .. } => match id.as_str() {
                "device-poll" => self.poll_device(&mut out),
                "device-expire" => {
                    self.cancel_device(&mut out);
                    self.error = Some("The sign-in expired: connect again.".into());
                }
                "after-token" => self.validate(&mut out),
                "refresh" => self.refresh(&mut out),
                "retry" => self.connect(&mut out),
                "watchdog" => {
                    // Nothing for two keepalives: the socket only looks open.
                    if let Some(ws) = self.ws.take() {
                        out.push(Command::Close { id: ws });
                    }
                    self.phase = Some(Phase::Connecting);
                    out.push(Command::Timer {
                        id: "retry".into(),
                        after_ms: RETRY_MS,
                    });
                }
                _ => {}
            },
            Event::Ui { event, .. } => out.extend(self.ui(event)),
            Event::Call {
                id,
                ref method,
                args,
                ..
            } => out.extend(self.call(id, method, &args)),
            _ => {}
        }
        out
    }
}

impl Twitch {
    /// What an answer to `pending` means.
    fn answered(
        &mut self,
        out: &mut Vec<Command>,
        pending: Pending,
        response: Result<vrc_we_sdk::HttpResponse, String>,
    ) {
        let (status, body) = match response {
            Ok(r) => (
                r.status,
                match r.body {
                    Data::Text(t) => t,
                    Data::Bytes(b) => String::from_utf8_lossy(&b).into_owned(),
                },
            ),
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let parsed: Json = serde_json::from_str(&body).unwrap_or(Json::Null);
        match pending {
            Pending::Validate => match (status, protocol::parse_validate(&parsed)) {
                (200, Ok(identity)) => {
                    let life = identity.expires_ms.saturating_sub(REFRESH_LEAD_MS);
                    out.push(Command::Timer {
                        id: "refresh".into(),
                        after_ms: u32::try_from(life.max(1_000)).unwrap_or(u32::MAX),
                    });
                    out.push(Command::Store {
                        key: K_IDENTITY.into(),
                        value: serde_json::to_string(&identity).unwrap_or_default(),
                    });
                    self.identity = Some(identity);
                    self.error = None;
                    self.connect(out);
                }
                // The token is dead: renew it from what the identity and the
                // refresh slot hold, or ask for a new one.
                (401, _) => out.push(Command::LoadSecret {
                    slot: SECRET_REFRESH.into(),
                }),
                (_, Err(e)) => self.error = Some(e),
                _ => self.error = protocol::parse_error(&parsed),
            },
            Pending::Refresh => match (status, protocol::parse_token(&parsed)) {
                (200, Ok(token)) => {
                    self.phase = Some(Phase::Connecting);
                    self.error = None;
                    self.accepted(out, token);
                }
                // Twitch refused the renewal itself (`invalid_grant` once the
                // refresh token is spent, which a public client's is after 30
                // days): say what it said, and ask for a new token.
                _ => {
                    let why = protocol::parse_error(&parsed)
                        .unwrap_or_else(|| format!("the renewal was refused ({status})"));
                    self.expired_with(out, why);
                }
            },
            Pending::Device => match (status, protocol::parse_device(&parsed)) {
                (200, Ok(device)) => {
                    let (interval, expires) = (device.interval_ms, device.expires_ms);
                    self.device = Some(device);
                    self.error = None;
                    out.push(Command::Timer {
                        id: "device-poll".into(),
                        after_ms: interval,
                    });
                    out.push(Command::Timer {
                        id: "device-expire".into(),
                        after_ms: expires,
                    });
                }
                (_, Ok(_)) => {
                    self.error = Some(
                        protocol::parse_error(&parsed)
                            .unwrap_or_else(|| format!("Twitch refused the sign-in ({status})")),
                    );
                }
                (_, Err(e)) => self.error = Some(e),
            },
            Pending::DeviceToken => match status {
                200 => match protocol::parse_token(&parsed) {
                    Ok(token) => {
                        self.cancel_device(out);
                        self.store_token(out, &token);
                        // The token is in the keyring now; a beat later the
                        // check reads it and names the account.
                        self.phase = Some(Phase::Connecting);
                        self.identity = None;
                        self.error = None;
                        out.push(Command::Timer {
                            id: "after-token".into(),
                            after_ms: 500,
                        });
                    }
                    Err(e) => self.error = Some(e),
                },
                400 => {
                    let why = parsed
                        .get("message")
                        .and_then(Json::as_str)
                        .unwrap_or_default()
                        .to_lowercase();
                    if why.contains("authorization_pending") {
                        let wait = self.device.as_ref().map_or(5_000, |d| d.interval_ms);
                        out.push(Command::Timer {
                            id: "device-poll".into(),
                            after_ms: wait,
                        });
                    } else if why.contains("slow_down") {
                        let wait = self.device.as_mut().map_or(10_000, |d| {
                            d.interval_ms = d.interval_ms.saturating_add(5_000);
                            d.interval_ms
                        });
                        out.push(Command::Timer {
                            id: "device-poll".into(),
                            after_ms: wait,
                        });
                    } else {
                        let why = protocol::parse_error(&parsed)
                            .unwrap_or_else(|| "the sign-in expired".to_owned());
                        self.cancel_device(out);
                        self.error = Some(why);
                    }
                }
                _ => {
                    let why = protocol::parse_error(&parsed)
                        .unwrap_or_else(|| format!("the sign-in was refused ({status})"));
                    self.cancel_device(out);
                    self.error = Some(why);
                }
            },
            Pending::Subscribe { group } => {
                if status == 202 {
                    self.error = None;
                } else {
                    let why = protocol::parse_error(&parsed).unwrap_or_default();
                    self.error = Some(format!("{}: {why}", group.key()));
                }
            }
            Pending::Chat
            | Pending::Clip
            | Pending::Marker
            | Pending::Announce
            | Pending::Title
            | Pending::Commercial
            | Pending::Poll => self.sent(out, status, &parsed, pending),
        }
        out.extend(self.publish());
    }

    /// What an action's answer means: done, a renewal, or the scope it lacks.
    fn sent(&mut self, out: &mut Vec<Command>, status: u16, parsed: &Json, pending: Pending) {
        let (what, scope) = match pending {
            Pending::Chat => ("chat", SCOPE_CHAT),
            Pending::Clip => ("clip", SCOPE_CLIP),
            Pending::Marker => ("marker", SCOPE_BROADCAST),
            Pending::Announce => ("announcement", SCOPE_ANNOUNCE),
            Pending::Title => ("title", SCOPE_BROADCAST),
            Pending::Commercial => ("commercial", SCOPE_COMMERCIAL),
            Pending::Poll => ("poll", SCOPE_POLLS),
            _ => return,
        };
        match status {
            200 | 202 | 204 => {
                self.error = None;
                self.retry = None;
                self.retrying = false;
            }
            401 => {
                if self.retrying {
                    self.retrying = false;
                    self.retry = None;
                    self.error = Some(format!("{what} was refused after the token was renewed"));
                } else {
                    self.retry = self.action.clone();
                    out.push(Command::LoadSecret {
                        slot: SECRET_REFRESH.into(),
                    });
                }
            }
            403 => self.error = Some(format!("{what} needs the token's {scope} scope")),
            _ => {
                self.error = Some(
                    protocol::parse_error(parsed)
                        .unwrap_or_else(|| format!("{what} refused: {status}")),
                );
            }
        }
    }

    /// A token that can't be renewed: the page asks for a new one.
    fn expired(&mut self, out: &mut Vec<Command>) {
        self.expired_with(out, "Twitch refused the token: connect again.".into());
    }

    /// The same, with what Twitch said about it.
    fn expired_with(&mut self, out: &mut Vec<Command>, why: String) {
        if let Some(ws) = self.ws.take() {
            out.push(Command::Close { id: ws });
        }
        self.subscribed.clear();
        self.missing.clear();
        self.retry = None;
        self.retrying = false;
        self.identity = None;
        self.phase = Some(Phase::Off);
        self.error = Some(why);
        out.extend(self.publish());
    }

    fn refresh(&mut self, out: &mut Vec<Command>) {
        out.push(Command::LoadSecret {
            slot: SECRET_REFRESH.into(),
        });
    }

    fn ui(&mut self, event: UiEvent) -> Vec<Command> {
        let mut out = Vec::new();
        match event {
            UiEvent::Pressed { id } if id == "connect" => self.start_device(&mut out),
            UiEvent::Pressed { id } if id == "cancel" => self.cancel_device(&mut out),
            UiEvent::Pressed { id } if id == "forget" => out.extend(self.logout()),
            UiEvent::Pressed { id } if id == "reconnect" => {
                if let Some(ws) = self.ws.take() {
                    out.push(Command::Close { id: ws });
                }
                self.connect(&mut out);
            }
            _ => {}
        }
        out.extend(self.publish());
        out
    }

    fn logout(&mut self) -> Vec<Command> {
        let mut out = vec![
            Command::UnstoreSecret {
                slot: SECRET_AUTH.into(),
            },
            Command::UnstoreSecret {
                slot: SECRET_REFRESH.into(),
            },
            Command::Unstore {
                key: K_IDENTITY.into(),
            },
            Command::CancelTimer {
                id: "refresh".into(),
            },
        ];
        if let Some(ws) = self.ws.take() {
            out.push(Command::Close { id: ws });
        }
        self.identity = None;
        self.device = None;
        self.subscribed.clear();
        self.phase = Some(Phase::Off);
        self.error = None;
        out
    }

    fn call(&mut self, id: u64, method: &str, args: &[Value]) -> Vec<Command> {
        let mut out = Vec::new();
        let text = |i: usize| match args.get(i) {
            Some(Value::Text(t)) => Some(t.clone()),
            _ => None,
        };
        match method {
            "clip" => {
                let Some(identity) = self.identity.clone() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                let title = text(0).unwrap_or_default();
                if title.len() > protocol::TITLE_MAX {
                    self.refuse(&mut out, id, "A clip title is at most 140 bytes.");
                    return out;
                }
                self.request(
                    &mut out,
                    "POST",
                    &protocol::clip_url(&identity.id, &title),
                    true,
                    None,
                    Pending::Clip,
                );
                out.push(Command::Reply {
                    id,
                    value: Value::Bool(true),
                });
            }
            "marker" => {
                let Some(identity) = self.identity.clone() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                let description = text(0).unwrap_or_default();
                if description.len() > protocol::TITLE_MAX {
                    self.refuse(&mut out, id, "A marker's description is at most 140 bytes.");
                    return out;
                }
                self.request(
                    &mut out,
                    "POST",
                    protocol::MARKER_URL,
                    true,
                    Some((
                        "application/json",
                        protocol::marker_body(&identity.id, &description),
                    )),
                    Pending::Marker,
                );
                out.push(Command::Reply {
                    id,
                    value: Value::Bool(true),
                });
            }
            "chat" => {
                let Some(message) = text(0) else {
                    self.refuse(&mut out, id, "A chat message needs text.");
                    return out;
                };
                let Some(identity) = self.identity.clone() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                if message.is_empty() || message.len() > protocol::CHAT_MAX {
                    self.refuse(&mut out, id, "A chat message is 1 to 500 bytes.");
                    return out;
                }
                self.request(
                    &mut out,
                    "POST",
                    protocol::CHAT_URL,
                    true,
                    Some((
                        "application/json",
                        protocol::chat_body(&identity.id, &identity.id, &message),
                    )),
                    Pending::Chat,
                );
                out.push(Command::Reply {
                    id,
                    value: Value::Bool(true),
                });
            }
            "announce" => {
                let Some(message) =
                    text(0).filter(|m| !m.is_empty() && m.len() <= protocol::CHAT_MAX)
                else {
                    self.refuse(&mut out, id, "An announcement is 1 to 500 bytes.");
                    return out;
                };
                let Some(identity) = self.identity.clone() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                self.request(
                    &mut out,
                    "POST",
                    protocol::ANNOUNCE_URL,
                    true,
                    Some((
                        "application/json",
                        protocol::announce_body(&identity.id, &identity.id, &message),
                    )),
                    Pending::Announce,
                );
                out.push(Command::Reply {
                    id,
                    value: Value::Bool(true),
                });
            }
            "title" => {
                let Some(title) =
                    text(0).filter(|t| !t.is_empty() && t.len() <= protocol::TITLE_MAX)
                else {
                    self.refuse(&mut out, id, "A title is 1 to 140 bytes.");
                    return out;
                };
                let Some(identity) = self.identity.clone() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                self.request(
                    &mut out,
                    "PATCH",
                    &protocol::channel_url(&identity.id),
                    true,
                    Some((
                        "application/json",
                        protocol::channel_body(Some(&title), None),
                    )),
                    Pending::Title,
                );
                out.push(Command::Reply {
                    id,
                    value: Value::Bool(true),
                });
            }
            "commercial" => {
                // Twitch runs a commercial for one of these lengths only.
                let seconds = match args.first() {
                    Some(Value::Number(n)) => *n,
                    _ => 60.0,
                };
                if !(30.0..=180.0).contains(&seconds) || (seconds % 30.0) != 0.0 {
                    self.refuse(
                        &mut out,
                        id,
                        "A commercial runs 30 to 180 seconds, in 30s steps.",
                    );
                    return out;
                }
                let Some(identity) = self.identity.clone() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                self.request(
                    &mut out,
                    "POST",
                    protocol::COMMERCIAL_URL,
                    true,
                    Some((
                        "application/json",
                        protocol::commercial_body(&identity.id, seconds as u64),
                    )),
                    Pending::Commercial,
                );
                out.push(Command::Reply {
                    id,
                    value: Value::Bool(true),
                });
            }
            "poll" => {
                let Some(title) =
                    text(0).filter(|t| !t.is_empty() && t.len() <= protocol::TITLE_MAX)
                else {
                    self.refuse(&mut out, id, "A poll needs a title of at most 140 bytes.");
                    return out;
                };
                // Two to five choices; the form's later ones may be left out.
                let choices: Vec<String> = (1..=5)
                    .filter_map(text)
                    .filter(|c| !c.is_empty() && c.len() <= protocol::TITLE_MAX)
                    .collect();
                if choices.len() < 2 {
                    self.refuse(&mut out, id, "A poll needs two choices.");
                    return out;
                }
                let seconds = match args.get(6) {
                    Some(Value::Number(n)) => {
                        n.clamp(POLL_MIN_SECONDS as f64, POLL_MAX_SECONDS as f64) as u64
                    }
                    _ => POLL_SECONDS,
                };
                let Some(identity) = self.identity.clone() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                self.request(
                    &mut out,
                    "POST",
                    protocol::POLLS_URL,
                    true,
                    Some((
                        "application/json",
                        protocol::poll_body(&identity.id, &title, &choices, seconds),
                    )),
                    Pending::Poll,
                );
                out.push(Command::Reply {
                    id,
                    value: Value::Bool(true),
                });
            }
            _ => self.refuse(&mut out, id, "No such method."),
        }
        out
    }

    /// One EventSub frame, as the work it implies.
    fn frame(&mut self, frame: Frame) -> Vec<Command> {
        let mut out = Vec::new();
        match frame {
            Frame::Welcome {
                session,
                keepalive_ms,
            } => {
                self.phase = Some(Phase::Live { session });
                self.keepalive_ms = keepalive_ms;
                if let Some(old) = self.old_ws.take() {
                    out.push(Command::Close { id: old });
                }
                self.subscribed.clear();
                self.subscribe(&mut out);
                out.push(Command::Timer {
                    id: "watchdog".into(),
                    after_ms: (keepalive_ms * 2) as u32,
                });
            }
            Frame::Keepalive => {
                out.push(Command::Timer {
                    id: "watchdog".into(),
                    after_ms: (self.keepalive_ms.max(10_000) * 2) as u32,
                });
            }
            Frame::Notification { kind, event } => {
                if let Some(fact) = protocol::decode(&kind, &event) {
                    if let Some((name, value)) = signal_of(&fact) {
                        out.push(Command::Signal {
                            name: name.into(),
                            value,
                        });
                    }
                    self.last = Some(fact);
                }
            }
            Frame::Reconnect { url } => {
                self.next_id += 1;
                let id = self.next_id;
                self.old_ws = self.ws.replace(id);
                out.push(Command::Open {
                    id,
                    to: Endpoint::Ws { url },
                });
            }
            Frame::Revocation { kind, status } => {
                self.error = Some(format!("{kind}: {status}"));
            }
            // A frame this build doesn't know needs nothing.
            Frame::Other(_) => {}
        }
        out.extend(self.publish());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vrc_we_sdk::Does;

    fn answer(status: u16, body: &str) -> Result<vrc_we_sdk::HttpResponse, String> {
        Ok(vrc_we_sdk::HttpResponse {
            status,
            headers: Vec::new(),
            body: Data::Text(body.into()),
        })
    }

    fn welcome(session: &str) -> Event {
        Event::Received {
            id: 9,
            data: Data::Text(format!(
                r#"{{"metadata":{{"message_type":"session_welcome"}},"payload":{{"session":{{"id":"{session}","keepalive_timeout_seconds":30}}}}}}"#
            )),
        }
    }

    #[test]
    fn its_manifest_is_one_the_client_accepts() {
        assert_eq!(Twitch::default().manifest().validate(), Ok(()));
    }

    #[test]
    fn a_mapping_can_only_send_the_three_things_it_is_for() {
        // Reacting is fields, the `fired` signal and the conditions; sending is
        // these two. Signing out and reconnecting are maintenance, so the page
        // holds them and nothing a trigger fires can reach them.
        let manifest = Twitch::default().manifest();
        let actions: Vec<String> = manifest
            .actions
            .iter()
            .map(|a| match &a.does {
                Does::Call(method) => method.clone(),
                Does::Set => "set".into(),
            })
            .collect();
        assert_eq!(
            actions,
            [
                "chat",
                "clip",
                "marker",
                "announce",
                "title",
                "commercial",
                "poll"
            ]
        );
        let methods: Vec<&str> = manifest
            .namespace
            .as_ref()
            .expect("a namespace")
            .methods
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(
            methods,
            [
                "chat",
                "clip",
                "marker",
                "announce",
                "title",
                "commercial",
                "poll"
            ],
            "each action calls its method"
        );
    }

    #[test]
    fn a_welcome_subscribes_what_is_enabled_and_arms_the_watchdog() {
        let mut t = Twitch {
            identity: Some(Identity {
                id: "1337".into(),
                login: "streamer".into(),
                client_id: "client".into(),
                scopes: vec![
                    "channel:read:redemptions".into(),
                    "channel:read:hype_train".into(),
                ],
                expires_ms: 3_600_000,
            }),
            watched: Watched {
                fields: Vec::new(),
                signals: vec!["redeemed".into(), "hype".into()],
            },
            ..Default::default()
        };
        let out = t.handle(welcome("s1"));
        let posts: Vec<&str> = out
            .iter()
            .filter_map(|c| match c {
                Command::Http { method, url, .. } if method == "POST" => Some(url.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            posts.len(),
            1 + 3,
            "a redemption, and the hype train's three"
        );
        assert!(posts.iter().all(|u| *u == protocol::SUBSCRIPTIONS_URL));
        assert!(
            out.iter().any(|c| matches!(c, Command::Timer { id, after_ms } if id == "watchdog" && *after_ms == 60_000)),
            "two keepalives of 30 s"
        );
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::Field { name, .. } if name == "connected")));
        assert_eq!(
            t.phase(),
            &Phase::Live {
                session: "s1".into()
            }
        );
    }

    #[test]
    fn a_redemption_reaches_the_fields_a_mapping_conditions_on() {
        let mut t = Twitch::default();
        let out = t.handle(Event::Received {
            id: 9,
            data: Data::Text(
                r#"{"metadata":{"message_type":"notification","subscription_type":"channel.channel_points_custom_reward_redemption.add"},"payload":{"event":{"user_name":"Cooler_User","user_input":"pogchamp","reward":{"title":"Dance","cost":100}}}}"#
                    .into(),
            ),
        });
        let field = |name: &str| {
            out.iter().find_map(|c| match c {
                Command::Field { name: n, value } if n == name => Some(value.clone()),
                _ => None,
            })
        };
        assert_eq!(field("reward"), Some(Value::Text("Dance".into())));
        assert_eq!(field("input"), Some(Value::Text("pogchamp".into())));
        assert_eq!(field("user_name"), Some(Value::Text("Cooler_User".into())));
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Signal { name, value }
                if name == "redeemed" && *value == Value::Text("Dance".into()))),
            "and its signal carries the reward, which is what a mapping matches on"
        );
    }

    #[test]
    fn a_number_group_signal_carries_its_number() {
        let mut t = Twitch::default();
        let out = t.handle(Event::Received {
            id: 9,
            data: Data::Text(
                r#"{"metadata":{"message_type":"notification","subscription_type":"channel.hype_train.progress"},"payload":{"event":{"level":3,"total":900,"progress":500,"goal":1000}}}"#
                    .into(),
            ),
        });
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Signal { name, value }
                if name == "hype" && *value == Value::Number(3.0))),
            "a mapping compares hype_level, hype_progress or hype_goal as numbers: {out:?}"
        );
        assert_eq!(
            t.manifest()
                .namespace
                .expect("a namespace")
                .signals
                .iter()
                .find(|s| s.name == "hype")
                .map(|s| s.payload.clone()),
            Some(Shape::Number(None)),
            "and the manifest says so"
        );
    }

    #[test]
    fn a_poll_end_carries_its_winner_and_votes() {
        let mut t = Twitch::default();
        let out = t.handle(Event::Received {
            id: 9,
            data: Data::Text(
                r#"{"metadata":{"message_type":"notification","subscription_type":"channel.poll.end"},"payload":{"event":{"title":"Shoes?","status":"completed","choices":[{"title":"Yeah","votes":120},{"title":"No","votes":140}]}}}"#
                    .into(),
            ),
        });
        let field = |name: &str| {
            out.iter().find_map(|c| match c {
                Command::Field { name: n, value } if n == name => Some(value.clone()),
                _ => None,
            })
        };
        assert_eq!(field("poll_title"), Some(Value::Text("Shoes?".into())));
        assert_eq!(field("poll_status"), Some(Value::Text("completed".into())));
        assert_eq!(field("poll_winner"), Some(Value::Text("No".into())));
        assert_eq!(field("poll_votes"), Some(Value::Number(260.0)));
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Signal { name, value }
                if name == "poll_ended" && *value == Value::Text("No".into()))),
            "the winner is what a mapping matches: {out:?}"
        );
    }

    #[test]
    fn a_gift_and_the_streams_own_state_have_their_own_signals() {
        let mut t = Twitch::default();
        let out = t.handle(Event::Received {
            id: 9,
            data: Data::Text(
                r#"{"metadata":{"message_type":"notification","subscription_type":"channel.subscription.gift"},"payload":{"event":{"user_name":"Giver","tier":2000,"total":5}}}"#
                    .into(),
            ),
        });
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Signal { name, value }
                if name == "gifted" && *value == Value::Number(5.0))),
            "five gifts, not five months: {out:?}"
        );
        let out = t.handle(Event::Received {
            id: 9,
            data: Data::Text(
                r#"{"metadata":{"message_type":"notification","subscription_type":"stream.online"},"payload":{"event":{"type":"live","started_at":"2020-07-15T17:16:03Z"}}}"#
                    .into(),
            ),
        });
        let live = out.iter().find_map(|c| match c {
            Command::Field { name, value } if name == "live" => Some(value.clone()),
            _ => None,
        });
        assert_eq!(live, Some(Value::Bool(true)));
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::Signal { name, .. } if name == "went_live")));
    }

    #[test]
    fn a_live_field_reading_subscribes_the_two_stream_events() {
        let mut t = Twitch {
            identity: Some(Identity {
                id: "1337".into(),
                login: "streamer".into(),
                client_id: "client".into(),
                scopes: Vec::new(),
                expires_ms: 3_600_000,
            }),
            watched: Watched {
                fields: vec!["live".into()],
                signals: Vec::new(),
            },
            ..Default::default()
        };
        let out = t.handle(welcome("s1"));
        let types: Vec<String> = out
            .iter()
            .filter_map(|c| match c {
                Command::Http {
                    method,
                    body: Some(Data::Text(b)),
                    ..
                } if method == "POST" => Some(b.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(types.len(), 2, "online and offline: {types:?}");
        assert!(types.iter().all(|b| b.contains("stream.")));
    }

    #[test]
    fn clip_and_marker_are_the_helix_calls_twitch_documents() {
        let mut t = Twitch {
            identity: Some(Identity {
                id: "1337".into(),
                login: "streamer".into(),
                client_id: "client".into(),
                scopes: Vec::new(),
                expires_ms: 3_600_000,
            }),
            ..Default::default()
        };
        let out = t.handle(Event::Call {
            id: 1,
            method: "clip".into(),
            args: vec![Value::Text("nice".into())],
            from: None,
        });
        let clip = out.iter().find_map(|c| match c {
            Command::Http {
                method, url, body, ..
            } if method == "POST" && url.contains("/clips") => Some((url.clone(), body.clone())),
            _ => None,
        });
        let (url, body) = clip.expect("a clip call");
        assert!(url.contains("broadcaster_id=1337"), "{url}");
        assert!(
            url.contains("title=nice"),
            "the title is a query parameter: {url}"
        );
        assert!(body.is_none());

        let out = t.handle(Event::Call {
            id: 2,
            method: "marker".into(),
            args: vec![Value::Text("redeem".into())],
            from: None,
        });
        let marker = out.iter().find_map(|c| match c {
            Command::Http {
                method,
                body: Some(Data::Text(b)),
                url,
                ..
            } if method == "POST" && url.ends_with("/streams/markers") => Some(b.clone()),
            _ => None,
        });
        let marker = marker.expect("a marker call");
        assert!(marker.contains("\"user_id\":\"1337\""), "{marker}");
        assert!(marker.contains("redeem"), "{marker}");
        // The scopes are the person's own token's; a refusal names the one
        // the action needed (`sent`), so nothing here asks for them.
    }

    #[test]
    fn a_chat_send_needs_a_login_and_a_message() {
        let mut t = Twitch {
            identity: Some(Identity {
                id: "1337".into(),
                login: "streamer".into(),
                client_id: "client".into(),
                scopes: Vec::new(),
                expires_ms: 3_600_000,
            }),
            ..Default::default()
        };
        let out = t.handle(Event::Call {
            id: 4,
            method: "chat".into(),
            args: vec![Value::Text("hello".into())],
            from: None,
        });
        assert!(out.iter().any(|c| matches!(
            c,
            Command::Reply {
                id: 4,
                value: Value::Bool(true)
            }
        )));
        assert!(out.iter().any(|c| matches!(c, Command::Http { method, url, body: Some(Data::Text(b)), .. }
            if method == "POST" && url == protocol::CHAT_URL && b.contains("\"sender_id\":\"1337\""))));

        let mut logged_out = Twitch::default();
        let out = logged_out.handle(Event::Call {
            id: 5,
            method: "chat".into(),
            args: vec![Value::Text("hello".into())],
            from: None,
        });
        assert!(out.iter().any(|c| matches!(
            c,
            Command::Reply {
                id: 5,
                value: Value::Bool(false)
            }
        )));
    }

    #[test]
    fn connecting_starts_a_device_sign_in_and_the_page_carries_its_link() {
        let mut t = Twitch::default();
        let out = t.handle(Event::Ui {
            page: PAGE.into(),
            event: UiEvent::Pressed {
                id: "connect".into(),
            },
        });
        let (id, body) = out
            .iter()
            .find_map(|c| match c {
                Command::Http { id, url, body, .. } if url == protocol::DEVICE_URL => Some((
                    *id,
                    match body {
                        Some(Data::Text(text)) => text.clone(),
                        _ => String::new(),
                    },
                )),
                _ => None,
            })
            .expect("a device request");
        assert!(
            body.contains("client_id=") && body.contains("scopes="),
            "the application and every scope it may need: {body}"
        );
        assert!(
            !out.iter().any(|c| matches!(c, Command::StoreSecret { .. })),
            "nothing is stored before the person approves"
        );

        // Twitch answers with the code to poll and the address to approve,
        // code already in it.
        let out = t.handle(Event::Http {
            id,
            response: answer(
                200,
                r#"{"device_code":"dc","expires_in":1800,"interval":5,"user_code":"ABCDEFGH","verification_uri":"https://www.twitch.tv/activate?public=true&device-code=ABCDEFGH"}"#,
            ),
        });
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Timer { id, after_ms }
                if id == "device-poll" && *after_ms == 5_000)),
            "the poll waits the interval it was given: {out:?}"
        );
        assert!(
            out.iter().any(|c| matches!(c, Command::Page { view, .. }
                if format!("{view:?}").contains("twitch.tv/activate"))),
            "the page offers the address, not a field"
        );
    }

    #[test]
    fn an_approved_sign_in_keeps_both_tokens_and_then_names_the_account() {
        let mut t = Twitch {
            device: Some(protocol::DeviceCode {
                code: "dc".into(),
                link: "https://www.twitch.tv/activate?public=true&device-code=ABCDEFGH".into(),
                interval_ms: 5_000,
                expires_ms: 1_800_000,
            }),
            ..Twitch::default()
        };
        t.pending.insert(1, Pending::DeviceToken);
        let out = t.handle(Event::Http {
            id: 1,
            response: answer(
                200,
                r#"{"access_token":"abc","refresh_token":"ref","expires_in":14400,"scope":["user:read:chat"],"token_type":"bearer"}"#,
            ),
        });
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::StoreSecret { slot, value }
                if slot == SECRET_AUTH && value == "Bearer abc")),
            "the access token is kept as both Helix and the check take it"
        );
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::StoreSecret { slot, value }
                if slot == SECRET_REFRESH && value == "ref")),
            "and its one-time refresh token, which Twitch rotates"
        );
        assert!(t.device.is_none(), "the waiting code is spent");
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Timer { id, .. } if id == "after-token")),
            "the check follows once the keyring holds it: {out:?}"
        );
    }

    #[test]
    fn a_check_arms_the_renewal_from_the_tokens_own_life() {
        let mut t = Twitch::default();
        let out = t.handle(Event::SecretLoaded {
            slot: SECRET_AUTH.into(),
            value: Some("Bearer x".into()),
        });
        let id = out
            .iter()
            .find_map(|c| match c {
                Command::Http { id, url, .. } if url == protocol::VALIDATE_URL => Some(*id),
                _ => None,
            })
            .expect("a check");
        let out = t.handle(Event::Http {
            id,
            response: answer(
                200,
                r#"{"client_id":"cid","login":"streamer","scopes":[],"user_id":"9","expires_in":3600}"#,
            ),
        });
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Timer { id, after_ms }
                if id == "refresh" && *after_ms == 3_540_000)),
            "an hour less the minute it renews ahead: {out:?}"
        );
    }

    #[test]
    fn a_refused_action_renews_the_token_and_goes_again_once() {
        let mut t = Twitch {
            identity: Some(Identity {
                id: "1337".into(),
                login: "streamer".into(),
                client_id: "cid".into(),
                scopes: vec![SCOPE_CHAT.into()],
                expires_ms: 3_600_000,
            }),
            ..Default::default()
        };
        let out = t.handle(Event::Call {
            id: 7,
            method: "chat".into(),
            args: vec![Value::Text("hi".into())],
            from: None,
        });
        let call = out
            .iter()
            .find_map(|c| match c {
                Command::Http { id, url, .. } if url == protocol::CHAT_URL => Some(*id),
                _ => None,
            })
            .expect("the chat request");
        let out = t.handle(Event::Http {
            id: call,
            response: answer(401, r#"{"message":"Invalid OAuth token"}"#),
        });
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::LoadSecret { slot } if slot == SECRET_REFRESH)),
            "an expiry is not an error: the renewal is read"
        );
        let out = t.handle(Event::SecretLoaded {
            slot: SECRET_REFRESH.into(),
            value: Some("r1".into()),
        });
        let renew = out
            .iter()
            .find_map(|c| match c {
                Command::Http { id, url, .. } if url == protocol::TOKEN_URL => Some(*id),
                _ => None,
            })
            .expect("a renewal");
        let out = t.handle(Event::Http {
            id: renew,
            response: answer(
                200,
                r#"{"access_token":"new","refresh_token":"r2","expires_in":3600,"scope":["user:write:chat"]}"#,
            ),
        });
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::StoreSecret { slot, value }
                if slot == SECRET_AUTH && value == "Bearer new")),
            "the new access token is kept: {out:?}"
        );
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::StoreSecret { slot, value }
                if slot == SECRET_REFRESH && value == "r2")),
            "and Twitch rotated the refresh token, so the new one replaces it"
        );
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Http { url, .. } if url == protocol::CHAT_URL)),
            "the refused action goes again, once, with the new token"
        );
        assert!(
            t.retrying,
            "and a second refusal will be reported, not renewed"
        );
    }

    #[test]
    fn a_spent_refresh_token_says_what_twitch_said() {
        let mut t = Twitch {
            identity: Some(Identity {
                id: "1337".into(),
                login: "streamer".into(),
                client_id: "cid".into(),
                scopes: Vec::new(),
                expires_ms: 3_600_000,
            }),
            ..Default::default()
        };
        let out = t.handle(Event::Timer {
            id: "refresh".into(),
            at_ms: 0,
        });
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::LoadSecret { slot } if slot == SECRET_REFRESH)));
        let out = t.handle(Event::SecretLoaded {
            slot: SECRET_REFRESH.into(),
            value: Some("spent".into()),
        });
        let renew = out
            .iter()
            .find_map(|c| match c {
                Command::Http { id, url, .. } if url == protocol::TOKEN_URL => Some(*id),
                _ => None,
            })
            .expect("a renewal");
        let out = t.handle(Event::Http {
            id: renew,
            response: answer(
                400,
                r#"{"error":"invalid_grant","message":"Invalid refresh token"}"#,
            ),
        });
        assert_eq!(
            t.error.as_deref(),
            Some("Invalid refresh token"),
            "Twitch's own message, not a parse failure"
        );
        assert_eq!(t.phase(), &Phase::Off);
        assert!(
            out.iter().any(|c| matches!(c, Command::Page { .. })),
            "the page asks for a new token"
        );
    }

    #[test]
    fn a_group_whose_scope_the_token_lacks_is_named_instead_of_subscribed() {
        let mut t = Twitch {
            identity: Some(Identity {
                id: "1337".into(),
                login: "streamer".into(),
                client_id: "client".into(),
                scopes: Vec::new(),
                expires_ms: 3_600_000,
            }),
            watched: Watched {
                fields: Vec::new(),
                signals: vec!["redeemed".into()],
            },
            ..Default::default()
        };
        let out = t.handle(welcome("s1"));
        assert!(
            !out.iter().any(|c| matches!(c, Command::Http { url, .. }
                if url == protocol::SUBSCRIPTIONS_URL)),
            "nothing is asked for that Twitch would refuse"
        );
        assert_eq!(
            t.missing,
            vec![(Group::Redeems, "channel:read:redemptions")]
        );
        let page = out.iter().find_map(|c| match c {
            Command::Page { view, .. } => Some(format!("{view:?}")),
            _ => None,
        });
        assert!(
            page.as_deref()
                .is_some_and(|p| p.contains("channel:read:redemptions")),
            "the page says which scope is missing: {page:?}"
        );
    }

    #[test]
    fn an_expired_token_asks_for_a_renewal_rather_than_showing_a_failure() {
        let mut t = Twitch::default();
        // The start path asks who the token belongs to; a 401 means renew.
        let out = t.handle(Event::SecretLoaded {
            slot: SECRET_AUTH.into(),
            value: Some("Bearer x".into()),
        });
        let id = out
            .iter()
            .find_map(|c| match c {
                Command::Http {
                    id, method, url, ..
                } if method == "GET" && url == protocol::VALIDATE_URL => Some(*id),
                _ => None,
            })
            .expect("a users call");
        let out = t.handle(Event::Http {
            id,
            response: answer(401, r#"{"message":"Invalid OAuth token"}"#),
        });
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::LoadSecret { slot } if slot == SECRET_REFRESH)),
            "the refresh token is read next, and no failure is shown: {out:?}"
        );
        assert!(
            t.error.is_none(),
            "an expiry is not something to report: {:?}",
            t.error
        );
    }
}
