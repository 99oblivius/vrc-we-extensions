`@CAMERA` drives VRChat's camera: where it is, how it moves, its
settings, and the tracks and positions saved in the camera's editor.

## Where it is

`x`, `y` and `z` are the camera's place in the world, in metres, and
`pitch`, `yaw` and `roll` its angles, in degrees. Set any of them to put
the camera there.

```
set @CAMERA.y to @CAMERA.y + 0.5
set @CAMERA.yaw to 90
```

`mode` is the camera's mode: 0 Off, 1 Photo, 2 Stream, 3 Emoji,
4 Multilayer, 5 Print, 6 Drone.

```
set @CAMERA.mode to 1
on @CAMERA.mode changes {
    set @filming to @CAMERA.mode is 2
}
```

## Moving and turning

`move_local(right, up, forward)` moves along the camera's own axes, and
`move_world(x, y, z)` along the world's, in metres. `turn_local(pitch,
yaw, roll)` and `turn_world(pitch, yaw, roll)` turn by degrees, about the
camera's axes or the world's.

```
run @CAMERA.move_local(0, 0, 1)      # a metre forward
run @CAMERA.turn_world(0, 45, 0)     # 45 degrees round
```

## Photos

`capture()` takes a photo, `capture_delayed()` takes one after VRChat's
timer, and `close()` closes the camera.

```
set @CAMERA.mode to 1
wait 1s
run @CAMERA.capture()
```

## Settings

The camera's sliders and switches are fields of their own, from `zoom`
and `exposure` to `green_screen` and `look_at_me`: the Reference below
lists each with its range. Set one to change it in VRChat.

```
set @CAMERA.zoom to 30
set @CAMERA.green_screen to true
```

A setting reads what was last set on this computer; before that, what
VRChat reported when the extension started, or the setting's default.
Contacts read only the camera's place and mode.

## Tracks and positions

Tracks and positions are made and named in the camera's editor.
`fly_to(pose, seconds)` flies to a position over seconds, and
`fly_path(steps)` plays a track. `position(name)` and `track(name)` give
a saved one by its name, `none` when there is none.

```
run @CAMERA.fly_to(@CAMERA.position("Stage"), 3)
run @CAMERA.fly_path(@CAMERA.track("Intro"))
```

Both wait until the camera arrives or the track ends; `launch` them to go
on without waiting. `stop()` ends every play. A play that runs to its
end sends `play_ended`; one that is stopped doesn't.

```
launch @CAMERA.fly_path(@CAMERA.track("Orbit"))
on @CAMERA.play_ended {
    run @CAMERA.capture()
}
```

`add_key()` adds the camera's place to the track chosen in the editor.

A position is a list of six numbers: x, y, z, pitch, yaw and roll. A
track is a list of steps, each a list of eleven numbers: the move right,
up and forward from the step before, the change of pitch, yaw and roll,
the seconds it takes (0 jumps), then its curve: the handle lengths where
it leaves and where it arrives (0 straight, 1 smooth), and where along
the step (0 to 1) and how fast there, as a multiple of its average pace.

```
set square to [
    [1, 0, 0, 0, 90, 0, 1, 0, 0, 0.5, 1],
    [1, 0, 0, 0, 90, 0, 1, 0, 0, 0.5, 1]
]
run @CAMERA.fly_path(square)
```

## Contacts

Every method but `track`, `position` and `add_key` also reaches a
contact you control: `run @CAMERA.|Amy|.capture()` takes a photo with
Amy's camera while she lets you control her.
