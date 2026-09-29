// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Dry run: what would a person standing here have been told?
//!
//! Replays collected posts through the real reader and the real relay for one imaginary
//! subscriber and returns the messages, without a database or a messenger. It is how a place,
//! a phrase, or a policy is checked against what a channel actually said.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use h_ua_core::category::Category;
use h_ua_core::geo::{Cell, GeoError};
use h_ua_core::memory::MemoryStore;
use h_ua_core::message::Message;
use h_ua_core::ports::{ClientError, Event, Messenger, SendError, Store};
use h_ua_core::relay::{Relay, RelayPolicy};
use h_ua_core::subscriber::{Recipient, Subscription};
use prism_signal_core::Evidence;
use prism_signal_normalize::Normalizer;

/// Seconds after a post is published that the replay reads it. Short, as a bot polling every
/// half minute would.
const READ_DELAY_SECS: i64 = 10;

/// The imaginary person.
#[derive(Clone, Debug)]
pub struct Person {
    /// Latitude in degrees.
    pub lat: f64,
    /// Longitude in degrees.
    pub lon: f64,
    /// Kinds they follow; all when `None`.
    pub categories: Option<BTreeSet<Category>>,
    /// Whether they want warnings about threats nearby.
    pub include_nearby: bool,
}

/// A message the person would have received.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Received {
    /// When, as an RFC 3339 instant.
    pub at: String,
    /// What it said.
    pub text: String,
}

#[derive(Default)]
struct Capture(Mutex<Vec<String>>);

#[async_trait]
impl Messenger for Capture {
    fn name(&self) -> &'static str {
        "telegram"
    }

    async fn send(&self, _to: &Recipient, message: &Message) -> Result<(), SendError> {
        self.0
            .lock()
            .map_err(|_| SendError::Unavailable("poisoned".into()))?
            .push(message.text.clone());
        Ok(())
    }

    async fn poll(&self) -> Result<Vec<Event>, ClientError> {
        Ok(Vec::new())
    }
}

/// Replays `posts`, oldest first, for one person.
pub async fn simulate(
    person: &Person,
    posts: &[Evidence],
    normalizer: &Normalizer,
) -> Result<Vec<Received>, GeoError> {
    let store = Arc::new(MemoryStore::new());
    let capture = Arc::new(Capture::default());
    let recipient = Recipient::new("telegram", "simulation");
    let mut subscription = Subscription::new(recipient);
    subscription.cell = Some(Cell::around(person.lat, person.lon)?);
    subscription.include_nearby = person.include_nearby;
    if let Some(categories) = &person.categories {
        subscription.categories = categories.clone();
    }
    store
        .save_subscription(&subscription, 0)
        .map_err(|_| GeoError::InvalidCell)?;

    // Every post is replayed as if just published, so the age limit would only get in the way.
    let policy = RelayPolicy {
        max_age_secs: i64::MAX / 2,
        ..RelayPolicy::default()
    };
    let relay = Relay::new(store, [capture.clone() as Arc<dyn Messenger>], policy);

    let mut received = Vec::new();
    for evidence in posts {
        let now = evidence.published_at.as_datetime().unix_timestamp() + READ_DELAY_SECS;
        let readings = normalizer.read(evidence);
        relay.relay_post(evidence, &readings, now).await;
        let texts: Vec<String> = capture
            .0
            .lock()
            .map(|mut sent| sent.drain(..).collect())
            .unwrap_or_default();
        let at = prism_signal_core::Timestamp::from_datetime(
            evidence.published_at.as_datetime() + time_seconds(READ_DELAY_SECS),
        )
        .to_string();
        received.extend(texts.into_iter().map(|text| Received {
            at: at.clone(),
            text,
        }));
    }
    Ok(received)
}

fn time_seconds(seconds: i64) -> std::time::Duration {
    std::time::Duration::from_secs(seconds.unsigned_abs())
}
