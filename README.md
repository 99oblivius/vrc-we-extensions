# vrc-we-extensions

Extensions for [vrc-WiredExpressions](https://github.com/99oblivius), a desktop app that controls VRChat avatar parameters locally and, with consent, between paired friends.

This repository holds the SDK an extension is built against, a template to start from, and the first-party extensions. The client itself stays closed source. Its own documentation states what it enforces, and it enforces it whatever repository an extension came from.

## What an extension is

A sandboxed WebAssembly module written in Rust against [`vrc-we-sdk`](sdk). It is a pure state machine. The client sends events, the module returns commands, and the client carries them out.

- It owns no socket, file, clock, thread or window. The client checks every command against the manifest and the declared shapes before acting on it.
- Its identity is its repository and its name. The author is shown, never part of the identity.
- It has one memory, no imports and no WASI. Its commands are the only way out.

The client runs it in [wasmi](https://github.com/wasmi-labs/wasmi). Fuel metering, one green thread per extension, and dispatch in fuel slices keep a loop or a panic inside that extension. Repeated faults switch it off.

## What is here

| Path | Is |
|---|---|
| [`sdk/`](sdk) | `vrc-we-sdk`: the boundary types and the checks, shared by the client and every extension. Apache-2.0. |
| [`template/`](template) | a counter extension to copy and rename: one namespace, one page, one action, one stored value. MIT. |
| [`tools/index/`](tools/index) | the index generator a release is made with. |
| [`extensions/`](extensions) | the extensions this repository publishes, one directory each. |

## Extensions

| Extension | What it does | Read |
|---|---|---|
| `camera` | VRChat's user camera as `@CAMERA`: pose, moves and turns, tracks and positions, its settings, a page and an editor modal. | [`extensions/camera/README.md`](extensions/camera/README.md) |
| `twitch` | Twitch as `@TWITCH`: chat, redemptions, subs, cheers, raids, hype trains, follows, polls and live state over EventSub, plus seven send actions. | [`extensions/twitch/README.md`](extensions/twitch/README.md) |

Each extension README describes its surface, its files, and how to build and test it. This page grows by one row per extension and nothing else.

## Start here

1. [`sdk/README.md`](sdk/README.md): the boundary, what a manifest declares, every limit, and how the version stays compatible.
2. [`template/`](template): the smallest complete extension. Copy it to start.
3. An extension README: a large surface in `camera`, a networked one in `twitch`.

## Building

An extension is a `cdylib` for `wasm32-unknown-unknown`. The SDK lives in this workspace, so a clone builds without fetching it:

```bash
cargo wasm -p camera
```

`cargo wasm` is the alias in [`.cargo/config.toml`](.cargo/config.toml), the one place the build spells the target. [`rust-toolchain.toml`](rust-toolchain.toml) has rustup install it, and `[workspace.metadata.vrc-we]` states it for tools that read cargo's output, which is how `cargo run -p index` builds each crate it lists.

The target is `wasm32-unknown-unknown`, so no vendor and no OS. The result is a bare module with no imports, which is what the client's sandbox runs. WASI targets link an ABI the client does not provide, and `wasm32v1-none` ships no `std`.

The module lands at `target/wasm32-unknown-unknown/release/camera.wasm`. In the client, **Settings → Extensions → Install from file** takes it, shows what its manifest grants, and runs it. An SDK change ships with the extension that embeds it. The client accepts a module inside the boundary range and refuses one outside it.

Authors never commit a built module.

## Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

A change to the boundary, such as a new event, command, variant or field that an older module would not read, bumps `BOUNDARY_VERSION` in the SDK. New fields carry `#[serde(default)]` and leave it alone.

## Licences

The SDK is Apache-2.0 ([`sdk/LICENSE`](sdk/LICENSE)). The template is MIT with its own copy ([`template/LICENSE`](template/LICENSE)), since it leaves this repository when copied. The extensions here are MIT ([`LICENSE`](LICENSE)). Every built extension embeds the SDK, so each release carries the Apache-2.0 text too, and the client shows an extension's licences.

## Releases

Every tag `v<version>` builds each extension in the index and publishes one release holding `index.json` and the modules it lists. A client adds a repository by that file's URL:

```text
https://github.com/99oblivius/vrc-we-extensions/releases/latest/download/index.json
```

It is the URL the client's add dialog starts from, so the first-party repository is two presses away and still the person's choice.

The modules sit beside the index in the same release, and the client looks for them there. GitHub answers that URL with a redirect to the files' own host, which the client follows up to five times.

Each module is checked against the size and SHA-256 the index states before it is installed. The client refreshes a repository once a day at launch, so a new tag appears without asking.

An extension joins the index by declaring itself in its own `Cargo.toml`. Nothing is restated, because the index reads what cargo already knows:

```toml
[package.metadata.vrc-we]
index = true
disclosures = ["account"]
```

`disclosures` is the author's own list: what the extension can do that the manifest's grants do not already say (`device_control` for one that drives hardware on your network, `account` for one that needs an account with a service it connects to, or any identifier you choose). The client prints each one as written — its own sentence for those two, your words for anything else — beside the extension's details before it installs, and summarises a repository's set when the repository is added.

`cargo run -p index -- --name vrc-we-extensions --out dist` writes `dist/index.json` and copies each module beside it, hashed. Its `min_boundary` is the SDK's `BOUNDARY_VERSION`, so a client refuses an extension built for a boundary it does not have.

## Listing policy

What may be listed here, what an extension must declare, what listing does and
does not mean, and what the store sees of it: [POLICY.md](POLICY.md).

## Status

CI runs format, clippy, tests, a wasm build and an index. The tag-driven release publishes them. `v1.1.0` carries the `vrc-we-extensions` index with the camera and Twitch.
