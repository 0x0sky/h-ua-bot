# h-ua-bot

A bot that tells people about air threats near them, and only those.

You give it one thing: where you are. It reads public Telegram channels, works out from each post
which places are in danger, and messages you only when that includes yours. Drones, guided
bombs, and missiles, each on its own switch.

```text
public channel ─▶ prism-hub ─────────────▶ Porter ─▶ this bot ─▶ Telegram ─▶ you
 (@vanek_nikolaev)  reads, judges (prism-signal),      delivers     ◀─ your position,
                    matches your cell                               kinds, /stop
```

**Not an official warning.** The sources are unofficial, human-run channels. Every alert says so,
names the source, and links the original post. No message does not mean safe. Official air-raid
alerts remain the authority.

## What a person sees

Send the bot `/start`, tap the location button, and it replies with the nearest place it knows.
From then on the hub sends, in words Porter writes:

```text
⚠️ БпЛА — Київ
Що: ударні дрони
Ймовірність: помірна

Повідомляли (Telegram, за київським часом):
• 14:57 — @vanek_nikolaev https://t.me/vanek_nikolaev/43220

Це неофіційне джерело, а не повітряна тривога. Стежте за офіційними оповіщеннями. Відсутність повідомлень не означає безпеки.
```

and, when the source calls a threat off (never on its own when a report merely lapses):

```text
ℹ️ Джерело повідомило про відбій: БпЛА, Київ
14:59 (за київським часом) джерело написало, що цю загрозу знято. https://t.me/vanek_nikolaev/43222
Це не офіційний відбій.
Ще діють повідомлення про цю загрозу: Бровари.

Відсутність повідомлень не означає безпеки.
```

| Command | Does |
| --- | --- |
| `/start`, `/help` | What this is and is not, what is kept, the location button |
| `/location` | Share a position. A live location is followed silently as you move |
| `/kinds дрони каби ракети` | Choose what to follow |
| `/nearby так` / `ні` | Also warn about threats passing near, not at, you |
| `/status` | What the bot knows about you |
| `/stop` | Delete your subscription and position |

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

Not built: other messengers, other kinds of source, reports about a whole region (`Київська
область`), webhooks, more than one process. Limits are listed in the architecture document.

Licensed under MIT.
