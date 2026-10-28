// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The delivery endpoint, called the way `prism-hub`'s bot delivery gateway calls it.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use h_ua_bot::delivery::{Deliver, DeliveryState, SECRET_HEADER, router};
use h_ua_core::ports::SendError;
use serde_json::{Value, json};

const SECRET: &str = "0123456789abcdef0123456789abcdef";

#[derive(Default)]
struct FakeTelegram {
    sent: Mutex<Vec<(i64, Option<i64>, String)>>,
    next: Mutex<Vec<Result<i64, SendError>>>,
    silent: Mutex<Vec<bool>>,
    edits: Mutex<Vec<(i64, i64, String)>>,
    edit_answers: Mutex<Vec<Result<(), SendError>>>,
    deleted: Mutex<Vec<(i64, i64)>>,
    pinned: Mutex<Vec<(i64, i64)>>,
}

#[async_trait]
impl Deliver for FakeTelegram {
    async fn edit(&self, chat: i64, id: i64, text: &str) -> Result<(), SendError> {
        self.edits.lock().unwrap().push((chat, id, text.to_owned()));
        let mut answers = self.edit_answers.lock().unwrap();
        if answers.is_empty() {
            Ok(())
        } else {
            answers.remove(0)
        }
    }

    async fn delete(&self, chat: i64, id: i64) -> Result<(), SendError> {
        self.deleted.lock().unwrap().push((chat, id));
        Err(SendError::Rejected("message can't be deleted".into()))
    }

    async fn pin(&self, chat: i64, id: i64) -> Result<(), SendError> {
        self.pinned.lock().unwrap().push((chat, id));
        Ok(())
    }

    async fn post(
        &self,
        chat: i64,
        thread: Option<i64>,
        text: &str,
        silent: bool,
    ) -> Result<i64, SendError> {
        self.silent.lock().unwrap().push(silent);
        self.sent
            .lock()
            .unwrap()
            .push((chat, thread, text.to_owned()));
        let mut next = self.next.lock().unwrap();
        if next.is_empty() {
            Ok(1000)
        } else {
            next.remove(0)
        }
    }
}

