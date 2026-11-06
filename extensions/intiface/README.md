# intiface

Devices as `@INTIFACE`. It drives what a local [Intiface](https://intiface.com) server has connected, from mappings and scripts, and from a contact who controls you when you consent.

It is the device-control worked example: a protocol spoken as plain JSON over a socket the client holds, with no client library, and a design where stopping is never left to one mechanism.

## Before you start

Run Intiface Central and start its server. This extension connects to `ws://127.0.0.1:12345` and keeps looking for it, so the order doesn't matter. If you moved the port, remap it in the extension's Settings. Devices are connected through Intiface, on the same computer.

## What it declares

| Part | What |
|---|---|
| namespace | `@INTIFACE`: `connected`, `devices`, `level` |
| methods | `vibrate(intensity, seconds, device?)`, `stop()`, both open to a contact who controls you |
| page | `intiface`: its state, its devices, the strongest allowed, stop, scan, reconnect |
| status | a line in the status bar |
| actions | Vibrate, Stop all devices |
| conditions | Connected, Connected devices |
| network | this computer, TCP 12345 |
| disclosures | `device_control` |

## Using it

`intensity` runs from 0 to 1, in whatever steps the device has. Anything above nothing is at least one step. `seconds` is at most 300, and a longer effect is a script asking again. `device` names one device as the page lists it; left out, it drives every device.

`vibrate` waits until its time is up and answers true. It answers false when it couldn't run, or when `stop` or a later call took over. An output has one holder: whoever asked last.

**Strongest allowed** on the page caps every call, whoever made it, and lowering it reaches what is already running.

| Line | Colour | When |
|---|---|---|
| Intiface isn't running | grey | nothing answers on its port, and it keeps looking |
| Connecting | yellow | a first try, or a server that was there is on its way back |
| Waiting for a device | yellow | Intiface answered and has nothing to drive |
| Connected | green | Intiface answered and has a device. The `connected` field is true only here |
| Intiface refused this client | red | usually an Intiface older than Buttplug spec 4. It holds until Reconnect |

## How it stops

A vibration has no duration in the protocol: it runs until something stops it. So stopping is layered, and these were measured against Intiface Engine 5.0.5 with a simulated device:

1. **Its own time.** Every call has a length, ended by a timer in the module.
2. **The caller.** A flow that stops waiting ends it, within a tick. So do `stop` and a device leaving.
3. **The socket.** When the module stops or faults, or the client exits or is killed, the socket closes and Intiface stops every device. Measured: within a millisecond, for a clean close and for a killed process.
4. **Your limit.** The strongest allowed applies on every path.
5. **The server's watchdog.** When a server sets a ping limit, the module pings at a third of it. Measured: a client that stops pinging has its device stopped at the limit.

**A hung client is the one case left.** A client frozen with its socket open stops nothing on its own, and Intiface ships with its ping limit off. Set a ping limit in Intiface Central to close that.

## Files

| File | What lives there |
|---|---|
| `manifest.rs` | what the module declares |
| `lib.rs` | the flow: every event is one step, one reconcile, and one publish of what changed |
| `protocol.rs` | the wire: messages sent and read, and steps from a level |
| `page.rs` | the page and the status line |

## Build and test

```bash
cargo wasm -p intiface
cargo test -p intiface
```
