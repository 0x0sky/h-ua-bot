// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! One live board and one status per chat, kept from the structured reports the hub sends.
//!
//! An attack becomes two messages instead of one per report. The board is posted quietly when the
//! first report arrives and edited after that. The status is posted, and so rings, only when the
//! danger to the person grows, and the status it replaces is deleted; otherwise it is edited.
//! When every report has been called off or has lapsed, the board gets its final edit, a quiet
//! status says the sources went quiet (never that it is safe), and the next report starts a new
//! board. That quiet status is the one place a stale position is mentioned.
//!
//! This is kept in memory. A restart forgets the boards, and the next report starts new ones.

use std::collections::HashMap;

use h_ua_core::board::{
    Alarm, Board, Hazard, Level, Threat, Update, parse_instant, position_reminder,
};
use h_ua_core::ports::SendError;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::warn;

use crate::delivery::Deliver;

/// The schema of the structured report the hub sends next to the text.
pub const SIGNAL_SCHEMA: &str = "prism-hub.signal-alert.v1";

/// A position is not mentioned again for this long after a reminder, in seconds.
const REMIND_EVERY_SECS: i64 = 24 * 3600;

/// The boards of every chat, and when each person last shared a position.
#[derive(Default)]
pub struct Boards {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    chats: HashMap<i64, Chat>,
    positions: HashMap<i64, Position>,
}

#[derive(Default)]
struct Chat {
    thread: Option<i64>,
    board: Board,
    board_message: Option<Shown>,
    status_message: Option<Shown>,
    /// What the person was last told; the status rings only when the danger grows past it.
    alarm: Alarm,
}

struct Shown {
    id: i64,
    text: String,
}

#[derive(Clone, Copy)]
struct Position {
    at: i64,
    reminded: Option<i64>,
}

impl Boards {
    /// No boards yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies a report to a chat's board and shows the result. Returns the id of the message
    /// that now carries it, or `None` when nothing needed showing, such as a report that lapsed
    /// before it arrived.
    pub async fn apply(
        &self,
        sender: &dyn Deliver,
        chat_id: i64,
        thread: Option<i64>,
        update: Update,
        now: i64,
    ) -> Result<Option<i64>, SendError> {
        let mut inner = self.inner.lock().await;
        let Inner { chats, positions } = &mut *inner;
        let chat = chats.entry(chat_id).or_default();
        chat.thread = thread;
        chat.board.apply(update, now);
        let outcome = show(sender, chat_id, chat, positions, now).await;
        if outcome == Err(SendError::Blocked) {
            chats.remove(&chat_id);
        }
        outcome
    }

    /// Drops lapsed reports from every board and shows what changed.
    pub async fn tick(&self, sender: &dyn Deliver, now: i64) {
        let mut inner = self.inner.lock().await;
        let Inner { chats, positions } = &mut *inner;
        let mut blocked = Vec::new();
        for (chat_id, chat) in chats.iter_mut() {
            if !chat.board.expire(now) {
                continue;
            }
            match show(sender, *chat_id, chat, positions, now).await {
                Ok(_) => {}
                Err(SendError::Blocked) => blocked.push(*chat_id),
                Err(error) => warn!(%error, "a lapsed report was not shown"),
            }
        }
        for chat_id in blocked {
            chats.remove(&chat_id);
        }
    }

    /// The person shared a position, live or not.
    pub async fn positioned(&self, chat_id: i64, now: i64) {
        self.inner.lock().await.positions.insert(
            chat_id,
            Position {
                at: now,
                reminded: None,
            },
        );
    }

    /// The person left: nothing about them is kept.
    pub async fn forget(&self, chat_id: i64) {
        let mut inner = self.inner.lock().await;
        inner.chats.remove(&chat_id);
        inner.positions.remove(&chat_id);
    }
}

