// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Telegram as a client of h-ua-bot: the first [`Messenger`].
//!
//! Talks to the Bot API by long polling, so it needs no public address. It understands four
//! things a person can do in a private chat: send a command, share a location, update a live
//! location, and tap a button under a message. Everything else is ignored.

mod transport;

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use h_ua_core::category::Category;
use h_ua_core::copy::Copy;
use h_ua_core::locale::Locale;
use h_ua_core::message::{Choice, Message};
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

/// Users may still have the previous menu stored in Telegram after an upgrade.
const LEGACY_KINDS_BUTTON: &str = "⚙️ Що стежити";

/// The command a button under the input field stands for. Telegram sends a button's label as
/// plain text, so the label is matched back to its command, in any language the bot speaks: a
/// person may change their app's language while an older keyboard is still on screen.
fn button_command(label: &str) -> Option<&'static str> {
    if label == LEGACY_KINDS_BUTTON {
        return Some("kinds");
    }
    Locale::ALL.into_iter().map(Copy::of).find_map(|copy| {
        [
            (copy.button_status, "status"),
            (copy.button_settings, "kinds"),
            (copy.button_help, "help"),
            (copy.button_back, "menu"),
        ]
        .into_iter()
        .find(|(button, _)| *button == label)
        .map(|(_, command)| command)
    })
}

/// What a person sees before typing anything, in one language: the command menu, in the order
/// a person needs it, and what the bot is.
struct Profile {
    commands: [(&'static str, &'static str); 5],
    /// Shown in an empty chat before /start. At most 512 characters.
    description: &'static str,
    /// The line under the bot's name in search and in its profile. At most 120 characters.
    short_description: &'static str,
}

impl Profile {
    fn of(locale: Locale) -> &'static Profile {
        match locale {
            Locale::Uk => &PROFILE_UK,
            Locale::Ru => &PROFILE_RU,
            Locale::En => &PROFILE_EN,
        }
    }
}

/// Telegram picks a profile by the language of a person's app, without a region. The languages
/// the bot answers in their own way get their profile; everyone else gets the default one, in
/// English.
const PROFILE_LANGUAGES: [&str; 3] = ["uk", "be", "ru"];

static PROFILE_UK: Profile = Profile {
    commands: [
        ("location", "📍 Оновити позицію"),
        ("status", "🛡 Що я стежу для вас"),
        ("kinds", "⚙️ За чим стежити? Дрони, КАБи, ракети, поруч"),
        ("help", "❓ Як це працює"),
        ("stop", "🗑 Видалити підписку й позицію"),
    ],
    description: "Повідомляю про повітряні загрози лише для вашої зони: дрони, КАБи, ракети.\n\n\
Одне табло оновлюється на місці, а статус дзвонить лише тоді, коли небезпека ближчає.\n\n\
Позицію зберігаю як клітинку близько 3 км, не точні координати.\n\n\
Неофіційний сервіс: орієнтуйтесь на офіційні сповіщення про тривогу.",
    short_description: "Повітряні загрози лише для вашої зони. Без спаму. Неофіційно.",
};

static PROFILE_RU: Profile = Profile {
    commands: [
        ("location", "📍 Обновить позицию"),
        ("status", "🛡 Что я отслеживаю для вас"),
        ("kinds", "⚙️ За чем следить? Дроны, КАБы, ракеты, рядом"),
        ("help", "❓ Как это работает"),
        ("stop", "🗑 Удалить подписку и позицию"),
    ],
    description: "Сообщаю о воздушных угрозах только для вашей зоны: дроны, КАБы, ракеты.\n\n\
Одно табло обновляется на месте, а статус звонит только тогда, когда опасность приближается.\n\n\
Позицию храню как клетку около 3 км, а не точные координаты.\n\n\
Неофициальный сервис: ориентируйтесь на официальные оповещения о тревоге.",
    short_description: "Воздушные угрозы только для вашей зоны. Без спама. Неофициально.",
};

