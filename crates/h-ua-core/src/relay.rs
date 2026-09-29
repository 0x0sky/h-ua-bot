// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Turns readings into messages to the right people, once.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use prism_signal_core::Evidence;
use prism_signal_normalize::{Phase, Reading};
use tracing::{info, warn};

use crate::category::Category;
use crate::message::{Message, all_clear, threat_alert};
use crate::ports::{Delivery, Messenger, SendError, Store};
use crate::relevance::affected_places;
use crate::subscriber::Recipient;

/// Longest a rate-limited send is made to wait before its one retry.
const MAX_RETRY_WAIT_SECS: u64 = 30;

/// How the relay behaves.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayPolicy {
    /// Posts older than this are not relayed. Keeps a restart, or a slow source, from waking
    /// people with a threat that is long over.
    pub max_age_secs: i64,
    /// The same kind of threat at the same place is not repeated within this window. The
    /// channel often posts several updates about one wave.
    pub cooldown_secs: i64,
    /// An all-clear reaches people who were alerted within this window.
    pub cleared_window_secs: i64,
    /// Quote the source's own words in the alert. Off, an alert carries only the structured
    /// summary and the link.
    pub include_text: bool,
}

impl Default for RelayPolicy {
    fn default() -> Self {
        Self {
            max_age_secs: 15 * 60,
            cooldown_secs: 3 * 60,
            cleared_window_secs: 60 * 60,
            include_text: true,
        }
    }
}

/// What one call to [`Relay::relay_post`] did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Report {
    /// Threat alerts sent.
    pub alerts: usize,
    /// All-clear corrections sent.
    pub all_clears: usize,
    /// People removed because they blocked the bot.
    pub removed: usize,
}

/// Sends readings to the people they concern.
pub struct Relay {
    store: Arc<dyn Store>,
    messengers: BTreeMap<String, Arc<dyn Messenger>>,
    policy: RelayPolicy,
}

impl Relay {
    /// A relay over a store and a set of messengers, keyed by client name.
    pub fn new(
        store: Arc<dyn Store>,
        messengers: impl IntoIterator<Item = Arc<dyn Messenger>>,
        policy: RelayPolicy,
    ) -> Self {
        let messengers = messengers
            .into_iter()
            .map(|messenger| (messenger.name().to_owned(), messenger))
            .collect();
        Self {
            store,
            messengers,
            policy,
        }
    }

    /// Relays the readings of one post. `now` is seconds since the Unix epoch.
    ///
    /// A failure to reach one person never stops the others.
    pub async fn relay_post(&self, evidence: &Evidence, readings: &[Reading], now: i64) -> Report {
        let mut report = Report::default();
        let text = self
            .policy
            .include_text
            .then_some(evidence.text.as_deref())
            .flatten();
        for reading in readings {
            let age = now - reading.published_at.as_datetime().unix_timestamp();
            if age > self.policy.max_age_secs {
                continue;
            }
            match reading.phase {
                Phase::Threat => self.threat(reading, text, age, now, &mut report).await,
                Phase::Cleared => {
                    self.cleared(reading, readings, text, age, now, &mut report)
                        .await
                }
            }
        }
        report
    }

    async fn threat(
        &self,
        reading: &Reading,
        text: Option<&str>,
        age: i64,
        now: i64,
        report: &mut Report,
    ) {
        let Some(kind) = reading.kind else { return };
        let category = Category::of(kind);
        let subscriptions = match self.store.located_subscriptions() {
            Ok(subscriptions) => subscriptions,
            Err(error) => {
                warn!(%error, "cannot read subscriptions");
                return;
            }
        };
        let external_id = reading.external_id.as_str();
        for subscription in subscriptions {
            let mut affected = affected_places(&subscription, reading);
            affected.retain(|a| {
                !self.already_told(
                    &subscription.recipient,
                    external_id,
                    category,
                    &a.place.place_id,
                    now,
                )
            });
            if affected.is_empty() {
                continue;
            }
            let message = threat_alert(reading, &affected, text, age);
            match self.send(&subscription.recipient, &message).await {
                Ok(()) => {
                    report.alerts += 1;
                    for a in &affected {
                        let delivery = Delivery {
                            recipient: subscription.recipient.clone(),
                            external_id: external_id.to_owned(),
                            category,
                            place_id: a.place.place_id.clone(),
                            at: now,
                        };
                        if let Err(error) = self.store.record_delivery(&delivery) {
                            warn!(%error, "cannot record a delivery");
                        }
                    }
                }
                Err(SendError::Blocked) => self.remove(&subscription.recipient, report),
                Err(error) => warn!(%error, to = %subscription.recipient, "alert not sent"),
            }
        }
    }

