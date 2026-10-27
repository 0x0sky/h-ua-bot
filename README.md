# h-ua-bot

A bot that tells people about air threats near them, and only those.

You give it one thing: where you are. Prism currently reads public Telegram channels, works out from each post
which places are in danger, and messages you only when that includes yours. Drones, guided
bombs, and missiles, each on its own switch.

```text
four public channels ─▶ prism-hub ─▶ Porter ─▶ h-ua-bot ─▶ Telegram ─▶ you
  (equal source status)    (Prism Signal     (renders    (sends)       ◀─ position,
                            + subscriptions)  report)                 kinds, /stop
```

**Not an official warning.** The sources are unofficial, human-run channels. Every alert says so,
names the source, and links the original post. No message does not mean safe. Official air-raid
alerts remain the authority.

## What a person sees

Send the bot `/start`, tap the location button, and it replies with the nearest place it knows.

During an attack the chat holds **two messages**, not one per report:

```text
📡 Повідомлення джерел · з 03:27            ← the board: edited in place, never rings

🔴 ☄️ балістика — Київ · 03:31 @vanek_nikolaev
🔴 ✈️ реактивні шахеди — Київ · 03:29 @kpszsu
🟡 🛵 шахеди — Бровари (поруч) · 03:20 @kpszsu

✔️ Завершились: 2
Оновлено 03:32
```

```text
🔴 Загроза у вашій зоні                       ← the status: rings only when danger grows

☄️ балістика
✈️ реактивні шахеди — 3 повідомлення
🟡 поруч: 🛵 шахеди

Неофіційні джерела. Орієнтуйтеся на повітряну тривогу.
```

The status is sent anew, and the old one deleted, only when something comes closer (near → at
your zone) or a new kind appears at the same distance. Everything else is a quiet edit. Reports
that lapsed before they reached the bot are dropped, not replayed. When every report is called
off or lapses, the board gets its final edit and a quiet status follows:

```text
⚪ Джерела затихли
Активних повідомлень для вашої зони немає. Це не відбій: орієнтуйтеся на повітряну тривогу.

📍 Ви досі там само? Позицію оновлено в суботу о 12:34 — /location
```

The last line appears only there, only when the position is over three days old, and at most once
a day. The bot never says the sky is clear: silence from unofficial sources is not an all-clear.

