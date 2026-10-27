# camera

VRChat's user camera as `@CAMERA`. Pose, moves and turns, tracks and positions, the camera's own settings, a right-panel page and an editor modal.

It is the large worked example: everything an extension can declare except secrets, network and log reading. It drives `/usercamera/*` of the client that runs a call. Unpiped, that is the author's VRChat. `@CAMERA.|Amy|` drives Amy's.

## What it declares

| Part | What |
|---|---|
| namespace | `@CAMERA`: fields `pose`, `mode` and the pads' speeds, the methods below, and one signal, `play_ended` |
| OSC | reads and writes `/usercamera/` only |
| page | `camera`: the pads, capture, mode, the settings sliders and toggles |
| modal | the editor: a 3D view of the path, a timeline of keys, the track library |
| actions | 12, from Capture and Play track to the four moves and turns |
| conditions | one, on `mode` |
| store | its library of tracks and positions, as JSON under one key |

## Methods

`move_local`, `move_world`, `turn_local`, `turn_world` (axes, in metres or degrees), `capture`, `capture_delayed`, `close`, `fly_to` (a pose over seconds), `fly_path` (a track), `track(name)` and `position(name)` (library data a script can read), `stop`, `add_key`.

Every method except `track`, `position` and `add_key` is marked for contacts. A controller may fire those on a contact's copy of the camera, with that contact's consent. Nothing here touches VRChat's avatar parameters.

## Files

| File | What lives there |
|---|---|
| `lib.rs` | the flow: the `Extension` implementation, the pose stream, OSC in and out, and command routing |
| `manifest.rs` | what the module declares: the namespace, fields, methods, OSC prefixes, page, actions, condition and icons |
| `motion.rs` | the maths: a heading-frame offset, the three angle changes, a play's interpolation and optics, `PATH_MAX` |
| `library.rs` | tracks and positions: their JSON form, naming, the store |
| `view.rs` | the two views as `Node` trees: the timeline, the 3D scene, the side panel |
| `widgets.rs` | the pieces they are built from: buttons, fields, sliders, the pace lane, the key panel |
| `icons.rs` | the packaged SVGs the views draw |
| `tests.rs` | the behaviour, driven as events and asserted as commands |

## Build and test

```bash
cargo wasm -p camera     # target/wasm32-unknown-unknown/release/camera.wasm
cargo test -p camera
```

Install the module from a file in the client's Extensions window, or publish it through a repository's index. The page and the editor are where its surface is exercised by hand.
