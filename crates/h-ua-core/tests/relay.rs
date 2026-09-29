// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Who is told what, when, and how often.

mod support;

use std::sync::Arc;

use h_ua_core::category::Category;
use h_ua_core::memory::MemoryStore;
use h_ua_core::ports::{SendError, Store};
use h_ua_core::relay::{Relay, RelayPolicy};
use h_ua_core::subscriber::Recipient;
use prism_signal_normalize::Normalizer;
use support::*;

struct World {
    store: Arc<MemoryStore>,
    messenger: Arc<FakeMessenger>,
    relay: Relay,
    normalizer: Normalizer,
}

fn world(policy: RelayPolicy) -> World {
    let store = Arc::new(MemoryStore::new());
    let messenger = FakeMessenger::new();
    let relay = Relay::new(store.clone(), [messenger.clone() as Arc<_>], policy);
    World {
        store,
        messenger,
        relay,
        normalizer: Normalizer::embedded().unwrap(),
    }
}

impl World {
    fn join(&self, subscription: h_ua_core::subscriber::Subscription) {
        self.store.save_subscription(&subscription, 0).unwrap();
    }

    /// Reads a post and relays it as of `now`.
    async fn hear(
        &self,
        id: u64,
        published: &str,
        text: &str,
        now: &str,
    ) -> h_ua_core::relay::Report {
        let evidence = post(id, published, text);
        let readings = self.normalizer.read(&evidence);
        self.relay.relay_post(&evidence, &readings, at(now)).await
    }
}

const T0: &str = "2026-09-28T22:19:42Z";
const T0_PLUS_30S: &str = "2026-09-28T22:20:12Z";

#[tokio::test]
async fn only_people_near_the_target_are_told() {
    let w = world(RelayPolicy::default());
    w.join(person("kyiv", 50.40, 30.60));
    w.join(person("lviv", LVIV.0, LVIV.1));
    w.join(person("odesa", ODESA.0, ODESA.1));

    let report = w
        .hear(43231, T0, "2 баллистики на Киев !", T0_PLUS_30S)
        .await;

    assert_eq!(report.alerts, 1);
    assert_eq!(w.messenger.count(), 1);
    let text = &w.messenger.texts_to("kyiv")[0];
    assert!(text.starts_with("⚠️ Балістика — Київ (ціль)"), "{text}");
    assert!(text.contains("«2 баллистики на Киев !»"));
    assert!(text.contains("Джерело: @vanek_nikolaev, неофіційне · щойно"));
    assert!(text.contains("https://t.me/vanek_nikolaev/43231"));
}

#[tokio::test]
async fn a_person_without_a_position_is_told_nothing() {
    let w = world(RelayPolicy::default());
    w.join(h_ua_core::subscriber::Subscription::new(Recipient::new(
        "telegram", "nowhere",
    )));
    let report = w.hear(1, T0, "2 баллистики на Киев !", T0_PLUS_30S).await;
    assert_eq!(report.alerts, 0);
}

#[tokio::test]
async fn a_place_the_source_only_launches_from_or_mentions_tells_nobody() {
    let w = world(RelayPolicy::default());
    // Mykolaiv is where the drone came from; only Odesa is in danger.
    w.join(person("mykolaiv", 46.9750, 31.9946));
    w.join(person("odesa", ODESA.0, ODESA.1));
    let report = w
        .hear(
            1,
            T0,
            "1 реактивный мопед летит к Одессе со стороны Николаева",
            T0_PLUS_30S,
        )
        .await;
    assert_eq!(report.alerts, 1);
    assert!(w.messenger.texts_to("mykolaiv").is_empty());
    assert_eq!(w.messenger.texts_to("odesa").len(), 1);

    // A place only named in passing is not the subject either.
    let passed = w
        .hear(
            2,
            T0,
            "3 реактивных мопеда пролетели Николаев дальше в сторону Одессы",
            T0_PLUS_30S,
        )
        .await;
    assert_eq!(passed.alerts, 0, "cool-down already covers Odesa");
    assert!(w.messenger.texts_to("mykolaiv").is_empty());
}

#[tokio::test]
async fn people_get_only_the_kinds_they_chose() {
    let w = world(RelayPolicy::default());
    let mut drones_only = person("d", KYIV.0, KYIV.1);
    drones_only.categories = [Category::Drone].into();
    w.join(drones_only);

    assert_eq!(
        w.hear(1, T0, "2 баллистики на Киев !", T0_PLUS_30S)
            .await
            .alerts,
        0
    );
    assert_eq!(
        w.hear(2, T0, "1 реактивный мопед над Киевом", T0_PLUS_30S)
            .await
            .alerts,
        1
    );
}

