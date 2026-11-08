`@TWITCH` reads what happens on your Twitch channel and acts on it. Sign
in on the Twitch page first.

## Reacting to events

Each event is a signal. An `on` runs its block once per event, the same
message twice included, and inside the block the fields describe that
event.

What each signal carries, what its `is` or `>=` compares:

- `chatted`: a chat message, its text. `@TWITCH.message` is the latest
  text any event carried, empty when it carried none.
- `redeemed`: a channel point redemption, the reward's title. What the
  viewer typed is `@TWITCH.input`.
- `subscribed`: a subscription, its tier: 1000, 2000 or 3000.
- `gifted`: gifted subscriptions, how many.
- `cheered`: a cheer, its bits.
- `raided`: a raid, its viewers.
- `hype`: the hype train starting, moving or ending, its level.
- `polled`: a poll starting or moving, its title.
- `poll_ended`: a poll ending, the winning choice.
- `followed`, `went_live`, `went_offline`: carry nothing.

A message this extension sends to chat never comes back as `chatted`, so a
reply can't answer itself. What you type yourself does.

## Reading an event's own values

A field holds the latest event's value, whatever kind it was. Inside an
`on` block those are its own event's values, from the block's first line
until a newer event arrives. Set the ones you need to variables at the
top of the block, and they keep their values:

```
on @TWITCH.subscribed {
    set who to @TWITCH.user_name
    set months to @TWITCH.months
    run @TWITCH.chat("Thanks {who}, {months} months in!")
}
```

`cheered` compares its bits as a number, and `@TWITCH.bits` holds that same
number. This block answers cheers of 100 bits or more:

```
on @TWITCH.cheered >= 100 {
    set who to @TWITCH.user_name
    set bits to @TWITCH.bits
    run @TWITCH.chat("{who} cheered {bits} bits!")
}
```

During a `wait` or after a `launch`, other events can arrive, and a field
read after it holds the latest one. Variables keep their values, so set
them first: without the first line, `{who}` can be someone else by the
time the chat message is sent.

## A command in chat

`chatted` carries the whole message, so a command is matched as written,
from its first character to its last, upper and lower case kept:

```
on @TWITCH.chatted is "!hug" {
    set who to @TWITCH.user_name
    run @TWITCH.chat("{who} gets a hug")
}
```

The whole message has to be the command. `!hug @someone` is a different
text, and so is `!HUG`, so neither runs this block.

## A redemption

`redeemed` carries the reward's title. What the viewer typed is `input`:

```
on @TWITCH.redeemed is "Dance" {
    set who to @TWITCH.user_name
    set what to @TWITCH.input
    if what is not "" {
        run @TWITCH.chat("{who} redeemed Dance: {what}")
    }
}
```

## Passing values to a recipe

Pass a recipe the values it needs as arguments. Its parameters are
copies, so they keep their values however long it waits:

```
recipe welcome(who, months) {
    if months >= 12 {
        run @TWITCH.announce("Welcome back, {who}!")
    }
    wait 2s
    run @TWITCH.chat("Say hi to {who}")
}

on @TWITCH.subscribed {
    set who to @TWITCH.user_name
    set months to @TWITCH.months
    launch welcome(who, months)
}
```

## An on inside an on

An `on` inside an `on` starts when the outer block reaches it, and stops
when that block ends. Each one reads its own event: the outer `who` stays
the subscriber, while the inner block sets `fan` to the cheerer. A second
subscriber during that minute restarts the outer block, with the new
subscriber as `who`.

```
on @TWITCH.subscribed {
    set who to @TWITCH.user_name
    run @TWITCH.chat("Welcome {who}! First cheer gets a shout.")
    on @TWITCH.cheered >= 500 {
        set fan to @TWITCH.user_name
        set bits to @TWITCH.bits
        run @TWITCH.chat("{fan} cheered {bits} bits for {who}")
    }
    wait 60s
}
```

## Other events

