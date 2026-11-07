`@TWITCH` is your channel: what happens on it, and what a script can do
there. Sign in on the Twitch page first.

## Reacting to events

Each event is a signal. An `on` runs its block once per event, the same
message twice included, and inside the block the fields describe that
event.

```
on @TWITCH.chatted is "!hug" {
    run @TWITCH.chat("{@TWITCH.user_name} gets a hug")
}
on @TWITCH.cheered >= 100 {
    set @Sparkle to true
}
on @TWITCH.followed {
    run @TWITCH.chat("Welcome, {@TWITCH.user_name}!")
}
```

What each signal carries, what its `is` or `>=` compares:

- `chatted`: a chat message, its first 64 bytes. The whole message is
  `@TWITCH.message`.
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
on @TWITCH.subscribed {
    if @TWITCH.months >= 12 {
        run @TWITCH.chat("A year, {@TWITCH.user_name}!")
    }
}
```

Read a field inside the block of the event it describes: the next event
replaces it.

## Acting on the channel

Each method gives true once Twitch has been asked, and false when it
refuses to ask: not signed in, or an empty message. Why is in
`@TWITCH.error`. A text longer than the reference allows never reaches the
extension: that call reads none.

- `chat(message)`: a chat message, up to 500 bytes.
- `announce(message)`: an announcement, up to 500 bytes.
- `clip()`, `clip(title)`: a clip of the last 30 seconds.
- `marker()`, `marker(description)`: a stream marker.
- `title(title)`: the stream title, up to 140 bytes.
- `commercial()`, `commercial(seconds)`: a commercial, 60 seconds unless
  you say.
- `poll(title, first, second)`: a poll, with up to three more choices and
  then its seconds.

```
on @TWITCH.chatted is "!clip" {
    run @TWITCH.clip("{@TWITCH.user_name} asked")
}
```

## When it listens

Twitch listens for an event only while something uses it: a mapping's
trigger, or a running script. The first events after a script starts may
take a second to arrive. An event the sign-in didn't grant shows on the
Twitch page with the permission it needs.
