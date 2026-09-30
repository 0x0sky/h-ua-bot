// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The endpoint `prism-hub` delivers to: `POST /api/v1/delivery`.
//!
//! The hub decides whom to tell and Porter writes the text. This only puts it in the right chat:
//! it checks the shared secret, sends, and answers in the shape the hub's delivery gateway reads.
//! The text is never inspected or changed.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use h_ua_core::ports::SendError;
use h_ua_telegram::{TelegramClient, Transport};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tracing::warn;

/// Header carrying the shared secret.
pub const SECRET_HEADER: &str = "x-prism-bot-delivery-secret";

/// Telegram's limit for the text of a message, in characters.
pub const TEXT_MAX_CHARS: usize = 4096;

/// Largest request body, in bytes. A message is at most 4096 characters.
const BODY_LIMIT: usize = 64 * 1024;

/// How many delivered keys are remembered.
const REMEMBERED: usize = 10_000;

/// Something that can put text in a chat.
#[async_trait]
pub trait Deliver: Send + Sync {
    /// Sends `text` to a chat, and to a topic in it when given, and returns Telegram's message id.
    async fn deliver(
        &self,
        chat_id: i64,
        thread_id: Option<i64>,
        text: &str,
    ) -> Result<i64, SendError>;
}

#[async_trait]
impl<T: Transport> Deliver for TelegramClient<T> {
    async fn deliver(
        &self,
        chat_id: i64,
        thread_id: Option<i64>,
        text: &str,
    ) -> Result<i64, SendError> {
        TelegramClient::deliver(self, chat_id, thread_id, text).await
    }
}

/// What the endpoint needs.
pub struct DeliveryState {
    secret: String,
    sender: Arc<dyn Deliver>,
    seen: Mutex<Seen>,
}

#[derive(Default)]
struct Seen {
    ids: HashMap<String, i64>,
    order: VecDeque<String>,
}

impl Seen {
    fn remember(&mut self, key: String, id: i64) {
        if self.ids.insert(key.clone(), id).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > REMEMBERED {
            if let Some(oldest) = self.order.pop_front() {
                self.ids.remove(&oldest);
            }
        }
    }
}

impl DeliveryState {
    /// State for an endpoint that sends through `sender` and accepts only `secret`.
    pub fn new(secret: impl Into<String>, sender: Arc<dyn Deliver>) -> Arc<Self> {
        Arc::new(Self {
            secret: secret.into(),
            sender,
            seen: Mutex::new(Seen::default()),
        })
    }
}

/// The endpoint as a router.
pub fn router(state: Arc<DeliveryState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/delivery", post(delivery))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(state)
}

/// Whether the process is up and serving. It says nothing about Telegram or the hub, and asks
/// for no secret, so a supervisor can poll it.
async fn health() -> Response {
    (StatusCode::OK, axum::Json(json!({"status": "ok"}))).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    chat_id: i64,
    #[serde(default)]
    message_thread_id: Option<i64>,
    text: String,
    idempotency_key: String,
}

fn failure(status: StatusCode, code: &str) -> Response {
    (
        status,
        axum::Json(json!({"status": "error", "error": {"code": code}})),
    )
        .into_response()
}

/// Compares in time that does not depend on where the strings differ.
fn same_secret(given: &[u8], expected: &[u8]) -> bool {
    let mut diff = given.len() ^ expected.len();
    for (index, byte) in expected.iter().enumerate() {
        diff |= usize::from(byte ^ given.get(index).copied().unwrap_or(0));
    }
    diff == 0
}

async fn delivery(
    State(state): State<Arc<DeliveryState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let given = headers
        .get(SECRET_HEADER)
        .map_or(&b""[..], |value| value.as_bytes());
    if given.is_empty() || !same_secret(given, state.secret.as_bytes()) {
        return failure(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let json_body = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    if !json_body {
        return failure(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");
    }
    let Ok(request) = serde_json::from_slice::<Request>(&body) else {
        return failure(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let text_length = request.text.chars().count();
    let usable = text_length > 0
        && text_length <= TEXT_MAX_CHARS
        && !request.idempotency_key.is_empty()
        && request.idempotency_key.len() <= 200
        && request.message_thread_id.is_none_or(|thread| thread > 0);
    if !usable {
        return failure(StatusCode::BAD_REQUEST, "invalid_request");
    }

    // One delivery at a time: a retry that arrives while the first is in flight must see its
    // outcome, not send a second copy.
    let mut seen = state.seen.lock().await;
    if let Some(id) = seen.ids.get(&request.idempotency_key) {
        return accepted(&request.idempotency_key, *id);
    }
    match state
        .sender
        .deliver(request.chat_id, request.message_thread_id, &request.text)
        .await
    {
        Ok(id) => {
            seen.remember(request.idempotency_key.clone(), id);
            accepted(&request.idempotency_key, id)
        }
        Err(SendError::Blocked) => failure(StatusCode::BAD_REQUEST, "recipient_unreachable"),
        Err(SendError::Rejected(_)) => failure(StatusCode::BAD_REQUEST, "message_rejected"),
        Err(SendError::RateLimited { retry_after_secs }) => (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(json!({
                "status": "error",
                "error": {"code": "rate_limited", "retry_after_seconds": retry_after_secs}
            })),
        )
            .into_response(),
        Err(SendError::Unavailable(reason)) => {
            // The reason can quote Telegram's answer; the log gets it and the hub does not.
            warn!(%reason, "Telegram did not take the message");
            failure(StatusCode::BAD_GATEWAY, "telegram_unavailable")
        }
    }
}

fn accepted(key: &str, message_id: i64) -> Response {
    let answer: Value = json!({
        "status": "ok",
        "delivery": {"idempotency_key": key, "provider_message_id": message_id}
    });
    (StatusCode::OK, axum::Json(answer)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_compare_exactly_and_only_whole() {
        assert!(same_secret(b"abc", b"abc"));
        assert!(!same_secret(b"abd", b"abc"));
        assert!(!same_secret(b"ab", b"abc"));
        assert!(!same_secret(b"abcd", b"abc"));
        assert!(!same_secret(b"", b"abc"));
    }

    #[test]
    fn only_the_newest_keys_are_remembered() {
        let mut seen = Seen::default();
        for number in 0..(REMEMBERED + 5) {
            seen.remember(format!("k{number}"), 1);
        }
        assert_eq!(seen.ids.len(), REMEMBERED);
        assert!(!seen.ids.contains_key("k0"));
        assert!(seen.ids.contains_key(&format!("k{}", REMEMBERED + 4)));
    }
}
