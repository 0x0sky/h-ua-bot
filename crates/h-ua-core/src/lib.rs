// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The conversation of h-ua-bot: what it says to a person and what it asks the hub to keep.
//!
//! The core does no I/O. A messenger and a subscription store are ports; Telegram and `prism-hub`
//! are adapters behind them. Reading sources, judging reports, and deciding whom to tell all live
//! in `prism-signal` and `prism-hub`, not here.
//!
//! What a person gives is a position, and it is kept as a coarse [`Cell`](geo::Cell), never as
//! coordinates.

pub mod category;
pub mod conversation;
pub mod geo;
pub mod memory;
pub mod message;
pub mod ports;
pub mod subscriber;
