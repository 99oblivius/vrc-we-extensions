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
//! the access token's life ends. The action that met the 401 is repeated once:
//! Twitch refused it before doing anything, so repeating it is not doing it
//! twice.
//!
//! Every event is one step, then one reconcile of what is wanted against what
//! exists, then one publish of what changed. A socket opens only while the
//! client's rules read something it can subscribe, since Twitch closes one
//! with no subscription after ten seconds. `connected` is true once the socket
//! is welcomed and everything wanted is active; anything short of that shows
//! as waiting, with what it is waiting for.
//!
//! Build: `cargo wasm -p twitch`.

mod manifest;
mod page;
mod protocol;

use std::collections::{BTreeMap, HashMap, VecDeque};

use vrc_we_sdk::{
    Command, Data, Endpoint, Event, Extension, Header, Manifest, Node, Shape, Status, UiEvent,
    Value,
};

use page::{Events, Snapshot};
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
/// The shortest and the longest wait before asking Twitch again.
const RETRY_MS: u32 = 5_000;
const RETRY_MAX_MS: u32 = 300_000;
/// How long a new socket has to be welcomed.
const WELCOME_MS: u32 = 30_000;
/// Creates out at once. The full surface is 16, and the host allows 16 requests
/// and sockets in all (`NET_OPEN_MAX`): this leaves room for two sockets and
/// the actions a mapping sends.
const SUB_WINDOW: usize = 4;
/// Notifications remembered, to count one delivered twice once.
const SEEN_MAX: usize = 64;
/// Chat messages it sent, remembered until their echo comes back.
const OWN_CHAT_MAX: usize = 16;
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

/// `text` cut to at most `max` bytes, on a character's edge.
fn at_most(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
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
            Value::Text(at_most(&fact.message, 64).to_owned()),
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
                    want(Group::Subs, &mut out);
                }
                "reward" | "input" => want(Group::Redeems, &mut out),
                "bits" => {
                    want(Group::Chat, &mut out);
                    want(Group::Cheer, &mut out);
                    want(Group::Hype, &mut out);
                }
                "tier" | "months" => want(Group::Subs, &mut out),
                "viewers" => want(Group::Raid, &mut out),
                "hype_level" | "hype_progress" | "hype_goal" => want(Group::Hype, &mut out),
                // `connected` holds a socket open, which needs one subscription,
                // and this is the group that needs no scope.
                "live" | "connected" => want(Group::Live, &mut out),
                "poll_title" | "poll_status" | "poll_winner" | "poll_votes" => {
                    want(Group::Poll, &mut out)
                }
                // `user` and `error` are its own state: no event.
                _ => {}
            }
        }
        out.sort_by_key(|g| GROUPS.iter().position(|x| x == g));
        out
    }
}

/// Whether the held token can be used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Login {
    /// No token is held, or none could be read: nothing is forgotten by it.
    #[default]
    Out,
    /// A token is being read or checked.
    Checking,
    /// Checked: `identity` names its account.
    In,
    /// Twitch refused it and its renewal.
    Expired,
}

/// One subscription this session wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Sub {
    Queued,
    /// Its create is out.
    Sent,
    /// Twitch answered 202, or that it already exists.
    Active,
    /// Twitch was busy or unreachable: it goes again after a wait.
    Retrying,
    /// Twitch said no, which holds until Reconnect or a new token.
    Refused(String),
}

/// A socket, and its session once Twitch welcomed it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Socket {
    id: u64,
    session: Option<String>,
}

/// An action a mapping can send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Chat,
    Clip,
    Marker,
    Announce,
    Title,
    Commercial,
    Poll,
}

impl Action {
    /// What a refusal calls it, and the scope it needs.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Chat => ("chat", SCOPE_CHAT),
            Self::Clip => ("clip", SCOPE_CLIP),
            Self::Marker => ("marker", SCOPE_BROADCAST),
            Self::Announce => ("announcement", SCOPE_ANNOUNCE),
            Self::Title => ("title", SCOPE_BROADCAST),
            Self::Commercial => ("commercial", SCOPE_COMMERCIAL),
            Self::Poll => ("poll", SCOPE_POLLS),
        }
    }
}

/// A chat message it sent: its text until Twitch answers, then the id
/// Twitch gave it. Its echo in chat is dropped, so a reply never hears
/// itself.
#[derive(Debug, Clone, PartialEq)]
struct OwnChat {
    text: String,
    id: Option<String>,
}

/// A request as asked, kept so a renewal can repeat it once: a 401 means
/// Twitch did not do the work, so repeating it is not doing it twice.
#[derive(Debug, Clone, PartialEq)]
struct Request {
    method: String,
    url: String,
    content: Option<(&'static str, String)>,
}

/// A request that is out, and what its answer is for. Dropping one is how an
/// answer the host can't cancel is ignored: it finds nothing here.
#[derive(Debug, Clone, PartialEq)]
enum Pending {
    Validate,
    Refresh,
    /// Asking Twitch for a device code.
    Device,
    /// Asking whether the person has approved it yet.
    DeviceToken,
    Subscribe {
        kind: String,
    },
    /// Whether the stream is live right now.
    Stream,
    Action {
        action: Action,
        request: Request,
        retried: bool,
    },
}

#[derive(Default)]
pub struct Twitch {
    login: Login,
    /// The account the token was last checked to be: it outlives a keyring
    /// that can't be read, and only Forget removes it.
    identity: Option<Identity>,
    /// What the client's rules and scripts read of it: the whole of what it
    /// listens for.
    watched: Watched,
    /// The subscriptions what is watched needs and the token's scopes allow.
    wanted: Vec<(Group, String)>,
    /// Groups the token's scopes can't subscribe, and the scope each needs.
    missing: Vec<(Group, &'static str)>,
    /// This session's subscriptions by type. It only ever grows while the
    /// socket lives, since Twitch retires a session's subscriptions with it
    /// and a wear changes what is watched often.
    subs: BTreeMap<String, Sub>,
    /// A 401 on a create already renewed the token: another is a refusal.
    subs_renewed: bool,
    /// How long the next wait for a create is.
    sub_backoff_ms: u32,
    ws: Option<Socket>,
    /// The socket a requested reconnect opened, until its welcome.
    next_ws: Option<u64>,
    keepalive_ms: u64,
    /// How long the next wait before a socket opens is; none is the shortest.
    backoff_ms: u32,
    retry_armed: bool,
    /// The renewal is being read or is out.
    refreshing: bool,
    /// Requests a renewal interrupted, each repeated once when it lands.
    waiting: Vec<(Action, Request)>,
    /// A device code has been asked for and Twitch hasn't answered.
    device_asked: bool,
    /// The sign-in that is waiting on the person: Twitch's code, the address
    /// to approve at, and how often to ask.
    device: Option<protocol::DeviceCode>,
    /// The stream's own state, kept across events of other kinds.
    stream_live: bool,
    /// This session has read the stream's state already.
    stream_asked: bool,
    /// The last notifications' ids: Twitch delivers at least once, and both
    /// sockets may carry one during a reconnect.
    seen: VecDeque<String>,
    own_chat: VecDeque<OwnChat>,
    last: Option<protocol::Fact>,
    error: Option<String>,
    pending: HashMap<u64, Pending>,
    next_id: u64,
    /// What was last published, so an event that changes nothing costs nothing.
    published: HashMap<&'static str, Value>,
    shown_page: Option<Node>,
    shown_status: Option<Status>,
}

impl Twitch {
    /// The client the held token was issued to: every Helix call names it.
    fn client_id(&self) -> Option<&str> {
        self.identity.as_ref().map(|i| i.client_id.as_str())
    }

    /// The account an action speaks for, while the token may still be good.
    fn account(&self) -> Option<Identity> {
        self.identity
            .clone()
            .filter(|_| matches!(self.login, Login::In | Login::Checking))
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
        if self.device.is_some() || self.device_asked {
            return;
        }
        self.device_asked = true;
        self.error = None;
        self.form(
            out,
            protocol::DEVICE_URL,
            protocol::device_body(CLIENT_ID, &Self::scopes()),
            Pending::Device,
        );
    }

    /// Asks whether the person has approved the waiting code yet.
    fn poll_device(&mut self, out: &mut Vec<Command>) {
        let Some(code) = self.device.as_ref().map(|d| d.code.clone()) else {
            return;
        };
        self.form(
            out,
            protocol::TOKEN_URL,
            protocol::device_token_body(CLIENT_ID, &Self::scopes(), &code),
            Pending::DeviceToken,
        );
    }

    /// A form-encoded POST that carries no token.
    fn form(&mut self, out: &mut Vec<Command>, url: &str, body: String, pending: Pending) {
        self.next_id += 1;
        let id = self.next_id;
        out.push(Command::Http {
            id,
            method: "POST".into(),
            url: url.into(),
            headers: vec![(
                "Content-Type".to_owned(),
                Header::Text("application/x-www-form-urlencoded".to_owned()),
            )],
            body: Some(Data::Text(body)),
        });
        self.pending.insert(id, pending);
    }

