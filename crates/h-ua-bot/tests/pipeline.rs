// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Source to person: a scripted source, the real reader, the real relay, a real SQLite store.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use h_ua_bot::ingest::Ingest;
use h_ua_bot::simulate::{Person, simulate};
use h_ua_core::category::Category;
use h_ua_core::geo::Cell;
use h_ua_core::message::Message;
use h_ua_core::ports::{ClientError, Event, Messenger, SendError, Store};
use h_ua_core::relay::{Relay, RelayPolicy};
use h_ua_core::subscriber::{Recipient, Subscription};
use h_ua_store::SqliteStore;
use prism_signal_core::{Evidence, SourceId, Timestamp};
use prism_signal_normalize::Normalizer;
use prism_signal_source::{
    Cursor, EvidenceSource, Page, PageRequest, SourceError, SourceErrorKind,
};

const KYIV: (f64, f64) = (50.4501, 30.5234);

#[derive(Default)]
struct Outbox(Mutex<Vec<(String, String)>>);

#[async_trait]
impl Messenger for Outbox {
    fn name(&self) -> &'static str {
        "telegram"
    }
    async fn send(&self, to: &Recipient, message: &Message) -> Result<(), SendError> {
        self.0
            .lock()
            .unwrap()
            .push((to.address.clone(), message.text.clone()));
        Ok(())
    }
    async fn poll(&self) -> Result<Vec<Event>, ClientError> {
        Ok(Vec::new())
    }
}

/// A source that answers from a script and remembers what it was asked.
struct Scripted {
    id: SourceId,
    answers: Mutex<VecDeque<Result<Page, SourceError>>>,
    asked: Mutex<Vec<PageRequest>>,
}

