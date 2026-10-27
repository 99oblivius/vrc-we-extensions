# vrc-we-sdk

The boundary between [vrc-WiredExpressions](https://github.com/99oblivius) and its extensions. It holds the types an extension declares itself with, the events it is sent, the commands it answers with, and the checks the client holds every one of them to.

It is data and validation only. No IO, no runtime, no client code. An extension compiles against exactly what the client enforces.

## The shape of one call

```
client                                   module (wasm32-unknown-unknown)
  |  Event (JSON) ------------------------>  Extension::handle
  |                                            |-> Vec<Command> (JSON)
  |  <-----------------------------------------+   the host carries them out
```

- The module exports two functions the host calls: `vrcwe_alloc(len) -> ptr` and `vrcwe_handle(ptr, len) -> packed`. `vrc_we_sdk::export!(YourType)` writes both, so a crate becomes an extension by implementing `Extension` and calling the macro.
- One event in, a list of commands out, per call. A module never blocks, never reads a clock, and opens nothing. Every effect is a command the host performs after checking it against the manifest.
- The module is a pure state machine. State that must survive a restart goes through `Command::Store`. State that must survive a reinstall lives in the client's keyring through `Command::StoreSecret`.

## What a module declares

`Manifest` is the whole surface. The client checks it before an install and holds every runtime call to it:

| Field | What it says |
|---|---|
| `name`, `author`, `version` | identity: the repository plus the name is the extension, and the author is shown |
| `namespace` | the Wex namespace it provides: fields (readable, some writable and shared), methods (callable from scripts and, if marked, from controllers), signals (what a rule can trigger on) |
| `osc_read` / `osc_write` | the OSC address prefixes it receives and may send, never under `/avatar/` |
| `pages`, `actions`, `conditions` | its right-panel page, the actions a mapping offers, the conditions a rule can hold to |
| `secrets` | the keyring slots it may keep, by name |
| `notifies` | it may post notifications |
| `network` | the hosts, ports and protocols it may reach, and what it may listen on |
| `log_prefixes` | the game-log lines it may read, which name players |
| `contacts` | it knows the contacts the person controls and may fire its contact methods on their copy |
| `icons` | packaged SVGs its views may draw |

Anything outside the manifest is refused at the boundary.

## Limits

Every count and size has a constant in this crate, and the client enforces it:

| Constant | Value | Bounds |
|---|---|---|
| `BOUNDARY_VERSION` | 2 | the boundary this SDK speaks |
| `TEXT_MAX` / `LIST_MAX` / `VALUES_MAX` | 4 KiB / 1024 / 16384 | a value, a list, one call's values |
| `FIELDS_MAX` / `METHODS_MAX` / `SIGNALS_MAX` / `PARAMS_MAX` | 256 / 256 / 64 / 16 | a namespace |
| `ICONS_MAX` / `ICON_MAX` | 64 / 8 KiB | packaged SVGs |
| `OSC_PREFIXES_MAX` / `OSC_MESSAGE_MAX` | 16 / 4 KiB | OSC |
| `NET_GRANTS_MAX` / `NET_BODY_MAX` / `NET_OPEN_MAX` | 16 / 1 MiB / 16 | network |
| `STORE_KEYS_MAX` / `STORE_VALUE_MAX` / `STORE_MAX` | 1024 / 1 MiB / 64 MiB | the store |
| `SECRETS_MAX` / `SECRET_MAX` | 16 / 4 KiB | keyring |
| `CALLS_MAX` / `CONTACT_CALLS_MAX` | 256 / 64 | calls in flight |

The client's sandbox adds what a module cannot lift: a memory cap, fuel slices between which other work runs, and one green thread per extension.

## Versioning

`BOUNDARY_VERSION` rises when a change could break a module built earlier, such as a removed field or a changed meaning. New fields carry `#[serde(default)]` and leave it alone, so a module built at 1 still loads at 2.

A repository's index states the oldest boundary it needs (`min_boundary`). The client refuses an extension newer than it can run, naming both.

## Files

| File | What lives there |
|---|---|
| `value.rs` | `Value`, `Type`, `Shape`, `Range`: what a field, a parameter or a result carries, and the limits a shape is held to |
| `manifest.rs` | `Manifest` and everything it declares: namespace, signal, field, method, param, action, condition, page, icon, contact |
| `node.rs` | the view tree (`Node`, `Mark`, `Key`, `Lane`) and the events widgets send back (`UiEvent`) |
| `osc.rs` | one OSC message in and out |
| `log.rs` | the game-log prefixes an extension may read |
| `event.rs` | what the client sends a module |
| `command.rs` | what a module answers with, and the values a rule or script reads |
| `net.rs` | grants, endpoints, bodies, headers |
| `validate.rs` | the rules: name and label predicates, `Manifest::validate`, `check_call`, `check_write`, shape checks |
| `lib.rs` | the `Extension` trait, `export!`, the boundary version, and the tests |

## Writing an extension

Start from [`template/`](../template). It has one namespace, one page, one action, one stored value, and tests. Then read [`extensions/camera/`](../extensions/camera) for a large surface (pages, a modal, OSC, a library in the store) and [`extensions/twitch/`](../extensions/twitch) for a networked one (OAuth, an EventSub socket, Helix calls).

The loop is the same for all three:

```bash
cargo wasm -p template          # build the module
cargo test -p template          # drive it with events, assert the commands
cargo run -p index -- --name official --out dist   # hash it into an index
```
