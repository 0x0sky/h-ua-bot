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
| Secret | `INFRA_DEPLOY_TOKEN` | Limited credential to dispatch and inspect `aiaiaiai-org/infra` workflows |
| Secret | `GHCR_PULL_TOKEN` | Dedicated, revocable, non-ephemeral GHCR credential with `read:packages` access to this bot image; must remain valid while infra is queued or awaiting approval |
| Variable | `GHCR_PULL_USER` | GitHub account associated with `GHCR_PULL_TOKEN` |
| Variable | `INFRA_AGE_RECIPIENT` | **Public** SSH key corresponding to `edge-prod-2` infra Environment's `SSH_PRIVATE_KEY` |

Obtain the public recipient through infra's manual **Show workload encryption
recipient** workflow (choose `edge-prod-2`). Never copy the private SSH key
to this repository.

The non-secret source configuration is reviewed in
`aiaiaiai-org/infra/contracts/services/h-ua-bot.yaml`: Hub origin, scope,
alert channel and source labels. Changing those requires a contract PR, not
production secrets.

The bot image is published on merge to master by the `Image` workflow. After
the image is available, manually run **Deploy production** on master. The
workflow validates the image and credentials, encrypts only the credentials
allowed by the contract, includes the separately scoped `GHCR_PULL_TOKEN` in the encrypted envelope,
and dispatches the exact release image to infra. This token must not be the
producer job\'s `GITHUB_TOKEN`: that token expires when the job completes,
possibly before infra authenticates to GHCR. Rotate the dedicated credential
according to its expiration policy and retain only package read access.

**The dispatch job confirms acceptance only, not live activation.** Confirm
the corresponding `infra → Deploy workload` job succeeds, then test
`/h-ua-bot/health`, Telegram `/start`, subscription changes and one
end-to-end alert via Prism Hub. In particular, the Prism Hub delivery worker's
global target must be reviewed before enabling delivery to multiple bots.

Do not delete the old target-Environment copies until the new end-to-end
deployment has succeeded and rollback/redeployment has been validated.
Do not deploy or merge automatically.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
