// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The endpoint `prism-hub` delivers to: `POST /api/v1/delivery`.
//!
//! The hub decides whom to tell and Porter writes the text. This only puts it in the right chat:
//! it checks the shared secret, sends, and answers in the shape the hub's delivery gateway reads.
//! The text is never inspected or changed.
//!
//! The bot is a view: what to show and when to ring are the hub's to decide. So a delivery may
//! also say to replace a message sent earlier instead of sending a new one (`edit_message_id`),
//! to remove messages it supersedes (`delete_message_ids`), and to arrive without a sound
//! (`silent`), and to be pinned above the chat (`pin`). The hub keeps the message ids from the
//! answers; the bot keeps none.

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

/// Most messages one delivery may remove.
const DELETE_MAX: usize = 10;

/// How many delivered keys are remembered.
const REMEMBERED: usize = 10_000;

/// Something that can put text in a chat and keep it current.
#[async_trait]
pub trait Deliver: Send + Sync {
    /// Sends `text` to a chat, and to a topic in it when given, and returns Telegram's message id.
    /// A `silent` message arrives without a sound.
    async fn post(
        &self,
        chat_id: i64,
        thread_id: Option<i64>,
        text: &str,
        silent: bool,
    ) -> Result<i64, SendError>;

    /// Replaces the text of a message sent earlier.
    async fn edit(&self, chat_id: i64, message_id: i64, text: &str) -> Result<(), SendError>;

    /// Deletes a message sent earlier.
    async fn delete(&self, chat_id: i64, message_id: i64) -> Result<(), SendError>;

    /// Pins a message above the chat, without a notification.
    async fn pin(&self, chat_id: i64, message_id: i64) -> Result<(), SendError>;
}

#[async_trait]
impl<T: Transport> Deliver for TelegramClient<T> {
    async fn post(
        &self,
        chat_id: i64,
        thread_id: Option<i64>,
        text: &str,
        silent: bool,
    ) -> Result<i64, SendError> {
        TelegramClient::post(self, chat_id, thread_id, text, silent).await
    }

    async fn edit(&self, chat_id: i64, message_id: i64, text: &str) -> Result<(), SendError> {
        TelegramClient::edit(self, chat_id, message_id, text).await
    }

    async fn delete(&self, chat_id: i64, message_id: i64) -> Result<(), SendError> {
        TelegramClient::delete(self, chat_id, message_id).await
    }

    async fn pin(&self, chat_id: i64, message_id: i64) -> Result<(), SendError> {
        TelegramClient::pin(self, chat_id, message_id).await
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
    /// Replace this message, sent earlier to the same chat, instead of sending a new one.
    #[serde(default)]
    edit_message_id: Option<i64>,
    /// Remove these messages, sent earlier to the same chat, once the text is shown.
    #[serde(default)]
    delete_message_ids: Vec<i64>,
    /// Arrive without a sound.
    #[serde(default)]
    silent: bool,
    /// Pin the message above the chat when it is sent anew. An edit keeps the pin it has.
    #[serde(default)]
    pin: bool,
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
        && request.message_thread_id.is_none_or(|thread| thread > 0)
        && request.edit_message_id.is_none_or(|id| id > 0)
        && request.delete_message_ids.len() <= DELETE_MAX
        && request.delete_message_ids.iter().all(|id| *id > 0);
    if !usable {
        return failure(StatusCode::BAD_REQUEST, "invalid_request");
    }

    // One delivery at a time: a retry that arrives while the first is in flight must see its
    // outcome, not send a second copy.
    let mut seen = state.seen.lock().await;
    if let Some(id) = seen.ids.get(&request.idempotency_key) {
        return accepted(&request.idempotency_key, *id);
    }
    match show(&*state.sender, &request).await {
        Ok((id, posted)) => {
            if request.pin && posted {
                // A board that is not pinned still shows; it is just not on top.
                if let Err(error) = state.sender.pin(request.chat_id, id).await {
                    warn!(%error, "a message was not pinned");
                }
            }
            for old in request.delete_message_ids.iter().filter(|old| **old != id) {
                // Telegram refuses after 48 hours, or when the person already deleted it. A
                // message that stays is no reason to show the text twice.
                if let Err(error) = state.sender.delete(request.chat_id, *old).await {
                    warn!(%error, "a superseded message was not deleted");
                }
            }
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

/// Shows the text, by an edit when asked and possible, and returns the id of the message that
/// carries it and whether it was sent anew.
async fn show(sender: &dyn Deliver, request: &Request) -> Result<(i64, bool), SendError> {
    let (chat, thread, text) = (request.chat_id, request.message_thread_id, &request.text);
    if let Some(id) = request.edit_message_id {
        match sender.edit(chat, id, text).await {
            Ok(()) => return Ok((id, false)),
            // Gone, deleted by the person, or otherwise not editable: the text is sent anew and
            // the answer carries the new id.
            Err(SendError::Rejected(reason)) => {
                warn!(%reason, "a message could not be edited; sending it anew");
            }
            Err(error) => return Err(error),
        }
    }
    let id = sender.post(chat, thread, text, request.silent).await?;
    Ok((id, true))
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
