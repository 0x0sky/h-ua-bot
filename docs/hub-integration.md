# Moving to `prism-hub`

Decision: `h-ua-bot` becomes a client of `prism-hub`, as the ecosystem rules require
(`prism/docs/ecosystem.md`: clients depend on the hub, never on `prism-signal` or on each other's
stores). This document records what that changed and in what order. **Steps 1 to 5 are built**
(see the table at the end); the standalone relay, ingest, and SQLite store that used to live in
this repository are gone.

## What the hub already has

Read from `prism-hub`, `prism-bot`, and `prism-porter` at their current `master`:

| Needed | Exists |
| --- | --- |
| A person as an identity with a personal workspace | `POST /api/v1/actors/onboard`, idempotent, Telegram provider evidence |
| A Telegram chat as a delivery target | `TelegramSurfaceBinding`: logical `(workspace, channel)` to a `chat_id` |
| A durable outbox with retries and idempotency keys | `delivery_outbox_entries` and `bin/prism-hub-delivery-worker` |
| Rendering text for a surface, bounded by its limit | `prism-porter` through the `PorterGateway` port |
| Pushing a rendered message to Telegram | `prism-bot`'s authenticated `POST /api/v1/delivery` |
| Running a worker process on a schedule | `bin/prism-hub-mail-digest-scheduler` (mail only) |
| Per-user stop and resume | bot lifecycle (`/stop`, `/resume`) |

## What is missing

1. **Subscriptions with a position.** The hub has no notion of where a person is.
2. **Reading sources on a schedule** and keeping a window of what they said.
3. **Deciding what is worth sending**: which readings become alerts, when a threat is over, when
   an all-clear would be false. Today this lives in this repository's relay.
4. **A `signal.alert` artifact** for Porter to render.
5. **A way to bind a person's chat to their workspace** over HTTP. The hub's own docs list this as
   a separate change.

## Where each piece goes

The split follows `prism-signal/docs/architecture.md`, which already assigns these.

```text
sources ──▶ hub scheduler ──▶ prism-signal runtime ──▶ assessments + cells ──▶ hub
                                 (stateless)                                    │
                                                                subscriptions ◀─┤ fan-out by cell
                                                                                ▼
                                    Porter (signal.alert) ◀── outbox ◀── one entry per person
                                          │
                                    delivery worker ──▶ prism-bot ──▶ Telegram
```

| Piece | Repository | Notes |
| --- | --- | --- |
| Reading text into places and threats | `prism-signal` | done: `prism-signal-normalize` |
| Place and reach to grid cells | `prism-signal` | new: `cover`, so the hub matches `cell = ANY(cells)` and never does geometry |
| Which readings become assessments, expiry, retraction | `prism-signal` | new: fusion v1 and the `prism-signal.v1` runtime |
| Subscriptions, scheduling, window, fan-out, cool-down | `prism-hub` | new tables and use cases |
| `signal.alert` rendering in `uk-UA` | `prism-porter` | new renderer |
| Location, kinds, `/stop` in Telegram | client | see below |

Geometry stays in Rust, in one tested place. The hub never sees a coordinate: it stores a cell and
compares cells. The signal layer never sees a person.

### The rules move with the logic

Everything this repository's relay learned about not sending false all-clears becomes **fusion
policy** in `prism-signal`, expressed as assessment events instead of message sends:

- only explicit all-clear words retract (done in the reader);
- a bare all-clear naming neither a kind nor a place retracts nothing;
- an all-clear contradicted by a threat of the same category in the same post is held back;
- a retraction reaches only the people the assessment was issued to (the hub's fan-out).

The hub then treats an `issued` event as an alert and a `retracted` event as the correction, with
`assessment_id` and the event sequence as the idempotency key. Cool-down and maximum age are hub
policy, as the architecture says (`an assessment is a proposal, not an authority`).

The regression tests in `h-ua-core/tests/relay.rs`, in the channel's own words, are the acceptance
tests for that policy. They move to `prism-signal` and must still pass.

### The client

`h-ua-bot` stays the product client in Rust, thin: it turns Telegram into hub API calls and hub
deliveries into Telegram messages, and owns no subscription or alert logic.

- onboards the person (`actors/onboard`), makes sure the bot has an instance
  (`bot-instances/personal/status`), and binds their chat (`telegram/surfaces/bind`);
- saves `alert-subscriptions/personal/save` with a cell (the client snaps the position; the hub
  never receives coordinates), the categories, and the nearby switch; `status` and `clear`
  likewise;
- receives deliveries at the same authenticated `POST /api/v1/delivery` contract `prism-bot`
  already defines, so a hub needs no new transport.

`h-ua-core`'s geometry (`Cell`), categories, conversation copy, and the Telegram adapter were kept.
`Store`, `Relay`, `Ingest`, `simulate`, and the SQLite crate were removed: the hub replaced them.

## Order

Each step ships behind a contract and tests, and nothing is switched over until the previous step
is verified.

| # | Step | Where | Verified by |
| --- | --- | --- | --- |
| 1 | `cover`, fusion v1 with the guards above, `prism-signal.v1` runtime | `prism-signal` | done: tests; the relay's regression cases replayed as policy tests |
| 2 | Subscription tables, use cases, API, OpenAPI contract, `alert_subscriptions:*` capabilities | `prism-hub` | done: hub CI |
| 3 | Scheduler running the collector and the runtime; window; fan-out into the outbox | `prism-hub` | done: hub CI (`docs/signal-alerts.md` there) |
| 4 | `signal.alert` renderer | `prism-porter` | done: porter CI |
| 5 | Thin client against the hub contract | `h-ua-bot` | done: tests here against a stand-in hub; by hand against a real hub; delivery endpoint checked with the hub's own gateway |
| 6 | End to end with a real bot token and a real chat | all | **not done**: by hand, once |

Two things the hub lacked and gained on the way: the surface-bind endpoint was not in the public
contract and required a bot instance id the API never returns, so it now belongs to the contract
and the id is optional (the caller's own instance); and the hub delivers all Telegram messages
through one bot origin, which is why this bot needs its own hub.

Steps 2 to 4 are Ruby. The development environment has Ruby 3.3.6 and cannot fetch the pinned
4.0.6, and PostgreSQL 16 where CI runs 18, so those steps were checked here on what runs and are
proved by each repository's CI.

## What does not change

- The bot says the source is unofficial and that silence is not safety.
- The position is kept as a coarse cell and never as coordinates.
- Sources are public Telegram channels for now.
- The channel's owner is told before this goes live.
