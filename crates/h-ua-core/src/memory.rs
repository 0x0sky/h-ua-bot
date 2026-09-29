// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! A [`Store`] that keeps everything in memory. For tests and for dry runs that must not touch
//! a database.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::category::Category;
use crate::ports::{Delivery, Store, StoreError};
use crate::subscriber::{Recipient, Subscription};

#[derive(Default)]
struct State {
    subscriptions: BTreeMap<Recipient, Subscription>,
    cursors: BTreeMap<String, String>,
    deliveries: Vec<Delivery>,
}

/// An in-memory [`Store`].
#[derive(Default)]
pub struct MemoryStore(Mutex<State>);

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, StoreError> {
        self.0
            .lock()
            .map_err(|_| StoreError("memory store poisoned".to_owned()))
    }
}

impl Store for MemoryStore {
    fn subscription(&self, recipient: &Recipient) -> Result<Option<Subscription>, StoreError> {
        Ok(self.lock()?.subscriptions.get(recipient).cloned())
    }

    fn save_subscription(&self, subscription: &Subscription, _now: i64) -> Result<(), StoreError> {
        self.lock()?
            .subscriptions
            .insert(subscription.recipient.clone(), subscription.clone());
        Ok(())
    }

    fn delete_recipient(&self, recipient: &Recipient) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        state.subscriptions.remove(recipient);
        state.deliveries.retain(|d| &d.recipient != recipient);
        Ok(())
    }

    fn located_subscriptions(&self) -> Result<Vec<Subscription>, StoreError> {
        Ok(self
            .lock()?
            .subscriptions
            .values()
            .filter(|s| s.cell.is_some())
            .cloned()
            .collect())
    }

    fn cursor(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self.lock()?.cursors.get(key).cloned())
    }

    fn set_cursor(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.lock()?
            .cursors
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn was_delivered(
        &self,
        recipient: &Recipient,
        external_id: &str,
        category: Category,
        place_id: &str,
    ) -> Result<bool, StoreError> {
        Ok(self.lock()?.deliveries.iter().any(|d| {
            &d.recipient == recipient
                && d.external_id == external_id
                && d.category == category
                && d.place_id == place_id
        }))
    }

    fn delivered_since(
        &self,
        recipient: &Recipient,
        category: Category,
        place_id: &str,
        since: i64,
    ) -> Result<bool, StoreError> {
        Ok(self.lock()?.deliveries.iter().any(|d| {
            &d.recipient == recipient
                && d.category == category
                && d.place_id == place_id
                && d.at >= since
        }))
    }

    fn record_delivery(&self, delivery: &Delivery) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        let key = |d: &Delivery| {
            (
                d.recipient.clone(),
                d.external_id.clone(),
                d.category,
                d.place_id.clone(),
            )
        };
        if !state.deliveries.iter().any(|d| key(d) == key(delivery)) {
            state.deliveries.push(delivery.clone());
        }
        Ok(())
    }

    fn take_deliveries(
        &self,
        since: i64,
        category: Option<Category>,
        place_ids: Option<&[String]>,
    ) -> Result<Vec<Delivery>, StoreError> {
        let mut state = self.lock()?;
        let (taken, kept): (Vec<_>, Vec<_>) = state.deliveries.drain(..).partition(|d| {
            d.at >= since
                && category.is_none_or(|c| c == d.category)
                && place_ids.is_none_or(|ids| ids.contains(&d.place_id))
        });
        state.deliveries = kept;
        Ok(taken)
    }

    fn purge_deliveries_before(&self, before: i64) -> Result<usize, StoreError> {
        let mut state = self.lock()?;
        let count = state.deliveries.len();
        state.deliveries.retain(|d| d.at >= before);
        Ok(count - state.deliveries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_memory_store_meets_the_store_contract() {
        crate::conformance::check_store(&MemoryStore::new());
    }
}
