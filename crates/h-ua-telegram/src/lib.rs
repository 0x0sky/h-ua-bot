// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Telegram as a client of h-ua-bot: the first [`Messenger`].
//!
//! Talks to the Bot API by long polling, so it needs no public address. It understands three
//! things a person can do in a private chat: send a command, share a location, and update a live
//! location. Everything else is ignored.

mod transport;

use std::sync::Mutex;

use async_trait::async_trait;
use h_ua_core::message::Message;
use h_ua_core::ports::{ClientError, Event, EventKind, Messenger, SendError};
use h_ua_core::subscriber::Recipient;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::debug;

pub use transport::{ReqwestTransport, Transport, TransportError};

/// Name of this client in a [`Recipient`].
pub const CLIENT_NAME: &str = "telegram";

/// Seconds Telegram holds a `getUpdates` request open waiting for something to happen.
const LONG_POLL_SECS: u64 = 30;

/// Label of the button that shares a position.
const LOCATION_BUTTON: &str = "📍 Поділитися позицією";

/// Telegram, over a [`Transport`].
pub struct TelegramClient<T> {
    transport: T,
    /// The `offset` for the next `getUpdates`: one past the last update handled.
    next_offset: Mutex<Option<i64>>,
}

impl<T: Transport> TelegramClient<T> {
    /// A client over a transport.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            next_offset: Mutex::new(None),
        }
    }

    /// The bot's own username, from `getMe`. Fails if the token is wrong.
    pub async fn username(&self) -> Result<String, ClientError> {
        let result = self
            .transport
            .call("getMe", &json!({}))
            .await
            .map_err(|e| ClientError(e.to_string()))?;
        let reply: Reply<Me> =
            serde_json::from_value(result).map_err(|e| ClientError(e.to_string()))?;
        match reply {
            Reply {
                ok: true,
                result: Some(me),
                ..
            } => Ok(me.username.unwrap_or_default()),
            other => Err(ClientError(
                other
                    .description
                    .unwrap_or_else(|| "getMe failed".to_owned()),
            )),
        }
    }
}

impl<T: Transport> TelegramClient<T> {
    /// Sends text to a chat, and to a topic in it when `thread_id` is given, and returns the id
    /// Telegram gave the message. This is what the hub's delivery endpoint uses.
    pub async fn deliver(
        &self,
        chat: i64,
        thread_id: Option<i64>,
        text: &str,
    ) -> Result<i64, SendError> {
        self.send_message(json!(chat), thread_id, text, false)
            .await?
            .ok_or_else(|| SendError::Unavailable("sendMessage returned no message_id".to_owned()))
    }

    async fn send_message(
        &self,
        chat: Value,
        thread_id: Option<i64>,
        text: &str,
        ask_location: bool,
    ) -> Result<Option<i64>, SendError> {
        let mut body = json!({
            "chat_id": chat,
            "text": text,
            // A preview of the source post under every alert is noise, and the link is there
            // for whoever wants the original.
            "link_preview_options": {"is_disabled": true},
        });
        if let Some(thread) = thread_id {
            body["message_thread_id"] = json!(thread);
        }
        if ask_location {
            body["reply_markup"] = json!({
                "keyboard": [[{"text": LOCATION_BUTTON, "request_location": true}]],
                "resize_keyboard": true,
                "one_time_keyboard": true,
            });
        }
        let raw = self
            .transport
            .call("sendMessage", &body)
            .await
            .map_err(|e| SendError::Unavailable(e.to_string()))?;
        let reply: Reply<Value> =
            serde_json::from_value(raw).map_err(|e| SendError::Unavailable(e.to_string()))?;
        if !reply.ok {
            return Err(send_error(&reply));
        }
        Ok(reply
            .result
            .as_ref()
            .and_then(|message| message.get("message_id"))
            .and_then(Value::as_i64))
    }
}

#[derive(Deserialize)]
struct Me {
    username: Option<String>,
}

#[derive(Deserialize)]
struct Reply<R> {
    ok: bool,
    result: Option<R>,
    error_code: Option<i64>,
    description: Option<String>,
    parameters: Option<Parameters>,
}

#[derive(Deserialize)]
struct Parameters {
    retry_after: Option<u64>,
}

#[derive(Deserialize)]
struct Update {
    update_id: i64,
    message: Option<Incoming>,
    edited_message: Option<Incoming>,
}

#[derive(Deserialize)]
struct Incoming {
    chat: Chat,
    from: Option<Sender>,
    text: Option<String>,
    location: Option<Location>,
}

#[derive(Deserialize)]
struct Chat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Sender {
    #[serde(default)]
    is_bot: bool,
}

#[derive(Deserialize)]
struct Location {
    latitude: f64,
    longitude: f64,
}

