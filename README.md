# h-ua-bot

A bot that tells people about air threats near them, and only those.

You give it one thing: where you are. It reads public Telegram channels, works out from each post
which places are in danger, and messages you only when that includes yours. Drones, guided
bombs, and missiles, each on its own switch.

```text
public channel ── posts ──▶ reader ── "drone, heading for Kyiv" ──▶ relevance ──▶ Telegram
 (@vanek_nikolaev)       prism-signal                       your cell within reach?     you
```

**Not an official warning.** The sources are unofficial, human-run channels. Every alert says so,
names the source, and links the original post. No message does not mean safe. Official air-raid
alerts remain the authority.

## What a person sees

Send the bot `/start`, tap the location button, and it replies with the nearest place it knows.
From then on:

```text
⚠️ БпЛА (дрон) — Київ (ціль)
«1 реактивный мопед над Киевом»
Джерело: @vanek_nikolaev, неофіційне · 2 хв тому
https://t.me/vanek_nikolaev/43219
```

and, when the source calls a threat off:

```text
✅ Джерело повідомляє про відбій: БпЛА (дрон). Це не офіційний відбій тривоги.
«минус по всем этим 4 реактивным мопедам»
Джерело: @vanek_nikolaev, неофіційне · щойно
```

| Command | Does |
| --- | --- |
| `/start`, `/help` | What this is and is not, what is kept, the location button |
| `/location` | Share a position. A live location is followed silently as you move |
| `/kinds дрони каби ракети` | Choose what to follow |
| `/nearby так` / `ні` | Also warn about threats passing near, not at, you |
| `/status` | What the bot knows about you |
| `/stop` | Delete everything about you |

## Privacy

The bot keeps a chat identifier, a coarse grid cell about 3 km across (H3 resolution 6), the
kinds you chose, and a short log of what it sent you. It never keeps your coordinates: they are
used once to find the cell and dropped. A blocked bot is forgotten automatically. `/stop`
deletes everything. The database file is readable by its owner only.

Telegram sees your location the way it sees any message you send. The cell is not anonymous, since
it sits next to your chat id, and the welcome message says so.

## Run it

```bash
cp .env.example .env        # set HUA_TELEGRAM_TOKEN, then load it into the environment
set -a && . ./.env && set +a
cargo run --release -p h-ua-bot
```

It long-polls Telegram, so it needs no public address. One process, one SQLite file.

### Try it without Telegram

`simulate` replays collected posts for an imaginary person and prints what they would have been
told. It is how a place, a phrase, or a policy is checked against what a channel really said.

```bash
prism-signal-collect telegram vanek_nikolaev backfill --max-pages 10 > posts.ndjson   # from prism-signal
cargo run -p h-ua-bot -- simulate --lat 50.45 --lon 30.52 --kinds drone,missile < posts.ndjson
```

## Layout

| Crate | Owns |
| --- | --- |
| `h-ua-core` | Domain, relevance, relay, conversation, ports. No I/O |
| `h-ua-store` | SQLite implementation of the store port |
| `h-ua-telegram` | Telegram as the first client |
| `h-ua-bot` | Configuration, reading sources, the running loops, `simulate` |

Reading text into places and threats is [`prism-signal`](https://github.com/aiaiaiai-org/prism-signal)'s
job (`prism-signal-normalize`, `prism-signal-source-telegram`). This repository decides who is
told and how. See [`docs/architecture.md`](docs/architecture.md).

## Status

Pre-release. Built and tested: the reader, relevance, alerts, all-clears, cool-down, restart
safety, the SQLite store, the Telegram client against a scripted Bot API. **Not yet run against
the real Bot API**: the JSON shapes follow its documentation and are checked with a fake
transport only.

Next: becoming a client of `prism-hub` ([plan](docs/hub-integration.md)).

Not built: other messengers, other kinds of source, reports about a whole region (`Київська
область`), webhooks, more than one process. Limits are listed in the architecture document.

Licensed under MIT.
