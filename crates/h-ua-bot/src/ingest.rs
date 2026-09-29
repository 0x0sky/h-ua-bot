// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Reading a source, making sense of what it says, and passing it on.

use std::sync::Arc;

use h_ua_core::ports::Store;
use h_ua_core::relay::{Relay, Report};
use prism_signal_normalize::Normalizer;
use prism_signal_source::{Cursor, EvidenceSource, PageRequest, SourceError};
use tracing::info;

/// Most pages read from one source in one poll. A source further behind than this catches up on
/// the next poll.
const MAX_PAGES_PER_POLL: usize = 10;

/// What one poll of a source did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PollReport {
    /// Posts read.
    pub posts: usize,
    /// Messages sent as a result.
    pub relayed: Report,
}

/// Reads sources and relays what they say.
pub struct Ingest {
    store: Arc<dyn Store>,
    relay: Arc<Relay>,
    normalizer: Normalizer,
}

impl Ingest {
    /// An ingest over a store, a relay, and a normalizer.
    pub fn new(store: Arc<dyn Store>, relay: Arc<Relay>, normalizer: Normalizer) -> Self {
        Self {
            store,
            relay,
            normalizer,
        }
    }

    /// The gazetteer the normalizer reads places from.
    pub fn normalizer(&self) -> &Normalizer {
        &self.normalizer
    }

    /// Reads whatever is new in a source and relays it. `now` is seconds since the Unix epoch.
    ///
    /// The first poll of a source has no cursor and reads its newest page; posts older than the
    /// relay's maximum age are read but never sent. The cursor is saved after each page, so a
    /// failure part-way never sends a post twice.
    pub async fn poll(
        &self,
        source: &dyn EvidenceSource,
        now: i64,
    ) -> Result<PollReport, SourceError> {
        let key = format!("source:{}", source.source_id());
        let mut request = match self.store.cursor(&key).ok().flatten() {
            Some(cursor) => PageRequest::After(Cursor::new(cursor)),
            None => PageRequest::Latest,
        };
        let mut report = PollReport::default();
        for _ in 0..MAX_PAGES_PER_POLL {
            let page = source.read(request).await?;
            if page.evidence.is_empty() {
                break;
            }
            for evidence in &page.evidence {
                report.posts += 1;
                let readings = self.normalizer.read(evidence);
                let sent = self.relay.relay_post(evidence, &readings, now).await;
                report.relayed.alerts += sent.alerts;
                report.relayed.all_clears += sent.all_clears;
                report.relayed.removed += sent.removed;
            }
            let Some(newest) = page.newest else { break };
            if let Err(error) = self.store.set_cursor(&key, newest.as_str()) {
                tracing::warn!(%error, "cannot save the cursor of {key}");
            }
            request = PageRequest::After(newest);
        }
        if report.posts > 0 {
            info!(source = %source.source_id(), posts = report.posts, alerts = report.relayed.alerts, all_clears = report.relayed.all_clears, "polled");
        }
        Ok(report)
    }
}
