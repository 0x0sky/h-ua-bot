// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! [`Subscriptions`] backed by the hub.

use std::collections::HashSet;
use std::sync::Mutex;

use async_trait::async_trait;
use h_ua_core::category::Category;
use h_ua_core::geo::Cell;
use h_ua_core::ports::{SubscriptionError, Subscriptions};
use h_ua_core::subscriber::{Recipient, Subscription};

use crate::client::{HubClient, HubError, SubscriptionState};

/// The client name in a [`Recipient`] this adapter serves.
const TELEGRAM: &str = "telegram";

/// Subscriptions kept in `prism-hub`.
///
/// A person is made known to the hub, and their chat is bound, the first time they have something
/// to save. That takes several calls, so it is remembered for the life of the process and later
/// saves are a single call. The hub stays the authority: everything is idempotent, so forgetting
/// only costs calls.
pub struct HubSubscriptions {
    client: HubClient,
    prepared: Mutex<HashSet<Recipient>>,
}

impl HubSubscriptions {
    /// Subscriptions over a hub client.
    pub fn new(client: HubClient) -> Self {
        Self {
            client,
            prepared: Mutex::new(HashSet::new()),
        }
    }

    fn chat_id(recipient: &Recipient) -> Result<i64, SubscriptionError> {
        if recipient.client != TELEGRAM {
            return Err(SubscriptionError(format!(
                "the hub adapter serves Telegram, not `{}`",
                recipient.client
            )));
        }
        recipient
            .address
            .parse()
            .map_err(|_| SubscriptionError("a Telegram address is a chat id".to_owned()))
    }

    async fn prepare(&self, recipient: &Recipient, chat_id: i64) -> Result<(), SubscriptionError> {
        if self.is_prepared(recipient) {
            return Ok(());
        }
        let subject = self.client.subject(&recipient.address);
        let workspace = self.client.onboard(&subject).await.map_err(failed)?;
        self.client.bot_status(&subject).await.map_err(failed)?;
        self.client
            .bind_surface(&workspace, chat_id, &subject)
            .await
            .map_err(failed)?;
        if let Ok(mut prepared) = self.prepared.lock() {
            prepared.insert(recipient.clone());
        }
        Ok(())
    }

    fn is_prepared(&self, recipient: &Recipient) -> bool {
        self.prepared
            .lock()
            .is_ok_and(|prepared| prepared.contains(recipient))
    }
}

fn failed(error: HubError) -> SubscriptionError {
    SubscriptionError(error.to_string())
}

fn subscription_of(
    recipient: &Recipient,
    state: SubscriptionState,
) -> Result<Subscription, SubscriptionError> {
    let cell: Cell = state
        .cell
        .parse()
        .map_err(|_| SubscriptionError("the hub holds a cell this bot cannot read".to_owned()))?;
    let categories = state
        .categories
        .iter()
        .map(|key| Category::from_key(key))
        .collect::<Option<_>>()
        .ok_or_else(|| SubscriptionError("the hub holds an unknown category".to_owned()))?;
    Ok(Subscription {
        recipient: recipient.clone(),
        cell,
        categories,
        include_nearby: state.include_nearby,
    })
}

#[async_trait]
impl Subscriptions for HubSubscriptions {
    async fn get(&self, recipient: &Recipient) -> Result<Option<Subscription>, SubscriptionError> {
        Self::chat_id(recipient)?;
        let subject = self.client.subject(&recipient.address);
        match self.client.subscription(&subject).await.map_err(failed)? {
            Some(state) => subscription_of(recipient, state).map(Some),
            None => Ok(None),
        }
    }

    async fn save(&self, subscription: &Subscription) -> Result<(), SubscriptionError> {
        let recipient = &subscription.recipient;
        let chat_id = Self::chat_id(recipient)?;
        self.prepare(recipient, chat_id).await?;
        let state = SubscriptionState {
            cell: subscription.cell.to_string(),
            categories: subscription
                .categories
                .iter()
                .map(|category| category.key().to_owned())
                .collect(),
            include_nearby: subscription.include_nearby,
        };
        let subject = self.client.subject(&recipient.address);
        self.client
            .save_subscription(&subject, &state)
            .await
            .map_err(failed)
    }

    async fn delete(&self, recipient: &Recipient) -> Result<(), SubscriptionError> {
        Self::chat_id(recipient)?;
        let subject = self.client.subject(&recipient.address);
        self.client
            .clear_subscription(&subject)
            .await
            .map_err(failed)?;
        if let Ok(mut prepared) = self.prepared.lock() {
            prepared.remove(recipient);
        }
        Ok(())
    }
}