`raided` compares the viewers, `poll_ended` carries the winner, and
`went_live` carries nothing:

```
on @TWITCH.raided >= 10 {
    run @TWITCH.chat("{@TWITCH.user_name} raided with {@TWITCH.viewers}!")
}
on @TWITCH.poll_ended {
    run @TWITCH.announce("The poll winner is {@TWITCH.poll_winner}")
}
on @TWITCH.went_live {
    run @TWITCH.title("Live now")
}
```

## More events

`gifted` carries the number of subs, `hype` the level, `polled` the
title, and `followed` and `went_offline` carry nothing:

```
on @TWITCH.gifted >= 5 {
    set n to @TWITCH.gifted
    run @TWITCH.chat("{@TWITCH.user_name} gifted {n} subs!")
}
on @TWITCH.hype >= 3 {
    set level to @TWITCH.hype
    set @hype to level
}
on @TWITCH.polled {
    set @poll to @TWITCH.poll_title
}
on @TWITCH.followed {
    set @wave to true
}
on @TWITCH.went_offline {
    run @TWITCH.title("Offline")
}
```

## Fields

The fields hold the latest event's details: `user_name` is who did it,
`message` what they wrote, `reward` and `input` a redemption's, `bits`,
`tier`, `months`, `viewers`, the hype train's `hype_level`,
`hype_progress` and `hype_goal`, and the poll's `poll_title`,
`poll_status`, `poll_winner` and `poll_votes`. `event` is Twitch's name for
the latest event's kind.

`connected` is true while every event in use is being received, `live`
while the stream is live, `user` is the account signed in, and `error` the
latest thing that went wrong.

```
if @TWITCH.connected is true and @TWITCH.live is true {
    set @status to "Live as {@TWITCH.user}"
}
```

```
on @TWITCH.subscribed {
    if @TWITCH.months >= 12 {
        run @TWITCH.chat("A year, {@TWITCH.user_name}!")
    }
}
```

Read a field inside the block of the event it describes, or into a name of
your own first: the next event replaces it.

A field holds the latest value. An event is a separate send: two chat
messages with the same text send two `chatted` signals and one `message`
update. `message` holds what the latest event said, empty when it said
nothing.

## Acting on the channel

Each method gives true once the request is sent to Twitch, and false when
it can't be sent: not signed in, or an empty message. Why is in
`@TWITCH.error`. A text longer than the reference allows never reaches the
extension: that call reads none.

- `chat(message)`: a chat message, up to 500 bytes.
- `announce(message)`: an announcement, up to 500 bytes.
- `clip()`, `clip(title)`: a clip of the last 30 seconds.
- `marker()`, `marker(description)`: a stream marker.
- `title(title)`: the stream title, up to 140 bytes.
- `commercial()`, `commercial(seconds)`: a commercial, 60 seconds unless
  given, 30 to 180 in 30 second steps.
- `poll(title, first, second, third, fourth, fifth, seconds)`: a poll of
  two to five choices, then its seconds: 5 minutes unless given, 15 to
  1800. Leave out the choices you don't need; to give seconds with fewer
  choices, pass `""` for the unused ones.

```
on @TWITCH.chatted is "!clip" {
    run @TWITCH.clip("{@TWITCH.user_name} asked")
}
run @TWITCH.marker("chapter two")
run @TWITCH.commercial(60)
run @TWITCH.poll("Shoes?", "Yes", "No")
run @TWITCH.poll("Song?", "Fast", "Slow", "", "", "", 60)
```

A call that fails says why in `@TWITCH.error`, and a later success clears
it:

```
on @TWITCH.error changes {
    if @TWITCH.error is not "" {
        run @TWITCH.chat("Twitch said no: {@TWITCH.error}")
    }
}
```

## When it listens

An event is received only while something uses it: a mapping's trigger,
or a running script. The first events after a script starts may
take a second to arrive. An event the sign-in didn't grant shows on the
Twitch page with the permission it needs.
