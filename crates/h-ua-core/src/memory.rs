// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! [`Subscriptions`] kept in memory. For tests and for dry runs that must not reach a hub.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::ports::{SubscriptionError, Subscriptions};
use crate::subscriber::{Recipient, Subscription};

/// In-memory [`Subscriptions`].
#[derive(Default)]
pub struct MemorySubscriptions(Mutex<BTreeMap<Recipient, Subscription>>);

impl MemorySubscriptions {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many people have a subscription.
    pub fn len(&self) -> usize {
        self.0.lock().map_or(0, |map| map.len())
    }

    /// Whether nobody has one.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<Recipient, Subscription>>, SubscriptionError>
    {
        self.0
            .lock()
            .map_err(|_| SubscriptionError("memory subscriptions poisoned".to_owned()))
    }
}

#[async_trait]
impl Subscriptions for MemorySubscriptions {
    async fn get(&self, recipient: &Recipient) -> Result<Option<Subscription>, SubscriptionError> {
        Ok(self.lock()?.get(recipient).cloned())
    }

    async fn save(&self, subscription: &Subscription) -> Result<(), SubscriptionError> {
        self.lock()?
            .insert(subscription.recipient.clone(), subscription.clone());
        Ok(())
    }

    async fn delete(&self, recipient: &Recipient) -> Result<(), SubscriptionError> {
        self.lock()?.remove(recipient);
        Ok(())
    }
}
