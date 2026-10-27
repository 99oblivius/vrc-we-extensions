# twitch

Twitch as `@TWITCH`: chat, channel point redemptions, subscriptions, cheers,
raids, hype trains, follows, polls and the stream's own live state arrive over
one EventSub WebSocket, and a mapping reacts to them. Sending is seven actions
over Helix.

It is the networked worked example: OAuth with the person's own token, an
authenticated WebSocket, HTTP calls the host performs, and a page that reports
what it is listening for.

## What it declares

| Part | What |
|---|---|
| namespace | `@TWITCH`: fields listed below; one signal per event group; eight conditions |
| page | `twitch`: the token fields, what is being listened for, the last event, the last error |
| actions | `chat`, `clip`, `marker`, `announce`, `title`, `commercial`, `poll` |
| secrets | `auth` (`Bearer <token>`) and `refresh` |
| network | `id.twitch.tv`, `api.twitch.tv`, `eventsub.wss.twitch.tv`, TCP 443 |

## Reacting

Fields: `event`, `user_name`, `message`, `reward`, `input`, `bits`, `tier`,
`months`, `viewers`, `hype_level`, `hype_progress`, `hype_goal`, `poll_title`,
`poll_status`, `poll_winner`, `poll_votes`, `live`, `connected`, `user`,
`error`.

Signals carry the value a mapping matches on, since the signal already says
which event it was: `redeemed` the reward title, `chatted` the message,
`subscribed` the tier, `gifted` how many, `cheered` the bits, `raided` the
viewers, `hype` the level, `polled` a poll's title, `poll_ended` its winner,
`followed`, `went_live`, `went_offline` nothing.

Conditions, for the rule editor: `connected`, `bits`, `hype_level`, `viewers`,
`tier`, `months`, `poll_votes`, `live`. Text matching is a script reading a
field; a condition is a switch or a number.

## Sending

| Action | Scope | Notes |
|---|---|---|
| `chat(message)` | `user:write:chat` | 1–500 bytes |
| `clip(title?)` | `clips:edit` | the last 30 seconds |
| `marker(description?)` | `channel:manage:broadcast` | only while live |
| `announce(message)` | `moderator:manage:announcements` | the channel's own chat |
| `title(title)` | `channel:manage:broadcast` | |
| `commercial(seconds?)` | `channel:edit:commercial` | 30–180 s in 30 s steps |
| `poll(title, first, second, third?, fourth?, fifth?, seconds?)` | `channel:manage:polls` | 2–5 choices, five minutes unless given |

It writes nothing into VRChat: the module has no OSC grant at all.

## The token

No Twitch application is registered for this module and no client id ships in
it. The person pastes a user access token (and optionally a refresh token) into
the masked fields on the page; `/oauth2/validate` reports the client it was
issued to, the account, the scopes and the remaining life; every Helix call then
carries that client id. A renewal posts `grant_type=refresh_token` with the
client id and no secret — a public client has none — and Twitch answers a new
access token *and* a new refresh token, both stored.

A group whose scope the token lacks is not subscribed, and the page names it. An
action refused with 403 names the scope it needed; a 401 renews the token and
sends the action again, once. A refused renewal says what Twitch said and asks
for a new token. The root README has the flow step by step.

## Files

| File | What lives there |
|---|---|
| `manifest.rs` | what the module declares: the `@TWITCH` namespace, its fields, methods and signals, the actions, the conditions and the grants |
| `lib.rs` | the flow: the state machine, the `Extension` implementation, the token renewal, the socket, and the page |
| `protocol.rs` | the wire: URLs, request bodies, the validate token check, EventSub frames, and the decode of each event into a fact |
| `tests` (in `lib.rs`) | the behaviour, driven as events and asserted as commands |

## Build and test

```bash
cargo wasm -p twitch
cargo test -p twitch
```
