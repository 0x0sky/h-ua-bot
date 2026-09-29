// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The heart of h-ua-bot: who is told about what, and when.
//!
//! The core does no I/O. It reads hazard [`Reading`](prism_signal_normalize::Reading)s produced
//! by `prism-signal-normalize`, decides which people they concern, and hands messages to
//! [`Messenger`](ports::Messenger)s. A messenger and a store are ports; Telegram and SQLite are
//! adapters behind them.
//!
//! What a person gives is a position, and it is kept as a coarse [`Cell`](geo::Cell), never as
//! coordinates. What they receive is only what concerns that cell.

pub mod category;
pub mod conformance;
pub mod conversation;
pub mod geo;
pub mod memory;
pub mod message;
pub mod ports;
pub mod relay;
pub mod relevance;
pub mod subscriber;