#[tokio::test]
async fn nearby_warnings_can_be_turned_off() {
    let w = world(RelayPolicy::default());
    let mut calm = person("calm", KYIV.0, KYIV.1);
    calm.include_nearby = false;
    w.join(calm);
    w.join(person("wary", KYIV.0, KYIV.1));

    let report = w
        .hear(
            1,
            T0,
            "1 реактивный мопед пролетает южнее Киева",
            T0_PLUS_30S,
        )
        .await;
    assert_eq!(report.alerts, 1);
    assert!(w.messenger.texts_to("calm").is_empty());
    assert!(w.messenger.texts_to("wary")[0].contains("(поруч)"));
}

#[tokio::test]
async fn the_same_post_is_never_sent_twice() {
    let w = world(RelayPolicy::default());
    w.join(person("kyiv", KYIV.0, KYIV.1));
    w.hear(43231, T0, "2 баллистики на Киев !", T0_PLUS_30S)
        .await;
    let again = w
        .hear(43231, T0, "2 баллистики на Киев !", T0_PLUS_30S)
        .await;
    assert_eq!(again.alerts, 0);
    assert_eq!(w.messenger.count(), 1);
}

#[tokio::test]
async fn a_burst_about_one_wave_is_one_alert_and_the_next_wave_is_another() {
    let w = world(RelayPolicy::default());
    w.join(person("kyiv", KYIV.0, KYIV.1));

    assert_eq!(
        w.hear(
            1,
            "2026-09-28T22:19:42Z",
            "2 баллистики на Киев !",
            "2026-09-28T22:19:50Z"
        )
        .await
        .alerts,
        1
    );
    // Inside the three-minute cool-down: an update about the same thing.
    assert_eq!(
        w.hear(
            2,
            "2026-09-28T22:21:25Z",
            "ещё 2 баллистики на Киев !",
            "2026-09-28T22:21:30Z"
        )
        .await
        .alerts,
        0
    );
    // Past it: a new wave deserves a new alert.
    assert_eq!(
        w.hear(
            3,
            "2026-09-28T22:24:00Z",
            "ещё 2 баллистики на Киев !",
            "2026-09-28T22:24:05Z"
        )
        .await
        .alerts,
        1
    );
    // A different kind at the same place is not the same alert.
    assert_eq!(
        w.hear(
            4,
            "2026-09-28T22:24:10Z",
            "1 реактивный мопед над Киевом",
            "2026-09-28T22:24:15Z"
        )
        .await
        .alerts,
        1
    );
}

#[tokio::test]
async fn a_stale_post_is_not_relayed() {
    let w = world(RelayPolicy::default());
    w.join(person("kyiv", KYIV.0, KYIV.1));
    // Published 20 minutes before it is read.
    let report = w
        .hear(
            1,
            "2026-09-28T22:00:00Z",
            "2 баллистики на Киев !",
            "2026-09-28T22:20:00Z",
        )
        .await;
    assert_eq!(report.alerts, 0);
}

#[tokio::test]
async fn an_all_clear_reaches_only_those_who_were_alerted_and_resets_the_cool_down() {
    let w = world(RelayPolicy::default());
    w.join(person("alerted", KYIV.0, KYIV.1));
    w.join(person("never", LVIV.0, LVIV.1));
    w.hear(
        1,
        "2026-09-28T22:19:42Z",
        "1 реактивный мопед над Киевом",
        "2026-09-28T22:19:50Z",
    )
    .await;

    let report = w
        .hear(
            2,
            "2026-09-28T22:27:00Z",
            "минус по всем этим реактивным мопедам",
            "2026-09-28T22:27:05Z",
        )
        .await;
    assert_eq!(report.all_clears, 1);
    let texts = w.messenger.texts_to("alerted");
    assert_eq!(texts.len(), 2);
    assert!(texts[1].starts_with("✅ Джерело повідомляє про відбій: БпЛА (дрон)"));
    assert!(texts[1].contains("не офіційний відбій"));
    assert!(w.messenger.texts_to("never").is_empty());

    // The threat is over, so a new one right away is a new alert, not a repeat.
    let next = w
        .hear(
            3,
            "2026-09-28T22:27:30Z",
            "1 реактивный мопед над Киевом",
            "2026-09-28T22:27:35Z",
        )
        .await;
    assert_eq!(next.alerts, 1);
}

