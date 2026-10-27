# twitch

Twitch as `@TWITCH`. Chat, channel point redemptions, subscriptions, cheers, raids, hype trains, follows, polls and the stream's own live state arrive over one EventSub WebSocket. Sending is seven actions over Helix.

It is the networked worked example: OAuth with the person's own token, an authenticated socket, HTTP calls the host performs, and a page that reports what it listens for.

## What it declares

| Part | What |
|---|---|
| namespace | `@TWITCH`: the fields below, one signal per event group, eight conditions |
| page | `twitch`: the token fields, what is being listened for, the last event, the last error |
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

No Twitch application is registered for this module, and no client id ships in it. The person pastes a user access token, and optionally a refresh token, into the masked fields on the page.

`/oauth2/validate` reports the client it was issued to, the account, the scopes and the remaining life. Every Helix call then carries that client id.

A renewal posts `grant_type=refresh_token` with the client id and no secret, because a public client has none. Twitch answers a new access token and a new refresh token, and both are stored.

A group whose scope the token lacks is not subscribed, and the page names it. An action refused with 403 names the scope it needed. A 401 renews the token and sends the action again, once. A refused renewal says what Twitch said and asks for a new token.

The flow, in order:

1. **Paste.** The page's fields are stored as keyring secrets. `auth` holds `Bearer <token>`, `refresh` holds the token as it came. The fields are then cleared.
2. **Check.** `GET /oauth2/validate` takes the token alone, with no client id. It answers the client id, the account, the scopes and the time left.
3. **Arm.** The renewal timer is set a minute before that life ends, so a session running for weeks renews before anything is refused.
4. **Renew.** `POST /oauth2/token` with `grant_type=refresh_token`, the discovered client id and the refresh token. A public client has no `client_secret` to send. Twitch answers a new access token and a new refresh token, and both replace the old pair.
5. **Repeat once.** An action that met a 401 is sent again after the renewal. Twitch refused it before doing anything, so it is not done twice. A second refusal is reported.
6. **Ask.** A refused renewal, which is what `invalid_grant` means once a public client's refresh token is 30 days old, shows Twitch's own message and asks for a new token. Nothing is silently retried.

No OAuth crate is used. `twitch_oauth2` and the older `twitch-oauth` are async clients with their own HTTP stack, client secrets and redirect listeners. A sandboxed module has no socket, no clock and no HTTP, because the host makes every request. The six steps above are the whole client.

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
