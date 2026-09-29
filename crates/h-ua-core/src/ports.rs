// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! What the core needs from the outside world: messengers to talk through and a store to keep
//! subscriptions in. Adapters implement these; the core never names one.

use async_trait::async_trait;
use thiserror::Error;

use crate::category::Category;
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

/// A store failed.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("store error: {0}")]
pub struct StoreError(pub String);

/// An alert that was sent, kept so it is neither repeated nor left uncorrected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Delivery {
    /// Who got it.
    pub recipient: Recipient,
    /// The post it came from.
    pub external_id: String,
    /// Which kind of threat.
    pub category: Category,
    /// Which place it was about.
    pub place_id: String,
    /// When it was sent, in seconds since the Unix epoch.
    pub at: i64,
}

/// Durable state: subscriptions, source cursors, and sent alerts.
///
/// Times are passed in as seconds since the Unix epoch, so behaviour is testable without a
/// clock.
pub trait Store: Send + Sync {
    /// One person's subscription.
    fn subscription(&self, recipient: &Recipient) -> Result<Option<Subscription>, StoreError>;

    /// Creates or replaces a subscription.
    fn save_subscription(&self, subscription: &Subscription, now: i64) -> Result<(), StoreError>;

    /// Removes everything kept about a person: subscription and sent alerts.
    fn delete_recipient(&self, recipient: &Recipient) -> Result<(), StoreError>;

    /// Every subscription that has a position.
    fn located_subscriptions(&self) -> Result<Vec<Subscription>, StoreError>;

    /// A saved position in a source or a messenger, such as the last post id read.
    fn cursor(&self, key: &str) -> Result<Option<String>, StoreError>;

    /// Saves a cursor.
    fn set_cursor(&self, key: &str, value: &str) -> Result<(), StoreError>;

    /// Whether this exact alert was already sent.
    fn was_delivered(
        &self,
        recipient: &Recipient,
        external_id: &str,
        category: Category,
        place_id: &str,
    ) -> Result<bool, StoreError>;

    /// Whether any alert of this category about this place was sent since `since`.
    fn delivered_since(
        &self,
        recipient: &Recipient,
        category: Category,
        place_id: &str,
        since: i64,
    ) -> Result<bool, StoreError>;

    /// Remembers a sent alert.
    fn record_delivery(&self, delivery: &Delivery) -> Result<(), StoreError>;

    /// Removes and returns the alerts sent since `since`, optionally only of one category and
    /// only about the given places. Used to send a correction to exactly the people who got the
    /// alert.
    fn take_deliveries(
        &self,
        since: i64,
        category: Option<Category>,
        place_ids: Option<&[String]>,
    ) -> Result<Vec<Delivery>, StoreError>;

    /// Forgets alerts sent before `before`.
    fn purge_deliveries_before(&self, before: i64) -> Result<usize, StoreError>;
}