static PROFILE_EN: Profile = Profile {
    commands: [
        ("location", "📍 Update position"),
        ("status", "🛡 What I follow for you"),
        (
            "kinds",
            "⚙️ What to follow? Drones, guided bombs, missiles, nearby",
        ),
        ("help", "❓ How it works"),
        ("stop", "🗑 Delete subscription and position"),
    ],
    description: "I report air threats for your area only: drones, guided bombs, missiles.\n\n\
One board updates in place, and the status rings only when danger gets closer.\n\n\
I keep your position as a cell of about 3 km, not as exact coordinates.\n\n\
Unofficial service: follow the official air raid alerts.",
    short_description: "Air threats for your area only. No spam. Unofficial.",
};

/// Telegram, over a [`Transport`].
pub struct TelegramClient<T> {
    transport: T,
    /// The `offset` for the next `getUpdates`: one past the last update handled.
    next_offset: Mutex<Option<i64>>,
    /// What each chat shows besides the hub's board and status, so a reply can tidy it.
    screens: Mutex<HashMap<i64, Screen>>,
}

/// The bot keeps a chat to what matters: the position the person shared last, the hub's board
/// and status, and one message from the bot. Everything else it sent or was sent is cleared.
///
/// Kept in memory: after a restart the bot forgets one old message per chat, which then stays.
#[derive(Default)]
struct Screen {
    /// The bot's one conversation message: the last reply.
    system: Option<i64>,
    /// The position the person shared last.
    location: Option<i64>,
    /// What the person sent that the next reply makes redundant: commands, button taps, text,
    /// and a position they have since replaced.
    stale: Vec<i64>,
    /// Text labels of the currently visible reply-keyboard choices.
    keyboard_choices: Option<HashMap<String, String>>,
}

impl<T: Transport> TelegramClient<T> {
    /// A client over a transport.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            next_offset: Mutex::new(None),
            screens: Mutex::new(HashMap::new()),
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
    /// Sets what a person sees before typing anything: the command menu and the bot's
    /// description, in English by default and in the bot's own language for Ukrainian,
    /// Belarusian and Russian apps. Telegram keeps them, so calling this on every start is
    /// enough.
    pub async fn set_up_profile(&self) -> Result<(), SendError> {
        self.set_profile(None, Profile::of(Locale::En)).await?;
        for language in PROFILE_LANGUAGES {
            self.set_profile(Some(language), Profile::of(Locale::from_tag(language)))
                .await?;
        }
        self.call_for_message(
            "setChatMenuButton",
            &json!({"menu_button": {"type": "commands"}}),
        )
        .await
        .map(|_| ())
    }

    /// Sets one language's profile, or the default one without a language.
    async fn set_profile(
        &self,
        language: Option<&str>,
        profile: &Profile,
    ) -> Result<(), SendError> {
        let commands: Vec<Value> = profile
            .commands
            .iter()
            .map(|(command, description)| json!({"command": command, "description": description}))
            .collect();
        let with_language = |mut body: Value| {
            if let Some(language) = language {
                body["language_code"] = json!(language);
            }
            body
        };
        self.call_for_message(
            "setMyCommands",
            &with_language(json!({"commands": commands})),
        )
        .await?;
        self.call_for_message(
            "setMyDescription",
            &with_language(json!({"description": profile.description})),
        )
        .await?;
        self.call_for_message(
            "setMyShortDescription",
            &with_language(json!({"short_description": profile.short_description})),
        )
        .await
        .map(|_| ())
    }

    /// Sends text to a chat, and to a topic in it when `thread_id` is given, and returns the id
    /// Telegram gave the message. This is what the hub's delivery endpoint uses.
    pub async fn deliver(
        &self,
        chat: i64,
        thread_id: Option<i64>,
        text: &str,
    ) -> Result<i64, SendError> {
        self.post(chat, thread_id, text, false).await
    }

    /// Like [`deliver`](Self::deliver), but `silent` delivers without a sound or a vibration.
    pub async fn post(
        &self,
        chat: i64,
        thread_id: Option<i64>,
        text: &str,
        silent: bool,
    ) -> Result<i64, SendError> {
        let mut body = message_body(json!(chat), thread_id, text);
        if silent {
            body["disable_notification"] = json!(true);
        }
        self.call_for_message("sendMessage", &body)
            .await?
            .ok_or_else(|| SendError::Unavailable("sendMessage returned no message_id".to_owned()))
    }

