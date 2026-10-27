# camera

VRChat's user camera as `@CAMERA`: pose, moves and turns, tracks and positions,
the camera's own settings, a right-panel page and an editor modal.

It is the large worked example of the SDK — everything an extension can declare
except secrets, network and game-log reading — and it drives only
`/usercamera/*` of the client that runs a call: unpiped, the author's VRChat;
`@CAMERA.|Amy|`, Amy's.

## What it declares

| Part | What |
|---|---|
| namespace | `@CAMERA`: fields `pose`, `mode`, plus the pads' speeds; methods below; one signal, `play_ended` |
| OSC | reads and writes `/usercamera/` only |
| page | `camera`: the pads, capture, mode, the settings sliders and toggles |
| modal | the editor: a 3D view of the path, a timeline of keys, the track library |
| actions | 12, from Capture and Play track to the four moves and turns |
| conditions | one, on `mode` |
| store | its library (tracks and positions), as JSON under one key |

## Methods

`move_local` / `move_world` / `turn_local` / `turn_world` (axes, in metres or
degrees), `capture`, `capture_delayed`, `close`, `fly_to` (a pose over seconds),
`fly_path` (a track), `track(name)` and `position(name)` (library data a script
can read), `stop`, `add_key`.

Every method but `track`, `position` and `add_key` is marked for contacts, so a
controller may fire it on a contact's copy of the camera under that contact's
consent. Nothing here touches VRChat's avatar parameters.

## Files

| File | What lives there |
|---|---|
| `manifest.rs` | what the module declares: the `@CAMERA` namespace, the fields and methods, the OSC prefixes, the page, the actions, the condition and the icons |
| `lib.rs` | the flow: the `Extension` implementation, the pose stream, OSC in and out, and the page's and the modal's command routing |
| `motion.rs` | the maths: a heading-frame offset, the three angle changes, a play's interpolation and optics, `PATH_MAX` |
| `library.rs` | tracks and positions: their JSON form, naming, the store |
| `view.rs` | the two views as `Node` trees — the timeline, the 3D scene, the side panel |
| `widgets.rs` | the pieces they are built from: buttons, fields, sliders, the pace lane, the key panel |
| `icons.rs` | the packaged SVGs the views draw |
| `tests.rs` | the behaviour, driven as events and asserted as commands |

## Build and test

```bash
cargo wasm -p camera     # target/wasm32-unknown-unknown/release/camera.wasm
cargo test -p camera
```

Install the module from a file in the client's Extensions window, or publish it
through a repository's index. In the client, the page and the editor are where
its surface is exercised by hand.
