# Architecture

## Shape

```text
 public channel                          prism-hub                                  h-ua-bot
 (t.me/s/<name>)                                                                 ┌──────────────┐
      │                 ┌──────────────────────────────────────────────┐        │              │
      └── collector ───▶│ signal loop ─▶ prism-signal-runtime          │        │ Conversation │◀── person
          (poll)        │      │          reads, fuses, covers          │        │      │       │     (Telegram)
                        │      ▼                                        │        │      ▼       │
                        │ fan-out by cell ◀── subscriptions ◀───────────┼── API ─┤ HubSubscriptions
                        │      │                                        │        │              │
                        │      ▼                                        │        │              │
                        │ outbox ─▶ Porter ─▶ delivery worker ──────────┼─ POST ▶│ /api/v1/delivery
                        └──────────────────────────────────────────────┘        │      │       │
                                                                                 │      ▼       │
                                                                                 │  Telegram ───┼──▶ person
                                                                                 └──────────────┘
```

The bot is a **client**. It turns Telegram into hub API calls and hub deliveries into Telegram
messages, and holds no subscription, source, or alert logic. Two things flow through it:

- **What a person says** (a command, a position) is answered by the `Conversation` and, when it
  changes what they asked for, saved to the hub as a subscription.
- **What the hub queued** arrives at the bot's delivery endpoint and is sent to the chat.

## Ownership

| Concern | Owner |
| --- | --- |
| Reading a channel, judging posts, places and roles, fusing them into assessments | `prism-signal` |
| Windows, whom to tell, once-only, cool-down, retractions, queueing | `prism-hub` |
| The words of an alert or a retraction, in Ukrainian | `prism-porter` (`signal.alert`) |
| The words of the conversation | `h-ua-core::conversation`, `h-ua-core::message` |
| Turning a position into a coarse cell | `h-ua-core::geo` |
| The hub's API as a client sees it | `h-ua-hub` |
| Talking to Telegram | `h-ua-telegram` |
| Receiving deliveries, and composing it all | `h-ua-bot` |

`h-ua-core` depends on no network, database, or messenger crate. It names a `Messenger` and a
`Subscriptions` as ports and never an implementation.

## What the bot asks of the hub

The first time a person has something to save, the bot makes them known and binds their chat, then
saves. Every call is idempotent, and later saves are one call.

| Call | Why |
| --- | --- |
| `POST /api/v1/actors/onboard` | the person gets an identity and a personal workspace |
| `POST /api/v1/bot-instances/personal/status` | makes sure this bot has an instance in that workspace |
| `POST /api/v1/telegram/surfaces/bind` | the person's chat becomes the destination of the workspace's `alerts` channel |
| `POST /api/v1/alert-subscriptions/personal/save` | a cell, the kinds, the nearby switch |
| `.../status`, `.../clear` | `/status`, `/stop` |

A person is a Telegram chat id under the provider `telegram`, scoped to this bot. The bot never
handles an internal hub identifier.

A subscription always has a position, because the hub holds none without one. So `/kinds` and
`/nearby` before a position is shared ask for the position first, and change nothing.

## Receiving deliveries

`POST /api/v1/delivery` accepts what the hub's delivery gateway sends: a chat, an optional topic,
the text, and an idempotency key, with the shared secret in `X-Prism-Bot-Delivery-Secret`.

- The secret is compared in time that does not depend on where it differs. Without it nothing is
  sent.
- The text is sent as it is, at most 4096 characters. It is never inspected or changed.
- A key already delivered gets the first answer and sends nothing again. This is kept in memory,
  so a restart forgets it: a hub retry that crosses a restart can send a message twice.
- Telegram's refusals map to what the hub does with them. A blocked chat or a rejected message is
  a `400` and is not retried; a rate limit is a `429` with `retry_after_seconds`; Telegram being
  down is a `502` and is retried.
- Only one delivery is in flight at a time, so a retry sees the first attempt's outcome.

## What is kept

The bot keeps nothing on disk.

| The hub keeps | Never kept, by the bot or the hub |
| --- | --- |
| A technical identifier of the chat | Coordinates |
| Grid cell, resolution 6 (about 3 km) | Name, username |
| Kinds chosen, nearby on or off | What a person typed |
| Which reports a person was told about (24 h) | |

`/stop` deletes the subscription, the cell, and the log of what the person was told. The chat
identifier and the workspace stay in the hub; without a subscription nothing is sent.

## Not saying "all clear" when it is not

A false all-clear is the worst message this can send. The guards live where the decisions are:

1. **The reader** (`prism-signal`) treats only explicit words as an all-clear, refuses a bare
   all-clear that names neither a kind nor a place, and refuses one contradicted by the same post.
2. **Fusion** retracts only the same class of threat at the same place, and an expiry is not a
   retraction.
3. **The hub** sends an expiry as nothing, and a retraction only to people who were told the
   alert, naming other reports that still cover them.
4. **Porter** words a retraction as "the source wrote that the threat was called off", says it is
   not official, and never says a place is safe.
5. **This bot** adds the standing notice to its welcome and `/status`: unofficial, and no message
   does not mean safe.

## Failure

- **The hub is unreachable** when someone saves. The bot says nothing was saved and to try again,
  rather than confirming a choice that was lost. A live-position update fails silently, and the
  next one retries.
- **The hub is unreachable** when it should deliver. The hub's outbox retries.
- **The bot is down.** Deliveries fail and the hub retries them until its own limits.
- **The source goes quiet.** The hub logs it and users are not told. Silence from the bot is not
  safety, and the welcome says so.

## Known limits

- **Regions.** A report about `Киевская область` reaches nobody. Regions need boundaries, and the
  reader deliberately does not map a region to its capital.
- **Places the gazetteer lacks** are dropped by the reader. Coverage grows in `prism-signal`.
- **A static position goes stale.** Someone who moves without a live location keeps getting alerts
  for where they were. `/status` shows the place; a reminder to refresh is not built.
- **One hub per bot origin.** The hub delivers every Telegram message through one bot origin, so
  this bot needs its own hub, or to be the only Telegram bot that hub delivers through.
- **One process.** Long polling and the in-memory delivery memory assume a single instance.
- **Deliveries are sequential.** At about a tenth of a second per message, a thousand people is
  minutes. Bounded concurrency under Telegram's rate limit is the first thing to add before the
  audience grows.
- **A source can be wrong, late, or silent.** Every message says so.
- **Telegram is not tested live** from the development environment, which cannot reach the Bot
  API. The client is tested against a scripted transport.
