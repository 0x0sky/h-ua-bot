// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! What the core needs from the outside world: messengers to talk through and somewhere subscriptions
//! live. Adapters implement these; the core never names one.

use async_trait::async_trait;
use thiserror::Error;

use crate::message::Message;
use crate::subscriber::{Recipient, Subscription};

/// Something a person did in a messenger.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// Who did it.
    pub from: Recipient,
    /// What they did.
    pub kind: EventKind,
}

/// The things the bot understands a person doing.
#[derive(Clone, Debug, PartialEq)]
pub enum EventKind {
    /// A command such as `/kinds дрони`, without the slash.
    Command {
        /// Command name, lowercase, without a bot suffix.
        name: String,
        /// Everything after the name, trimmed.
        args: String,
    },
    /// The person shared where they are.
    Location {
        /// Latitude in degrees.
        lat: f64,
        /// Longitude in degrees.
        lon: f64,
        /// The position is a live one, updated by the messenger as the person moves. Such
        /// updates are applied without a reply.
        live: bool,
    },
    /// Anything else, such as free text.
    Other,
}

/// Why a message could not be sent.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SendError {
    /// The person blocked the bot or deleted the chat. They must not be messaged again.
    #[error("recipient unreachable")]
    Blocked,
    /// The messenger asked the sender to slow down.
    #[error("rate limited, retry after {retry_after_secs}s")]
    RateLimited {
        /// Seconds to wait.
        retry_after_secs: u64,
    },
    /// The messenger refused this message; retrying it unchanged will not help.
    #[error("message rejected: {0}")]
    Rejected(String),
    /// The messenger could not be reached.
    #[error("messenger unavailable: {0}")]
    Unavailable(String),
}

/// A messenger could not be read from.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("client error: {0}")]
pub struct ClientError(pub String);

/// A way to talk to people: Telegram first, others behind the same port.
#[async_trait]
pub trait Messenger: Send + Sync {
    /// Client name used in [`Recipient::client`].
    fn name(&self) -> &'static str;

    /// Sends a message to a person.
    async fn send(&self, to: &Recipient, message: &Message) -> Result<(), SendError>;

    /// Waits for the next batch of things people did. An empty batch is a normal answer.
    async fn poll(&self) -> Result<Vec<Event>, ClientError>;
}

/// The subscriptions could not be read or changed.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("subscriptions unavailable: {0}")]
pub struct SubscriptionError(pub String);

/// Where subscriptions live. In production that is `prism-hub`; the bot keeps nothing itself.
///
/// A subscription always has a position, because without one nothing is relevant to a person and
/// the hub holds no subscription for them.
#[async_trait]
pub trait Subscriptions: Send + Sync {
    /// One person's subscription, or `None` if they have none.
    async fn get(&self, recipient: &Recipient) -> Result<Option<Subscription>, SubscriptionError>;

    /// Creates or replaces a person's subscription.
    async fn save(&self, subscription: &Subscription) -> Result<(), SubscriptionError>;

    /// Removes the person's subscription, and with it the position it held.
    async fn delete(&self, recipient: &Recipient) -> Result<(), SubscriptionError>;
}