async fn show(
    sender: &dyn Deliver,
    chat_id: i64,
    chat: &mut Chat,
    positions: &mut HashMap<i64, Position>,
    now: i64,
) -> Result<Option<i64>, SendError> {
    let alarm = chat.board.alarm();
    let quiet = alarm.level == Level::Quiet;
    if quiet && chat.alarm.level == Level::Quiet {
        // Nothing active, and the person was not told of anything: a report that lapsed on the
        // way, or a retraction of one this board never showed.
        chat.board = Board::new();
        return Ok(chat.status_message.as_ref().map(|shown| shown.id));
    }

    let board_text = chat.board.render();
    match &chat.board_message {
        Some(shown) if shown.text == board_text => {}
        Some(shown) => match sender.edit(chat_id, shown.id, &board_text).await {
            Ok(()) => {
                chat.board_message = Some(Shown {
                    id: shown.id,
                    text: board_text,
                });
            }
            // Deleted by the person, or otherwise gone: a new board takes its place.
            Err(SendError::Rejected(_)) => {
                let id = sender.post(chat_id, chat.thread, &board_text, true).await?;
                chat.board_message = Some(Shown {
                    id,
                    text: board_text,
                });
            }
            Err(error) => return Err(error),
        },
        None => {
            let id = sender.post(chat_id, chat.thread, &board_text, true).await?;
            chat.board_message = Some(Shown {
                id,
                text: board_text,
            });
        }
    }

    let grew = alarm.grew_since(&chat.alarm);
    let closing = quiet;
    let position = positions.get(&chat_id).copied();
    let reminder = position
        .filter(|_| closing)
        .filter(|p| p.reminded.is_none_or(|at| now - at >= REMIND_EVERY_SECS))
        .and_then(|p| position_reminder(p.at, now));
    let status_text = chat.board.status(reminder.as_deref());
    let status_id = if grew || closing {
        // A new status rings, unless it only says the sources went quiet.
        let id = sender
            .post(chat_id, chat.thread, &status_text, closing)
            .await?;
        if let Some(old) = chat.status_message.take() {
            // Telegram refuses after 48 hours; an old status then simply stays.
            let _ = sender.delete(chat_id, old.id).await;
        }
        chat.status_message = Some(Shown {
            id,
            text: status_text,
        });
        id
    } else {
        match &chat.status_message {
            Some(shown) if shown.text == status_text => shown.id,
            Some(shown) => match sender.edit(chat_id, shown.id, &status_text).await {
                Ok(()) => {
                    let id = shown.id;
                    chat.status_message = Some(Shown {
                        id,
                        text: status_text,
                    });
                    id
                }
                Err(SendError::Rejected(_)) => {
                    let id = sender
                        .post(chat_id, chat.thread, &status_text, true)
                        .await?;
                    chat.status_message = Some(Shown {
                        id,
                        text: status_text,
                    });
                    id
                }
                Err(error) => return Err(error),
            },
            None => {
                let id = sender
                    .post(chat_id, chat.thread, &status_text, true)
                    .await?;
                chat.status_message = Some(Shown {
                    id,
                    text: status_text,
                });
                id
            }
        }
    };
    chat.alarm = alarm;
    if reminder.is_some() {
        if let Some(position) = positions.get_mut(&chat_id) {
            position.reminded = Some(now);
        }
    }
    if closing {
        // The attack is over as far as the sources say. The next report starts a new board.
        chat.board = Board::new();
        chat.board_message = None;
    }
    Ok(Some(status_id))
}

#[derive(Deserialize)]
struct Signal {
    schema_version: String,
    event: String,
    assessment_id: String,
    #[serde(default)]
    hazard: Option<SignalHazard>,
    #[serde(default)]
    place: Option<SignalPlace>,
    #[serde(default)]
    proximity: Option<String>,
    #[serde(default)]
    valid_until: Option<String>,
    #[serde(default)]
    sources: Vec<SignalSource>,
}

#[derive(Deserialize)]
struct SignalHazard {
    class: String,
    #[serde(default)]
    kinds: Vec<String>,
}

#[derive(Deserialize)]
struct SignalPlace {
    name: Option<String>,
}

#[derive(Deserialize)]
struct SignalSource {
    source_id: String,
    observed_at: String,
}

