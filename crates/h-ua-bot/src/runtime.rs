// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The running bot: source loops and client loops side by side.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use h_ua_core::conversation::Conversation;
use h_ua_core::message::source_label;
use h_ua_core::ports::{Messenger, Store};
use h_ua_core::relay::Relay;
use h_ua_store::SqliteStore;
use h_ua_telegram::{ReqwestTransport, TelegramClient};
use prism_signal_normalize::Normalizer;
use prism_signal_source::{EvidenceSource, SourceErrorKind};
use prism_signal_source_telegram::{ReqwestPreviewFetcher, TelegramPreviewSource};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::ingest::Ingest;

/// Alerts older than this are forgotten. Longer than any window that reads them.
const DELIVERY_RETENTION_SECS: i64 = 24 * 60 * 60;

/// The longest a failing loop waits before trying again.
const MAX_BACKOFF: Duration = Duration::from_secs(300);

const USER_AGENT: &str = concat!(
    "h-ua-bot/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/0x0sky/h-ua-bot)"
);

/// Seconds since the Unix epoch.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// Runs the bot until interrupted.
pub async fn run(config: Config) -> Result<(), String> {
    let token = config
        .telegram_token
        .clone()
        .ok_or("HUA_TELEGRAM_TOKEN is not set")?;
    let store: Arc<dyn Store> =
        Arc::new(SqliteStore::open(&config.db_path).map_err(|e| e.to_string())?);
    let normalizer = Normalizer::embedded().map_err(|e| e.to_string())?;

    let transport = ReqwestTransport::new(token, config.telegram_api_base.as_deref())
        .map_err(|e| e.to_string())?;
    let telegram = Arc::new(TelegramClient::new(transport));
    let username = telegram
        .username()
        .await
        .map_err(|e| format!("Telegram refused the token: {e}"))?;
    info!(%username, "connected to Telegram");

    let http = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let mut sources: Vec<Arc<dyn EvidenceSource>> = Vec::new();
    for channel in &config.sources {
        let fetcher = ReqwestPreviewFetcher::new(http.clone()).map_err(|e| e.to_string())?;
        sources.push(Arc::new(TelegramPreviewSource::new(
            channel.clone(),
            fetcher,
        )));
    }
    let labels: Vec<String> = sources
        .iter()
        .map(|s| source_label(s.source_id()))
        .collect();
    info!(sources = %labels.join(", "), "reading");

    let messenger: Arc<dyn Messenger> = telegram;
    let relay = Arc::new(Relay::new(
        store.clone(),
        [messenger.clone()],
        config.policy.clone(),
    ));
    let ingest = Arc::new(Ingest::new(store.clone(), relay, normalizer));

    let mut tasks = tokio::task::JoinSet::new();
    let count = u32::try_from(sources.len()).unwrap_or(1).max(1);
    for (index, source) in sources.into_iter().enumerate() {
        // Spread the sources over the interval instead of reading them all at once.
        let offset = config.poll_interval / count * u32::try_from(index).unwrap_or(0);
        tasks.spawn(source_loop(
            ingest.clone(),
            source,
            store.clone(),
            config.poll_interval,
            offset,
        ));
    }
    tasks.spawn(client_loop(messenger, ingest, store, labels));

    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("interrupted, stopping"),
        finished = tasks.join_next() => error!(?finished, "a loop ended, which it should never do"),
    }
    Ok(())
}

async fn source_loop(
    ingest: Arc<Ingest>,
    source: Arc<dyn EvidenceSource>,
    store: Arc<dyn Store>,
    interval: Duration,
    offset: Duration,
) {
    tokio::time::sleep(offset).await;
    loop {
        let now = unix_now();
        let wait = match ingest.poll(&*source, now).await {
            Ok(_) => interval,
            Err(error) => {
                warn!(source = %source.source_id(), %error, "cannot read source");
                match error.kind {
                    SourceErrorKind::RateLimited => interval * 4,
                    _ => (interval * 2).min(MAX_BACKOFF),
                }
            }
        };
        if let Err(error) = store.purge_deliveries_before(now - DELIVERY_RETENTION_SECS) {
            warn!(%error, "cannot purge old deliveries");
        }
        tokio::time::sleep(wait).await;
    }
}

async fn client_loop(
    messenger: Arc<dyn Messenger>,
    ingest: Arc<Ingest>,
    store: Arc<dyn Store>,
    labels: Vec<String>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        match messenger.poll().await {
            Ok(events) => {
                backoff = Duration::from_secs(1);
                for event in events {
                    // The replies are worked out before anything is awaited, so no borrow of the
                    // store crosses a suspension point.
                    let replies =
                        Conversation::new(&*store, ingest.normalizer().gazetteer(), labels.clone())
                            .handle(&event, unix_now());
                    match replies {
                        Ok(replies) => {
                            for reply in replies {
                                if let Err(error) = messenger.send(&event.from, &reply).await {
                                    warn!(%error, to = %event.from, "reply not sent");
                                }
                            }
                        }
                        Err(error) => warn!(%error, "cannot handle an event"),
                    }
                }
            }
            Err(error) => {
                warn!(%error, "cannot read from the messenger");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
}
