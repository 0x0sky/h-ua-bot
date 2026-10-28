# h-ua-bot

A bot that tells people about air threats near them, and only those.

You share your approximate location. Prism evaluates incoming threat reports and determines
which places may be affected. The bot delivers relevant warnings for your area, according
to the categories you select: drones, guided bombs, and missiles.

```text
threat reports ─▶ Prism Hub ─▶ Porter ─▶ h-ua-bot ─▶ Telegram ─▶ you
                   (assessment,    (renders    (sends)       ◀─ position,
                   subscriptions)  alert)                   kinds, /stop
```

**Not an official warning.** The bot does not replace official air-raid alerts.
Every warning explains this limitation. No message does not mean safe: always
follow official air-raid notifications.

## What a person sees

Send the bot `/start`, tap the location button, and it replies with the nearest place it knows.
From then on the hub sends, in words Porter writes:

```text
⚠️ БпЛА — Київ
Що: ударні дрони
Оновлено: 14:59 (за київським часом)

Це неофіційне повідомлення, а не сигнал повітряної тривоги. Орієнтуйтесь на офіційні сповіщення. Відсутність повідомлень не означає безпеки.
```

The hub can also ask the bot to update or remove a prior message, or to deliver
without a sound. These behaviors are controlled by the upstream delivery contract;
the bot does not independently decide whether a threat has ended.

An explicit retraction of a prior report is shown without implying official all-clear:

```text
ℹ️ Уточнення щодо загрози: БпЛА, Київ
Оновлено: 14:59 (за київським часом)
Попереднє повідомлення про загрозу відкликано.
Це не офіційний відбій. Загрозу не можна вважати знятою лише за цим повідомленням.

Відсутність повідомлень не означає безпеки.
```

| Command | Does |
| --- | --- |
| `/start`, `/help` | What this is and is not, what is kept, the location button |
| `/location` | Share a position. A live location is followed silently as you move |
| `/kinds`, ⚙️ За чим стежити? | A custom reply keyboard to switch drones, bombs, missiles and threats passing nearby. Each selection refreshes the buttons; ⬅️ Меню returns to the main keyboard |
| `/status` | What the bot knows about you |
| `/stop` | Delete your subscription and position |

The same commands are in Telegram's menu, set with `setMyCommands` on every start, and every reply
carries a keyboard that stays under the input field: 📍 Оновити позицію, 🛡 Стан, ⚙️ За чим стежити?,
❓ Довідка. An empty chat shows what the bot is before the first `/start`.

The chat stays tidy. The bot keeps one reply of its own: each new reply deletes the previous one,
and the command, button or text it answers. The position shared last stays; a new one replaces it.
The hub's board and status are left to the hub. The bot remembers these message ids in memory, so
after a restart one old message per chat may stay.

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

## Layout

| Crate | Owns |
| --- | --- |
| `h-ua-core` | What the bot says, the subscription model, the ports. No I/O |
| `h-ua-hub` | The hub's API as a client sees it, and subscriptions kept there |
| `h-ua-telegram` | Telegram as the first client |
| `h-ua-bot` | Configuration, the running loops, the endpoint the hub delivers to |

Evaluating threat reports and deciding whom to notify are
[`prism-signal`](https://github.com/aiaiaiai-org/prism-signal)'s and
[`prism-hub`](https://github.com/aiaiaiai-org/prism-hub)'s jobs. Alerts are worded by
[`prism-porter`](https://github.com/aiaiaiai-org/prism-porter). See
[`docs/architecture.md`](docs/architecture.md).

## Status

The bot, hub connection, Telegram subscription flow and delivery endpoint have
automated regression coverage. Production HTTP routing and delivery-secret
authentication have been verified. **An end-to-end real Telegram alert receipt
is not yet confirmed**; health and authentication tests do not demonstrate it.

Limitations: the first client is Telegram; broad regional reports and further
messenger integrations require additional work. See the architecture document.

Licensed under MIT.