#[tokio::test]
async fn an_all_clear_for_one_kind_leaves_another_kind_standing() {
    let w = world(RelayPolicy::default());
    w.join(person("kyiv", KYIV.0, KYIV.1));
    w.hear(
        1,
        "2026-09-28T22:19:42Z",
        "2 баллистики на Киев !",
        "2026-09-28T22:19:50Z",
    )
    .await;
    let report = w
        .hear(
            2,
            "2026-09-28T22:21:00Z",
            "минус по всем этим реактивным мопедам",
            "2026-09-28T22:21:05Z",
        )
        .await;
    assert_eq!(report.all_clears, 0);
}

#[tokio::test]
async fn an_all_clear_about_one_place_leaves_the_other_places_standing() {
    let w = world(RelayPolicy::default());
    w.join(person("kyiv", KYIV.0, KYIV.1));
    w.hear(
        1,
        "2026-09-28T22:19:42Z",
        "1 реактивный мопед над Киевом",
        "2026-09-28T22:19:50Z",
    )
    .await;
    let report = w
        .hear(
            2,
            "2026-09-28T22:21:00Z",
            "минус по мопеду на Ровно",
            "2026-09-28T22:21:05Z",
        )
        .await;
    assert_eq!(report.all_clears, 0);
}

#[tokio::test]
async fn an_all_clear_that_names_no_kind_and_no_place_ends_nothing() {
    let w = world(RelayPolicy::default());
    w.join(person("kyiv", KYIV.0, KYIV.1));
    w.hear(
        1,
        "2026-09-28T22:19:42Z",
        "1 реактивный мопед над Киевом",
        "2026-09-28T22:19:50Z",
    )
    .await;
    // "no threat from MiG-31K for now": says nothing about the drone.
    let report = w
        .hear(
            2,
            "2026-09-28T22:21:00Z",
            "по угрозе от МиГ-31К пока минуса",
            "2026-09-28T22:21:05Z",
        )
        .await;
    assert_eq!(report.all_clears, 0);
    assert_eq!(w.messenger.count(), 1);
}

#[tokio::test]
async fn someone_who_blocked_the_bot_is_removed_and_the_rest_are_still_told() {
    let w = world(RelayPolicy::default());
    w.join(person("gone", KYIV.0, KYIV.1));
    w.join(person("here", KYIV.0, KYIV.1));
    w.messenger.fail("gone", vec![SendError::Blocked]);

    let report = w.hear(1, T0, "2 баллистики на Киев !", T0_PLUS_30S).await;

    assert_eq!(report.removed, 1);
    assert_eq!(report.alerts, 1);
    assert!(
        w.store
            .subscription(&Recipient::new("telegram", "gone"))
            .unwrap()
            .is_none()
    );
    assert_eq!(w.messenger.texts_to("here").len(), 1);
}

#[tokio::test]
async fn a_failed_send_is_not_recorded_so_the_next_post_can_still_reach_the_person() {
    let w = world(RelayPolicy::default());
    w.join(person("flaky", KYIV.0, KYIV.1));
    w.messenger
        .fail("flaky", vec![SendError::Unavailable("down".into())]);
    assert_eq!(
        w.hear(1, T0, "2 баллистики на Киев !", T0_PLUS_30S)
            .await
            .alerts,
        0
    );
    assert_eq!(
        w.hear(
            2,
            "2026-09-28T22:19:50Z",
            "ещё 2 баллистики на Киев !",
            "2026-09-28T22:20:00Z"
        )
        .await
        .alerts,
        1
    );
}

#[tokio::test]
async fn a_rate_limited_send_is_retried_once() {
    let w = world(RelayPolicy::default());
    w.join(person("busy", KYIV.0, KYIV.1));
    w.messenger.fail(
        "busy",
        vec![SendError::RateLimited {
            retry_after_secs: 0,
        }],
    );
    let report = w.hear(1, T0, "2 баллистики на Киев !", T0_PLUS_30S).await;
    assert_eq!(report.alerts, 1);
}

#[tokio::test]
async fn the_quote_can_be_left_out() {
    let w = world(RelayPolicy {
        include_text: false,
        ..RelayPolicy::default()
    });
    w.join(person("kyiv", KYIV.0, KYIV.1));
    w.hear(1, T0, "2 баллистики на Киев !", T0_PLUS_30S).await;
    let text = &w.messenger.texts_to("kyiv")[0];
    assert!(!text.contains('«'));
    assert!(text.contains("https://t.me/vanek_nikolaev/1"));
}

