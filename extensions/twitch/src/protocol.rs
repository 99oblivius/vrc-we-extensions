//! Twitch's wire: the token check, the Helix calls the extension makes, and
//! the EventSub frames it reads.
//!
//! Everything here is a string or a `serde_json::Value`: the module has no
//! HTTP client, an async runtime or a clock, so it turns the client's
//! commands into these bodies and reads what comes back.

use serde_json::{json, Value};

/// The token check: it names the client the token was issued to (which every
/// Helix call carries), the account, the scopes, and how long it lasts. No
/// client id is needed to ask, and it takes the `Bearer` prefix Helix does.
pub const VALIDATE_URL: &str = "https://id.twitch.tv/oauth2/validate";
pub const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
pub const SUBSCRIPTIONS_URL: &str = "https://api.twitch.tv/helix/eventsub/subscriptions";
pub const CHAT_URL: &str = "https://api.twitch.tv/helix/chat/messages";
pub const CLIP_URL: &str = "https://api.twitch.tv/helix/clips";
pub const MARKER_URL: &str = "https://api.twitch.tv/helix/streams/markers";
pub const ANNOUNCE_URL: &str = "https://api.twitch.tv/helix/chat/announcements";
pub const CHANNELS_URL: &str = "https://api.twitch.tv/helix/channels";
pub const COMMERCIAL_URL: &str = "https://api.twitch.tv/helix/channels/commercial";
pub const POLLS_URL: &str = "https://api.twitch.tv/helix/polls";

/// The clip's length: Twitch's default, 30 s of the 90 s it keeps.
pub const CLIP_SECONDS: f32 = 30.0;

/// The longest clip title or marker description Twitch takes.
pub const TITLE_MAX: usize = 140;

/// One WebSocket, with a keepalive the client can hold a watchdog against:
/// Twitch sends one at least this often, and a longer gap means the socket is
/// dead whatever it still looks like.
pub const WS_URL: &str = "wss://eventsub.wss.twitch.tv/ws?keepalive_timeout_seconds=30";

/// The longest chat message Twitch takes.
pub const CHAT_MAX: usize = 500;

/// What the extension asks for: one scope each, so a token that carries them
/// costs nothing per subscription (Twitch charges 2 for an event whose scope
/// the token lacks, and the budget is 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Redeems,
    Chat,
    Subs,
    Cheer,
    Raid,
    Hype,
    Follow,
    Live,
    Poll,
}

/// Every group, in the order the page lists them.
pub const GROUPS: [Group; 9] = [
    Group::Redeems,
    Group::Chat,
    Group::Subs,
    Group::Cheer,
    Group::Raid,
    Group::Hype,
    Group::Follow,
    Group::Live,
    Group::Poll,
];

impl Group {
    /// Its store key and the page's node id.
    pub fn key(self) -> &'static str {
        match self {
            Self::Redeems => "redeems",
            Self::Chat => "chat",
            Self::Subs => "subs",
            Self::Cheer => "cheer",
            Self::Raid => "raid",
            Self::Hype => "hype",
            Self::Follow => "follow",
            Self::Live => "live",
            Self::Poll => "poll",
        }
    }

    /// The signals its events are sent under; a group whose events differ in
    /// kind has one each, so a mapping names the thing it cares about.
    pub fn signals(self) -> &'static [&'static str] {
        match self {
            Self::Redeems => &["redeemed"],
            Self::Chat => &["chatted"],
            Self::Subs => &["subscribed", "gifted"],
            Self::Cheer => &["cheered"],
            Self::Raid => &["raided"],
            Self::Hype => &["hype"],
            Self::Follow => &["followed"],
            Self::Live => &["went_live", "went_offline"],
            Self::Poll => &["polled", "poll_ended"],
        }
    }

    /// The scope the token must carry for its subscriptions to cost nothing.
    pub fn scope(self) -> Option<&'static str> {
        match self {
            Self::Redeems => Some("channel:read:redemptions"),
            Self::Chat => Some("user:read:chat"),
            Self::Subs => Some("channel:read:subscriptions"),
            Self::Cheer => Some("bits:read"),
            Self::Raid => None,
            Self::Hype => Some("channel:read:hype_train"),
            Self::Follow => Some("moderator:read:followers"),
            // No scope: a stream's own going live and offline are public.
            Self::Live => None,
            Self::Poll => Some("channel:read:polls"),
        }
    }

    /// The subscriptions a token for `user` creates for this group: type,
    /// version, and the condition that names the channel.
    pub fn subscriptions(self, user: &str) -> Vec<(&'static str, u32, Value)> {
        let broadcaster = json!({ "broadcaster_user_id": user });
        match self {
            Self::Redeems => vec![(
                "channel.channel_points_custom_reward_redemption.add",
                1,
                broadcaster,
            )],
            Self::Chat => vec![(
                "channel.chat.message",
                1,
                json!({ "broadcaster_user_id": user, "user_id": user }),
            )],
            Self::Subs => vec![
                ("channel.subscribe", 1, broadcaster.clone()),
                ("channel.subscription.message", 1, broadcaster.clone()),
                ("channel.subscription.gift", 1, broadcaster),
            ],
            Self::Cheer => vec![("channel.cheer", 1, broadcaster)],
            Self::Raid => vec![("channel.raid", 1, json!({ "to_broadcaster_user_id": user }))],
            Self::Hype => vec![
                ("channel.hype_train.begin", 2, broadcaster.clone()),
                ("channel.hype_train.progress", 2, broadcaster.clone()),
                ("channel.hype_train.end", 2, broadcaster),
            ],
            // v2 wants both ids, and both are this user's when the person
            // streams from the account that authorized.
            Self::Follow => vec![(
                "channel.follow",
                2,
                json!({ "broadcaster_user_id": user, "moderator_user_id": user }),
            )],
            Self::Live => vec![
                ("stream.online", 1, broadcaster.clone()),
                ("stream.offline", 1, broadcaster),
            ],
            Self::Poll => vec![
                ("channel.poll.begin", 1, broadcaster.clone()),
                ("channel.poll.progress", 1, broadcaster.clone()),
                ("channel.poll.end", 1, broadcaster),
            ],
        }
    }
}