/// Turns an update into what the person did, if it is something the bot understands.
fn event_from(update: Update) -> Option<Event> {
    let (message, edited) = match (update.message, update.edited_message) {
        (Some(message), _) => (message, false),
        (None, Some(message)) => (message, true),
        (None, None) => return None,
    };
    if message.chat.kind != "private" || message.from.as_ref().is_some_and(|s| s.is_bot) {
        return None;
    }
    let from = Recipient::new(CLIENT_NAME, message.chat.id.to_string());
    if let Some(location) = message.location {
        return Some(Event {
            from,
            kind: EventKind::Location {
                lat: location.latitude,
                lon: location.longitude,
                // A live location arrives once as a message and afterwards as edits of it.
                live: edited,
            },
        });
    }
    if edited {
        return None;
    }
    let text = message.text?;
    let kind = match text.strip_prefix('/') {
        Some(rest) => {
            let (word, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            // `/start@ua_alerts_bot` addresses this bot; the suffix is not part of the name.
            let name = word.split('@').next().unwrap_or(word).to_lowercase();
            EventKind::Command {
                name,
                args: args.trim().to_owned(),
            }
        }
        None => EventKind::Other,
    };
    Some(Event { from, kind })
}

fn send_error(reply: &Reply<Value>) -> SendError {
    let description = reply.description.clone().unwrap_or_default();
    match reply.error_code {
        Some(403) => SendError::Blocked,
        Some(429) => SendError::RateLimited {
            retry_after_secs: reply
                .parameters
                .as_ref()
                .and_then(|p| p.retry_after)
                .unwrap_or(1),
        },
        Some(400) if description.contains("chat not found") => SendError::Blocked,
        Some(code) if (400..500).contains(&code) => SendError::Rejected(description),
        _ => SendError::Unavailable(description),
    }
}

fn chat_id(recipient: &Recipient) -> Value {
    match recipient.address.parse::<i64>() {
        Ok(id) => json!(id),
        Err(_) => json!(recipient.address),
    }
}

#[async_trait]
impl<T: Transport> Messenger for TelegramClient<T> {
    fn name(&self) -> &'static str {
        CLIENT_NAME
    }

    async fn send(&self, to: &Recipient, message: &Message) -> Result<(), SendError> {
        self.send_message(chat_id(to), None, &message.text, message.ask_location)
            .await
            .map(|_| ())
    }

    async fn poll(&self) -> Result<Vec<Event>, ClientError> {
        let offset = *self
            .next_offset
            .lock()
            .map_err(|_| ClientError("poisoned".to_owned()))?;
        let mut body = json!({
            "timeout": LONG_POLL_SECS,
            "allowed_updates": ["message", "edited_message"],
        });
        if let Some(offset) = offset {
            body["offset"] = json!(offset);
        }
        let raw = self
            .transport
            .call("getUpdates", &body)
            .await
            .map_err(|e| ClientError(e.to_string()))?;
        let reply: Reply<Vec<Update>> =
            serde_json::from_value(raw).map_err(|e| ClientError(e.to_string()))?;
        let updates = match reply {
            Reply {
                ok: true,
                result: Some(updates),
                ..
            } => updates,
            other => {
                return Err(ClientError(
                    other
                        .description
                        .unwrap_or_else(|| "getUpdates failed".to_owned()),
                ));
            }
        };
        if let Some(last) = updates.iter().map(|u| u.update_id).max() {
            *self
                .next_offset
                .lock()
                .map_err(|_| ClientError("poisoned".to_owned()))? = Some(last + 1);
        }
        debug!(count = updates.len(), "telegram updates");
        Ok(updates.into_iter().filter_map(event_from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    /// A transport that answers from a script and records what it was asked.
    #[derive(Default)]
    struct Script {
        answers: StdMutex<Vec<Result<Value, TransportError>>>,
        calls: StdMutex<Vec<(String, Value)>>,
    }

    struct Fake(Arc<Script>);

    impl Fake {
        fn answering(answers: Vec<Result<Value, TransportError>>) -> (Self, Arc<Script>) {
            let script = Arc::new(Script {
                answers: StdMutex::new(answers),
                ..Script::default()
            });
            (Self(script.clone()), script)
        }
    }

    #[async_trait]
    impl Transport for Fake {
        async fn call(&self, method: &str, body: &Value) -> Result<Value, TransportError> {
            self.0
                .calls
                .lock()
                .unwrap()
                .push((method.to_owned(), body.clone()));
            self.0.answers.lock().unwrap().remove(0)
        }
    }

    fn updates(list: Value) -> Value {
        json!({"ok": true, "result": list})
    }

    fn private(update_id: i64, chat: i64, rest: Value) -> Value {
        let mut message = json!({"message_id": 1, "chat": {"id": chat, "type": "private"}, "from": {"id": chat, "is_bot": false}});
        for (key, value) in rest.as_object().unwrap() {
            message[key] = value.clone();
        }
        json!({"update_id": update_id, "message": message})
    }

    async fn heard(list: Value) -> Vec<Event> {
        let (fake, _) = Fake::answering(vec![Ok(updates(list))]);
        TelegramClient::new(fake).poll().await.unwrap()
    }

    #[tokio::test]
    async fn commands_are_read_with_their_arguments_and_without_the_bot_suffix() {
        let events = heard(json!([
            private(1, 42, json!({"text": "/start"})),
            private(
                2,
                42,
                json!({"text": "/Kinds@ua_alerts_bot  дрони   каби "})
            ),
            private(3, 42, json!({"text": "привіт"})),
        ]))
        .await;
        let me = Recipient::new("telegram", "42");
        assert_eq!(
            events,
            vec![
                Event {
                    from: me.clone(),
                    kind: EventKind::Command {
                        name: "start".into(),
                        args: "".into()
                    }
                },
                Event {
                    from: me.clone(),
                    kind: EventKind::Command {
                        name: "kinds".into(),
                        args: "дрони   каби".into()
                    }
                },
                Event {
                    from: me,
                    kind: EventKind::Other
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_shared_location_and_a_live_update_are_told_apart() {
        let live = json!({"update_id": 2, "edited_message": {
            "message_id": 1, "chat": {"id": 42, "type": "private"}, "from": {"id": 42, "is_bot": false},
            "location": {"latitude": 50.0, "longitude": 30.0, "live_period": 3600}}});
        let events = heard(json!([
            private(
                1,
                42,
                json!({"location": {"latitude": 50.45, "longitude": 30.52}})
            ),
            live,
        ]))
        .await;
        assert_eq!(
            events[0].kind,
            EventKind::Location {
                lat: 50.45,
                lon: 30.52,
                live: false
            }
        );
        assert_eq!(
            events[1].kind,
            EventKind::Location {
                lat: 50.0,
                lon: 30.0,
                live: true
            }
        );
    }

    #[tokio::test]
    async fn groups_bots_edited_text_and_other_updates_are_ignored() {
        let group = json!({"update_id": 1, "message": {"message_id": 1, "chat": {"id": -100, "type": "supergroup"}, "from": {"id": 5, "is_bot": false}, "text": "/start"}});
        let from_bot = json!({"update_id": 2, "message": {"message_id": 1, "chat": {"id": 7, "type": "private"}, "from": {"id": 7, "is_bot": true}, "text": "/start"}});
        let edited_text = json!({"update_id": 3, "edited_message": {"message_id": 1, "chat": {"id": 8, "type": "private"}, "from": {"id": 8, "is_bot": false}, "text": "/stop"}});
        let other = json!({"update_id": 4, "poll": {}});
        assert!(
            heard(json!([group, from_bot, edited_text, other]))
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn the_offset_moves_past_everything_seen_even_when_it_was_ignored() {
        let (fake, script) = Fake::answering(vec![
            Ok(updates(json!([
                json!({"update_id": 10, "poll": {}}),
                private(11, 1, json!({"text": "/help"}))
            ]))),
            Ok(updates(json!([]))),
        ]);
        let client = TelegramClient::new(fake);
        client.poll().await.unwrap();
        client.poll().await.unwrap();
        let calls = script.calls.lock().unwrap();
        assert_eq!(calls[0].0, "getUpdates");
        assert_eq!(calls[0].1.get("offset"), None);
        assert_eq!(calls[0].1["timeout"], 30);
        assert_eq!(
            calls[0].1["allowed_updates"],
            json!(["message", "edited_message"])
        );
        assert_eq!(calls[1].1["offset"], 12);
    }

    #[tokio::test]
    async fn a_refused_poll_is_an_error_not_an_empty_answer() {
        let (fake, _) = Fake::answering(vec![Ok(
            json!({"ok": false, "error_code": 401, "description": "Unauthorized"}),
        )]);
        let error = TelegramClient::new(fake).poll().await.unwrap_err();
        assert_eq!(error.0, "Unauthorized");
    }

    fn ok() -> Result<Value, TransportError> {
        Ok(json!({"ok": true, "result": {}}))
    }

    #[tokio::test]
    async fn an_alert_is_plain_text_without_a_keyboard_or_a_preview() {
        let (fake, script) = Fake::answering(vec![ok()]);
        TelegramClient::new(fake)
            .send(
                &Recipient::new("telegram", "42"),
                &Message::text("⚠️ x_y *z*"),
            )
            .await
            .unwrap();
        let calls = script.calls.lock().unwrap();
        assert_eq!(calls[0].0, "sendMessage");
        assert_eq!(calls[0].1["chat_id"], 42);
        assert_eq!(calls[0].1["text"], "⚠️ x_y *z*");
        assert_eq!(calls[0].1["link_preview_options"]["is_disabled"], true);
        assert_eq!(
            calls[0].1.get("parse_mode"),
            None,
            "no markup, so nothing to escape"
        );
        assert_eq!(calls[0].1.get("reply_markup"), None);
    }

    #[tokio::test]
    async fn asking_for_a_location_adds_the_share_button() {
        let (fake, script) = Fake::answering(vec![ok()]);
        TelegramClient::new(fake)
            .send(
                &Recipient::new("telegram", "42"),
                &Message::asking_location("де ви?"),
            )
            .await
            .unwrap();
        let markup = script.calls.lock().unwrap()[0].1["reply_markup"].clone();
        assert_eq!(markup["keyboard"][0][0]["request_location"], true);
        assert_eq!(markup["one_time_keyboard"], true);
    }

    async fn failing_with(answer: Result<Value, TransportError>) -> SendError {
        let (fake, _) = Fake::answering(vec![answer]);
        TelegramClient::new(fake)
            .send(&Recipient::new("telegram", "1"), &Message::text("x"))
            .await
            .unwrap_err()
    }

    #[tokio::test]
    async fn telegram_errors_map_to_what_the_relay_needs_to_know() {
        let blocked = json!({"ok": false, "error_code": 403, "description": "Forbidden: bot was blocked by the user"});
        assert_eq!(failing_with(Ok(blocked)).await, SendError::Blocked);
        let gone =
            json!({"ok": false, "error_code": 400, "description": "Bad Request: chat not found"});
        assert_eq!(failing_with(Ok(gone)).await, SendError::Blocked);
        let slow = json!({"ok": false, "error_code": 429, "description": "Too Many Requests", "parameters": {"retry_after": 7}});
        assert_eq!(
            failing_with(Ok(slow)).await,
            SendError::RateLimited {
                retry_after_secs: 7
            }
        );
        let bad = json!({"ok": false, "error_code": 400, "description": "Bad Request: message is too long"});
        assert_eq!(
            failing_with(Ok(bad)).await,
            SendError::Rejected("Bad Request: message is too long".into())
        );
        let down = json!({"ok": false, "error_code": 502, "description": "Bad Gateway"});
        assert!(matches!(
            failing_with(Ok(down)).await,
            SendError::Unavailable(_)
        ));
        assert!(matches!(
            failing_with(Err(TransportError("timeout".into()))).await,
            SendError::Unavailable(_)
        ));
    }

    #[tokio::test]
    async fn the_username_comes_from_get_me() {
        let (fake, script) = Fake::answering(vec![Ok(
            json!({"ok": true, "result": {"username": "ua_alerts_bot"}}),
        )]);
        assert_eq!(
            TelegramClient::new(fake).username().await.unwrap(),
            "ua_alerts_bot"
        );
        assert_eq!(script.calls.lock().unwrap()[0].0, "getMe");
    }

    #[tokio::test]
    async fn deliver_sends_to_the_chat_and_topic_and_returns_the_message_id() {
        let (fake, script) =
            Fake::answering(vec![Ok(json!({"ok": true, "result": {"message_id": 777}}))]);

        let id = TelegramClient::new(fake)
            .deliver(-1001, Some(5), "текст")
            .await
            .unwrap();

        assert_eq!(id, 777);
        let calls = script.calls.lock().unwrap();
        assert_eq!(calls[0].0, "sendMessage");
        assert_eq!(calls[0].1["chat_id"], -1001);
        assert_eq!(calls[0].1["message_thread_id"], 5);
        assert_eq!(calls[0].1["text"], "текст");
        assert!(calls[0].1.get("reply_markup").is_none());
    }

    #[tokio::test]
    async fn deliver_without_a_topic_omits_it_and_maps_telegrams_refusals() {
        let (fake, script) = Fake::answering(vec![
            Ok(json!({"ok": true, "result": {"message_id": 1}})),
            Ok(
                json!({"ok": false, "error_code": 403, "description": "Forbidden: bot was blocked by the user"}),
            ),
            Ok(
                json!({"ok": false, "error_code": 429, "description": "Too Many Requests", "parameters": {"retry_after": 7}}),
            ),
            Ok(json!({"ok": true, "result": {}})),
        ]);
        let client = TelegramClient::new(fake);

        client.deliver(42, None, "x").await.unwrap();
        assert!(
            script.calls.lock().unwrap()[0]
                .1
                .get("message_thread_id")
                .is_none()
        );
        assert_eq!(client.deliver(42, None, "x").await, Err(SendError::Blocked));
        assert_eq!(
            client.deliver(42, None, "x").await,
            Err(SendError::RateLimited {
                retry_after_secs: 7
            })
        );
        assert!(matches!(
            client.deliver(42, None, "x").await,
            Err(SendError::Unavailable(_))
        ));
    }
}