    /// Replaces the text of a message the bot sent. An edit that changes nothing is not an error.
    pub async fn edit(&self, chat: i64, message_id: i64, text: &str) -> Result<(), SendError> {
        let mut body = message_body(json!(chat), None, text);
        body["message_id"] = json!(message_id);
        match self.call_for_message("editMessageText", &body).await {
            Err(SendError::Rejected(reason)) if reason.contains("message is not modified") => {
                Ok(())
            }
            outcome => outcome.map(|_| ()),
        }
    }

    /// Pins a message above the chat, without a notification.
    pub async fn pin(&self, chat: i64, message_id: i64) -> Result<(), SendError> {
        self.call_for_message(
            "pinChatMessage",
            &json!({"chat_id": chat, "message_id": message_id, "disable_notification": true}),
        )
        .await
        .map(|_| ())
    }

    /// Deletes a message the bot sent. Telegram refuses once it is older than 48 hours.
    pub async fn delete(&self, chat: i64, message_id: i64) -> Result<(), SendError> {
        self.call_for_message(
            "deleteMessage",
            &json!({"chat_id": chat, "message_id": message_id}),
        )
        .await
        .map(|_| ())
    }

    async fn send_message(&self, chat: Value, message: &Message) -> Result<(), SendError> {
        let chat_number = chat.as_i64();
        let mut body = message_body(chat, None, &message.text);
        // A reply keyboard replaces the letter keyboard. It cannot be attached to an edited
        // message, so a settings toggle sends the refreshed panel and tidies the old one.
        let copy = Copy::of(message.locale);
        body["reply_markup"] = if message.choices.is_empty() {
            menu(copy, message.ask_location)
        } else {
            buttons(copy, &message.choices)
        };
        let sent = self.call_for_message("sendMessage", &body).await?;
        if let Some(chat) = chat_number {
            let choices = message
                .choices
                .iter()
                .flatten()
                .map(|choice| (choice.label.clone(), choice.data.clone()))
                .collect();
            // The message is already out: a poisoned lock must not report it as failed.
            if let Ok(mut screens) = self.screens.lock() {
                screens.entry(chat).or_default().keyboard_choices = Some(choices);
            }
        }
        if let (Some(chat), Some(sent)) = (chat_number, sent) {
            self.tidy(chat, sent).await;
        }
        Ok(())
    }

    /// Remembers what a person sent, so the reply to it can clear it away.
    fn note(&self, message: &Incoming) {
        if message.message_id == 0 {
            return;
        }
        let Ok(mut screens) = self.screens.lock() else {
            return;
        };
        let screen = screens.entry(message.chat.id).or_default();
        if message.location.is_some() {
            // The newest position stays on screen; the one it replaces goes.
            if let Some(old) = screen.location.replace(message.message_id) {
                screen.stale.push(old);
            }
        } else {
            screen.stale.push(message.message_id);
        }
    }

    /// After a reply: the previous reply and what the person sent before it are deleted.
    async fn tidy(&self, chat: i64, sent: i64) {
        let doomed = {
            let Ok(mut screens) = self.screens.lock() else {
                return;
            };
            let screen = screens.entry(chat).or_default();
            let mut doomed = std::mem::take(&mut screen.stale);
            doomed.extend(screen.system.replace(sent));
            doomed
        };
        for message_id in doomed {
            // Best effort: Telegram refuses after 48 hours, and a person may delete first.
            if let Err(error) = self.delete(chat, message_id).await {
                debug!(%error, message_id, "could not clear a message");
            }
        }
    }

    /// Stops the spinner on a tapped button. Telegram shows it until this is called.
    async fn acknowledge(&self, query: &str) {
        if let Err(error) = self
            .transport
            .call("answerCallbackQuery", &json!({"callback_query_id": query}))
            .await
        {
            debug!(%error, "could not acknowledge a button");
        }
    }

