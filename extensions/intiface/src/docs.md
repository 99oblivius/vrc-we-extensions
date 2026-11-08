`@INTIFACE` drives the devices connected to Intiface Central on this
computer. Start Intiface's server first; the extension keeps looking for
it, so the order doesn't matter.

## Vibrating

`vibrate(intensity, seconds)` runs every device at `intensity`, from 0
to 1, for `seconds`, at most 300. Any intensity above 0 is at least the
device's lowest step. Name one device, as the Intiface page lists it, to
drive only that one.

```
run @INTIFACE.vibrate(0.5, 2)
run @INTIFACE.vibrate(1, 0.5, "Lush 3")
```

`vibrate` waits until its time is up and gives true. It gives false when
it couldn't run, or when `stop` or a later call took over: a device
follows whoever asked last. `launch` it to go on without waiting.

```
launch @INTIFACE.vibrate(0.3, 10)
wait 2s
run @INTIFACE.vibrate(0.8, 1)    # takes over for a second
```

`stop()` stops every device at once.

```
on @stop_all is true {
    run @INTIFACE.stop()
}
```

## A pattern

A longer or changing effect is a script asking again:

```
repeat 3 times {
    run @INTIFACE.vibrate(1, 0.5)
    wait 0.5s
}
every 1s until @done is true {
    launch @INTIFACE.vibrate(@level_in, 1)
}
```

## Fields

- `connected`: true while Intiface answers and has a device.
- `devices`: how many devices Intiface has connected.
- `level`: the strongest vibration running, from 0 to 1.

```
if @INTIFACE.connected is true {
    run @INTIFACE.vibrate(0.4, 1)
}
on @INTIFACE.devices changes {
    set @toys to @INTIFACE.devices
}
```

## Limits and stopping

The page's **Strongest allowed** caps every call, from scripts, actions
and contacts alike, and lowering it reaches what is already running.

A vibration ends when its time is up, when the flow that asked stops
waiting, at `stop()`, or when the device leaves. If the extension or the
app stops, Intiface stops every device.

## Contacts

`vibrate` and `stop` also reach a contact you control:
`run @INTIFACE.|Amy|.vibrate(0.5, 2)` drives Amy's devices while she
lets you control her. A contact who controls you can do the same to
yours, and your Strongest allowed still applies.