    /// The sign-in was cancelled or expired: stop asking, and what is out
    /// finds nothing when it answers.
    fn cancel_device(&mut self, out: &mut Vec<Command>) {
        self.device = None;
        self.device_asked = false;
        self.pending
            .retain(|_, p| !matches!(p, Pending::Device | Pending::DeviceToken));
        cancel(out, &["device-poll", "device-expire"]);
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
        let body = content.map(|(kind, body)| {
            headers.push(("Content-Type".to_owned(), Header::Text(kind.to_owned())));
            Data::Text(body)
        });
        out.push(Command::Http {
            id,
            method: method.to_owned(),
            url: url.to_owned(),
            headers,
            body,
        });
        self.pending.insert(id, pending);
    }

    /// Sends an action, keeping what a renewal needs to repeat it.
    fn act(
        &mut self,
        out: &mut Vec<Command>,
        action: Action,
        method: &str,
        url: &str,
        content: Option<(&'static str, String)>,
    ) {
        self.send_action(
            out,
            action,
            Request {
                method: method.to_owned(),
                url: url.to_owned(),
                content,
            },
            false,
        );
    }

    fn send_action(
        &mut self,
        out: &mut Vec<Command>,
        action: Action,
        request: Request,
        retried: bool,
    ) {
        self.request(
            out,
            &request.method.clone(),
            &request.url.clone(),
            true,
            request.content.clone(),
            Pending::Action {
                action,
                request,
                retried,
            },
        );
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

    /// Opens the EventSub socket. A welcome that never comes counts as the
    /// socket lost.
    fn connect(&mut self, out: &mut Vec<Command>) {
        self.next_id += 1;
        let id = self.next_id;
        self.ws = Some(Socket { id, session: None });
        out.push(Command::Open {
            id,
            to: Endpoint::Ws {
                url: protocol::WS_URL.to_owned(),
            },
        });
        out.push(Command::Timer {
            id: "watchdog".into(),
            after_ms: WELCOME_MS,
        });
    }

    /// The socket is gone without a reconnect to replace it. What it held is
    /// gone with it, what Twitch refused stays refused, and another opens
    /// after a wait that grows until one holds.
    fn lost_socket(&mut self, out: &mut Vec<Command>) {
        self.forget_session(out);
        let wait = self.backoff_ms.max(RETRY_MS);
        self.backoff_ms = wait.saturating_mul(2).min(RETRY_MAX_MS);
        self.retry_armed = true;
        out.push(Command::Timer {
            id: "retry".into(),
            after_ms: wait,
        });
    }

    fn forget_session(&mut self, out: &mut Vec<Command>) {
        self.subs.retain(|_, s| matches!(s, Sub::Refused(_)));
        self.pending
            .retain(|_, p| !matches!(p, Pending::Subscribe { .. } | Pending::Stream));
        self.stream_asked = false;
        cancel(out, &["watchdog", "sub-retry"]);
    }

    /// Closes every socket and forgets everything they held.
    fn drop_link(&mut self, out: &mut Vec<Command>) {
        for id in self
            .ws
            .take()
            .map(|w| w.id)
            .into_iter()
            .chain(self.next_ws.take())
        {
            out.push(Command::Close { id });
        }
        self.forget_session(out);
        self.subs.clear();
        self.subs_renewed = false;
        self.backoff_ms = 0;
        self.sub_backoff_ms = 0;
        self.retry_armed = false;
        cancel(out, &["retry"]);
    }

    /// Brings what exists in line with what is wanted, after every event: the
    /// socket opens when something can be subscribed, never otherwise, since
    /// Twitch closes one with no subscription after ten seconds.
    fn reconcile(&mut self, out: &mut Vec<Command>) {
        self.wanted.clear();
        self.missing.clear();
        if self.login != Login::In {
            return;
        }
        let Some(identity) = self.identity.clone() else {
            return;
        };
        let groups = self.watched.groups();
        let mut creates = Vec::new();
        for &group in &groups {
            if let Some(scope) = group
                .scope()
                .filter(|scope| !identity.scopes.iter().any(|held| held == scope))
            {
                self.missing.push((group, scope));
                continue;
            }
            for (kind, version, condition) in group.subscriptions(&identity.id) {
                self.wanted.push((group, kind.to_owned()));
                self.subs.entry(kind.to_owned()).or_insert(Sub::Queued);
                creates.push((kind, version, condition));
            }
        }
        let usable = self
            .wanted
            .iter()
            .any(|(_, kind)| !matches!(self.subs[kind], Sub::Refused(_)));
        if usable && self.ws.is_none() && self.next_ws.is_none() && !self.retry_armed {
            self.connect(out);
        }
        let Some(session) = self.ws.as_ref().and_then(|w| w.session.clone()) else {
            return;
        };
        if groups.contains(&Group::Live) && !self.stream_asked {
            self.stream_asked = true;
            let url = protocol::streams_url(&identity.id);
            self.request(out, "GET", &url, true, None, Pending::Stream);
        }
        // A create sent now would carry the token being replaced.
        if self.refreshing {
            return;
        }
        let sent = self.subs.values().filter(|s| **s == Sub::Sent).count();
        let mut room = SUB_WINDOW.saturating_sub(sent);
        for (kind, version, condition) in creates {
            if room == 0 {
                break;
            }
            if self.subs[kind] != Sub::Queued {
                continue;
            }
            room -= 1;
            self.subs.insert(kind.to_owned(), Sub::Sent);
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
                Pending::Subscribe {
                    kind: kind.to_owned(),
                },
            );
        }
    }

    /// What the socket and its subscriptions come to.
    fn events(&self) -> Events {
        let active = |kind: &String| self.subs.get(kind) == Some(&Sub::Active);
        let nothing_wanted = self.wanted.is_empty() && self.missing.is_empty();
        if self.ws.as_ref().is_some_and(|w| w.session.is_some()) {
            if !self.subs.values().any(|s| *s == Sub::Active) && nothing_wanted {
                return Events::Idle;
            }
            let all = self.missing.is_empty() && self.wanted.iter().all(|(_, k)| active(k));
            return match all {
                true => Events::Connected,
                false => Events::Waiting,
            };
        }
        if nothing_wanted {
            return Events::Idle;
        }
        let usable = self
            .wanted
            .iter()
            .any(|(_, kind)| !matches!(self.subs.get(kind), Some(Sub::Refused(_))));
        match usable {
            true => Events::Connecting,
            false => Events::Waiting,
        }
    }

    /// Publishes what changed since the last publish, of what the module is.
    fn publish(&mut self, events: Events, out: &mut Vec<Command>) {
        let fact = self.last.clone().unwrap_or_default();
        let mut fields = vec![
            (
                "connected",
                Value::Bool(self.login == Login::In && events == Events::Connected),
            ),
            ("live", Value::Bool(self.stream_live)),
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
        for (name, value) in fields {
            if self.published.get(name) != Some(&value) {
                self.published.insert(name, value.clone());
                out.push(Command::Field {
                    name: name.to_owned(),
                    value,
                });
            }
        }
        let snapshot = self.snapshot(events);
        let view = page::view(&snapshot);
        let status = page::bar(&snapshot);
        if self.shown_page.as_ref() != Some(&view) {
            self.shown_page = Some(view.clone());
            out.push(Command::Page {
                page: PAGE.into(),
                view,
            });
        }
        if self.shown_status.as_ref() != Some(&status) {
            self.shown_status = Some(status.clone());
            out.push(Command::Status(Some(status)));
        }
    }

    /// What the page and the status bar read.
    fn snapshot(&self, events: Events) -> Snapshot<'_> {
        let state = |kind: &String| self.subs.get(kind);
        let mut listening: Vec<Group> = Vec::new();
        let mut refused: Vec<(Group, &str)> = Vec::new();
        for (group, _) in &self.wanted {
            let kinds = || self.wanted.iter().filter(|(g, _)| g == group);
            if !listening.contains(group)
                && kinds().all(|(_, kind)| state(kind) == Some(&Sub::Active))
            {
                listening.push(*group);
            }
            if let Some(why) = kinds().find_map(|(_, kind)| match state(kind) {
                Some(Sub::Refused(why)) => Some(why.as_str()),
                _ => None,
            }) {
                if !refused.iter().any(|(g, _)| g == group) {
                    refused.push((*group, why));
                }
            }
        }
        let count = |wanted: fn(&Sub) -> bool| {
            self.wanted
                .iter()
                .filter(|(_, kind)| state(kind).is_some_and(wanted))
                .count()
        };
        Snapshot {
            login: self.login,
            events,
            who: self.identity.as_ref().map(|i| i.login.as_str()),
            device: self.device.as_ref().map(|d| d.link.as_str()),
            device_asked: self.device_asked,
            listening,
            refused,
            active: count(|s| *s == Sub::Active),
            total: self.wanted.len(),
            busy: count(|s| *s == Sub::Retrying) > 0,
            retry_armed: self.retry_armed,
            missing: &self.missing,
            last: self.last.as_ref(),
            error: self.error.as_deref(),
        }
    }

    /// Answers an action that can't run now.
    fn refuse(&mut self, out: &mut Vec<Command>, id: u64, why: &str) {
        self.error = Some(why.to_owned());
        out.push(Command::Reply {
            id,
            value: Value::Bool(false),
        });
    }

    /// One event, as the work it implies.
    fn step(&mut self, event: Event, out: &mut Vec<Command>) {
        match event {
            Event::Start => {
                self.login = Login::Checking;
                // The identity first: a renewal needs the client id it holds,
                // and it is read before the token is checked.
                out.push(Command::Load {
                    key: K_IDENTITY.into(),
                });
            }
            Event::Watched { fields, signals } => self.watched = Watched { fields, signals },
            Event::Loaded { key, value } if key == K_IDENTITY => {
                self.identity = value.and_then(|t| serde_json::from_str(&t).ok());
                out.push(Command::LoadSecret {
                    slot: SECRET_AUTH.into(),
                });
            }
            Event::SecretLoaded { slot, value } if slot == SECRET_AUTH => {
                if self.login != Login::Checking {
                    return;
                }
                match value {
                    // A token is held: ask what it is. None may be a keyring
                    // that can't be read now, so nothing is forgotten.
                    Some(_) => self.validate(out),
                    None => self.login = Login::Out,
                }
            }
            Event::SecretLoaded { slot, value } if slot == SECRET_REFRESH => {
                if !self.refreshing {
                    return;
                }
                let (refresh, client) = match (value, self.client_id().map(str::to_owned)) {
                    (Some(refresh), Some(client)) => (refresh, client),
                    _ => {
                        self.expired(out, "Twitch refused the token: connect again.".into());
                        return;
                    }
                };
                self.request(
                    out,
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
                    self.answered(out, pending, response);
                }
            }
            Event::Received { id, data } => {
                let Data::Text(text) = data else {
                    return;
                };
                match protocol::parse_frame(&text) {
                    Ok(frame) => self.frame(id, frame, out),
                    Err(e) => self.error = Some(format!("bad frame: {e}")),
                }
            }
            Event::Closed { id, error } => {
                if self.next_ws == Some(id) {
                    self.next_ws = None;
                    if self.ws.is_some() {
                        return;
                    }
                } else if self.ws.as_ref().is_some_and(|w| w.id == id) {
                    self.ws = None;
                    if self.next_ws.is_some() {
                        return;
                    }
                } else {
                    return;
                }
                if let Some(why) = error {
                    self.error = Some(why);
                }
                self.lost_socket(out);
            }
            Event::Timer { id, .. } => match id.as_str() {
                "device-poll" => self.poll_device(out),
                "device-expire" => {
                    self.cancel_device(out);
                    self.error = Some("The sign-in expired: connect again.".into());
                }
                "after-token" => self.validate(out),
                "refresh" => self.refresh(out),
                "retry" => self.retry_armed = false,
                "sub-retry" => {
                    for sub in self.subs.values_mut().filter(|s| **s == Sub::Retrying) {
                        *sub = Sub::Queued;
                    }
                }
                "watchdog" => {
                    // Nothing for two keepalives: the socket only looks open.
                    if self.ws.is_none() && self.next_ws.is_none() {
                        return;
                    }
                    for id in self
                        .ws
                        .take()
                        .map(|w| w.id)
                        .into_iter()
                        .chain(self.next_ws.take())
                    {
                        out.push(Command::Close { id });
                    }
                    self.lost_socket(out);
                }
                _ => {}
            },
            Event::Ui { event, .. } => self.ui(event, out),
            Event::Call {
                id,
                ref method,
                args,
                ..
            } => out.extend(self.call(id, method, &args)),
            _ => {}
        }
    }

    /// What an answer to `pending` means.
    fn answered(
        &mut self,
        out: &mut Vec<Command>,
        pending: Pending,
        response: Result<vrc_we_sdk::HttpResponse, String>,
    ) {
        let (status, parsed) = match response {
            Ok(r) => {
                let body = match r.body {
                    Data::Text(t) => t,
                    Data::Bytes(b) => String::from_utf8_lossy(&b).into_owned(),
                };
                (r.status, serde_json::from_str(&body).unwrap_or(Json::Null))
            }
            Err(e) => {
                self.unreachable(out, pending, e);
                return;
            }
        };
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
                    self.login = Login::In;
                    self.error = None;
                }
                // The token is dead: renew it from what the identity and the
                // refresh slot hold, or ask for a new one.
                (401, _) => self.refresh(out),
                (_, Err(e)) => self.check_again(out, e),
                _ => {
                    let why = protocol::parse_error(&parsed)
                        .unwrap_or_else(|| format!("the check failed ({status})"));
                    self.check_again(out, why);
                }
            },
            Pending::Refresh => match (status, protocol::parse_token(&parsed)) {
                (200, Ok(token)) => {
                    self.refreshing = false;
                    self.error = None;
                    self.accepted(out, token);
                }
                (500.., _) => {
                    let why = format!("the renewal failed ({status})");
                    self.renew_again(out, why);
                }
                // Twitch refused the renewal itself (`invalid_grant` once the
                // refresh token is spent, which a public client's is after 30
                // days): say what it said, and ask for a new token.
                _ => {
                    let why = protocol::parse_error(&parsed)
                        .unwrap_or_else(|| format!("the renewal was refused ({status})"));
                    self.expired(out, why);
                }
            },
            Pending::Device => {
                self.device_asked = false;
                match (status, protocol::parse_device(&parsed)) {
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
                        self.error =
                            Some(protocol::parse_error(&parsed).unwrap_or_else(|| {
                                format!("Twitch refused the sign-in ({status})")
                            }));
                    }
                    (_, Err(e)) => self.error = Some(e),
                }
            }
            Pending::DeviceToken => self.device_answered(out, status, &parsed),
            Pending::Subscribe { kind } => {
                let sub = match status {
                    202 | 409 => {
                        self.subs_renewed = false;
                        Sub::Active
                    }
                    401 if !self.subs_renewed => {
                        self.subs_renewed = true;
                        self.refresh(out);
                        Sub::Queued
                    }
                    429 | 500.. => self.retry_sub(out),
                    _ => Sub::Refused(
                        protocol::parse_error(&parsed)
                            .unwrap_or_else(|| format!("answered {status}")),
                    ),
                };
                if self.subs.contains_key(&kind) {
                    self.subs.insert(kind, sub);
                }
            }
            Pending::Stream => {
                if status == 200 {
                    self.stream_live = protocol::parse_stream_live(&parsed);
                }
            }
            Pending::Action {
                action,
                request,
                retried,
            } => self.sent(out, status, &parsed, action, request, retried),
        }
    }

    /// `pending` never reached Twitch.
    fn unreachable(&mut self, out: &mut Vec<Command>, pending: Pending, why: String) {
        match pending {
            Pending::Validate => self.check_again(out, why),
            Pending::Refresh => self.renew_again(out, why),
            Pending::Subscribe { kind } => {
                let sub = self.retry_sub(out);
                if self.subs.contains_key(&kind) {
                    self.subs.insert(kind, sub);
                }
                self.error = Some(why);
            }
            Pending::Device => {
                self.device_asked = false;
                self.error = Some(why);
            }
            // The next poll or the next session asks again.
            Pending::DeviceToken => {
                let wait = self.device.as_ref().map_or(5_000, |d| d.interval_ms);
                out.push(Command::Timer {
                    id: "device-poll".into(),
                    after_ms: wait,
                });
                self.error = Some(why);
            }
            Pending::Stream => {}
            Pending::Action {
                action, request, ..
            } => {
                if action == Action::Chat {
                    self.chat_answered(&request, None);
                }
                self.error = Some(why);
            }
        }
    }

    /// Twitch answered a chat message it was asked to send: the id it gave
    /// the message, none when it sent nothing.
    fn chat_answered(&mut self, request: &Request, id: Option<String>) {
        let Some(text) = request
            .content
            .as_ref()
            .and_then(|(_, body)| protocol::chat_text(body))
        else {
            return;
        };
        let Some(at) = self
            .own_chat
            .iter()
            .position(|c| c.id.is_none() && c.text == text)
        else {
            return;
        };
        match id {
            Some(id) => self.own_chat[at].id = Some(id),
            None => {
                self.own_chat.remove(at);
            }
        }
    }

    /// Whether `fact` is the echo of a chat message it sent: by the id
    /// Twitch gave it, or by its text while Twitch hasn't answered yet.
    fn own_echo(&mut self, fact: &protocol::Fact) -> bool {
        let mine = self
            .identity
            .as_ref()
            .is_some_and(|i| fact.kind == "channel.chat.message" && i.id == fact.user_id);
        if !mine {
            return false;
        }
        let Some(at) = self.own_chat.iter().position(|c| match &c.id {
            Some(id) => *id == fact.id,
            None => c.text == fact.message,
        }) else {
            return false;
        };
        self.own_chat.remove(at);
        true
    }

    /// Twitch couldn't say what the token is: nothing is known against it, so
    /// the check goes again.
    fn check_again(&mut self, out: &mut Vec<Command>, why: String) {
        self.error = Some(why);
        let wait = self.backoff_ms.max(RETRY_MS);
        self.backoff_ms = wait.saturating_mul(2).min(RETRY_MAX_MS);
        out.push(Command::Timer {
            id: "after-token".into(),
            after_ms: wait,
        });
    }

    /// The renewal didn't reach an answer: what it interrupted keeps waiting.
    fn renew_again(&mut self, out: &mut Vec<Command>, why: String) {
        self.refreshing = false;
        self.error = Some(why);
        out.push(Command::Timer {
            id: "refresh".into(),
            after_ms: RETRY_MAX_MS,
        });
    }

    /// A create goes again after a wait that grows until the socket holds.
    fn retry_sub(&mut self, out: &mut Vec<Command>) -> Sub {
        let wait = self.sub_backoff_ms.max(RETRY_MS);
        self.sub_backoff_ms = wait.saturating_mul(2).min(RETRY_MAX_MS);
        out.push(Command::Timer {
            id: "sub-retry".into(),
            after_ms: wait,
        });
        Sub::Retrying
    }

    /// Whether the person has approved the waiting code yet.
    fn device_answered(&mut self, out: &mut Vec<Command>, status: u16, parsed: &Json) {
        match status {
            200 => match protocol::parse_token(parsed) {
                Ok(token) => {
                    self.cancel_device(out);
                    self.store_token(out, &token);
                    // Another account's token may be replacing this one's: the
                    // socket's subscriptions go with it. The token is in the
                    // keyring now; a beat later the check names the account.
                    self.drop_link(out);
                    self.pending
                        .retain(|_, p| !matches!(p, Pending::Validate | Pending::Refresh));
                    self.refreshing = false;
                    self.login = Login::Checking;
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
                    let why = protocol::parse_error(parsed)
                        .unwrap_or_else(|| "the sign-in expired".to_owned());
                    self.cancel_device(out);
                    self.error = Some(why);
                }
            }
            _ => {
                let why = protocol::parse_error(parsed)
                    .unwrap_or_else(|| format!("the sign-in was refused ({status})"));
                self.cancel_device(out);
                self.error = Some(why);
            }
        }
    }

    /// Reads the token a renewal carried: keeps it, and what it interrupted
    /// goes again, once, since Twitch refused that before doing anything. A
    /// renewal changes nothing the socket is holding.
    fn accepted(&mut self, out: &mut Vec<Command>, token: Token) {
        self.store_token(out, &token);
        // A renewal answers with the token's own scopes: the held identity
        // follows them, so a group that came into scope starts listening.
        if let Some(identity) = self.identity.as_mut() {
            identity.scopes = token.scopes.clone();
        }
        out.push(Command::Timer {
            id: "refresh".into(),
            after_ms: u32::try_from(token.expires_ms.saturating_sub(REFRESH_LEAD_MS))
                .unwrap_or(u32::MAX),
        });
        for (action, request) in std::mem::take(&mut self.waiting) {
            self.send_action(out, action, request, true);
        }
        if self.login == Login::Checking {
            self.validate(out);
        }
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

    /// What an action's answer means: done, a renewal, or the scope it lacks.
    fn sent(
        &mut self,
        out: &mut Vec<Command>,
        status: u16,
        parsed: &Json,
        action: Action,
        request: Request,
        retried: bool,
    ) {
        if action == Action::Chat && (status != 401 || retried) {
            let id = match status {
                200 => protocol::parse_sent_chat(parsed),
                _ => None,
            };
            self.chat_answered(&request, id);
        }
        let (what, scope) = action.names();
        match status {
            200 | 202 | 204 => self.error = None,
            401 if retried => {
                self.error = Some(format!("{what} was refused after the token was renewed"));
            }
            401 => {
                self.waiting.push((action, request));
                self.refresh(out);
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

    /// A token that can't be renewed: the page asks for a new one. The
    /// keyring keeps what it holds, since only Forget removes it.
    fn expired(&mut self, out: &mut Vec<Command>, why: String) {
        self.drop_link(out);
        cancel(out, &["refresh"]);
        self.waiting.clear();
        self.refreshing = false;
        self.login = Login::Expired;
        self.error = Some(why);
    }

    fn refresh(&mut self, out: &mut Vec<Command>) {
        if self.refreshing {
            return;
        }
        self.refreshing = true;
        out.push(Command::LoadSecret {
            slot: SECRET_REFRESH.into(),
        });
    }

    fn ui(&mut self, event: UiEvent, out: &mut Vec<Command>) {
        let UiEvent::Pressed { id } = event else {
            return;
        };
        match id.as_str() {
            "connect" => self.start_device(out),
            "cancel" => self.cancel_device(out),
            "forget" => self.logout(out),
            // Everything is asked again: what was refused, the token, the
            // socket.
            "reconnect" => {
                self.drop_link(out);
                self.pending
                    .retain(|_, p| matches!(p, Pending::Action { .. }));
                cancel(out, &["after-token"]);
                self.refreshing = false;
                self.login = Login::Checking;
                self.error = None;
                out.push(Command::LoadSecret {
                    slot: SECRET_AUTH.into(),
                });
            }
            _ => {}
        }
    }

    /// Forgets the account: every token, every timer, every answer still out.
    fn logout(&mut self, out: &mut Vec<Command>) {
        out.extend([
            Command::UnstoreSecret {
                slot: SECRET_AUTH.into(),
            },
            Command::UnstoreSecret {
                slot: SECRET_REFRESH.into(),
            },
            Command::Unstore {
                key: K_IDENTITY.into(),
            },
        ]);
        self.cancel_device(out);
        self.drop_link(out);
        cancel(out, &["refresh", "after-token"]);
        self.pending.clear();
        self.waiting.clear();
        self.refreshing = false;
        self.identity = None;
        self.login = Login::Out;
        self.stream_live = false;
        self.last = None;
        self.error = None;
    }

    fn call(&mut self, id: u64, method: &str, args: &[Value]) -> Vec<Command> {
        let mut out = Vec::new();
        let text = |i: usize| match args.get(i) {
            Some(Value::Text(t)) => Some(t.clone()),
            _ => None,
        };
        match method {
            "clip" => {
                let Some(identity) = self.account() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                let title = text(0).unwrap_or_default();
                if title.len() > protocol::TITLE_MAX {
                    self.refuse(&mut out, id, "A clip title is at most 140 bytes.");
                    return out;
                }
                self.act(
                    &mut out,
                    Action::Clip,
                    "POST",
                    &protocol::clip_url(&identity.id, &title),
                    None,
                );
                out.push(Command::Reply {
                    id,
                    value: Value::Bool(true),
                });
            }
            "marker" => {
                let Some(identity) = self.account() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                let description = text(0).unwrap_or_default();
                if description.len() > protocol::TITLE_MAX {
                    self.refuse(&mut out, id, "A marker's description is at most 140 bytes.");
                    return out;
                }
                self.act(
                    &mut out,
                    Action::Marker,
                    "POST",
                    protocol::MARKER_URL,
                    Some((
                        "application/json",
                        protocol::marker_body(&identity.id, &description),
                    )),
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
                let Some(identity) = self.account() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                if message.is_empty() || message.len() > protocol::CHAT_MAX {
                    self.refuse(&mut out, id, "A chat message is 1 to 500 bytes.");
                    return out;
                }
                self.act(
                    &mut out,
                    Action::Chat,
                    "POST",
                    protocol::CHAT_URL,
                    Some((
                        "application/json",
                        protocol::chat_body(&identity.id, &identity.id, &message),
                    )),
                );
                if self.own_chat.len() >= OWN_CHAT_MAX {
                    self.own_chat.pop_front();
                }
                self.own_chat.push_back(OwnChat {
                    text: message,
                    id: None,
                });
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
                let Some(identity) = self.account() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                self.act(
                    &mut out,
                    Action::Announce,
                    "POST",
                    protocol::ANNOUNCE_URL,
                    Some((
                        "application/json",
                        protocol::announce_body(&identity.id, &identity.id, &message),
                    )),
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
                let Some(identity) = self.account() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                self.act(
                    &mut out,
                    Action::Title,
                    "PATCH",
                    &protocol::channel_url(&identity.id),
                    Some((
                        "application/json",
                        protocol::channel_body(Some(&title), None),
                    )),
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
                let Some(identity) = self.account() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                self.act(
                    &mut out,
                    Action::Commercial,
                    "POST",
                    protocol::COMMERCIAL_URL,
                    Some((
                        "application/json",
                        protocol::commercial_body(&identity.id, seconds as u64),
                    )),
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
                let Some(identity) = self.account() else {
                    self.refuse(&mut out, id, "Not logged in to Twitch.");
                    return out;
                };
                self.act(
                    &mut out,
                    Action::Poll,
                    "POST",
                    protocol::POLLS_URL,
                    Some((
                        "application/json",
                        protocol::poll_body(&identity.id, &title, &choices, seconds),
                    )),
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

    /// One EventSub frame from socket `socket`. Only the socket expected to
    /// say so can welcome or end a session; a replacement's notifications
    /// count from its first frame, and one seen on both counts once.
    fn frame(&mut self, socket: u64, frame: Frame, out: &mut Vec<Command>) {
        let current = self.ws.as_ref().is_some_and(|w| w.id == socket);
        let replacing = self.next_ws == Some(socket);
        match frame {
            Frame::Welcome {
                session,
                keepalive_ms,
            } => {
                if replacing {
                    // A requested reconnect: Twitch carried the subscriptions
                    // over, and the old socket has served its purpose.
                    if let Some(old) = self.ws.take() {
                        out.push(Command::Close { id: old.id });
                    }
                    self.next_ws = None;
                } else if !current {
                    return;
                }
                self.ws = Some(Socket {
                    id: socket,
                    session: Some(session),
                });
                self.keepalive_ms = keepalive_ms;
                self.arm_watchdog(out);
            }
            Frame::Keepalive if current => self.arm_watchdog(out),
            Frame::Notification { id, kind, event } if current || replacing => {
                if current {
                    self.arm_watchdog(out);
                }
                if !id.is_empty() {
                    if self.seen.contains(&id) {
                        return;
                    }
                    if self.seen.len() >= SEEN_MAX {
                        self.seen.pop_front();
                    }
                    self.seen.push_back(id);
                }
                if let Some(fact) = protocol::decode(&kind, &event) {
                    if self.own_echo(&fact) {
                        return;
                    }
                    match fact.kind.as_str() {
                        "stream.online" => self.stream_live = true,
                        "stream.offline" => self.stream_live = false,
                        _ => {}
                    }
                    if let Some((name, value)) = signal_of(&fact) {
                        out.push(Command::Signal {
                            name: name.into(),
                            value,
                        });
                    }
                    self.last = Some(fact);
                }
            }
            Frame::Reconnect { url } if current && self.next_ws.is_none() => {
                self.next_id += 1;
                let id = self.next_id;
                self.next_ws = Some(id);
                out.push(Command::Open {
                    id,
                    to: Endpoint::Ws { url },
                });
            }
            Frame::Revocation { kind, status } if current => {
                self.subs.insert(kind, Sub::Refused(status));
            }
            _ => {}
        }
    }

    /// Twitch sends a keepalive when nothing else is due, so two of them
    /// missing means the socket only looks open.
    fn arm_watchdog(&self, out: &mut Vec<Command>) {
        out.push(Command::Timer {
            id: "watchdog".into(),
            after_ms: u32::try_from(self.keepalive_ms.max(10_000) * 2).unwrap_or(u32::MAX),
        });
    }
}

/// Cancels the timers named.
fn cancel(out: &mut Vec<Command>, timers: &[&str]) {
    for id in timers {
        out.push(Command::CancelTimer {
            id: (*id).to_owned(),
        });
    }
}

impl Extension for Twitch {
    fn manifest(&self) -> Manifest {
        crate::manifest::build()
    }

    fn handle(&mut self, event: Event) -> Vec<Command> {
        let mut out = Vec::new();
        self.step(event, &mut out);
        self.reconcile(&mut out);
        let events = self.events();
        if events == Events::Connected {
            self.backoff_ms = 0;
            self.sub_backoff_ms = 0;
        }
        self.publish(events, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vrc_we_sdk::{Does, Tone};

    fn answer(status: u16, body: &str) -> Result<vrc_we_sdk::HttpResponse, String> {
        Ok(vrc_we_sdk::HttpResponse {
            status,
            headers: Vec::new(),
            body: Data::Text(body.into()),
        })
    }

    fn frame(socket: u64, kind: &str, payload: &str) -> Event {
        Event::Received {
            id: socket,
            data: Data::Text(format!(
                r#"{{"metadata":{{"message_type":"{kind}"}},"payload":{payload}}}"#
            )),
        }
    }

    fn welcome(socket: u64, session: &str) -> Event {
        frame(
            socket,
            "session_welcome",
            &format!(r#"{{"session":{{"id":"{session}","keepalive_timeout_seconds":30}}}}"#),
        )
    }

    /// The requests to `url` in `out`, by id.
    fn requests_to(out: &[Command], url: &str) -> Vec<u64> {
        out.iter()
            .filter_map(|c| match c {
                Command::Http { id, url: to, .. } if to.starts_with(url) => Some(*id),
                _ => None,
            })
            .collect()
    }

    fn request_to(out: &[Command], url: &str) -> u64 {
        *requests_to(out, url)
            .first()
            .unwrap_or_else(|| panic!("a request to {url}: {out:?}"))
    }

    /// The socket `out` opens.
    fn opened(out: &[Command]) -> Option<u64> {
        out.iter().find_map(|c| match c {
            Command::Open { id, .. } => Some(*id),
            _ => None,
        })
    }

    fn field(out: &[Command], name: &str) -> Option<Value> {
        out.iter().rev().find_map(|c| match c {
            Command::Field { name: n, value } if n == name => Some(value.clone()),
            _ => None,
        })
    }

    /// The status bar line `out` publishes.
    fn line(out: &[Command]) -> Option<(Tone, String)> {
        out.iter().rev().find_map(|c| match c {
            Command::Status(Some(s)) => Some((s.tone, s.label.clone())),
            _ => None,
        })
    }

    fn page(out: &[Command]) -> Option<String> {
        out.iter().rev().find_map(|c| match c {
            Command::Page { view, .. } => Some(format!("{view:?}")),
            _ => None,
        })
    }

    fn every_scope() -> Vec<&'static str> {
        GROUPS.iter().filter_map(|g| g.scope()).collect()
    }

    fn validated(scopes: &[&str]) -> Result<vrc_we_sdk::HttpResponse, String> {
        let scopes = serde_json::to_string(scopes).unwrap();
        answer(
            200,
            &format!(
                r#"{{"client_id":"cid","login":"streamer","scopes":{scopes},"user_id":"1337","expires_in":3600}}"#
            ),
        )
    }

    /// A module started and signed in the way a launch does.
    fn signed_in(scopes: &[&str]) -> Twitch {
        let mut t = Twitch::default();
        t.handle(Event::Start);
        t.handle(Event::Loaded {
            key: K_IDENTITY.into(),
            value: None,
        });
        let out = t.handle(Event::SecretLoaded {
            slot: SECRET_AUTH.into(),
            value: Some("Bearer x".into()),
        });
        t.handle(Event::Http {
            id: request_to(&out, protocol::VALIDATE_URL),
            response: validated(scopes),
        });
        t
    }

    fn watch(t: &mut Twitch, fields: &[&str], signals: &[&str]) -> Vec<Command> {
        let names = |n: &[&str]| n.iter().map(|s| (*s).to_owned()).collect();
        t.handle(Event::Watched {
            fields: names(fields),
            signals: names(signals),
        })
    }

    /// Signed in with every scope, reading `signals`, its socket welcomed: the
    /// module, the socket, and what the welcome sent.
    fn welcomed(signals: &[&str]) -> (Twitch, u64, Vec<Command>) {
        let mut t = signed_in(&every_scope());
        let out = watch(&mut t, &[], signals);
        let socket = opened(&out).expect("demand opens a socket");
        let out = t.handle(welcome(socket, "s1"));
        (t, socket, out)
    }

    /// Answers every create in `out` and those it leads to with `status`.
    fn answer_creates(t: &mut Twitch, out: Vec<Command>, status: u16) -> Vec<Command> {
        let mut out = out;
        let mut all = Vec::new();
        loop {
            let ids = requests_to(&out, protocol::SUBSCRIPTIONS_URL);
            all.append(&mut out);
            if ids.is_empty() {
                return all;
            }
            for id in ids {
                out.extend(t.handle(Event::Http {
                    id,
                    response: answer(status, r#"{"message":"said no"}"#),
                }));
            }
        }
    }

    #[test]
    fn its_manifest_is_one_the_client_accepts() {
        assert_eq!(Twitch::default().manifest().validate(), Ok(()));
    }

    #[test]
    fn a_mapping_can_only_send_the_things_it_is_for() {
        // Signing out and reconnecting are maintenance: the page holds them,
        // and nothing a trigger fires can reach them.
        let manifest = Twitch::default().manifest();
        let actions: Vec<String> = manifest
            .actions
            .iter()
            .map(|a| match &a.does {
                Does::Call(method) => method.clone(),
                Does::Set => "set".into(),
            })
            .collect();
        let sends = [
            "chat",
            "clip",
            "marker",
            "announce",
            "title",
            "commercial",
            "poll",
        ];
        assert_eq!(actions, sends);
        let methods: Vec<&str> = manifest
            .namespace
            .as_ref()
            .expect("a namespace")
            .methods
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(methods, sends, "each action calls its method");
    }

    /// Twitch closes a socket with no subscription after ten seconds, so with
    /// nothing to subscribe none opens, and the page and the bar say signed in
    /// rather than connected.
    #[test]
    fn with_nothing_to_subscribe_no_socket_opens() {
        let mut t = Twitch::default();
        let out = t.handle(Event::Start);
        assert_eq!(opened(&out), None);
        let mut t = signed_in(&[]);
        let out = watch(&mut t, &["user", "error"], &[]);
        assert_eq!(opened(&out), None, "reading its own state wants no event");
        assert_eq!(
            t.handle(Event::Timer {
                id: "retry".into(),
                at_ms: 0
            })
            .iter()
            .filter(|c| matches!(c, Command::Open { .. }))
            .count(),
            0
        );
        assert_eq!(t.events(), Events::Idle);
        let mut t = signed_in(&[]);
        let out = watch(&mut t, &[], &["redeemed"]);
        assert_eq!(
            opened(&out),
            None,
            "its scope is missing, so nothing can be"
        );
        assert_eq!(
            t.events(),
            Events::Waiting,
            "and that is waiting on the person"
        );
        assert_eq!(
            line(&out),
            Some((Tone::Waiting, "Waiting".to_owned())),
            "{out:?}"
        );
        assert!(
            page(&out).is_some_and(|p| p.contains("channel:read:redemptions")),
            "the page says which scope: {out:?}"
        );
    }

    /// The full surface goes through the host's bound a window at a time and
    /// reaches connected only when every create is confirmed.
    #[test]
    fn demand_connects_and_subscribes_within_its_window_until_connected() {
        let mut t = signed_in(&every_scope());
        let out = watch(&mut t, &["event"], &[]);
        let socket = opened(&out).expect("a socket");
        assert_eq!(t.events(), Events::Connecting);
        assert_eq!(line(&out), Some((Tone::Waiting, "Connecting".to_owned())));

        let mut out = t.handle(welcome(socket, "s1"));
        assert_eq!(
            requests_to(&out, protocol::SUBSCRIPTIONS_URL).len(),
            SUB_WINDOW,
            "the first go out with the welcome, within Twitch's ten seconds"
        );
        assert_eq!(t.events(), Events::Waiting);
        assert_eq!(
            field(&out, "connected"),
            None,
            "it reads false until it holds"
        );
        let mut created = 0;
        loop {
            let ids = requests_to(&out, protocol::SUBSCRIPTIONS_URL);
            if ids.is_empty() {
                break;
            }
            assert!(ids.len() <= SUB_WINDOW, "{}", ids.len());
            assert_ne!(t.events(), Events::Connected, "not with creates still out");
            created += ids.len();
            let mut next = Vec::new();
            for id in ids {
                next.extend(t.handle(Event::Http {
                    id,
                    response: answer(202, "{}"),
                }));
            }
            out = next;
        }
        assert_eq!(created, 16);
        assert_eq!(t.events(), Events::Connected);
        assert_eq!(field(&out, "connected"), Some(Value::Bool(true)));
        assert_eq!(line(&out), Some((Tone::Good, "Connected".to_owned())));
        assert!(page(&out).is_some_and(|p| p.contains("Listening for")));
    }

    #[test]
    fn reading_connected_holds_a_socket_open_with_the_group_that_needs_no_scope() {
        let mut t = signed_in(&[]);
        let out = watch(&mut t, &["connected"], &[]);
        let socket = opened(&out).expect("a socket");
        let out = t.handle(welcome(socket, "s1"));
        let out = answer_creates(&mut t, out, 202);
        assert_eq!(field(&out, "connected"), Some(Value::Bool(true)));
    }

    /// A refusal is sticky: the socket Twitch then closes doesn't come back to
    /// ask again, and the person sees why until they press Reconnect.
    #[test]
    fn a_refused_group_waits_with_its_reason_and_never_loops() {
        let (mut t, socket, out) = welcomed(&["redeemed"]);
        let out = answer_creates(&mut t, out, 403);
        assert_eq!(t.events(), Events::Waiting);
        assert_eq!(line(&out), Some((Tone::Waiting, "Waiting".to_owned())));
        assert!(
            page(&out).is_some_and(|p| p.contains("said no")),
            "Twitch's reason is on the page: {out:?}"
        );

        let out = t.handle(Event::Closed {
            id: socket,
            error: None,
        });
        assert_eq!(opened(&out), None);
        let out = t.handle(Event::Timer {
            id: "retry".into(),
            at_ms: 0,
        });
        assert_eq!(opened(&out), None, "nothing it could ask has changed");

        let out = t.handle(Event::Ui {
            page: PAGE.into(),
            event: UiEvent::Pressed {
                id: "reconnect".into(),
            },
        });
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::LoadSecret { slot } if slot == SECRET_AUTH)));
        assert_eq!(t.subs.len(), 0, "Reconnect asks again");
    }

    /// Twitch carries the subscriptions over a requested reconnect, so nothing
    /// is created again and the old socket's close changes nothing.
    #[test]
    fn a_requested_reconnect_stays_connected() {
        let (mut t, old, out) = welcomed(&["cheered"]);
        answer_creates(&mut t, out, 202);
        assert_eq!(t.events(), Events::Connected);
        let out = t.handle(frame(
            old,
            "session_reconnect",
            r#"{"session":{"reconnect_url":"wss://edge.example/ws"}}"#,
        ));
        let new = opened(&out).expect("the replacement");
        assert_eq!(t.events(), Events::Connected, "the old one still holds it");

        let out = t.handle(welcome(new, "s2"));
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Close { id } if *id == old)),
            "the old socket goes at the new welcome: {out:?}"
        );
        assert!(requests_to(&out, protocol::SUBSCRIPTIONS_URL).is_empty());
        assert_eq!(t.events(), Events::Connected);

        let out = t.handle(Event::Closed {
            id: old,
            error: None,
        });
        assert_eq!(opened(&out), None);
        assert_eq!(t.events(), Events::Connected);

        let stale = t.handle(welcome(old, "s3"));
        assert_eq!(
            t.ws.as_ref().and_then(|w| w.session.as_deref()),
            Some("s2"),
            "a socket that isn't expected can't start a session: {stale:?}"
        );
    }

    /// The old socket closing first doesn't lose what the replacement brings.
    #[test]
    fn the_old_socket_closing_before_the_new_welcome_keeps_the_subscriptions() {
        let (mut t, old, out) = welcomed(&["cheered"]);
        answer_creates(&mut t, out, 202);
        let out = t.handle(frame(
            old,
            "session_reconnect",
            r#"{"session":{"reconnect_url":"wss://edge.example/ws"}}"#,
        ));
        let new = opened(&out).expect("the replacement");
        let out = t.handle(Event::Closed {
            id: old,
            error: None,
        });
        assert_eq!(opened(&out), None, "the replacement is already on its way");
        let out = t.handle(welcome(new, "s2"));
        assert!(requests_to(&out, protocol::SUBSCRIPTIONS_URL).is_empty());
        assert_eq!(t.events(), Events::Connected);
    }

    /// A socket lost for any other reason takes its subscriptions with it, and
    /// another opens after a wait that grows until one holds.
    #[test]
    fn a_lost_socket_comes_back_after_a_growing_wait_and_subscribes_again() {
        let (mut t, socket, out) = welcomed(&["cheered"]);
        answer_creates(&mut t, out, 202);
        let wait = |out: &[Command]| {
            out.iter().find_map(|c| match c {
                Command::Timer { id, after_ms } if id == "retry" => Some(*after_ms),
                _ => None,
            })
        };
        let out = t.handle(Event::Closed {
            id: socket,
            error: None,
        });
        assert_eq!(wait(&out), Some(RETRY_MS));
        assert_eq!(opened(&out), None);
        assert_eq!(field(&out, "connected"), Some(Value::Bool(false)));
        assert_eq!(line(&out), Some((Tone::Waiting, "Connecting".to_owned())));

        let out = t.handle(Event::Timer {
            id: "retry".into(),
            at_ms: 0,
        });
        let again = opened(&out).expect("it opens again");
        let out = t.handle(Event::Closed {
            id: again,
            error: Some("refused".into()),
        });
        assert_eq!(wait(&out), Some(RETRY_MS * 2));

        let out = t.handle(Event::Timer {
            id: "retry".into(),
            at_ms: 0,
        });
        let third = opened(&out).expect("it opens again");
        let out = t.handle(welcome(third, "s2"));
        assert_eq!(
            requests_to(&out, protocol::SUBSCRIPTIONS_URL).len(),
            1,
            "the session's subscriptions went with it"
        );
        answer_creates(&mut t, out, 202);
        assert_eq!(t.backoff_ms, 0, "connected resets the wait");
    }

    /// A notification counts as life, and one delivered twice counts once.
    #[test]
    fn a_notification_resets_the_watchdog_and_a_repeat_counts_once() {
        let (mut t, socket, _) = welcomed(&["cheered"]);
        let cheer = Event::Received {
            id: socket,
            data: Data::Text(
                r#"{"metadata":{"message_type":"notification","message_id":"m1","subscription_type":"channel.cheer"},"payload":{"event":{"user_name":"a","bits":5,"message":"hi"}}}"#
                    .into(),
            ),
        };
        let out = t.handle(cheer.clone());
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::Timer { id, after_ms } if id == "watchdog" && *after_ms == 60_000)));
        assert!(out.iter().any(|c| matches!(c, Command::Signal { .. })));
        let out = t.handle(cheer);
        assert!(!out.iter().any(|c| matches!(c, Command::Signal { .. })));
    }

    #[test]
    fn a_frame_from_a_socket_it_doesnt_know_changes_nothing() {
        let (mut t, _, _) = welcomed(&["cheered"]);
        let out = t.handle(Event::Received {
            id: 999,
            data: Data::Text(
                r#"{"metadata":{"message_type":"notification","subscription_type":"channel.cheer"},"payload":{"event":{"user_name":"a","bits":5}}}"#
                    .into(),
            ),
        });
        assert!(!out.iter().any(|c| matches!(c, Command::Signal { .. })));
        assert_eq!(field(&out, "bits"), None);
    }

    #[test]
    fn a_renewal_while_connected_opens_nothing_and_a_create_waits_for_one() {
        let (mut t, _, out) = welcomed(&["cheered", "raided"]);
        let creates = requests_to(&out, protocol::SUBSCRIPTIONS_URL);
        // The token ran out under a create: it waits for the new one.
        let out = t.handle(Event::Http {
            id: creates[0],
            response: answer(401, "{}"),
        });
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::LoadSecret { slot } if slot == SECRET_REFRESH)));
        assert!(
            requests_to(&out, protocol::SUBSCRIPTIONS_URL).is_empty(),
            "it would only meet the same refusal"
        );
        let out = t.handle(Event::SecretLoaded {
            slot: SECRET_REFRESH.into(),
            value: Some("r1".into()),
        });
        let renew = request_to(&out, protocol::TOKEN_URL);
        let out = t.handle(Event::Http {
            id: renew,
            response: answer(
                200,
                &format!(
                    r#"{{"access_token":"new","refresh_token":"r2","expires_in":3600,"scope":{}}}"#,
                    serde_json::to_string(&every_scope()).unwrap()
                ),
            ),
        });
        assert_eq!(opened(&out), None, "the socket holds through a renewal");
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::StoreSecret { slot, value }
            if slot == SECRET_AUTH && value == "Bearer new")));
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::StoreSecret { slot, value }
            if slot == SECRET_REFRESH && value == "r2")));
        assert_eq!(requests_to(&out, protocol::SUBSCRIPTIONS_URL).len(), 1);
        let mut rest = out;
        rest.extend(t.handle(Event::Http {
            id: creates[1],
            response: answer(202, "{}"),
        }));
        answer_creates(&mut t, rest, 202);
        assert_eq!(t.events(), Events::Connected);
    }

    /// Forgetting ends everything: no answer still out and no timer brings the
    /// account or a socket back.
    #[test]
    fn forgetting_drops_every_late_answer_and_nothing_reconnects() {
        let (mut t, socket, out) = welcomed(&["cheered"]);
        let create = request_to(&out, protocol::SUBSCRIPTIONS_URL);
        let chat = t.handle(Event::Call {
            id: 1,
            method: "chat".into(),
            args: vec![Value::Text("hi".into())],
            from: None,
        });
        let chat = request_to(&chat, protocol::CHAT_URL);
        let out = t.handle(Event::Ui {
            page: PAGE.into(),
            event: UiEvent::Pressed {
                id: "forget".into(),
            },
        });
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::UnstoreSecret { slot } if slot == SECRET_AUTH)));
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::Close { id } if *id == socket)));
        assert_eq!(line(&out), Some((Tone::Idle, "Not signed in".to_owned())));
        for event in [
            Event::Http {
                id: create,
                response: answer(202, "{}"),
            },
            Event::Http {
                id: chat,
                response: answer(401, "{}"),
            },
            Event::Timer {
                id: "watchdog".into(),
                at_ms: 0,
            },
            Event::Timer {
                id: "retry".into(),
                at_ms: 0,
            },
            Event::Closed {
                id: socket,
                error: None,
            },
            Event::SecretLoaded {
                slot: SECRET_AUTH.into(),
                value: Some("Bearer late".into()),
            },
        ] {
            let out = t.handle(event);
            assert_eq!(opened(&out), None, "{out:?}");
            assert!(requests_to(&out, "https://").is_empty(), "{out:?}");
            assert!(out.iter().all(|c| !matches!(c, Command::LoadSecret { .. })));
        }
        assert_eq!(t.login, Login::Out);
    }

    /// An unreadable keyring reads the same as no token, so it must never lose
    /// what a later launch or Reconnect could still use.
    #[test]
    fn a_token_that_cant_be_read_forgets_nothing() {
        let mut t = Twitch::default();
        t.handle(Event::Start);
        let identity =
            r#"{"id":"1337","login":"streamer","client_id":"cid","scopes":[],"expires_ms":1000}"#;
        t.handle(Event::Loaded {
            key: K_IDENTITY.into(),
            value: Some(identity.into()),
        });
        let out = t.handle(Event::SecretLoaded {
            slot: SECRET_AUTH.into(),
            value: None,
        });
        assert_eq!(t.login, Login::Out);
        assert!(out
            .iter()
            .all(|c| !matches!(c, Command::UnstoreSecret { .. } | Command::Unstore { .. })));
        assert_eq!(
            t.identity.as_ref().map(|i| i.login.as_str()),
            Some("streamer")
        );
        assert_eq!(line(&out), Some((Tone::Idle, "Not signed in".to_owned())));
        let page = page(&out).expect("a page");
        assert!(
            page.contains("reconnect"),
            "the keyring may answer now: {page}"
        );
        assert!(page.contains("forget"));
    }

    /// Each refused action goes again itself, once, on one renewal.
    #[test]
    fn overlapping_refused_actions_each_repeat_their_own_request_once() {
        let mut t = signed_in(&[SCOPE_CHAT]);
        let call = |t: &mut Twitch, id, text: &str| {
            let out = t.handle(Event::Call {
                id,
                method: "chat".into(),
                args: vec![Value::Text(text.into())],
                from: None,
            });
            request_to(&out, protocol::CHAT_URL)
        };
        let first = call(&mut t, 1, "first");
        let second = call(&mut t, 2, "second");
        let refused = |id| Event::Http {
            id,
            response: answer(401, "{}"),
        };
        let out = t.handle(refused(first));
        assert_eq!(
            out.iter()
                .filter(|c| matches!(c, Command::LoadSecret { slot } if slot == SECRET_REFRESH))
                .count(),
            1
        );
        let out = t.handle(refused(second));
        assert!(
            !out.iter().any(|c| matches!(c, Command::LoadSecret { .. })),
            "one renewal serves both"
        );
        let out = t.handle(Event::SecretLoaded {
            slot: SECRET_REFRESH.into(),
            value: Some("r1".into()),
        });
        let out = t.handle(Event::Http {
            id: request_to(&out, protocol::TOKEN_URL),
            response: answer(
                200,
                r#"{"access_token":"new","refresh_token":"r2","expires_in":3600,"scope":["user:write:chat"]}"#,
            ),
        });
        let bodies: Vec<String> = out
            .iter()
            .filter_map(|c| match c {
                Command::Http {
                    url,
                    body: Some(Data::Text(b)),
                    ..
                } if url == protocol::CHAT_URL => Some(b.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(bodies.len(), 2);
        assert!(bodies[0].contains("first") && bodies[1].contains("second"));
        let again = requests_to(&out, protocol::CHAT_URL);
        let out = t.handle(refused(again[0]));
        assert!(
            !out.iter().any(|c| matches!(c, Command::LoadSecret { .. })),
            "a second refusal is reported, not renewed"
        );
        assert!(t
            .error
            .as_deref()
            .is_some_and(|e| e.contains("after the token was renewed")));
    }

    /// `live` is the stream's own state: read when something wants it, moved by
    /// its two events, and untouched by any other.
    #[test]
    fn live_survives_other_events_and_starts_from_what_twitch_says() {
        let (mut t, socket, out) = welcomed(&["went_live", "cheered"]);
        let read = request_to(&out, protocol::STREAMS_URL);
        let out = t.handle(Event::Http {
            id: read,
            response: answer(200, r#"{"data":[{"type":"live"}]}"#),
        });
        assert_eq!(field(&out, "live"), Some(Value::Bool(true)));
        let cheer = Event::Received {
            id: socket,
            data: Data::Text(
                r#"{"metadata":{"message_type":"notification","subscription_type":"channel.cheer"},"payload":{"event":{"user_name":"a","bits":5}}}"#
                    .into(),
            ),
        };
        let out = t.handle(cheer);
        assert_eq!(field(&out, "bits"), Some(Value::Number(5.0)));
        assert_eq!(field(&out, "live"), None, "unchanged, so unsent");
        assert!(t.stream_live);
        let out = t.handle(Event::Received {
            id: socket,
            data: Data::Text(
                r#"{"metadata":{"message_type":"notification","subscription_type":"stream.offline"},"payload":{"event":{}}}"#
                    .into(),
            ),
        });
        assert_eq!(field(&out, "live"), Some(Value::Bool(false)));
    }

    #[test]
    fn a_chat_signal_stays_within_its_bytes_on_a_character() {
        let (mut t, socket, _) = welcomed(&["chatted"]);
        let message = "é".repeat(40);
        let out = t.handle(Event::Received {
            id: socket,
            data: Data::Text(format!(
                r#"{{"metadata":{{"message_type":"notification","subscription_type":"channel.chat.message"}},"payload":{{"event":{{"chatter_user_name":"a","message":{{"text":"{message}"}}}}}}}}"#
            )),
        });
        let carried = out.iter().find_map(|c| match c {
            Command::Signal {
                value: Value::Text(text),
                ..
            } => Some(text.clone()),
            _ => None,
        });
        let carried = carried.expect("a signal");
        assert!(carried.len() <= 64, "{}", carried.len());
        assert_eq!(carried, "é".repeat(32));
    }

    /// A chat message it sent comes back without a signal, whether its echo
    /// arrives before Twitch answers the send or after; the person's own
    /// message from the same account still signals.
    #[test]
    fn its_own_chat_messages_never_signal() {
        let (mut t, socket, _) = welcomed(&["chatted"]);
        let chat = |t: &mut Twitch, id: u64, text: &str| {
            let out = t.handle(Event::Call {
                id,
                method: "chat".into(),
                args: vec![Value::Text(text.into())],
                from: None,
            });
            request_to(&out, protocol::CHAT_URL)
        };
        let echo = |t: &mut Twitch, message_id: &str, text: &str| {
            let out = t.handle(Event::Received {
                id: socket,
                data: Data::Text(format!(
                    r#"{{"metadata":{{"message_type":"notification","message_id":"n-{message_id}","subscription_type":"channel.chat.message"}},"payload":{{"event":{{"message_id":"{message_id}","chatter_user_id":"1337","chatter_user_name":"streamer","message":{{"text":"{text}"}}}}}}}}"#
                )),
            });
            out.iter().any(|c| matches!(c, Command::Signal { .. }))
        };
        let answered = chat(&mut t, 1, "thanks");
        t.handle(Event::Http {
            id: answered,
            response: answer(200, r#"{"data":[{"message_id":"m1","is_sent":true}]}"#),
        });
        assert!(!echo(&mut t, "m1", "thanks"), "its echo after the answer");
        chat(&mut t, 2, "welcome");
        assert!(!echo(&mut t, "m2", "welcome"), "its echo before the answer");
        assert!(echo(&mut t, "m3", "thanks"), "the person typed it themself");
    }

    /// A press before Twitch answers doesn't ask twice, and an answer to a
    /// cancelled sign-in stores nothing.
    #[test]
    fn a_device_sign_in_asks_once_and_a_cancelled_one_stays_cancelled() {
        let mut t = Twitch::default();
        let press = |id: &str| Event::Ui {
            page: PAGE.into(),
            event: UiEvent::Pressed { id: id.into() },
        };
        let out = t.handle(press("connect"));
        assert_eq!(
            line(&out),
            Some((Tone::Waiting, "Waiting for your approval".to_owned()))
        );
        let asked = request_to(&out, protocol::DEVICE_URL);
        let again = t.handle(press("connect"));
        assert!(requests_to(&again, protocol::DEVICE_URL).is_empty());
        let out = t.handle(Event::Http {
            id: asked,
            response: answer(
                200,
                r#"{"device_code":"dc","expires_in":1800,"interval":5,"user_code":"ABCDEFGH","verification_uri":"https://www.twitch.tv/activate?public=true&device-code=ABCDEFGH"}"#,
            ),
        });
        assert!(page(&out).is_some_and(|p| p.contains("twitch.tv/activate")));
        let out = t.handle(Event::Timer {
            id: "device-poll".into(),
            at_ms: 0,
        });
        let poll = request_to(&out, protocol::TOKEN_URL);
        t.handle(press("cancel"));
        let out = t.handle(Event::Http {
            id: poll,
            response: answer(
                200,
                r#"{"access_token":"abc","refresh_token":"ref","expires_in":14400,"scope":[],"token_type":"bearer"}"#,
            ),
        });
        assert!(
            !out.iter().any(|c| matches!(c, Command::StoreSecret { .. })),
            "the late approval stores nothing: {out:?}"
        );
    }

    #[test]
    fn an_approved_sign_in_keeps_both_tokens_and_then_names_the_account() {
        let mut t = Twitch::default();
        let out = t.handle(Event::Ui {
            page: PAGE.into(),
            event: UiEvent::Pressed {
                id: "connect".into(),
            },
        });
        t.handle(Event::Http {
            id: request_to(&out, protocol::DEVICE_URL),
            response: answer(
                200,
                r#"{"device_code":"dc","expires_in":1800,"interval":5,"user_code":"A","verification_uri":"https://www.twitch.tv/activate"}"#,
            ),
        });
        let out = t.handle(Event::Timer {
            id: "device-poll".into(),
            at_ms: 0,
        });
        let out = t.handle(Event::Http {
            id: request_to(&out, protocol::TOKEN_URL),
            response: answer(
                200,
                r#"{"access_token":"abc","refresh_token":"ref","expires_in":14400,"scope":["user:read:chat"],"token_type":"bearer"}"#,
            ),
        });
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::StoreSecret { slot, value }
            if slot == SECRET_AUTH && value == "Bearer abc")));
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::StoreSecret { slot, value }
            if slot == SECRET_REFRESH && value == "ref")));
        assert!(t.device.is_none(), "the waiting code is spent");
        let out = t.handle(Event::Timer {
            id: "after-token".into(),
            at_ms: 0,
        });
        let out = t.handle(Event::Http {
            id: request_to(&out, protocol::VALIDATE_URL),
            response: validated(&[]),
        });
        assert_eq!(line(&out), Some((Tone::Idle, "Signed in".to_owned())));
        assert!(page(&out).is_some_and(|p| p.contains("Signed in as streamer")));
    }

    #[test]
    fn a_check_arms_the_renewal_from_the_tokens_own_life() {
        let mut t = Twitch::default();
        t.handle(Event::Start);
        let out = t.handle(Event::SecretLoaded {
            slot: SECRET_AUTH.into(),
            value: Some("Bearer x".into()),
        });
        let out = t.handle(Event::Http {
            id: request_to(&out, protocol::VALIDATE_URL),
            response: validated(&[]),
        });
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Timer { id, after_ms }
                if id == "refresh" && *after_ms == 3_540_000)),
            "an hour less the minute it renews ahead: {out:?}"
        );
    }

    /// Unreachable is not refused: the check goes again, and nothing reads as
    /// signed out while it does.
    #[test]
    fn a_check_that_cant_reach_twitch_goes_again() {
        let mut t = Twitch::default();
        t.handle(Event::Start);
        let out = t.handle(Event::SecretLoaded {
            slot: SECRET_AUTH.into(),
            value: Some("Bearer x".into()),
        });
        let out = t.handle(Event::Http {
            id: request_to(&out, protocol::VALIDATE_URL),
            response: Err("no route".into()),
        });
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::Timer { id, .. } if id == "after-token")));
        assert_eq!(t.login, Login::Checking);
        assert_eq!(field(&out, "error"), Some(Value::Text("no route".into())));
    }

    #[test]
    fn a_spent_refresh_token_says_what_twitch_said_and_keeps_the_account() {
        let mut t = signed_in(&[]);
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
        let out = t.handle(Event::Http {
            id: request_to(&out, protocol::TOKEN_URL),
            response: answer(
                400,
                r#"{"error":"invalid_grant","message":"Invalid refresh token"}"#,
            ),
        });
        assert_eq!(t.error.as_deref(), Some("Invalid refresh token"));
        assert_eq!(t.login, Login::Expired);
        assert_eq!(line(&out), Some((Tone::Bad, "Sign-in expired".to_owned())));
        assert!(page(&out).is_some_and(|p| p.contains("connect")));
    }

    #[test]
    fn a_dead_token_asks_for_a_renewal_rather_than_showing_a_failure() {
        let mut t = Twitch::default();
        t.handle(Event::Start);
        let out = t.handle(Event::SecretLoaded {
            slot: SECRET_AUTH.into(),
            value: Some("Bearer x".into()),
        });
        let out = t.handle(Event::Http {
            id: request_to(&out, protocol::VALIDATE_URL),
            response: answer(401, r#"{"message":"Invalid OAuth token"}"#),
        });
        assert!(out
            .iter()
            .any(|c| matches!(c, Command::LoadSecret { slot } if slot == SECRET_REFRESH)));
        assert!(t.error.is_none(), "{:?}", t.error);
    }

    #[test]
    fn clip_and_marker_are_the_helix_calls_twitch_documents() {
        let mut t = signed_in(&[]);
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
        assert!(url.contains("title=nice"), "a query parameter: {url}");
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
    }

    #[test]
    fn a_chat_send_needs_a_login_and_a_message() {
        let mut t = signed_in(&[]);
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
    fn a_redemption_reaches_the_fields_a_mapping_conditions_on() {
        let (mut t, socket, _) = welcomed(&["redeemed"]);
        let out = t.handle(Event::Received {
            id: socket,
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
        let (mut t, socket, _) = welcomed(&["cheered"]);
        let out = t.handle(Event::Received {
            id: socket,
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
        let (mut t, socket, _) = welcomed(&["poll_ended"]);
        let out = t.handle(Event::Received {
            id: socket,
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
        let (mut t, socket, _) = welcomed(&["gifted", "went_live"]);
        let out = t.handle(Event::Received {
            id: socket,
            data: Data::Text(
                r#"{"metadata":{"message_type":"notification","subscription_type":"channel.subscription.gift"},"payload":{"event":{"user_name":"Giver","tier":"2000","total":5}}}"#
                    .into(),
            ),
        });
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Signal { name, value }
                if name == "gifted" && *value == Value::Number(5.0))),
            "five gifts, not five months: {out:?}"
        );
        assert_eq!(
            field(&out, "tier"),
            Some(Value::Number(2000.0)),
            "a tier comes as text"
        );
        let out = t.handle(Event::Received {
            id: socket,
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
}