/// A user token as Twitch hands it out. A renewal answers with a new refresh
/// token as well as the access one; the field is optional so a token answer
/// that rotates nothing leaves the held refresh token in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub access: String,
    pub refresh: Option<String>,
    pub expires_ms: u64,
    pub scopes: Vec<String>,
}

/// What the token is, from `/oauth2/validate`: the account, the client it was
/// issued to (every Helix call names that id), and what it may do.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Identity {
    pub id: String,
    pub login: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    /// How long the token that answered has left, from the check: the page
    /// arms its renewal from this, so a session that runs for weeks renews
    /// before anything is refused.
    pub expires_ms: u64,
}

/// `POST /oauth2/token`'s form body to renew a token. A public client needs
/// no secret, which is why the module can do this at all.
pub fn refresh_body(client_id: &str, refresh: &str) -> String {
    form(&[
        ("client_id", client_id),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh),
    ])
}

/// `PATCH /helix/channels` for one broadcaster.
pub fn channel_url(broadcaster: &str) -> String {
    format!("{CHANNELS_URL}?broadcaster_id={}", encode(broadcaster))
}

/// `POST /helix/chat/announcements`'s body: the token's user speaks in their
/// own chat.
pub fn announce_body(broadcaster: &str, moderator: &str, message: &str) -> String {
    json!({
        "broadcaster_id": broadcaster,
        "moderator_id": moderator,
        "message": message,
    })
    .to_string()
}

/// `PATCH /helix/channels`'s body.
pub fn channel_body(title: Option<&str>, game: Option<&str>) -> String {
    let mut body = json!({});
    if let Some(title) = title {
        body["title"] = json!(title);
    }
    if let Some(game) = game {
        body["game_id"] = json!(game);
    }
    body.to_string()
}

/// `POST /helix/channels/commercial`'s body.
pub fn commercial_body(broadcaster: &str, seconds: u64) -> String {
    json!({ "broadcaster_id": broadcaster, "length": seconds }).to_string()
}

/// `POST /helix/polls`'s body.
pub fn poll_body(broadcaster: &str, title: &str, choices: &[String], seconds: u64) -> String {
    json!({
        "broadcaster_id": broadcaster,
        "title": title,
        "choices": choices
            .iter()
            .map(|c| json!({ "title": c }))
            .collect::<Vec<_>>(),
        "duration": seconds,
    })
    .to_string()
}

/// The JSON `POST /helix/chat/messages` takes.
pub fn chat_body(broadcaster: &str, sender: &str, message: &str) -> String {
    json!({
        "broadcaster_id": broadcaster,
        "sender_id": sender,
        "message": message,
    })
    .to_string()
}

/// `POST /helix/clips` takes its arguments as query parameters.
pub fn clip_url(broadcaster: &str, title: &str) -> String {
    let mut url = format!(
        "{CLIP_URL}?broadcaster_id={}&duration={CLIP_SECONDS}",
        encode(broadcaster)
    );
    if !title.is_empty() {
        url.push_str(&format!("&title={}", encode(title)));
    }
    url
}

