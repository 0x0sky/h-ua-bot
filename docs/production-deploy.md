# Production deployment (producer-owned secrets)

This repository owns the Telegram bot and its runtime credentials. The
`aiaiaiai-org/infra` repository owns the host, SSH, routing and activation.
Deploy is **manual** and does not run on PR merge.

## Initial setup

In **0x0sky/h-ua-bot → Settings → Environments → production** configure:

| Kind | Name | Notes |
| --- | --- | --- |
| Secret | `HUA_TELEGRAM_TOKEN` | BotFather token |
| Secret | `HUA_HUB_TOKEN` | Scoped Prism Hub machine credential |
| Secret | `HUA_DELIVERY_SECRET` | At least 32 characters; must match the Prism Hub delivery gateway |
| Secret | `INFRA_DEPLOY_TOKEN` | Dispatch `aiaiaiai-org/infra` (Contents: write), plus read its Actions runs and artifacts (Actions: read) |

Neither `INFRA_AGE_RECIPIENT` nor `GHCR_PULL_TOKEN`/`GHCR_PULL_USER` nor
SSH/VPS configuration belongs in this repository. Infra owns its private SSH
key, target placement, public encryption recipient, and GHCR login.

The bot requests an infra recipient using `issue-workload-recipient`; infra
resolves its target and returns a *public-only*, one-day GitHub Actions
artifact, correlated to the bot workflow run ID. The bot fetches and
validates it, then encrypts its own three runtime secrets. The target private
key never appears in the artifact or dispatch payload. Infra's new recipient
workflow must be merged before the producer is deployed.

The non-secret source configuration is reviewed in
`aiaiaiai-org/infra/contracts/services/h-ua-bot.yaml`: Hub origin, scope,
alert channel and source labels. Changing those requires a contract PR, not
production secrets.

The bot image is published on merge to master by the `Image` workflow. After
the image is available, manually run **Deploy production** on master.
The producer verifies the image using its own job-scoped `GITHUB_TOKEN`
but does not forward that token to infra. It requests and validates an
infra-owned public encryption recipient, encrypts **only** the three HUA
runtime secrets, and dispatches the immutable release image to infra.

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
