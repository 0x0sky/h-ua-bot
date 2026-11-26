# Production deployment (producer-owned secrets)

This repository owns the Telegram bot and its runtime credentials. The
`aiaiaiai-org/infra` repository owns the host, SSH, routing and activation.
Delivery is **pull-based**: this repository publishes an attested image and a
sealed secret envelope, and infra pulls them on an explicit dispatch. This
repository holds **no credential for infra** and calls no infra API.

## Internal alert processing

The bot does not publish a list of input feeds or any individual provider names
in `/start`, `/help`, `/status`, or alert messages. The active collection,
source metadata, deduplication and provenance are maintained by Prism Hub and
Prism Signal, not the Telegram client.

Legacy `HUA_SOURCES` is still accepted by the current production configuration
for compatibility but is **not used to compose user-facing messages**. Do not
try to synchronize its value with Prism Hub to change the bot's visible text.
Keep source monitoring and operational diagnostics in their owning services.

## Non-delivering Hub ↔ bot handshake

After a separately authorized Hub and bot deployment, verify routing and
shared-secret parity **without sending a Telegram message**:

```bash
# Read the Hub runtime's PRISM_BOT_DELIVERY_SECRET securely; do not paste it in a
# shell command line, CI log or issue comment.
bash scripts/check-delivery-handshake.sh
```

The script reads `PRISM_BOT_DELIVERY_SECRET` from the environment, checks the
public `/prism-hub/healthz` and `/hua-bot/health` routes, then sends
`{}` to `/hua-bot/api/v1/delivery` with the Hub's secret. The bot must return
HTTP `400` with `invalid_request` **after** authentication. HTTP `401`
indicates that Hub and bot secrets do not match. The incomplete body has no
chat, message or idempotency key, so **cannot trigger a Telegram send**.
The script does not echo the secret; its temporary header file is private and
removed on exit.

The Hub secret `PRISM_BOT_DELIVERY_SECRET` (prism-hub production Environment) and bot secret
`HUA_DELIVERY_SECRET` (bot production Environment, encrypted at handoff)
**must contain exactly the same value**. There is no safe way to infer that
from GitHub variable names or green CI.

This only checks HTTP reachability and delivery authorization. It does
**not** check the bot's scoped `HUA_HUB_TOKEN`, a running Prism Signal
scheduler, per-channel cursors, subscription and binding state or actual
Telegram delivery. After authorized activation, perform a controlled
end-to-end test with a consenting test chat and examine Hub's
`signal_source_cursors`, signal scheduler errors and delivery outbox. Do not
fake threat notices in public channels or infer official safety status.

## Initial setup

In **0x0sky/h-ua-bot → Settings → Environments → production** configure:

| Kind | Name | Notes |
| --- | --- | --- |
| Secret | `TELEGRAM_BOT_TOKEN` | BotFather token; mapped to runtime `HUA_TELEGRAM_TOKEN` by the publish workflow |
| Secret | `HUA_HUB_TOKEN` | Scoped Prism Hub machine credential |
| Secret | `HUA_DELIVERY_SECRET` | At least 32 characters; must match the Prism Hub delivery gateway |
| Variable | `HUA_HUB_ORIGIN` | Optional; defaults to `https://api.aiaiaiai.org/prism-hub` |
| Variable | `HUA_HUB_PROVIDER_SCOPE` | Optional; defaults to `h-ua-bot` |
| Variable | `HUA_ALERT_CHANNEL` | Optional; defaults to `alerts` |
| Variable | `HUA_SOURCES` | Legacy compatibility setting; never exposed to users |

No infra token, `INFRA_AGE_RECIPIENT`, `GHCR_PULL_TOKEN`/`GHCR_PULL_USER` or
SSH/VPS configuration belongs in this repository. Infra owns its private SSH
key, target placement, public encryption recipient, and GHCR login.

## How a release reaches the server

1. **CI** passes on `master`. The **Publish image** workflow (`publish.yml`,
   triggered by that CI run alone) builds `ghcr.io/0x0sky/h-ua-bot:<commit>` and
   attests its provenance.
2. It reads infra's **public** contracts (`aiaiaiai-org/infra`,
   `contracts/`): the server `h-ua-bot` is placed on, that server's public
   encryption recipient, and the exact names the workload may receive (its three
   runtime secrets and four producer variables). It seals those seven values
   with `age` to that recipient and publishes the ciphertext as
   `ghcr.io/0x0sky/h-ua-bot:<commit>.secrets`, attested too. The recipient comes
   from infra's contract, so moving the bot to another server needs no change here.
3. Infra, on an explicit **Reconcile deliveries** dispatch for `h-ua-bot`, finds
   the master head, verifies the digests and provenance of both, opens the
   envelope with its own deploy key inside the target Environment and activates
   the release. A head without an envelope is not delivered.

The `Image` workflow only builds (pull requests and `master`): a second push of
the commit tag would replace the attested digest, so it never publishes.

Infra's `contracts/services/h-ua-bot.yaml` owns placement, network, health,
registry and an explicit allowlist of environment variable **names**; their
values belong to the bot's Environment or its in-repository defaults.
Infra needs **Read** access to the `h-ua-bot` package (package settings, not a
secret). Application secrets are not copied to infra's GitHub Secrets store.

**Publishing is not activation.** Confirm the corresponding `infra →
Reconcile deliveries` run succeeds, then test `/hua-bot/health`, Telegram
`/start`, subscription changes and one end-to-end alert via Prism Hub. In
particular, the Prism Hub delivery worker's global target must be reviewed
before enabling delivery to multiple bots.

Do not delete the old target-Environment copies until the new end-to-end
deployment has succeeded and rollback/redeployment has been validated.
Do not deploy or merge automatically.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