/// `POST /helix/streams/markers`'s body; `user_id` must be the token's user.
pub fn marker_body(user: &str, description: &str) -> String {
    let mut body = json!({ "user_id": user });
    if !description.is_empty() {
        body["description"] = json!(description);
    }
    body.to_string()
}

/// `application/x-www-form-urlencoded`, with every value percent-encoded.
fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A token answer, or why it isn't one. `refresh` is absent when Twitch
/// renews without rotating it.
pub fn parse_token(body: &Value) -> Result<Token, String> {
    let text = |key: &str| {
        body.get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("{key} missing"))
    };
    Ok(Token {
        access: text("access_token")?,
        refresh: body
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_owned),
        expires_ms: body
            .get("expires_in")
            .and_then(Value::as_u64)
            .unwrap_or(3600)
            * 1000,
        scopes: body
            .get("scope")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// What `/oauth2/validate` says, or why it isn't that.
pub fn parse_validate(body: &Value) -> Result<Identity, String> {
    let text = |key: &str| {
        body.get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("{key} missing"))
    };
    Ok(Identity {
        id: text("user_id")?,
        login: text("login")?,
        client_id: text("client_id")?,
        expires_ms: body
            .get("expires_in")
            .and_then(Value::as_u64)
            .unwrap_or(3600)
            * 1000,
        scopes: body
            .get("scopes")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Twitch's own message for a refused call, when it gives one.
pub fn parse_error(body: &Value) -> Option<String> {
    body.get("message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| body.get("error").and_then(Value::as_str).map(str::to_owned))
}

/// What an EventSub frame is.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Welcome { session: String, keepalive_ms: u64 },
    Keepalive,
    Notification { kind: String, event: Value },
    Reconnect { url: String },
    Revocation { kind: String, status: String },
    Other(String),
}

/// Reads one EventSub text frame.
pub fn parse_frame(text: &str) -> Result<Frame, String> {
    let frame: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let kind = frame
        .get("metadata")
        .and_then(|m| m.get("message_type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload = frame.get("payload").cloned().unwrap_or(Value::Null);
    Ok(match kind {
        "session_welcome" => Frame::Welcome {
            session: payload
                .get("session")
                .and_then(|s| s.get("id"))
                .and_then(Value::as_str)
                .ok_or("no session id")?
                .to_owned(),
            keepalive_ms: payload
                .get("session")
                .and_then(|s| s.get("keepalive_timeout_seconds"))
                .and_then(Value::as_u64)
                .unwrap_or(10)
                * 1000,
        },
        "session_keepalive" => Frame::Keepalive,
        "session_reconnect" => Frame::Reconnect {
            url: payload
                .get("session")
                .and_then(|s| s.get("reconnect_url"))
                .and_then(Value::as_str)
                .ok_or("no reconnect url")?
                .to_owned(),
        },
        "revocation" => Frame::Revocation {
            kind: payload
                .get("subscription")
                .and_then(|s| s.get("type"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            status: payload
                .get("subscription")
                .and_then(|s| s.get("status"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        },
        "notification" => Frame::Notification {
            kind: frame
                .get("metadata")
                .and_then(|m| m.get("subscription_type"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            event: payload.get("event").cloned().unwrap_or(Value::Null),
        },
        other => Frame::Other(other.to_owned()),
    })
}

/// One thing that happened, as the fields carry it: whichever of these an
/// event doesn't have stays empty or zero.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fact {
    /// Its subscription type, which is what a mapping conditions on.
    pub kind: String,
    pub user: String,
    pub message: String,
    pub reward: String,
    pub input: String,
    pub bits: f64,
    pub tier: f64,
    pub months: f64,
    pub viewers: f64,
    pub level: f64,
    pub progress: f64,
    pub goal: f64,
    /// A gifted sub's count.
    pub gifts: f64,
    /// A poll's title, status, leading-or-winning choice and total votes.
    pub title: String,
    pub status: String,
    pub winner: String,
    pub votes: f64,
    /// A stream's own state, from `stream.online` and `stream.offline`.
    pub live: bool,
}

fn text(event: &Value, key: &str) -> String {
    event
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn number(event: &Value, key: &str) -> f64 {
    event.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// Reads `kind`'s event. A type this build doesn't handle is `None`, not an
/// error: Twitch may add one the token's scopes also cover.
pub fn decode(kind: &str, event: &Value) -> Option<Fact> {
    let mut fact = Fact {
        kind: kind.to_owned(),
        ..Fact::default()
    };
    match kind {
        "channel.chat.message" => {
            fact.user = text(event, "chatter_user_name");
            fact.message = event
                .get("message")
                .map(|m| text(m, "text"))
                .unwrap_or_default();
            fact.bits = event.get("cheer").map(|c| number(c, "bits")).unwrap_or(0.0);
            fact.reward = text(event, "channel_points_custom_reward_id");
        }
        "channel.channel_points_custom_reward_redemption.add" => {
            fact.user = text(event, "user_name");
            fact.input = text(event, "user_input");
            fact.reward = event
                .get("reward")
                .map(|r| text(r, "title"))
                .unwrap_or_default();
            fact.progress = event
                .get("reward")
                .map(|r| number(r, "cost"))
                .unwrap_or(0.0);
        }
        "channel.subscribe" => {
            fact.user = text(event, "user_name");
            fact.tier = number(event, "tier");
        }
        "channel.subscription.message" => {
            fact.user = text(event, "user_name");
            fact.tier = number(event, "tier");
            fact.months = number(event, "cumulative_months");
            fact.message = event
                .get("message")
                .map(|m| text(m, "text"))
                .unwrap_or_default();
        }
        "channel.subscription.gift" => {
            fact.user = text(event, "user_name");
            fact.tier = number(event, "tier");
            fact.gifts = number(event, "total");
        }
        "channel.cheer" => {
            fact.user = text(event, "user_name");
            fact.bits = number(event, "bits");
            fact.message = text(event, "message");
        }
        "channel.raid" => {
            fact.user = text(event, "from_broadcaster_user_name");
            fact.viewers = number(event, "viewers");
        }
        "channel.follow" => {
            fact.user = text(event, "user_name");
        }
        "channel.hype_train.begin" | "channel.hype_train.progress" | "channel.hype_train.end" => {
            fact.level = number(event, "level");
            fact.progress = number(event, "progress");
            fact.goal = number(event, "goal");
            // v2 renamed the totals; keep the one that is there.
            fact.bits = number(event, "total");
        }
        "stream.online" => fact.live = true,
        "stream.offline" => fact.live = false,
        "channel.poll.begin" | "channel.poll.progress" | "channel.poll.end" => {
            fact.title = text(event, "title");
            fact.status = text(event, "status");
            let mut best: Option<(&str, f64)> = None;
            let mut total = 0.0;
            for choice in event
                .get("choices")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let votes = number(choice, "votes");
                total += votes;
                if best.is_none_or(|(_, b)| votes > b) {
                    best = Some((
                        choice.get("title").and_then(Value::as_str).unwrap_or(""),
                        votes,
                    ));
                }
            }
            fact.votes = total;
            if let Some((title, _)) = best {
                fact.winner = title.to_owned();
            }
        }
        _ => return None,
    }
    Some(fact)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_check_names_the_client_the_account_and_the_scopes() {
        let identity = parse_validate(&json!({
            "client_id": "wbmytr93xzw8zbg0p1izqyzzc5mbiz",
            "login": "cool_user",
            "scopes": ["user:read:chat", "channel:manage:broadcast"],
            "user_id": "1337",
            "expires_in": 3600
        }))
        .expect("what validate says");
        assert_eq!(identity.id, "1337");
        assert_eq!(identity.login, "cool_user");
        assert_eq!(identity.client_id, "wbmytr93xzw8zbg0p1izqyzzc5mbiz");
        assert_eq!(identity.scopes.len(), 2);

        let missing = parse_validate(&json!({ "login": "cool_user" }));
        assert!(missing.is_err(), "a token without a client names nothing");
    }

    #[test]
    fn a_renewed_token_reads_its_life_and_scopes() {
        let token = parse_token(&json!({
            "access_token": "a",
            "refresh_token": "r",
            "expires_in": 14124,
            "scope": ["user:read:chat"],
            "token_type": "bearer"
        }))
        .expect("a token");
        assert_eq!(token.expires_ms, 14_124_000);
        assert_eq!(token.scopes, ["user:read:chat"]);
    }

    #[test]
    fn the_subscriptions_follow_the_documented_conditions() {
        let chat = Group::Chat.subscriptions("1337");
        assert_eq!(chat[0].0, "channel.chat.message");
        assert_eq!(
            chat[0].2,
            json!({ "broadcaster_user_id": "1337", "user_id": "1337" })
        );
        let follow = Group::Follow.subscriptions("1337");
        assert_eq!(follow[0].1, 2, "follow is version 2");
        assert_eq!(
            follow[0].2,
            json!({ "broadcaster_user_id": "1337", "moderator_user_id": "1337" })
        );
        assert_eq!(
            Group::Raid.subscriptions("1337")[0].2["to_broadcaster_user_id"],
            "1337"
        );
        assert_eq!(
            Group::Subs.subscriptions("1337").len(),
            3,
            "subscribe, resubscribe and gift"
        );
        assert_eq!(Group::Hype.subscriptions("1337").len(), 3);
        for (kind, version, _) in Group::Hype.subscriptions("1337") {
            assert_eq!(version, 2, "{kind} is version 2");
        }
    }

    #[test]
    fn eventsub_frames_read_by_message_type() {
        let welcome = parse_frame(
            r#"{"metadata":{"message_type":"session_welcome"},"payload":{"session":{"id":"s1","keepalive_timeout_seconds":10}}}"#,
        )
        .expect("welcome");
        assert_eq!(
            welcome,
            Frame::Welcome {
                session: "s1".into(),
                keepalive_ms: 10_000
            }
        );
        assert_eq!(
            parse_frame(r#"{"metadata":{"message_type":"session_keepalive"},"payload":{}}"#),
            Ok(Frame::Keepalive)
        );
        let reconnect = parse_frame(
            r#"{"metadata":{"message_type":"session_reconnect"},"payload":{"session":{"reconnect_url":"wss://x/ws"}}}"#,
        )
        .expect("reconnect");
        assert_eq!(
            reconnect,
            Frame::Reconnect {
                url: "wss://x/ws".into()
            }
        );
    }

    #[test]
    fn a_redemption_carries_the_reward_and_what_the_viewer_typed() {
        let event = json!({
            "id": "17fa2df1",
            "broadcaster_user_id": "1337",
            "user_id": "9001",
            "user_login": "cooler_user",
            "user_name": "Cooler_User",
            "user_input": "pogchamp",
            "status": "unfulfilled",
            "reward": { "id": "92af", "title": "Dance", "cost": 100, "prompt": "" },
            "redeemed_at": "2020-07-15T17:16:03.17106713Z"
        });
        let fact = decode(
            "channel.channel_points_custom_reward_redemption.add",
            &event,
        )
        .unwrap();
        assert_eq!(fact.user, "Cooler_User");
        assert_eq!(fact.reward, "Dance");
        assert_eq!(fact.input, "pogchamp");
        assert_eq!(fact.progress, 100.0, "the reward's cost");
    }

    #[test]
    fn a_hype_train_reads_v2s_shape() {
        let event = json!({
            "id": "1b0A",
            "level": 2,
            "total": 137,
            "progress": 137,
            "goal": 500,
            "top_contributions": [],
            "shared_train_participants": null,
            "is_shared_train": false,
            "started_at": "2020-07-15T17:16:03.17106713Z",
            "expires_at": "2020-07-15T17:16:11.17106713Z",
            "type": "regular"
        });
        let fact = decode("channel.hype_train.progress", &event).unwrap();
        assert_eq!(fact.level, 2.0);
        assert_eq!(fact.progress, 137.0);
        assert_eq!(fact.goal, 500.0);
        assert_eq!(fact.bits, 137.0, "the running total");
        assert!(
            decode("channel.shield_mode.begin", &event).is_none(),
            "an event this build doesn't handle is skipped, not guessed"
        );
    }

    #[test]
    fn a_chat_message_reads_its_text_and_cheer() {
        let event = json!({
            "broadcaster_user_id": "1971641",
            "chatter_user_id": "4145994",
            "chatter_user_login": "viewer32",
            "chatter_user_name": "viewer32",
            "message_id": "cc10",
            "message": { "text": "Hi chat", "fragments": [] },
            "color": "#00FF7F",
            "badges": [],
            "message_type": "text",
            "cheer": { "bits": 50 },
            "reply": null,
            "channel_points_custom_reward_id": null
        });
        let fact = decode("channel.chat.message", &event).unwrap();
        assert_eq!(fact.user, "viewer32");
        assert_eq!(fact.message, "Hi chat");
        assert_eq!(fact.bits, 50.0);
        assert_eq!(fact.reward, "", "a null reward id is empty, not \"null\"");
    }

    #[test]
    fn a_chat_send_is_the_body_twitch_documents() {
        let body: Value = serde_json::from_str(&chat_body("1337", "9001", "hello")).expect("json");
        assert_eq!(body["broadcaster_id"], "1337");
        assert_eq!(body["sender_id"], "9001");
        assert_eq!(body["message"], "hello");
        assert_eq!(CHAT_MAX, 500);
    }
}