The board needs the hub to send the structured report next to the text (`signal`, see
[the architecture](docs/architecture.md#receiving-deliveries)). Until it does, every report
arrives as the text Porter wrote:

```text
⚠️ БпЛА — Київ
Що: ударні дрони
Повідомляли (Telegram, за київським часом):
• 14:57 — @vanek_nikolaev https://t.me/vanek_nikolaev/43220

Це неофіційне джерело, а не повітряна тривога. Стежте за офіційними оповіщеннями. Відсутність повідомлень не означає безпеки.
```

| Command | Does |
| --- | --- |
| `/start`, `/help` | What this is and is not, what is kept, the location button |
| `/location` | Share a position. A live location is followed silently as you move |
| `/kinds дрони каби ракети` | Choose what to follow |
| `/nearby так` / `ні` | Also warn about threats passing near, not at, you |
| `/status` | What the bot knows about you |
| `/stop` | Delete your subscription and position |

Four equal configured public Telegram sources are
[@vanek_nikolaev](https://t.me/vanek_nikolaev),
[@kpszsu](https://t.me/kpszsu),
[@war_monitor](https://t.me/war_monitor) and
[@monitorwarr](https://t.me/monitorwarr).
Their names in the bot are a **display catalog**, not collector configuration:
Prism Hub must independently poll the same channels, with the reviewed
Prism Signal runtime pinned in its image. Multiple channels may repeat
a single original post; the number of channels is not a probability.

For a non-delivering production auth/routing test before any controlled
live-chat verification, see [the deployment handshake](docs/production-deploy.md).

## Privacy

The bot keeps nothing itself. `prism-hub` keeps a technical identifier of your chat, a coarse grid
cell about 3 km across (H3 resolution 6), the kinds you chose, and a short log of which reports you
were told about. Your coordinates are used once, inside the bot, to find the cell, and only the cell is sent
to the hub. `/stop` deletes the subscription, the cell, and the log. Your
chat identifier stays in the hub, and without a subscription nothing is sent to it.

Telegram sees your location the way it sees any message you send. The cell is not anonymous, since
it sits next to your chat id, and the welcome message says so.

## Run it

The bot is a client of a [`prism-hub`](https://github.com/aiaiaiai-org/prism-hub) that runs its
signal loop (`bin/prism-hub-signal-scheduler`, see the hub's `docs/signal-alerts.md`) and its
delivery worker. It needs two ways to reach that hub: it calls the hub's API, and the hub calls it
back to deliver.

```bash
cp .env.example .env        # set the token, the hub, and the secrets; then load it
set -a && . ./.env && set +a
cargo run --release -p h-ua-bot
```

It long-polls Telegram, so Telegram needs no public address. The hub does: it delivers over
`https://`, so put a TLS proxy in front of `HUA_DELIVERY_LISTEN` and point the hub's
`PRISM_BOT_ORIGIN` at it. The hub's `PRISM_BOT_DELIVERY_SECRET` is this bot's `HUA_DELIVERY_SECRET`.

A hub delivers every Telegram message through one bot origin, so give this bot its own hub, or
make sure it is the only Telegram bot that hub delivers through.

## Container

`Dockerfile` builds a small image (Rust build stage, `distroless` runtime, not root). It listens on
`1927`, the port infra gives every production workload: `GET /health` answers without a secret,
and `POST /api/v1/delivery` is where the hub delivers. Configuration is the `HUA_*` environment.

```bash
docker build -t h-ua-bot .
```

`.github/workflows/image.yml` proves the image builds on every pull request and publishes
`ghcr.io/0x0sky/h-ua-bot:<commit>` on a merge to `master`. Publishing an image does not deploy it.

## Preparing multiple sources

The bot can **describe** a typed source catalog without assuming every source is a Telegram
channel. Set `HUA_SOURCES_JSON` (and unset legacy `HUA_SOURCES`) to a JSON array containing
`id` (stable source identifier), `kind` (`telegram_channel`, `api`, `rss`, `website`, or
`webhook`), and `label` (what the bot shows in `/start` and `/status`). The older
comma-separated `HUA_SOURCES` is still supported.

This catalog is **display-only**. It must contain only sources already active in Prism Hub;
configuring a name here never creates a collector, proves its authority, or enables an
incoming webhook. Prism Hub collects feeds and retains provenance, `prism-signal` evaluates
reports, and Porter includes attribution in each alert. An API's official status does not
make this bot an official warning service. Do not count a Telegram repost and its upstream
API record as independent corroboration.

## Layout

| Crate | Owns |
| --- | --- |
| `h-ua-core` | What the bot says, the subscription model, the ports. No I/O |
| `h-ua-hub` | The hub's API as a client sees it, and subscriptions kept there |
| `h-ua-telegram` | Telegram as the first client |
| `h-ua-bot` | Configuration, the running loops, the endpoint the hub delivers to |

Reading sources, judging reports, and deciding whom to tell are
[`prism-signal`](https://github.com/aiaiaiai-org/prism-signal)'s and
[`prism-hub`](https://github.com/aiaiaiai-org/prism-hub)'s jobs, and the wording of an alert is
[`prism-porter`](https://github.com/aiaiaiai-org/prism-porter)'s. See
[`docs/architecture.md`](docs/architecture.md).

## Status

Pre-release. Built and tested here: the conversation, the hub client (against a stand-in and, by
hand, against a real hub), the delivery endpoint in the format the hub's gateway sends (checked
against the hub's own gateway), the Telegram client against a scripted Bot API. **Not yet run
against the real Bot API**: the JSON shapes follow its documentation and are checked with a fake
transport only. The whole path, from a channel post to a message in a chat, has not been run
end to end.

Not built: other messengers, actual collection of non-Telegram sources, reports about a whole region (`Київська
область`), webhooks, more than one process. Limits are listed in the architecture document.

Licensed under MIT.