async fn serve() -> (String, Arc<FakeTelegram>) {
    let telegram = Arc::new(FakeTelegram::default());
    let app = router(DeliveryState::new(SECRET, telegram.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{address}/api/v1/delivery"), telegram)
}

fn body(key: &str) -> Value {
    json!({"chat_id": 42, "message_thread_id": null, "text": "⚠️ БпЛА — Київ", "idempotency_key": key})
}

async fn post(url: &str, secret: Option<&str>, body: &Value) -> (u16, Value) {
    let mut request = reqwest::Client::new()
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(body.to_string());
    if let Some(secret) = secret {
        request = request.header(SECRET_HEADER, secret);
    }
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn a_delivery_is_sent_and_answered_in_the_shape_the_hub_reads() {
    let (url, telegram) = serve().await;

    let (status, answer) = post(&url, Some(SECRET), &body("k1")).await;

    assert_eq!(status, 200);
    assert_eq!(
        answer,
        json!({"status": "ok", "delivery": {"idempotency_key": "k1", "provider_message_id": 1000}})
    );
    assert_eq!(
        telegram.sent.lock().unwrap().as_slice(),
        [(42, None, "⚠️ БпЛА — Київ".to_owned())]
    );
}

#[tokio::test]
async fn a_topic_is_passed_through() {
    let (url, telegram) = serve().await;
    let mut request = body("k1");
    request["message_thread_id"] = json!(9);

    post(&url, Some(SECRET), &request).await;

    assert_eq!(telegram.sent.lock().unwrap()[0].1, Some(9));
}

#[tokio::test]
async fn a_retry_gets_the_first_answer_and_sends_nothing_twice() {
    let (url, telegram) = serve().await;

    let first = post(&url, Some(SECRET), &body("same")).await;
    telegram.next.lock().unwrap().push(Ok(2000));
    let second = post(&url, Some(SECRET), &body("same")).await;

    assert_eq!(first, second);
    assert_eq!(telegram.sent.lock().unwrap().len(), 1);
    let (_, other) = post(&url, Some(SECRET), &body("different")).await;
    assert_eq!(other["delivery"]["provider_message_id"], 2000);
}

#[tokio::test]
async fn nothing_is_sent_without_the_secret() {
    let (url, telegram) = serve().await;

    for secret in [None, Some(""), Some("wrong"), Some(&SECRET[..31])] {
        let (status, answer) = post(&url, secret, &body("k")).await;
        assert_eq!(status, 401, "{secret:?}");
        assert_eq!(answer["error"]["code"], "unauthorized");
    }
    assert!(telegram.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn malformed_requests_are_refused_before_anything_is_sent() {
    let (url, telegram) = serve().await;
    let bad = [
        json!({"chat_id": 42, "text": "x"}),
        json!({"chat_id": "42", "text": "x", "idempotency_key": "k"}),
        json!({"chat_id": 42, "text": "", "idempotency_key": "k"}),
        json!({"chat_id": 42, "text": "x".repeat(4097), "idempotency_key": "k"}),
        json!({"chat_id": 42, "text": "x", "idempotency_key": "k", "message_thread_id": 0}),
        json!({"chat_id": 42, "text": "x", "idempotency_key": "k", "parse_mode": "HTML"}),
        json!({"chat_id": 42, "text": "x", "idempotency_key": "k", "edit_message_id": 0}),
        json!({"chat_id": 42, "text": "x", "idempotency_key": "k", "delete_message_ids": [-1]}),
        json!({"chat_id": 42, "text": "x", "idempotency_key": "k", "delete_message_ids": (1..=11).collect::<Vec<_>>()}),
        json!({"chat_id": 42, "text": "x", "idempotency_key": "k", "silent": "yes"}),
        json!([1, 2]),
    ];
    for request in bad {
        let (status, answer) = post(&url, Some(SECRET), &request).await;
        assert_eq!(status, 400, "{request}");
        assert_eq!(answer["error"]["code"], "invalid_request");
    }
    let plain = reqwest::Client::new()
        .post(&url)
        .header(SECRET_HEADER, SECRET)
        .body("x")
        .send()
        .await
        .unwrap();
    assert_eq!(plain.status().as_u16(), 415);
    assert!(telegram.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn telegrams_refusals_map_to_what_the_hub_does_with_them() {
    let (url, telegram) = serve().await;
    let cases = [
        (SendError::Blocked, 400, "recipient_unreachable"),
        (SendError::Rejected("nope".into()), 400, "message_rejected"),
        (
            SendError::Unavailable("timeout, token=SECRET".into()),
            502,
            "telegram_unavailable",
        ),
    ];
    for (number, (error, status, code)) in cases.into_iter().enumerate() {
        telegram.next.lock().unwrap().push(Err(error));
        let (got, answer) = post(&url, Some(SECRET), &body(&format!("k{number}"))).await;
        assert_eq!(
            (got, answer["error"]["code"].as_str().unwrap()),
            (status, code)
        );
        assert!(!answer.to_string().contains("SECRET"));
    }
    telegram
        .next
        .lock()
        .unwrap()
        .push(Err(SendError::RateLimited {
            retry_after_secs: 7,
        }));
    let (status, answer) = post(&url, Some(SECRET), &body("limited")).await;
    assert_eq!(status, 429);
    assert_eq!(answer["error"]["retry_after_seconds"], 7);
}

#[tokio::test]
async fn a_failed_delivery_can_be_retried_and_then_goes_through() {
    let (url, telegram) = serve().await;
    telegram
        .next
        .lock()
        .unwrap()
        .push(Err(SendError::Unavailable("down".into())));

    let (first, _) = post(&url, Some(SECRET), &body("retry")).await;
    let (second, answer) = post(&url, Some(SECRET), &body("retry")).await;

    assert_eq!((first, second), (502, 200));
    assert_eq!(answer["delivery"]["provider_message_id"], 1000);
}

#[tokio::test]
async fn health_answers_without_a_secret_and_sends_nothing() {
    let (url, telegram) = serve().await;
    let health = url.replace("/api/v1/delivery", "/health");

    let response = reqwest::get(&health).await.unwrap();

    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"status": "ok"})
    );
    assert!(telegram.sent.lock().unwrap().is_empty());
    let posted = reqwest::Client::new().post(&health).send().await.unwrap();
    assert_eq!(posted.status().as_u16(), 405);
}

#[tokio::test]
async fn a_plain_delivery_rings_edits_nothing_and_deletes_nothing() {
    let (url, telegram) = serve().await;

    post(&url, Some(SECRET), &body("k")).await;

    assert_eq!(telegram.silent.lock().unwrap().as_slice(), [false]);
    assert!(telegram.edits.lock().unwrap().is_empty());
    assert!(telegram.deleted.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_board_is_pinned_when_sent_anew_and_an_edit_keeps_its_pin() {
    let (url, telegram) = serve().await;
    let mut request = body("board-v1");
    request["pin"] = json!(true);
    request["silent"] = json!(true);
    post(&url, Some(SECRET), &request).await;
    assert_eq!(telegram.pinned.lock().unwrap().as_slice(), [(42, 1000)]);

    let mut edit = body("board-v2");
    edit["pin"] = json!(true);
    edit["edit_message_id"] = json!(1000);
    post(&url, Some(SECRET), &edit).await;
    assert_eq!(
        telegram.pinned.lock().unwrap().len(),
        1,
        "an edit is not pinned again"
    );

    post(&url, Some(SECRET), &body("plain")).await;
    assert_eq!(
        telegram.pinned.lock().unwrap().len(),
        1,
        "nothing else is pinned"
    );
}

#[tokio::test]
async fn a_view_is_kept_current_by_editing_the_message_the_hub_names() {
    let (url, telegram) = serve().await;
    let mut request = body("board-v2");
    request["text"] = json!("📡 табло, версія 2");
    request["edit_message_id"] = json!(55);

    let (status, answer) = post(&url, Some(SECRET), &request).await;

    assert_eq!(status, 200);
    assert_eq!(answer["delivery"]["provider_message_id"], 55);
    assert_eq!(
        telegram.edits.lock().unwrap().as_slice(),
        [(42, 55, "📡 табло, версія 2".to_owned())]
    );
    assert!(
        telegram.sent.lock().unwrap().is_empty(),
        "an edit sends nothing new"
    );
}

#[tokio::test]
async fn a_message_that_cannot_be_edited_is_sent_anew_and_its_new_id_returned() {
    let (url, telegram) = serve().await;
    telegram
        .edit_answers
        .lock()
        .unwrap()
        .push(Err(SendError::Rejected("message to edit not found".into())));
    let mut request = body("board-v3");
    request["edit_message_id"] = json!(55);
    request["silent"] = json!(true);

    let (_, answer) = post(&url, Some(SECRET), &request).await;

    assert_eq!(answer["delivery"]["provider_message_id"], 1000);
    assert_eq!(telegram.sent.lock().unwrap().len(), 1);
    assert_eq!(telegram.silent.lock().unwrap().as_slice(), [true]);
}

#[tokio::test]
async fn an_edit_that_fails_for_another_reason_is_retried_by_the_hub_not_resent() {
    let (url, telegram) = serve().await;
    telegram
        .edit_answers
        .lock()
        .unwrap()
        .push(Err(SendError::Unavailable("down".into())));
    let mut request = body("board-v4");
    request["edit_message_id"] = json!(55);

    let (status, _) = post(&url, Some(SECRET), &request).await;

    assert_eq!(status, 502);
    assert!(telegram.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_new_status_replaces_the_old_ones_which_are_deleted_best_effort() {
    let (url, telegram) = serve().await;
    let mut request = body("status-2");
    request["text"] = json!("🔴 Загроза у вашій зоні");
    request["delete_message_ids"] = json!([77, 78]);

    let (status, answer) = post(&url, Some(SECRET), &request).await;

    assert_eq!(
        status, 200,
        "a message Telegram would not delete fails nothing"
    );
    assert_eq!(answer["delivery"]["provider_message_id"], 1000);
    assert_eq!(
        telegram.deleted.lock().unwrap().as_slice(),
        [(42, 77), (42, 78)]
    );
    assert_eq!(telegram.silent.lock().unwrap().as_slice(), [false]);
}

#[tokio::test]
async fn the_message_carrying_the_text_is_never_deleted() {
    let (url, telegram) = serve().await;
    let mut request = body("board-v5");
    request["edit_message_id"] = json!(55);
    request["delete_message_ids"] = json!([55, 54]);

    post(&url, Some(SECRET), &request).await;

    assert_eq!(telegram.deleted.lock().unwrap().as_slice(), [(42, 54)]);
}
