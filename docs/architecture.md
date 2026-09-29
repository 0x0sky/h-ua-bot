# Architecture

## Shape

```text
                         ┌──────────────────────────── h-ua-bot ─────────────────────────────┐
 t.me/s/<channel> ──────▶│ Ingest ──▶ Normalizer ──▶ Relay ──▶ Messenger port ──▶ Telegram   │──▶ person
   (public preview)      │  cursor     (prism-signal)   │  ▲                                  │
                         │                              ▼  │                                  │
                         │                           Store port ──▶ SQLite                    │
                         │ Conversation ◀── events ── Messenger port ◀── Telegram ◀───────── │◀── person
                         └────────────────────────────────────────────────────────────────────┘
```

Two loops share one store. The **source loop** reads a channel, turns each post into readings,
and relays them. The **client loop** reads what people send and answers.

## Ownership

| Concern | Owner |
| --- | --- |
| Reading a channel's public preview | `prism-signal-source-telegram` |
| Post text to threats, places, and roles | `prism-signal-normalize` |
| Who is concerned, what is sent, when, and how often | `h-ua-core` |
| Wording, in Ukrainian | `h-ua-core::message`, `h-ua-core::conversation` |
| Talking to Telegram | `h-ua-telegram` |
| Keeping subscriptions and sent alerts | `h-ua-store` |

`h-ua-core` depends on no network, database, or messenger crate. It names a `Messenger` and a
`Store` as ports and never an implementation. A second messenger is a new crate implementing
`Messenger`; nothing in the core changes.

## Relevance

A reading concerns a person when all of these hold:

1. it reports a **threat** of a kind they follow (three categories: drones, bombs, missiles, the
   last covering cruise, ballistic, and unspecified);
2. one of its places is a **target**, or is **nearby** and they asked for nearby warnings;
   `origin` and `mention` places never concern anyone, since a launch site is not the danger
   to people beside it;
3. their cell centre is within the place's reach (5 to 20 km, set by the gazetteer) plus 4 km.
   The 4 km covers rounding a position to a cell, whose corners are up to 3.7 km from its
   centre; a test walks people round a place at 0.99 of its reach to prove none is missed.

## Not saying too much, and not saying too little

- **Once.** An alert is recorded per person, post, category, and place. The same post is never
  sent twice, and the same kind of threat at the same place is not repeated within the cool-down
  (default 3 minutes): the channel posts several updates about one wave.
- **Not stale.** A post older than the maximum age (15 minutes) is read and never sent, so a
  restart cannot wake anyone with a threat that is long over.
- **A failed send is not recorded**, so the next post can still reach the person. A blocked bot is
  forgotten. A rate-limited send waits and retries once.

## Not saying "all clear" when it is not

A false all-clear is the worst message this bot can send, so several layers guard it. All were
found by replaying 495 real posts and reading every all-clear a resident of Kyiv would have got.

1. **The reader** treats only explicit words (`минус`, `отбой`, `відбій`) as an all-clear. An
   interception (`сбито`), a lost track (`не фиксируется`), and no further launches (`больше не
   было`) are not. `до отбоя` (until the all-clear) is not one either.
2. **A bare all-clear** that names neither a kind nor a place is ignored. `по угрозе от МиГ-31К
   пока минуса` must not end a drone alert.
3. **A contradicted all-clear** is held back. If the same post calls a threat off and reports
   another of the same category still standing, nobody is told: `минус по ракетам … угроза
   баллистики пока актуальна`. With places named, the standing threat must touch one of them.
4. **Only those alerted** get an all-clear, within a window (default 1 hour), for the kind and
   places it names. A person who never got the alert is not told it ended.
5. **The wording** says the source reports it and that it is not an official all-clear.

The price is a missed all-clear now and then; the safe failure is the person keeps being careful.

## What is kept

| Kept | Never kept |
| --- | --- |
| Chat identifier | Coordinates |
| Grid cell, resolution 6 | Name, username |
| Kinds chosen, nearby on or off | Message text |
| Alerts sent: person, post, category, place, time (24 h) | What a person typed |
| Where each source was last read | |

`/stop` deletes a person's rows. The database file is created readable by its owner only.

## Ecosystem

In the Prism ecosystem clients talk to `prism-hub`, which owns subscriptions and delivery. This
repository is a self-contained prototype that does not: it is built directly on `prism-signal`
with its own store. The decision is to move behind the hub, and
[`hub-integration.md`](hub-integration.md) says how and in what order. The ports (`Store`,
`Messenger`) are the seam the move follows.

## Known limits

- **Regions.** A report about `Киевская область` reaches nobody. Regions need boundaries, and the
  reader deliberately does not map a region to its capital.
- **Places the gazetteer lacks.** They are reported by the reader as unresolved and dropped.
  Coverage grows by adding places to `prism-signal`.
- **A static position goes stale.** Someone who moves without a live location keeps getting alerts
  for where they were. `/status` shows the place; a reminder to refresh is not built.
- **One process.** Long polling and SQLite assume a single instance. Running two would split
  updates between them.
- **Sequential sends.** Alerts go out one by one, so the last person in a large audience waits
  for everyone before them: at roughly a tenth of a second per send, a thousand people is
  minutes, not seconds. Bounded concurrency under Telegram's rate limit is the first thing to add
  before the audience grows.
- **A source can be wrong, late, or silent.** The bot relays it and says so.
- **Telegram is not tested live** from the development environment, which cannot reach the Bot
  API. The client is tested against a scripted transport.