    /// Whether this person already got this alert, or one like it a moment ago.
    ///
    /// When the store cannot answer, the person is treated as not yet told: a repeated alert is
    /// a nuisance, a missed one is not.
    fn already_told(
        &self,
        recipient: &Recipient,
        external_id: &str,
        category: Category,
        place_id: &str,
        now: i64,
    ) -> bool {
        let same_post = self
            .store
            .was_delivered(recipient, external_id, category, place_id)
            .unwrap_or(false);
        let cooling = self
            .store
            .delivered_since(
                recipient,
                category,
                place_id,
                now - self.policy.cooldown_secs,
            )
            .unwrap_or(false);
        same_post || cooling
    }

    async fn cleared(
        &self,
        reading: &Reading,
        siblings: &[Reading],
        text: Option<&str>,
        age: i64,
        now: i64,
        report: &mut Report,
    ) {
        // An all-clear that names neither a kind nor a place says nothing about anything the
        // bot alerted on. `по угрозе от МиГ-31К пока минуса` must not end a drone alert.
        if reading.kind.is_none() && reading.places.is_empty() {
            return;
        }
        // A post that calls a threat off and reports one of the same kind still standing is
        // not a clean all-clear: `минус по ракетам ... угроза баллистики пока актуальна`.
        if contradicted(reading, siblings) {
            return;
        }
        let category = reading.kind.map(Category::of);
        let place_ids: Vec<String> = reading.places.iter().map(|p| p.place_id.clone()).collect();
        let places = (!place_ids.is_empty()).then_some(place_ids.as_slice());
        let deliveries = match self.store.take_deliveries(
            now - self.policy.cleared_window_secs,
            category,
            places,
        ) {
            Ok(deliveries) => deliveries,
            Err(error) => {
                warn!(%error, "cannot read deliveries");
                return;
            }
        };
        let mut told: Vec<Recipient> = Vec::new();
        for delivery in deliveries {
            if told.contains(&delivery.recipient) {
                continue;
            }
            told.push(delivery.recipient.clone());
            match self
                .send(&delivery.recipient, &all_clear(reading, text, age))
                .await
            {
                Ok(()) => report.all_clears += 1,
                Err(SendError::Blocked) => self.remove(&delivery.recipient, report),
                Err(error) => warn!(%error, to = %delivery.recipient, "all-clear not sent"),
            }
        }
    }

    fn remove(&self, recipient: &Recipient, report: &mut Report) {
        info!(%recipient, "recipient unreachable, removing");
        if let Err(error) = self.store.delete_recipient(recipient) {
            warn!(%error, "cannot remove a recipient");
        }
        report.removed += 1;
    }

    async fn send(&self, to: &Recipient, message: &Message) -> Result<(), SendError> {
        let Some(messenger) = self.messengers.get(&to.client) else {
            return Err(SendError::Rejected(format!(
                "no client named {}",
                to.client
            )));
        };
        match messenger.send(to, message).await {
            Err(SendError::RateLimited { retry_after_secs }) => {
                tokio::time::sleep(Duration::from_secs(
                    retry_after_secs.min(MAX_RETRY_WAIT_SECS),
                ))
                .await;
                messenger.send(to, message).await
            }
            other => other,
        }
    }
}

/// Whether another reading of the same post reports, as still standing, the threat this
/// all-clear calls off.
///
/// The threat counts as the same when it is of the same category, and, if the all-clear names
/// places, when it touches one of them. An all-clear for the drones over Kyiv is not undone by a
/// drone reported near Vasylkiv, but `минус по ракетам` is undone by `угроза баллистики`.
fn contradicted(cleared: &Reading, readings: &[Reading]) -> bool {
    let category = cleared.kind.map(Category::of);
    readings.iter().any(|other| {
        let Some(kind) = other.kind else { return false };
        other.phase == Phase::Threat
            && category.is_none_or(|c| c == Category::of(kind))
            && (cleared.places.is_empty()
                || other
                    .places
                    .iter()
                    .any(|p| cleared.places.iter().any(|q| q.place_id == p.place_id)))
    })
}
