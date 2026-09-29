// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Shared fakes for the core's behaviour tests.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use h_ua_core::geo::Cell;
use h_ua_core::message::Message;
use h_ua_core::ports::{ClientError, Event, Messenger, SendError};
use h_ua_core::subscriber::{Recipient, Subscription};
use prism_signal_core::Evidence;

/// A messenger that records what it is asked to send and can be told to fail.
#[derive(Default)]
pub struct FakeMessenger {
    pub sent: Mutex<Vec<(Recipient, Message)>>,
    failures: Mutex<BTreeMap<String, Vec<SendError>>>,
}

impl FakeMessenger {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The next sends to `address` fail with these errors, in order.
    pub fn fail(&self, address: &str, errors: Vec<SendError>) {
        self.failures
            .lock()
            .unwrap()
            .insert(address.to_owned(), errors);
    }

    pub fn texts_to(&self, address: &str) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(to, _)| to.address == address)
            .map(|(_, message)| message.text.clone())
            .collect()
    }

    pub fn count(&self) -> usize {
        self.sent.lock().unwrap().len()
    }
}

#[async_trait]
impl Messenger for FakeMessenger {
    fn name(&self) -> &'static str {
        "telegram"
    }

    async fn send(&self, to: &Recipient, message: &Message) -> Result<(), SendError> {
        if let Some(errors) = self.failures.lock().unwrap().get_mut(&to.address) {
            if !errors.is_empty() {
                return Err(errors.remove(0));
            }
        }
        self.sent
            .lock()
            .unwrap()
            .push((to.clone(), message.clone()));
        Ok(())
    }

    async fn poll(&self) -> Result<Vec<Event>, ClientError> {
        Ok(Vec::new())
    }
}

pub fn person(address: &str, lat: f64, lon: f64) -> Subscription {
    let mut subscription = Subscription::new(Recipient::new("telegram", address));
    subscription.cell = Some(Cell::around(lat, lon).unwrap());
    subscription
}

/// A post from the channel, published at `published_at` (RFC 3339).
pub fn post(id: u64, published_at: &str, text: &str) -> Evidence {
    serde_json::from_value(serde_json::json!({
        "source_id": "telegram.channel:vanek_nikolaev",
        "external_id": format!("vanek_nikolaev/{id}"),
        "published_at": published_at,
        "text": text,
        "provenance": {"url": format!("https://t.me/vanek_nikolaev/{id}"), "collector": "test/0"}
    }))
    .unwrap()
}

pub const KYIV: (f64, f64) = (50.4501, 30.5234);
pub const LVIV: (f64, f64) = (49.8397, 24.0297);
pub const ODESA: (f64, f64) = (46.4825, 30.7233);

/// Seconds since the epoch of an RFC 3339 instant.
pub fn at(instant: &str) -> i64 {
    prism_signal_core::Timestamp::parse(instant)
        .unwrap()
        .as_datetime()
        .unix_timestamp()
}