#[tokio::test]
async fn a_recipient_of_an_unknown_client_is_skipped_not_fatal() {
    let w = world(RelayPolicy::default());
    let mut stranger = person("x", KYIV.0, KYIV.1);
    stranger.recipient = Recipient::new("signal", "x");
    w.join(stranger);
    w.join(person("kyiv", KYIV.0, KYIV.1));
    let report = w.hear(1, T0, "2 баллистики на Киев !", T0_PLUS_30S).await;
    assert_eq!(report.alerts, 1);
}

// ---- False all-clears found by replaying a real channel ----
//
// Each of these was read as an all-clear at some point. Telling someone a threat is over when it
// is not is the worst thing this bot can do, so the cases stay here in the channel's own words.

async fn alerted_in_kyiv(kind_text: &str) -> World {
    let w = world(RelayPolicy::default());
    w.join(person("kyiv", KYIV.0, KYIV.1));
    let told = w
        .hear(1, "2026-09-28T22:19:42Z", kind_text, "2026-09-28T22:19:50Z")
        .await;
    assert_eq!(told.alerts, 1, "setup: the alert itself");
    w
}

#[tokio::test]
async fn until_the_all_clear_is_not_the_all_clear() {
    let w = alerted_in_kyiv("2 баллистики на Киев !").await;
    let report = w
        .hear(
            2,
            "2026-09-28T22:25:00Z",
            "угроза баллистики с брянска актуальна до отбоя тревоги",
            "2026-09-28T22:25:05Z",
        )
        .await;
    assert_eq!(report.all_clears, 0);
    assert_eq!(w.messenger.count(), 1);
}

#[tokio::test]
async fn an_interception_is_not_the_all_clear() {
    let w = alerted_in_kyiv("2 ракеты на Киев !").await;
    let report = w
        .hear(2, "2026-09-28T22:25:00Z", "все ракеты летят курсом на/через Черкассы\n\nесть уже первые сбития, по ракетам работают", "2026-09-28T22:25:05Z")
        .await;
    assert_eq!(report.all_clears, 0);
}

#[tokio::test]
async fn a_lost_track_is_not_the_all_clear() {
    let w = alerted_in_kyiv("1 реактивный мопед над Киевом").await;
    let report = w
        .hear(
            2,
            "2026-09-28T22:25:00Z",
            "этот мопед больше не фиксируется",
            "2026-09-28T22:25:05Z",
        )
        .await;
    assert_eq!(report.all_clears, 0);
}

#[tokio::test]
async fn an_all_clear_for_one_thing_in_a_post_that_still_reports_another_of_the_kind_is_held_back()
{
    let w = alerted_in_kyiv("2 баллистики на Киев !").await;
    // "no missiles for now" and, in the same post, "the ballistic threat is still relevant".
    let report = w
        .hear(
            2,
            "2026-09-28T22:27:00Z",
            "на сейчас минус по ракетам, что писал выше\n\nвсе эти пуски были с курской губернии\n\nмогут пустить остальные, угроза баллистики/противокорабельных ракет пока актуальна",
            "2026-09-28T22:27:05Z",
        )
        .await;
    assert_eq!(report.all_clears, 0);
}

#[tokio::test]
async fn an_all_clear_still_reaches_people_when_the_rest_of_the_post_is_about_somewhere_else() {
    let w = alerted_in_kyiv("1 реактивный мопед над Киевом").await;
    // Over Kyiv: minus. A drone near Vasylkiv is a different place, 30 km from Kyiv's centre.
    let report = w
        .hear(
            2,
            "2026-09-28T22:27:00Z",
            "по мопедам над Киевом на сейчас минуса\n\n1 реактивный мопед пролетел в районе Белой Церкви в сторону Василькова/Фастова",
            "2026-09-28T22:27:05Z",
        )
        .await;
    assert_eq!(report.all_clears, 1);
    assert!(w.messenger.texts_to("kyiv")[1].starts_with("✅"));
}

#[tokio::test]
async fn an_all_clear_that_names_places_and_no_kind_is_worded_by_its_places() {
    let w = world(RelayPolicy::default());
    w.join(person("odesa", ODESA.0, ODESA.1));
    w.hear(
        1,
        "2026-09-28T22:19:42Z",
        "пуски КАБ (УМПБ-5) курсом на Одессу",
        "2026-09-28T22:19:50Z",
    )
    .await;
    let report = w
        .hear(
            2,
            "2026-09-28T22:27:00Z",
            "минус по всему на Маяки или Одессу",
            "2026-09-28T22:27:05Z",
        )
        .await;
    assert_eq!(report.all_clears, 1);
    let text = &w.messenger.texts_to("odesa")[1];
    assert!(
        text.starts_with("✅ Джерело повідомляє про відбій: Маяки, Одеса."),
        "{text}"
    );
}
