// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! `prism-hub` as a client of h-ua-bot sees it.
//!
//! The bot keeps nothing about people. It tells the hub who they are (a Telegram chat), where
//! their chat is (a bound surface), and what they asked for (a subscription), and the hub decides
//! whom to tell about what. The only position that ever leaves the bot is a coarse grid cell.

mod client;
mod subscriptions;

pub use client::{HubClient, HubConfig, HubError, Subject, SubscriptionState};
pub use subscriptions::HubSubscriptions;