/// Reads the structured report the hub sends next to the text. Anything this bot cannot vouch
/// for is `None`, and the text is delivered as it is instead.
pub fn signal_update(value: &Value) -> Option<Update> {
    let signal: Signal = serde_json::from_value(value.clone()).ok()?;
    let id = signal.assessment_id;
    if signal.schema_version != SIGNAL_SCHEMA || id.is_empty() || id.len() > 200 {
        return None;
    }
    match signal.event.as_str() {
        "retraction" => Some(Update::Retraction { id }),
        "alert" => {
            let hazard = signal.hazard?;
            let hazard = Hazard::of(&hazard.class, &hazard.kinds)?;
            let nearby = match signal.proximity?.as_str() {
                "target" => false,
                "nearby" => true,
                _ => return None,
            };
            let valid_until = parse_instant(&signal.valid_until?)?;
            let newest = signal
                .sources
                .iter()
                .map(|source| Some((parse_instant(&source.observed_at)?, source)))
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .max_by_key(|(at, _)| *at)?;
            let place = signal
                .place
                .and_then(|place| place.name)
                .map(|name| name.trim().to_owned())
                .filter(|name| {
                    !name.is_empty()
                        && name.chars().count() <= 120
                        && !name.chars().any(char::is_control)
                });
            Some(Update::Alert {
                id,
                threat: Threat {
                    hazard,
                    place,
                    nearby,
                    reported_at: newest.0,
                    source: channel(&newest.1.source_id),
                    valid_until,
                },
            })
        }
        _ => None,
    }
}

/// `@kpszsu` for `telegram.channel:kpszsu`.
fn channel(source_id: &str) -> Option<String> {
    let name = source_id.strip_prefix("telegram.channel:")?;
    let valid = (4..=32).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then(|| format!("@{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn signal() -> Value {
        json!({
            "schema_version": SIGNAL_SCHEMA,
            "event": "alert",
            "assessment_id": "as-1",
            "hazard": {"class": "missile", "kinds": ["air.ballistic_missile"]},
            "place": {"name": "Київ"},
            "proximity": "target",
            "likelihood": null,
            "valid_until": "2026-10-27T04:21:00Z",
            "event_at": "2026-10-27T03:51:00Z",
            "sources": [
                {"source_id": "telegram.channel:vanek_nikolaev", "url": "https://t.me/vanek_nikolaev/1", "observed_at": "2026-10-27T03:50:00Z"},
                {"source_id": "telegram.channel:kpszsu", "url": "https://t.me/kpszsu/2", "observed_at": "2026-10-27T03:51:00Z"}
            ],
            "still_active": []
        })
    }

    #[test]
    fn an_alert_is_read_with_its_newest_source() {
        let Some(Update::Alert { id, threat }) = signal_update(&signal()) else {
            panic!("not read");
        };
        assert_eq!(id, "as-1");
        assert_eq!(threat.hazard, Hazard::Ballistic);
        assert_eq!(threat.place.as_deref(), Some("Київ"));
        assert!(!threat.nearby);
        assert_eq!(threat.source.as_deref(), Some("@kpszsu"));
        assert_eq!(
            threat.reported_at,
            parse_instant("2026-10-27T03:51:00Z").unwrap()
        );
        assert_eq!(
            threat.valid_until,
            parse_instant("2026-10-27T04:21:00Z").unwrap()
        );
    }

    #[test]
    fn a_retraction_needs_only_its_id() {
        let retraction = json!({"schema_version": SIGNAL_SCHEMA, "event": "retraction", "assessment_id": "as-1"});
        assert_eq!(
            signal_update(&retraction),
            Some(Update::Retraction { id: "as-1".into() })
        );
    }

    #[test]
    fn anything_unvouched_for_falls_back_to_the_text() {
        let broken: [(&str, Value); 7] = [
            ("schema_version", json!("prism-hub.signal-alert.v2")),
            ("event", json!("expiry")),
            ("assessment_id", json!("")),
            ("proximity", json!("far")),
            ("hazard", json!({"class": "balloon"})),
            ("valid_until", json!("soon")),
            ("sources", json!([])),
        ];
        for (key, value) in broken {
            let mut signal = signal();
            signal[key] = value;
            assert_eq!(signal_update(&signal), None, "{key}");
        }
        assert_eq!(signal_update(&json!("text")), None);
    }

    #[test]
    fn a_strange_place_or_source_is_left_out_rather_than_shown() {
        let mut odd = signal();
        odd["place"]["name"] = json!("Київ\u{0007}");
        odd["sources"] = json!([{"source_id": "api:x", "observed_at": "2026-10-27T03:51:00Z"}]);
        let Some(Update::Alert { threat, .. }) = signal_update(&odd) else {
            panic!("not read");
        };
        assert_eq!(threat.place, None);
        assert_eq!(threat.source, None);
    }
}