    /// Calls a method that answers with a message, or with `true`, and returns the message id.
    async fn call_for_message(&self, method: &str, body: &Value) -> Result<Option<i64>, SendError> {
        let raw = self
            .transport
            .call(method, body)
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

/// The keyboard every reply in a conversation carries, so what the bot can do is always one tap
/// away. Alerts arrive through [`TelegramClient::post`] without one, and the keyboard stays.
fn menu(copy: &Copy, ask_location: bool) -> Value {
    let row: Vec<Value> = [copy.button_status, copy.button_settings, copy.button_help]
        .into_iter()
        .map(|label| json!({"text": label}))
        .collect();
    let placeholder = if ask_location {
        copy.hint_location
    } else {
        copy.hint_menu
    };
    json!({
        "keyboard": [[{"text": copy.button_location, "request_location": true}], row],
        "resize_keyboard": true,
        "is_persistent": true,
        "input_field_placeholder": placeholder,
    })
}

/// Custom reply keyboard for settings. Text replies are mapped back to choice data per chat.
fn buttons(copy: &Copy, choices: &[Vec<Choice>]) -> Value {
    let mut rows: Vec<Vec<Value>> = choices
        .iter()
        .map(|row| {
            row.iter()
                .map(|choice| json!({"text": choice.label}))
                .collect()
        })
        .collect();
    rows.push(vec![json!({"text": copy.button_back})]);
    json!({
        "keyboard": rows,
        "resize_keyboard": true,
        "is_persistent": true,
        "one_time_keyboard": false,
        "input_field_placeholder": copy.hint_settings,
    })
}

/// A Telegram client may retain the reply keyboard across bot restarts. Decode only
/// our known category labels, in any language, when no keyboard state has been established for
/// this chat.
fn restored_choice(label: &str) -> Option<String> {
    let kind = label
        .strip_prefix("✅ ")
        .or_else(|| label.strip_prefix("▫️ "))?
        .to_lowercase();
    Locale::ALL.into_iter().map(Copy::of).find_map(|copy| {
        if kind == copy.nearby_choice.to_lowercase() {
            return Some("toggle:nearby".to_owned());
        }
        Category::ALL
            .into_iter()
            .find(|category| copy.category(*category).to_lowercase() == kind)
            .map(|category| format!("toggle:{}", category.key()))
    })
}

fn message_body(chat: Value, thread_id: Option<i64>, text: &str) -> Value {
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
    body
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
    callback_query: Option<CallbackQuery>,
}

/// A tap on a button under a message.
#[derive(Deserialize)]
struct CallbackQuery {
    id: String,
    from: Option<Sender>,
    data: Option<String>,
    message: Option<Tapped>,
}

#[derive(Deserialize)]
struct Tapped {
    message_id: i64,
    chat: Chat,
}

#[derive(Deserialize)]
struct Incoming {
    #[serde(default)]
    message_id: i64,
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
    /// The language of the person's Telegram app: `uk`, `ru`, sometimes with a region.
    language_code: Option<String>,
}

/// The language to answer a sender in; Ukrainian when Telegram does not say.
fn locale_of(sender: Option<&Sender>) -> Locale {
    sender
        .and_then(|sender| sender.language_code.as_deref())
        .map(Locale::from_tag)
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct Location {
    latitude: f64,
    longitude: f64,
}

/// Turns an update into what the person did, if it is something the bot understands.
fn event_from(update: Update) -> Option<Event> {
    if let Some(query) = update.callback_query {
        let tapped = query.message?;
        if tapped.chat.kind != "private" {
            return None;
        }
        return Some(Event {
            from: Recipient::new(CLIENT_NAME, tapped.chat.id.to_string()),
            locale: locale_of(query.from.as_ref()),
            kind: EventKind::Choice {
                data: query.data?,
                message: tapped.message_id.to_string(),
            },
        });
    }
    let (message, edited) = match (update.message, update.edited_message) {
        (Some(message), _) => (message, false),
        (None, Some(message)) => (message, true),
        (None, None) => return None,
    };
    if message.chat.kind != "private" || message.from.as_ref().is_some_and(|s| s.is_bot) {
        return None;
    }
    let from = Recipient::new(CLIENT_NAME, message.chat.id.to_string());
    let locale = locale_of(message.from.as_ref());
    if let Some(location) = message.location {
        return Some(Event {
            from,
            locale,
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
    if let Some(command) = button_command(text.trim()) {
        return Some(Event {
            from,
            locale,
            kind: EventKind::Command {
                name: command.to_owned(),
                args: String::new(),
            },
        });
    }
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
    Some(Event { from, locale, kind })
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
        self.send_message(chat_id(to), message).await
    }

    async fn poll(&self) -> Result<Vec<Event>, ClientError> {
        let offset = *self
            .next_offset
            .lock()
            .map_err(|_| ClientError("poisoned".to_owned()))?;
        let mut body = json!({
            "timeout": LONG_POLL_SECS,
            "allowed_updates": ["message", "edited_message", "callback_query"],
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
        let queries: Vec<String> = updates
            .iter()
            .filter_map(|update| update.callback_query.as_ref().map(|q| q.id.clone()))
            .collect();
        for query in &queries {
            self.acknowledge(query).await;
        }
        for update in &updates {
            if let Some(message) = &update.message {
                let from_bot = message.from.as_ref().is_some_and(|s| s.is_bot);
                if message.chat.kind == "private" && !from_bot {
                    self.note(message);
                }
            }
        }
        let mut events = Vec::new();
        for update in updates {
            // Reply-keyboard buttons arrive as ordinary text, not callback queries.
            let choice = update.message.as_ref().and_then(|message| {
                if message.chat.kind != "private"
                    || message.from.as_ref().is_some_and(|sender| sender.is_bot)
                {
                    return None;
                }
                let label = message.text.as_deref()?.trim();
                let screens = self.screens.lock().ok()?;
                match screens
                    .get(&message.chat.id)
                    .and_then(|screen| screen.keyboard_choices.as_ref())
                {
                    Some(choices) => choices.get(label).cloned(),
                    None => restored_choice(label),
                }
            });
            if let Some(mut event) = event_from(update) {
                if let Some(data) = choice {
                    if matches!(event.kind, EventKind::Other) {
                        event.kind = EventKind::Choice {
                            data,
                            message: String::new(),
                        };
                    }
                }
                events.push(event);
            }
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use h_ua_core::copy::UK;
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
                    locale: Locale::Uk,
                    kind: EventKind::Command {
                        name: "start".into(),
                        args: "".into()
                    }
                },
                Event {
                    from: me.clone(),
                    locale: Locale::Uk,
                    kind: EventKind::Command {
                        name: "kinds".into(),
                        args: "дрони   каби".into()
                    }
                },
                Event {
                    from: me,
                    locale: Locale::Uk,
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
            json!(["message", "edited_message", "callback_query"])
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
    async fn a_reply_is_plain_text_with_the_menu_and_without_a_preview() {
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
        let markup = &calls[0].1["reply_markup"];
        assert_eq!(markup["is_persistent"], true);
        assert_eq!(markup.get("one_time_keyboard"), None);
        assert_eq!(markup["keyboard"][1][0]["text"], "🛡 Стан");
    }

    #[tokio::test]
    async fn the_menu_buttons_are_read_as_their_commands() {
        let events = heard(json!([
            private(1, 42, json!({"text": "🛡 Стан"})),
            private(2, 42, json!({"text": "⚙️ За чим стежити?"})),
            private(3, 42, json!({"text": "❓ Довідка"})),
            private(4, 42, json!({"text": "⚙️ Що стежити"})),
        ]))
        .await;
        let names: Vec<String> = events
            .into_iter()
            .map(|event| match event.kind {
                EventKind::Command { name, args } => {
                    assert!(args.is_empty());
                    name
                }
                other => panic!("not a command: {other:?}"),
            })
            .collect();
        assert_eq!(names, ["status", "kinds", "help", "kinds"]);
    }

    #[tokio::test]
    async fn the_profile_is_set_in_english_by_default_and_per_language() {
        let (fake, script) = Fake::answering(vec![Ok(json!({"ok": true, "result": true})); 13]);
        TelegramClient::new(fake).set_up_profile().await.unwrap();
        let calls = script.calls.lock().unwrap();
        assert_eq!(calls.len(), 13);
        assert_eq!(calls[12].0, "setChatMenuButton");
        let commands_by_language: Vec<(Value, Value)> = calls
            .iter()
            .filter(|(method, _)| method == "setMyCommands")
            .map(|(_, body)| {
                (
                    body["language_code"].clone(),
                    body["commands"][0]["description"].clone(),
                )
            })
            .collect();
        assert_eq!(
            commands_by_language,
            [
                (Value::Null, json!("📍 Update position")),
                (json!("uk"), json!("📍 Оновити позицію")),
                (json!("be"), json!("📍 Оновити позицію")),
                (json!("ru"), json!("📍 Обновить позицию")),
            ]
        );
        let short: Vec<&Value> = calls
            .iter()
            .filter(|(method, _)| method == "setMyShortDescription")
            .map(|(_, body)| &body["short_description"])
            .collect();
        assert_eq!(
            short[0],
            "Air threats for your area only. No spam. Unofficial."
        );
    }

    #[test]
    fn every_profile_fits_telegram_and_lists_the_same_commands() {
        for locale in Locale::ALL {
            let profile = Profile::of(locale);
            assert!(profile.description.chars().count() <= 512, "{locale:?}");
            assert!(
                profile.short_description.chars().count() <= 120,
                "{locale:?}"
            );
            let names: Vec<&str> = profile.commands.iter().map(|(name, _)| *name).collect();
            assert_eq!(names, ["location", "status", "kinds", "help", "stop"]);
            for (_, description) in profile.commands {
                assert!(description.chars().count() <= 256);
            }
        }
    }

    #[tokio::test]
    async fn the_language_of_the_sender_is_carried_on_the_event() {
        let tap = json!({"update_id": 9, "callback_query": {
            "id": "q1", "data": "toggle:bomb",
            "from": {"id": 42, "is_bot": false, "language_code": "ru-RU"},
            "message": {"message_id": 77, "chat": {"id": 42, "type": "private"}}
        }});
        let said = |id, language: Value| {
            let mut update = private(id, 42, json!({"text": "/status"}));
            update["message"]["from"]["language_code"] = language;
            update
        };
        let (fake, _) = Fake::answering(vec![
            Ok(updates(json!([
                said(1, json!("en")),
                said(2, json!("ru")),
                said(3, json!("uk-RU")),
                said(4, Value::Null),
                tap,
            ]))),
            Ok(json!({"ok": true, "result": true})),
        ]);
        let events = TelegramClient::new(fake).poll().await.unwrap();
        let locales: Vec<Locale> = events.iter().map(|event| event.locale).collect();
        assert_eq!(
            locales,
            [Locale::En, Locale::Ru, Locale::Ru, Locale::Uk, Locale::Ru]
        );
    }

    #[tokio::test]
    async fn a_tapped_button_is_read_and_acknowledged() {
        let tap = json!({"update_id": 5, "callback_query": {
            "id": "q1", "data": "toggle:bomb",
            "from": {"id": 42, "is_bot": false},
            "message": {"message_id": 77, "chat": {"id": 42, "type": "private"}}
        }});
        let group_tap = json!({"update_id": 6, "callback_query": {
            "id": "q2", "data": "toggle:bomb",
            "from": {"id": 42, "is_bot": false},
            "message": {"message_id": 78, "chat": {"id": -5, "type": "group"}}
        }});
        let (fake, script) = Fake::answering(vec![
            Ok(updates(json!([tap, group_tap]))),
            Ok(json!({"ok": true, "result": true})),
            Ok(json!({"ok": true, "result": true})),
        ]);
        let events = TelegramClient::new(fake).poll().await.unwrap();
        assert_eq!(
            events,
            vec![Event {
                from: Recipient::new("telegram", "42"),
                locale: Locale::Uk,
                kind: EventKind::Choice {
                    data: "toggle:bomb".to_owned(),
                    message: "77".to_owned()
                },
            }]
        );
        let calls = script.calls.lock().unwrap();
        assert_eq!(calls[1].0, "answerCallbackQuery");
        assert_eq!(calls[1].1["callback_query_id"], "q1");
        assert_eq!(calls[2].1["callback_query_id"], "q2");
    }

    #[tokio::test]
    async fn settings_use_a_persistent_reply_keyboard_and_taps_refresh_it() {
        let panel =
            Message::text("⚙️").with_choices(vec![vec![Choice::new("✅ Дрони", "toggle:drone")]]);
        let sent = |id| Ok(json!({"ok": true, "result": {"message_id": id}}));
        let (fake, script) = Fake::answering(vec![
            sent(77),
            Ok(updates(json!([private(
                1,
                42,
                json!({"message_id": 80, "text": "✅ Дрони"})
            )]))),
            sent(81),
            Ok(json!({"ok": true, "result": true})),
            Ok(json!({"ok": true, "result": true})),
        ]);
        let client = TelegramClient::new(fake);
        let me = Recipient::new("telegram", "42");
        client.send(&me, &panel).await.unwrap();
        let events = client.poll().await.unwrap();
        assert_eq!(
            events[0].kind,
            EventKind::Choice {
                data: "toggle:drone".to_owned(),
                message: String::new(),
            }
        );
        client
            .send(&me, &panel.clone().replacing(Some("77".to_owned())))
            .await
            .unwrap();
        let calls = script.calls.lock().unwrap();
        assert_eq!(calls[0].0, "sendMessage");
        let markup = &calls[0].1["reply_markup"];
        assert_eq!(markup["keyboard"][0][0]["text"], "✅ Дрони");
        assert_eq!(markup["keyboard"][1][0]["text"], UK.button_back);
        assert_eq!(markup["is_persistent"], true);
        assert_eq!(markup["one_time_keyboard"], false);
        assert!(markup.get("inline_keyboard").is_none());
        assert_eq!(calls[2].0, "sendMessage");
        assert_eq!(calls[3].0, "deleteMessage");
        assert_eq!(calls[3].1["message_id"], 80);
        assert_eq!(calls[4].1["message_id"], 77);
    }

    #[tokio::test]
    async fn keyboards_follow_the_message_language_and_any_language_is_read_back() {
        let (fake, script) = Fake::answering(vec![ok(), ok()]);
        let client = TelegramClient::new(fake);
        let me = Recipient::new("telegram", "42");
        client
            .send(&me, &Message::text("Main menu").in_locale(Locale::En))
            .await
            .unwrap();
        let settings = Message::text("⚙️")
            .with_choices(vec![vec![Choice::new("✅ Дроны", "toggle:drone")]])
            .in_locale(Locale::Ru);
        client.send(&me, &settings).await.unwrap();
        {
            let calls = script.calls.lock().unwrap();
            let menu = &calls[0].1["reply_markup"];
            assert_eq!(menu["keyboard"][0][0]["text"], "📍 Update position");
            assert_eq!(menu["keyboard"][1][0]["text"], "🛡 Status");
            assert_eq!(
                menu["input_field_placeholder"],
                "A command or a button below"
            );
            let panel = &calls[1].1["reply_markup"];
            assert_eq!(panel["input_field_placeholder"], "Выберите, за чем следить");
        }

        let names: Vec<String> = heard(json!([
            private(1, 42, json!({"text": "⚙️ What to follow?"})),
            private(2, 42, json!({"text": "🛡 Статус"})),
            private(3, 42, json!({"text": "⬅️ Menu"})),
        ]))
        .await
        .into_iter()
        .map(|event| match event.kind {
            EventKind::Command { name, .. } => name,
            other => panic!("not a command: {other:?}"),
        })
        .collect();
        assert_eq!(names, ["kinds", "status", "menu"]);
    }

    #[tokio::test]
    async fn settings_buttons_in_any_language_survive_a_bot_restart() {
        let events = heard(json!([
            private(1, 42, json!({"text": "▫️ Guided bombs"})),
            private(2, 42, json!({"text": "✅ Угрозы рядом"})),
        ]))
        .await;
        let toggles: Vec<String> = events
            .into_iter()
            .map(|event| match event.kind {
                EventKind::Choice { data, .. } => data,
                other => panic!("not a choice: {other:?}"),
            })
            .collect();
        assert_eq!(toggles, ["toggle:bomb", "toggle:nearby"]);
    }

    #[tokio::test]
    async fn settings_reply_buttons_survive_a_bot_restart() {
        let events = heard(json!([
            private(1, 42, json!({"text": "▫️ КАБи"})),
            private(2, 42, json!({"text": "✅ Загрози поруч"})),
        ]))
        .await;
        assert_eq!(
            events[0].kind,
            EventKind::Choice {
                data: "toggle:bomb".into(),
                message: String::new(),
            }
        );
        assert_eq!(
            events[1].kind,
            EventKind::Choice {
                data: "toggle:nearby".into(),
                message: String::new(),
            }
        );
    }

    #[tokio::test]
    async fn returning_to_menu_restores_navigation_and_clears_old_toggles() {
        let panel =
            Message::text("⚙️").with_choices(vec![vec![Choice::new("✅ Дрони", "toggle:drone")]]);
        let (fake, script) = Fake::answering(vec![
            ok(),
            ok(),
            Ok(updates(json!([
                private(1, 42, json!({"text": "✅ Дрони"})),
                private(2, 42, json!({"text": UK.button_back})),
            ]))),
        ]);
        let client = TelegramClient::new(fake);
        let me = Recipient::new("telegram", "42");
        client.send(&me, &panel).await.unwrap();
        client
            .send(&me, &Message::text("Головне меню"))
            .await
            .unwrap();
        let events = client.poll().await.unwrap();
        assert!(matches!(events[0].kind, EventKind::Other));
        assert_eq!(
            events[1].kind,
            EventKind::Command {
                name: "menu".into(),
                args: "".into(),
            }
        );
        let calls = script.calls.lock().unwrap();
        assert!(calls[1].1["reply_markup"].get("keyboard").is_some());
        assert_eq!(
            calls[1].1["reply_markup"]["keyboard"][1][1]["text"],
            "⚙️ За чим стежити?"
        );
    }

    #[tokio::test]
    async fn a_reply_clears_what_it_answered_and_the_reply_before_it() {
        let sent = |id: i64| Ok(json!({"ok": true, "result": {"message_id": id}}));
        let deleted = || Ok(json!({"ok": true, "result": true}));
        let (fake, script) = Fake::answering(vec![
            Ok(updates(json!([private(
                1,
                42,
                json!({"message_id": 10, "location": {"latitude": 50.4, "longitude": 30.6}})
            ),]))),
            sent(11),
            Ok(updates(json!([private(
                2,
                42,
                json!({"message_id": 12, "text": "🛡 Стан"})
            ),]))),
            sent(13),
            deleted(),
            deleted(),
            Ok(updates(json!([private(
                3,
                42,
                json!({"message_id": 14, "location": {"latitude": 50.5, "longitude": 30.6}})
            ),]))),
            sent(15),
            deleted(),
            deleted(),
        ]);
        let client = TelegramClient::new(fake);
        let me = Recipient::new("telegram", "42");
        client.poll().await.unwrap();
        client
            .send(&me, &Message::text("Позицію збережено"))
            .await
            .unwrap();
        client.poll().await.unwrap();
        client
            .send(&me, &Message::text("Позиція: Київ"))
            .await
            .unwrap();
        client.poll().await.unwrap();
        client
            .send(&me, &Message::text("Позицію збережено"))
            .await
            .unwrap();

        let calls = script.calls.lock().unwrap();
        let deletions: Vec<i64> = calls
            .iter()
            .filter(|(method, _)| method == "deleteMessage")
            .map(|(_, body)| body["message_id"].as_i64().unwrap())
            .collect();
        // The first position stays until a new one comes; the tap and each old reply go.
        assert_eq!(deletions, [12, 11, 10, 13]);
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
        assert_eq!(
            markup["input_field_placeholder"],
            "Натисніть 📍, щоб поділитися позицією"
        );
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
    async fn a_board_is_posted_quietly_edited_in_place_and_deleted() {
        let (fake, script) = Fake::answering(vec![
            Ok(json!({"ok": true, "result": {"message_id": 5}})),
            Ok(json!({"ok": true, "result": {"message_id": 5}})),
            Ok(
                json!({"ok": false, "error_code": 400, "description": "Bad Request: message is not modified: specified new message content and reply markup are exactly the same"}),
            ),
            Ok(
                json!({"ok": false, "error_code": 400, "description": "Bad Request: message to edit not found"}),
            ),
            Ok(json!({"ok": true, "result": true})),
        ]);
        let client = TelegramClient::new(fake);

        assert_eq!(client.post(42, None, "a", true).await, Ok(5));
        client.edit(42, 5, "b").await.unwrap();
        client.edit(42, 5, "b").await.unwrap();
        assert!(matches!(
            client.edit(42, 5, "c").await,
            Err(SendError::Rejected(_))
        ));
        client.delete(42, 5).await.unwrap();

        let calls = script.calls.lock().unwrap();
        assert_eq!(calls[0].1["disable_notification"], true);
        assert_eq!(calls[1].0, "editMessageText");
        assert_eq!(calls[1].1["message_id"], 5);
        assert_eq!(calls[1].1["text"], "b");
        assert_eq!(calls[1].1["link_preview_options"]["is_disabled"], true);
        assert_eq!(
            calls[4],
            (
                "deleteMessage".to_owned(),
                json!({"chat_id": 42, "message_id": 5})
            )
        );
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
