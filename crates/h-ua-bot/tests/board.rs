// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! An attack, delivered the way the hub delivers it, kept as one board and one status.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use h_ua_bot::board::{Boards, SIGNAL_SCHEMA};
use h_ua_bot::delivery::{Deliver, DeliveryState, SECRET_HEADER, router, unix_now};
use h_ua_core::ports::SendError;
use serde_json::{Value, json};

const SECRET: &str = "0123456789abcdef0123456789abcdef";

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Post { id: i64, text: String, silent: bool },
    Edit { id: i64, text: String },
    Delete { id: i64 },
}

#[derive(Default)]
struct FakeTelegram {
    calls: Mutex<Vec<Call>>,
    next_id: Mutex<i64>,
}

impl FakeTelegram {
    fn take(&self) -> Vec<Call> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
}

#[async_trait]
impl Deliver for FakeTelegram {
    async fn post(
        &self,
        chat: i64,
        _: Option<i64>,
        text: &str,
        silent: bool,
    ) -> Result<i64, SendError> {
        assert_eq!(chat, 42);
        let mut next = self.next_id.lock().unwrap();
        *next += 1;
        self.calls.lock().unwrap().push(Call::Post {
            id: *next,
            text: text.to_owned(),
            silent,
        });
        Ok(*next)
    }

    async fn edit(&self, _: i64, id: i64, text: &str) -> Result<(), SendError> {
        self.calls.lock().unwrap().push(Call::Edit {
            id,
            text: text.to_owned(),
        });
        Ok(())
    }

    async fn delete(&self, _: i64, id: i64) -> Result<(), SendError> {
        self.calls.lock().unwrap().push(Call::Delete { id });
        Ok(())
    }
}

struct Bot {
    url: String,
    telegram: Arc<FakeTelegram>,
    boards: Arc<Boards>,
}

