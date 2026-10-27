# vrc-we-extensions

Extensions for [vrc-WiredExpressions](https://github.com/99oblivius), a desktop app that controls VRChat avatar parameters locally and, with consent, between paired friends.

This repository is the extension system's public half: the SDK an extension is built against, a template to start from, and the first-party extensions, the camera and Twitch. The client ships with this repository added, so an official extension is installed from the Extensions window without typing a URL. The client that hosts them is closed source; what it enforces is specified in its own documentation, and every check runs there whatever repository an extension came from.

## What an extension is

A sandboxed WebAssembly module written in Rust against [`vrc-we-sdk`](sdk). It is a pure state machine: the client feeds it events, it returns commands, and the client carries them out.

- It owns no socket, file, clock, thread or window. Every command it sends is checked against its manifest and its declared shapes before anything happens.
- Its identity is the repository it came from and its name; the author is shown, not part of the identity.
- One memory, no imports, no WASI: it reaches nothing outside itself except through the commands the client acts on.

The client runs it in [wasmi](https://github.com/wasmi-labs/wasmi), an interpreter with fuel metering, one green thread per extension, slices of a dispatch's fuel at a time, so an extension that loops or panics can fail only itself. Repeated faults switch it off.

## Layout

| Path | Is |
|---|---|
| [`sdk/`](sdk) | `vrc-we-sdk`: the boundary types and the checks — manifest, shapes, views, calls — shared by the client and every extension. Apache-2.0. |
| [`extensions/camera/`](extensions/camera) | VRChat's user camera as `@CAMERA`: pose, moves and turns, tracks and positions, the camera's settings, a right-panel page and an editor modal. MIT. |
| [`template/`](template) | A counter extension to start from: one namespace, one page, one action, its own store. MIT. |
| [`extensions/twitch/`](extensions/twitch) | Twitch as `@TWITCH`: chat messages, channel point redemptions, subs, cheers, raids, hype trains and follows over EventSub, with one page for setup. MIT. |

## Start here

Four reads, in this order:

1. [`sdk/README.md`](sdk/README.md) — what the boundary is, what a manifest
   declares, what every limit is, and how the version is kept compatible.
2. [`template/`](template) — the smallest complete extension: one namespace,
   one page, one action, one stored value. Copy it to start.
3. [`extensions/camera/README.md`](extensions/camera/README.md) — a large
   surface: a page, a modal, OSC in and out, a library in the store.
4. [`extensions/twitch/README.md`](extensions/twitch/README.md) — a networked
   one: OAuth with the person's own token, an EventSub socket, Helix calls.

This page covers the repository as a whole; each crate's README covers that
crate's manifest, layout, and build.

## Building

An extension is a `cdylib` for `wasm32-unknown-unknown`, and the SDK is in this workspace, so a clone builds without fetching it:

```bash
cargo wasm -p camera
```

`cargo wasm` is the alias in [`.cargo/config.toml`](.cargo/config.toml), the one place the build spells the target; [`rust-toolchain.toml`](rust-toolchain.toml) has rustup install it, and `[workspace.metadata.vrc-we]` states it for tools that read cargo's own output, which is how `cargo run -p index` builds each crate it lists. The target is `wasm32-unknown-unknown`: `<arch>-<vendor>-<os>`, so "no vendor, no OS" — a bare module with no imports, which is exactly what the client's sandbox runs. (WASI targets would link an ABI the client doesn't provide; `wasm32v1-none` says `os = none` but ships no `std`.)

The module lands at `target/wasm32-unknown-unknown/release/camera.wasm`. In the client, **Settings → Extensions → Install from file** takes it, shows what its manifest grants, and runs it. A change to the SDK ships with the extension that embeds it; the client accepts a module built against its boundary version's range and refuses one outside it.

Authors never commit a built module.

## The camera

The camera drives `/usercamera/*` of the client that runs it: unpiped, the author's VRChat; `@CAMERA.|Amy|`, Amy's. It offers a page with the mode, capture and the hold-to-fly pads, tracks and positions as actions, and an editor with a 3D view and a timeline. It is English only for now. Its own README has the full surface and its file map: [`extensions/camera/README.md`](extensions/camera/README.md).

## The Twitch extension

`twitch` reads a channel's events over one EventSub WebSocket and offers them to mappings as `@TWITCH` fields (`event`, `user_name`, `message`, `reward`, `input`, `bits`, `tier`, `months`, `viewers`, `hype_level`, `hype_progress`, `hype_goal`, `poll_title`, `poll_status`, `poll_winner`, `poll_votes`, `live`, `connected`, `user`, `error`).

- **Reacting** is those fields, a signal per event, and eight conditions the editor can hold to (`connected`, `bits`, `hype_level`, `viewers`, `tier`, `months`, `poll_votes`, `live`). Matching text — a `!dance` command, a redemption's typed input — is a script reading `@TWITCH.message` or `@TWITCH.input`; a condition is a switch or a number, never text.
- **Sending** is seven actions and their methods: `chat` (a message, `user:write:chat`), `clip` (the last 30 seconds, `clips:edit`), `marker` (a VOD marker at this moment, `channel:manage:broadcast`), `announce` (a highlighted message, `moderator:manage:announcements`), `title` (the stream's title, `channel:manage:broadcast`), `commercial` (30 to 180 seconds in 30s steps, `channel:edit:commercial`) and `poll` (a title and two to five choices, `channel:manage:polls`, five minutes unless a script passes another length). Optional values say `(optional)` in the form, so a script may leave them out. It writes nothing into VRChat: the module has no OSC grant at all.
- **Signals carry the value a mapping matches on**, since the group already says which event it was: `redeemed` the reward title, `chatted` the message (64 bytes), `subscribed` the tier, `gifted` how many, `cheered` the bits, `raided` the viewers, `hype` the level, `polled` a poll's title and `poll_ended` its winner, `followed`, `went_live` and `went_offline` nothing. A numeric one matches a numeric condition, a text one equality.
- **Polls** land in `poll_title`, `poll_status`, `poll_winner` and `poll_votes` as well, so a script reads the result and a mapping matches the winner by name. A stream's own `live` state comes from `stream.online`/`stream.offline` — no scope and no polling.
- **Maintenance** — forgetting the token, reconnecting — is on the page alone. Nothing a trigger fires, and no script, can sign you out of Twitch.

- **The token is yours.** This module registers no Twitch application and ships no client id: the person pastes a user access token on the page (masked), and `/oauth2/validate` reports which client issued it, the account, and the scopes it carries. Every Helix call then names that client id, so the token works with its own application's limits and the stricter shared limits of an app access token never apply.

  The flow, in order:
  1. **Paste.** The page's fields are stored as the extension's keyring secrets — `auth` as `Bearer <token>`, `refresh` as it came — and the fields are cleared.
  2. **Check.** `GET /oauth2/validate` with the token alone (no client id): it answers the client id, the account, the scopes and how long the token has left.
  3. **Arm.** The renewal timer is set a minute before that life ends, so a session that runs for weeks renews before anything is refused.
  4. **Renew.** `POST /oauth2/token` with `grant_type=refresh_token`, the discovered client id and the refresh token — no `client_secret`, which a public client does not have. Twitch answers a new access token **and a new refresh token**; both are stored, replacing the old pair.
  5. **Repeat once.** An action that met a 401 is sent again after the renewal. Twitch refused it before doing anything, so it is not done twice; a second refusal is reported rather than renewing again.
  6. **Ask.** A refused renewal (`invalid_grant`, which is what a public client's refresh token gives 30 days after issue) shows Twitch's own message on the page and asks for a new token. Nothing is silently retried.

  **No OAuth crate is used.** `twitch_oauth2` and the older `twitch-oauth` are async clients with their own HTTP stack, client secrets and redirect listeners; a sandboxed module has no socket, no clock and no HTTP — the host makes every request — so the four commands above are the whole client.
- **Scopes are the token's.** A group whose scope the token lacks is not subscribed: the page names it, so a token minted for chat alone shows exactly that. A send that Twitch refuses with 403 names the scope the action needed; a 401 renews the token and retries.
- **It listens to what is read of it.** The client tells the module which of its fields and signals the person's enabled mappings and scripts use (`Event::Watched`, boundary 2), and the module subscribes exactly those groups — a mapping on `redeemed` is a redemption subscription, a condition on `hype_level` is a hype train one. Nothing subscribes "just in case", and the page only reports what is being listened for.
- **Nothing is unsubscribed.** Twitch retires a WebSocket session's subscriptions when the socket closes, so the set only grows for as long as a connection lives, and a subscription whose scope the token carries costs nothing: 0 of the 10-cost budget, and eight of 800 points a minute.
- **One WebSocket**, reopened and resubscribed when Twitch says reconnect or the keepalive goes quiet for two intervals; nothing is replayed by Twitch, so a gap is a gap.

## Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

A change to `vrc-we-sdk`'s boundary — an event, a command, a variant or a field a module built earlier would not read — bumps `BOUNDARY_VERSION` in the SDK. New fields carry `#[serde(default)]` instead.

## Licences

The SDK is Apache-2.0 ([`sdk/LICENSE`](sdk/LICENSE)); the template is MIT with its own copy ([`template/LICENSE`](template/LICENSE)) since it is copied out of this repository, and the extensions here are MIT ([`LICENSE`](LICENSE)). Every built extension embeds the SDK, so its release carries the Apache-2.0 text too, and the client shows an extension's licences.

## Releases

Every tag `v<version>` builds each extension in the index and publishes one release holding `index.json` and the modules it lists. A client adds a repository by that file's URL:

```text
https://github.com/99oblivius/vrc-we-extensions/releases/latest/download/index.json
```

The modules sit beside the index in the same release, which is where a client looks for them; GitHub answers that URL with a redirect to the files' own host, which the client follows (at most five), and each module is checked against the size and SHA-256 the index states before it is installed. The client also refreshes a repository once a day at launch, so a new tag's update appears without asking.

An extension joins the index by declaring itself in its own `Cargo.toml` — nothing is restated in the index, which reads what cargo already knows:

```toml
[package.metadata.vrc-we]
index = true
mature = false
```

`cargo run -p index -- --name community --out dist` writes `dist/index.json` and copies each module beside it, hashed. Its `min_boundary` is the SDK's `BOUNDARY_VERSION`, so a client refuses an extension built for a boundary it doesn't have.

## Status

CI (format, clippy, tests, a wasm build and an index) and the tag-driven release are in place; nothing is published yet. The index the client expects is `official`, the name the first-party repository's listing takes as its source. Installs from a file work today, with the camera and the Twitch extension.
