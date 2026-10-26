# Production deployment (producer-owned secrets)

This repository owns the Telegram bot and its runtime credentials. The
`aiaiaiai-org/infra` repository owns the host, SSH, routing and activation.
Deploy is **manual** and does not run on PR merge.

## Four Telegram sources

The production deployment profile uses `vanek_nikolaev,kpszsu,war_monitor,monitorwarr`.
The collector list is owned by `aiaiaiai-org/infra` in
`contracts/services/prism-hub.yaml` (`PRISM_SIGNAL_TELEGRAM_CHANNELS`).
The bot's `HUA_SOURCES` controls display only.

Roll out the matching infra contract to Prism Hub first. Check the signal
scheduler's collection results/cursors for each channel; HTTP health alone
does not prove collection. Then deploy the bot and check /start and /status.
An existing production `HUA_SOURCES` Variable overrides the workflow default:
set it to the same four-channel list or remove the override before bot deployment.
Do not set `PRISM_SIGNAL_SOURCES_JSON` alongside the comma-separated Hub setting.

Keep the previous source list for rollback. If collection must return to one
channel, restore the bot display to `vanek_nikolaev` and restore the Hub contract
to the same list in the coordinated rollback. No source history or cursor needs
to be deleted. Merging these changes does not activate them.

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

The Hub secret `PRISM_BOT_DELIVERY_SECRET` (infra Environment) and bot secret
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
| Secret | `HUA_TELEGRAM_TOKEN` | BotFather token |
| Secret | `HUA_HUB_TOKEN` | Scoped Prism Hub machine credential |
| Secret | `HUA_DELIVERY_SECRET` | At least 32 characters; must match the Prism Hub delivery gateway |
| Secret | `INFRA_DEPLOY_TOKEN` | Dispatch `aiaiaiai-org/infra` (Contents: write), plus read its Actions runs and artifacts (Actions: read) |
| Variable | `HUA_HUB_ORIGIN` | Optional; defaults to `https://api.aiaiaiai.org/prism-hub` |
| Variable | `HUA_HUB_PROVIDER_SCOPE` | Optional; defaults to `h-ua-bot` |
| Variable | `HUA_ALERT_CHANNEL` | Optional; defaults to `alerts` |
| Variable | `HUA_SOURCES` | Optional; deployment defaults to `vanek_nikolaev,kpszsu,war_monitor,monitorwarr` |

Neither `INFRA_AGE_RECIPIENT` nor `GHCR_PULL_TOKEN`/`GHCR_PULL_USER` nor
SSH/VPS configuration belongs in this repository. Infra owns its private SSH
key, target placement, public encryption recipient, and GHCR login.

Before running deployment, make sure the recipient workflow from
[aiaiaiai-org/infra#47](https://github.com/aiaiaiai-org/infra/pull/47)
is merged into **infra/master**. The producer stops before dispatch if the
infra repository cannot be read, the workflow is absent, or its Actions
metadata cannot be read. A GitHub HTTP 404 may mean an unmerged workflow
or a token not authorized to access a private repository. Verify that
`INFRA_DEPLOY_TOKEN` has explicit access to `aiaiaiai-org/infra`,
`Contents: write`, `Actions: read`, and any required organization approval.
Do not paste the token into CI logs.

The bot requests an infra recipient using `issue-workload-recipient`; infra
resolves its target and returns a *public-only*, one-day GitHub Actions
artifact, correlated to the bot workflow run ID. The bot fetches and
validates it, then encrypts its own three runtime secrets and four runtime variables. The target private
key never appears in the artifact or dispatch payload. Infra's new recipient
workflow must be merged before the producer is deployed.

Infra's `contracts/services/h-ua-bot.yaml` owns placement, network, health,
registry and an explicit allowlist of environment variable **names**; their
values belong to the bot's Environment or its in-repository defaults.
The encrypted envelope is checked against this allowlist before activation.

The bot image is published on merge to master by the `Image` workflow. After
the image is available, manually run **Deploy production** on master.
The producer verifies the image using its own job-scoped `GITHUB_TOKEN`
but does not forward that token to infra. It requests and validates an
infra-owned public encryption recipient, encrypts **only** the three HUA
runtime secrets plus the four producer-owned HUA variables, and dispatches the
immutable release image to infra.

Infra authenticates to GHCR using its **own** Environment registry
credential (`GHCR_PULL_TOKEN`, `GHCR_PULL_USER`) or its job-scoped
`GITHUB_TOKEN` when that token is authorized to read the package.
For private images, ensure infra has package-read access.
Application secrets are not copied to infra's GitHub Secrets store.

**The dispatch job confirms acceptance only, not live activation.** Confirm
the corresponding `infra → Deploy workload` job succeeds, then test
`/h-ua-bot/health`, Telegram `/start`, subscription changes and one
end-to-end alert via Prism Hub. In particular, the Prism Hub delivery worker's
global target must be reviewed before enabling delivery to multiple bots.

Do not delete the old target-Environment copies until the new end-to-end
deployment has succeeded and rollback/redeployment has been validated.
Do not deploy or merge automatically.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