async fn serve() -> Bot {
    let telegram = Arc::new(FakeTelegram::default());
    let boards = Arc::new(Boards::new());
    let app = router(DeliveryState::new(SECRET, telegram.clone(), boards.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Bot {
        url: format!("http://{address}/api/v1/delivery"),
        telegram,
        boards,
    }
}

fn alert(
    id: &str,
    class: &str,
    kind: &str,
    place: &str,
    proximity: &str,
    valid_until: &str,
) -> Value {
    json!({
        "schema_version": SIGNAL_SCHEMA,
        "event": "alert",
        "assessment_id": id,
        "hazard": {"class": class, "kinds": [kind]},
        "place": {"name": place},
        "proximity": proximity,
        "likelihood": null,
        "first_reported_at": "2026-10-27T03:51:00Z",
        "valid_until": valid_until,
        "event_at": "2026-10-27T03:51:00Z",
        "event_url": null,
        "sources": [{"source_id": "telegram.channel:kpszsu", "url": "https://t.me/kpszsu/1", "observed_at": "2026-10-27T03:51:00Z"}],
        "still_active": []
    })
}

fn retraction(id: &str) -> Value {
    json!({"schema_version": SIGNAL_SCHEMA, "event": "retraction", "assessment_id": id})
}

const LATER: &str = "2099-01-01T00:00:00Z";

impl Bot {
    async fn deliver(&self, key: &str, signal: Value) -> Value {
        let body = json!({
            "chat_id": 42,
            "message_thread_id": null,
            "text": "⚠️ the text Porter wrote",
            "idempotency_key": key,
            "signal": signal,
        });
        let response = reqwest::Client::new()
            .post(&self.url)
            .header("content-type", "application/json")
            .header(SECRET_HEADER, SECRET)
            .body(body.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        response.json().await.unwrap()
    }
}

fn texts(calls: &[Call]) -> Vec<(&str, &str)> {
    calls
        .iter()
        .map(|call| match call {
            Call::Post {
                text, silent: true, ..
            } => ("post quietly", first_line(text)),
            Call::Post { text, .. } => ("post and ring", first_line(text)),
            Call::Edit { text, .. } => ("edit", first_line(text)),
            Call::Delete { .. } => ("delete", ""),
        })
        .collect()
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

#[tokio::test]
async fn an_attack_is_one_board_and_a_status_that_rings_only_when_danger_grows() {
    let bot = serve().await;

    // Something passes near: a board, quietly, and a status that rings.
    let answer = bot
        .deliver(
            "1",
            alert("a", "drone", "air.attack_drone", "Бровари", "nearby", LATER),
        )
        .await;
    let calls = bot.telegram.take();
    assert_eq!(
        texts(&calls),
        [
            ("post quietly", "📡 Повідомлення джерел · з 05:51"),
            ("post and ring", "🟡 Загроза поруч із вашою зоною"),
        ]
    );
    assert_eq!(answer["delivery"]["provider_message_id"], 2);

    // A hub retry changes nothing.
    bot.deliver(
        "1",
        alert("a", "drone", "air.attack_drone", "Бровари", "nearby", LATER),
    )
    .await;
    assert!(bot.telegram.take().is_empty());

    // Now at the zone: the board is edited, a new status rings and the old one goes.
    bot.deliver(
        "2",
        alert(
            "b",
            "missile",
            "air.ballistic_missile",
            "Київ",
            "target",
            LATER,
        ),
    )
    .await;
    let calls = bot.telegram.take();
    assert_eq!(
        texts(&calls),
        [
            ("edit", "📡 Повідомлення джерел · з 05:51"),
            ("post and ring", "🔴 Загроза у вашій зоні"),
            ("delete", ""),
        ]
    );
    assert!(matches!(calls[0], Call::Edit { id: 1, .. }));
    assert_eq!(calls[2], Call::Delete { id: 2 });

    // More of the same is an edit of both, and rings nothing.
    bot.deliver(
        "3",
        alert(
            "c",
            "missile",
            "air.ballistic_missile",
            "Київ",
            "target",
            LATER,
        ),
    )
    .await;
    let calls = bot.telegram.take();
    assert_eq!(
        texts(&calls),
        [
            ("edit", "📡 Повідомлення джерел · з 05:51"),
            ("edit", "🔴 Загроза у вашій зоні"),
        ]
    );
    let Call::Edit { text, .. } = &calls[1] else {
        unreachable!()
    };
    assert!(text.contains("☄️ балістика — 2 повідомлення"), "{text}");

    // A report that lapsed before it arrived is history, not news.
    let stale = bot
        .deliver(
            "4",
            alert(
                "old",
                "drone",
                "air.attack_drone",
                "Київ",
                "target",
                "2000-01-01T00:00:00Z",
            ),
        )
        .await;
    assert!(bot.telegram.take().is_empty());
    assert_eq!(stale["status"], "ok");

    // Everything called off: the board closes, and a quiet status says it is not an all-clear.
    for (key, id) in [("5", "a"), ("6", "b")] {
        bot.deliver(key, retraction(id)).await;
    }
    bot.telegram.take();
    bot.deliver("7", retraction("c")).await;
    let calls = bot.telegram.take();
    assert_eq!(
        texts(&calls)
            .iter()
            .map(|(action, _)| *action)
            .collect::<Vec<_>>(),
        ["edit", "post quietly", "delete"]
    );
    let Call::Edit { text: board, .. } = &calls[0] else {
        unreachable!()
    };
    assert!(board.contains("Активних немає · завершились: 3"), "{board}");
    let Call::Post { text: status, .. } = &calls[1] else {
        unreachable!()
    };
    assert!(status.contains("Це не відбій"));

    // The next report starts a new board.
    bot.deliver(
        "8",
        alert("d", "drone", "air.attack_drone", "Київ", "target", LATER),
    )
    .await;
    let calls = bot.telegram.take();
    assert_eq!(
        texts(&calls),
        [
            ("post quietly", "📡 Повідомлення джерел · з 05:51"),
            ("post and ring", "🔴 Загроза у вашій зоні"),
            ("delete", ""),
        ]
    );
}

#[tokio::test]
async fn reports_lapse_on_the_tick_and_a_stale_position_is_mentioned_once() {
    let bot = serve().await;
    let now = unix_now();
    bot.boards.positioned(42, now - 4 * 24 * 3600).await;
    bot.deliver(
        "1",
        alert("a", "drone", "air.attack_drone", "Київ", "target", LATER),
    )
    .await;
    bot.telegram.take();

    bot.boards.tick(&*bot.telegram, now).await;
    assert!(bot.telegram.take().is_empty(), "nothing lapsed yet");

    bot.boards.tick(&*bot.telegram, 4_070_908_800).await; // 2099-01-01
    let calls = bot.telegram.take();
    let Call::Post { text, silent, .. } = &calls[1] else {
        panic!("{calls:?}")
    };
    assert!(silent);
    assert!(text.starts_with("⚪ Джерела затихли"));
    assert!(
        text.contains("📍 Ви досі там само? Позицію оновлено"),
        "{text}"
    );

    bot.deliver(
        "2",
        alert("b", "drone", "air.attack_drone", "Київ", "target", LATER),
    )
    .await;
    bot.boards.tick(&*bot.telegram, 4_070_908_800 + 3600).await;
    let calls = bot.telegram.take();
    let quiet = calls
        .iter()
        .find_map(|call| match call {
            Call::Post {
                text, silent: true, ..
            } if text.starts_with('⚪') => Some(text),
            _ => None,
        })
        .unwrap();
    assert!(!quiet.contains("📍"), "a reminder at most once a day");
}

#[tokio::test]
async fn a_report_the_bot_cannot_read_is_delivered_as_its_text() {
    let bot = serve().await;
    let mut odd = alert("a", "drone", "air.attack_drone", "Київ", "target", LATER);
    odd["schema_version"] = json!("prism-hub.signal-alert.v9");

    bot.deliver("1", odd).await;

    assert_eq!(
        bot.telegram.take(),
        [Call::Post {
            id: 1,
            text: "⚠️ the text Porter wrote".into(),
            silent: false
        }]
    );
}
