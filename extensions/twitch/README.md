# twitch

Twitch as `@TWITCH`. Chat, channel point redemptions, subscriptions, cheers, raids, hype trains, follows, polls and the stream's own live state arrive over one EventSub WebSocket. Sending is seven actions over Helix.

It is the networked worked example: OAuth through Twitch's device grant with the person's own token, an authenticated socket, HTTP calls the host performs, and a page that reports what it listens for.

## What it declares

| Part | What |
|---|---|
| namespace | `@TWITCH`: the fields below, one signal per event group, eight conditions |
| page | `twitch`: the connect link, what is being listened for, the last event, the last error |
| actions | `chat`, `clip`, `marker`, `announce`, `title`, `commercial`, `poll` |
| secrets | `auth` (`Bearer <token>`) and `refresh` |
| network | `id.twitch.tv`, `api.twitch.tv`, `eventsub.wss.twitch.tv`, TCP 443 |

## Reacting

Fields: `event`, `user_name`, `message`, `reward`, `input`, `bits`, `tier`, `months`, `viewers`, `hype_level`, `hype_progress`, `hype_goal`, `poll_title`, `poll_status`, `poll_winner`, `poll_votes`, `live`, `connected`, `user`, `error`.

Signals carry the value a mapping matches on, and the signal name already says which event it was. `redeemed` carries the reward title. `chatted` the message. `subscribed` the tier. `gifted` how many. `cheered` the bits. `raided` the viewers. `hype` the level. `polled` a poll's title. `poll_ended` its winner. `followed`, `went_live` and `went_offline` carry nothing.

Conditions for the rule editor: `connected`, `bits`, `hype_level`, `viewers`, `tier`, `months`, `poll_votes`, `live`. Text matching is a script reading a field. A condition is a switch or a number.

## Sending

| Action | Scope | Notes |
|---|---|---|
| `chat(message)` | `user:write:chat` | 1 to 500 bytes |
| `clip(title?)` | `clips:edit` | the last 30 seconds |
| `marker(description?)` | `channel:manage:broadcast` | only while live |
| `announce(message)` | `moderator:manage:announcements` | the channel's own chat |
| `title(title)` | `channel:manage:broadcast` | |
| `commercial(seconds?)` | `channel:edit:commercial` | 30 to 180 seconds, in 30 second steps |
| `poll(title, first, second, third?, fourth?, fifth?, seconds?)` | `channel:manage:polls` | 2 to 5 choices, five minutes unless given |

It writes nothing into VRChat. The module has no OSC grant at all.

## The token

The module ships one application's client id (`CLIENT_ID`), which Twitch treats as public: it travels in every authorize URL and is not a secret. A fork must register its own and replace it; Twitch forbids sharing one client id among applications.

The sign-in is the **device authorization grant** for a public client — the only secret-free flow that also yields a refresh token. Nothing is typed into the client and no redirect URI is involved:

1. **Ask.** `POST /oauth2/device` with the client id and every scope the module can need. Twitch answers a `device_code` this build polls, an `interval`, an expiry, and a `verification_uri` **with the user code already in it**.
2. **Approve.** The page shows that address as a link; Twitch asks the person to sign in and approve the client there.
3. **Poll.** `POST /oauth2/token` with `grant_type=urn:ietf:params:oauth:grant-type:device_code` at the interval Twitch gave, until it answers the pair — or `authorization_pending` (ask again), `slow_down` (widen the interval), or an expiry (start over).
4. **Keep.** `auth` holds `Bearer <access>`; `refresh` holds the one-time refresh token.
5. **Name.** `GET /oauth2/validate` reports the account, the scopes and the remaining life; the renewal timer is set a minute before it ends.
6. **Renew.** `POST /oauth2/token` with `grant_type=refresh_token` and no secret. Twitch rotates the pair, and both halves replace the old ones. A refresh token unused for 30 days expires, and the page asks the person to connect again: one click.

A group whose scope the token lacks is not subscribed, and the page names it. An action refused with 403 names the scope it needed. A 401 renews the token and sends the action again, once. A refused renewal says what Twitch said and asks to connect again.

No OAuth crate is used, and none would fit: `twitch_oauth2` and the older `twitch-oauth` are async clients with their own HTTP stack, client secrets and redirect listeners. A sandboxed module has no socket, no clock and no HTTP of its own, because the host makes every request. The flow above is the whole client.

## Files

| File | What lives there |
|---|---|
| `manifest.rs` | what the module declares: the `@TWITCH` namespace, fields, methods, signals, actions, conditions and grants |
| `lib.rs` | the flow: the state machine, the `Extension` implementation, token renewal, the socket, the page |
| `protocol.rs` | the wire: URLs, request bodies, the token check, EventSub frames, and the decode of each event into a fact |
| `tests` (in `lib.rs`) | the behaviour, driven as events and asserted as commands |

## Build and test

```bash
cargo wasm -p twitch
cargo test -p twitch
```