impl Scripted {
    fn new(answers: Vec<Result<Page, SourceError>>) -> Self {
        Self {
            id: SourceId::new("telegram.channel", "vanek_nikolaev").unwrap(),
            answers: Mutex::new(answers.into()),
            asked: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl EvidenceSource for Scripted {
    fn source_id(&self) -> &SourceId {
        &self.id
    }
    async fn read(&self, request: PageRequest) -> Result<Page, SourceError> {
        self.asked.lock().unwrap().push(request);
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(Page::default()))
    }
}

fn post(id: u64, published_at: &str, text: &str) -> Evidence {
    serde_json::from_value(serde_json::json!({
        "source_id": "telegram.channel:vanek_nikolaev",
        "external_id": format!("vanek_nikolaev/{id}"),
        "published_at": published_at,
        "text": text,
        "provenance": {"url": format!("https://t.me/vanek_nikolaev/{id}"), "collector": "test/0"}
    }))
    .unwrap()
}

fn page(evidence: Vec<Evidence>) -> Result<Page, SourceError> {
    let newest = evidence
        .last()
        .map(|e| Cursor::new(e.external_id.as_str().rsplit('/').next().unwrap()));
    Ok(Page {
        evidence,
        older: None,
        newest,
    })
}

fn at(instant: &str) -> i64 {
    Timestamp::parse(instant)
        .unwrap()
        .as_datetime()
        .unix_timestamp()
}

fn kyivan(store: &dyn Store) -> Recipient {
    let who = Recipient::new("telegram", "kyiv");
    let mut subscription = Subscription::new(who.clone());
    subscription.cell = Some(Cell::around(KYIV.0, KYIV.1).unwrap());
    store.save_subscription(&subscription, 0).unwrap();
    who
}

fn ingest_over(store: Arc<dyn Store>, outbox: Arc<Outbox>) -> Ingest {
    let relay = Arc::new(Relay::new(
        store.clone(),
        [outbox as Arc<dyn Messenger>],
        RelayPolicy::default(),
    ));
    Ingest::new(store, relay, Normalizer::embedded().unwrap())
}

const T: &str = "2026-09-28T22:19:42Z";
const NOW: &str = "2026-09-28T22:20:00Z";

#[tokio::test]
async fn the_first_poll_reads_the_newest_page_and_later_polls_continue_after_it() {
    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    kyivan(&*store);
    let outbox = Arc::new(Outbox::default());
    let ingest = ingest_over(store.clone(), outbox.clone());
    let source = Scripted::new(vec![
        page(vec![post(43231, T, "2 баллистики на Киев !")]),
        Ok(Page::default()),
    ]);

    let first = ingest.poll(&source, at(NOW)).await.unwrap();
    assert_eq!((first.posts, first.relayed.alerts), (1, 1));
    assert_eq!(
        store
            .cursor("source:telegram.channel:vanek_nikolaev")
            .unwrap()
            .as_deref(),
        Some("43231")
    );

    let second = ingest.poll(&source, at(NOW)).await.unwrap();
    assert_eq!(second.posts, 0);

    let asked = source.asked.lock().unwrap().clone();
    assert_eq!(asked[0], PageRequest::Latest);
    assert_eq!(
        asked[1],
        PageRequest::After(Cursor::new("43231")),
        "the empty page after the first"
    );
    assert_eq!(
        asked[2],
        PageRequest::After(Cursor::new("43231")),
        "the next poll"
    );
    assert_eq!(outbox.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_restart_continues_from_the_saved_cursor_and_never_repeats_an_alert() {
    let dir = std::env::temp_dir().join(format!("h-ua-bot-restart-{}", std::process::id()));
    let path = dir.join("h-ua.sqlite");
    let outbox = Arc::new(Outbox::default());
    {
        let store: Arc<dyn Store> = Arc::new(SqliteStore::open(&path).unwrap());
        kyivan(&*store);
        let ingest = ingest_over(store, outbox.clone());
        let source = Scripted::new(vec![
            page(vec![post(43231, T, "2 баллистики на Киев !")]),
            Ok(Page::default()),
        ]);
        ingest.poll(&source, at(NOW)).await.unwrap();
    }
    // The process restarts. The source still shows the same post at the top of its page.
    let store: Arc<dyn Store> = Arc::new(SqliteStore::open(&path).unwrap());
    let ingest = ingest_over(store, outbox.clone());
    let source = Scripted::new(vec![Ok(Page::default())]);
    ingest.poll(&source, at(NOW)).await.unwrap();
    assert_eq!(
        source.asked.lock().unwrap()[0],
        PageRequest::After(Cursor::new("43231"))
    );
    assert_eq!(outbox.0.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_source_that_is_far_behind_is_caught_up_page_by_page() {
    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    kyivan(&*store);
    store
        .set_cursor("source:telegram.channel:vanek_nikolaev", "100")
        .unwrap();
    let ingest = ingest_over(store.clone(), Arc::new(Outbox::default()));
    let source = Scripted::new(vec![
        page(vec![post(101, T, "привіт"), post(102, T, "ще привіт")]),
        page(vec![post(103, T, "2 баллистики на Киев !")]),
    ]);
    let report = ingest.poll(&source, at(NOW)).await.unwrap();
    assert_eq!(report.posts, 3);
    assert_eq!(report.relayed.alerts, 1);
    assert_eq!(
        store
            .cursor("source:telegram.channel:vanek_nikolaev")
            .unwrap()
            .as_deref(),
        Some("103")
    );
    let asked = source.asked.lock().unwrap().clone();
    assert_eq!(asked[1], PageRequest::After(Cursor::new("102")));
    assert_eq!(asked[2], PageRequest::After(Cursor::new("103")));
}

#[tokio::test]
async fn posts_read_long_after_they_were_written_are_not_relayed() {
    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    kyivan(&*store);
    let outbox = Arc::new(Outbox::default());
    let ingest = ingest_over(store, outbox.clone());
    // A bot started after two hours down: the newest page is stale.
    let source = Scripted::new(vec![page(vec![post(
        1,
        "2026-09-28T20:00:00Z",
        "2 баллистики на Киев !",
    )])]);
    let report = ingest.poll(&source, at(NOW)).await.unwrap();
    assert_eq!((report.posts, report.relayed.alerts), (1, 0));
    assert!(outbox.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_source_failure_is_reported_and_moves_no_cursor() {
    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    store
        .set_cursor("source:telegram.channel:vanek_nikolaev", "50")
        .unwrap();
    let ingest = ingest_over(store.clone(), Arc::new(Outbox::default()));
    let source = Scripted::new(vec![Err(SourceError::new(
        SourceErrorKind::RateLimited,
        "telegram.preview.rate_limited",
    ))]);
    let error = ingest.poll(&source, at(NOW)).await.unwrap_err();
    assert_eq!(error.kind, SourceErrorKind::RateLimited);
    assert_eq!(
        store
            .cursor("source:telegram.channel:vanek_nikolaev")
            .unwrap()
            .as_deref(),
        Some("50")
    );
}

#[tokio::test]
async fn a_failure_on_a_later_page_keeps_what_was_already_relayed() {
    let store: Arc<dyn Store> = Arc::new(SqliteStore::open_in_memory().unwrap());
    kyivan(&*store);
    let outbox = Arc::new(Outbox::default());
    let ingest = ingest_over(store.clone(), outbox.clone());
    let source = Scripted::new(vec![
        page(vec![post(1, T, "2 баллистики на Киев !")]),
        Err(SourceError::new(
            SourceErrorKind::Unavailable,
            "telegram.preview.unavailable",
        )),
    ]);
    assert!(ingest.poll(&source, at(NOW)).await.is_err());
    // Post 1 went out and its cursor was saved, so the next poll starts after it.
    assert_eq!(outbox.0.lock().unwrap().len(), 1);
    assert_eq!(
        store
            .cursor("source:telegram.channel:vanek_nikolaev")
            .unwrap()
            .as_deref(),
        Some("1")
    );
}

// ---- simulate ----

fn person(lat: f64, lon: f64) -> Person {
    Person {
        lat,
        lon,
        categories: None,
        include_nearby: true,
    }
}

fn day() -> Vec<Evidence> {
    vec![
        post(1, "2026-09-28T22:19:42Z", "2 баллистики на Киев !"),
        post(2, "2026-09-28T22:21:00Z", "1 реактивный мопед над Одессой"),
        post(3, "2026-09-28T22:34:00Z", "минус по балістиці"),
    ]
}

#[tokio::test]
async fn a_dry_run_shows_what_a_person_at_a_place_would_have_been_told() {
    let normalizer = Normalizer::embedded().unwrap();
    let kyiv = simulate(&person(KYIV.0, KYIV.1), &day(), &normalizer)
        .await
        .unwrap();
    assert_eq!(kyiv.len(), 2);
    assert!(kyiv[0].text.starts_with("⚠️ Балістика — Київ (ціль)"));
    assert!(kyiv[1].text.starts_with("✅"));
    assert_eq!(kyiv[0].at, "2026-09-28T22:19:52Z");

    let odesa = simulate(&person(46.4825, 30.7233), &day(), &normalizer)
        .await
        .unwrap();
    assert_eq!(odesa.len(), 1);
    assert!(odesa[0].text.contains("Одеса"));

    let lviv = simulate(&person(49.8397, 24.0297), &day(), &normalizer)
        .await
        .unwrap();
    assert!(lviv.is_empty());
}

#[tokio::test]
async fn a_dry_run_honours_the_kinds_a_person_chose_and_rejects_bad_coordinates() {
    let normalizer = Normalizer::embedded().unwrap();
    let mut drones_only = person(KYIV.0, KYIV.1);
    drones_only.categories = Some([Category::Drone].into());
    assert!(
        simulate(&drones_only, &day(), &normalizer)
            .await
            .unwrap()
            .is_empty()
    );

    assert!(
        simulate(&person(200.0, 0.0), &day(), &normalizer)
            .await
            .is_err()
    );
}
