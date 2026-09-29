// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! What the bot does when a person writes to it.

use std::collections::BTreeSet;

use prism_signal_normalize::{Gazetteer, Place};

use crate::category::Category;
use crate::geo::Cell;
use crate::message::{Message, NOT_OFFICIAL, PRIVACY};
use crate::ports::{Event, EventKind, Store, StoreError};
use crate::subscriber::Subscription;

/// A place from the gazetteer is named to a person only if it is this close to their cell, in
/// km. Further away it would say nothing about where they are.
const NEAREST_PLACE_MAX_KM: f64 = 40.0;

/// Handles what people send and answers them.
pub struct Conversation<'a> {
    store: &'a dyn Store,
    gazetteer: &'a Gazetteer,
    sources: Vec<String>,
}

impl<'a> Conversation<'a> {
    /// `sources` are the names shown to people as where alerts come from.
    pub fn new(store: &'a dyn Store, gazetteer: &'a Gazetteer, sources: Vec<String>) -> Self {
        Self {
            store,
            gazetteer,
            sources,
        }
    }

    /// Handles one event and returns the replies, in order. `now` is seconds since the Unix
    /// epoch.
    pub fn handle(&self, event: &Event, now: i64) -> Result<Vec<Message>, StoreError> {
        match &event.kind {
            EventKind::Command { name, args } => self.command(event, name, args, now),
            EventKind::Location { lat, lon, live } => self.location(event, *lat, *lon, *live, now),
            EventKind::Other => Ok(vec![Message::text(
                "Я розумію команди та позицію. Надішліть /help, щоб побачити, що вмію.",
            )]),
        }
    }

    fn command(
        &self,
        event: &Event,
        name: &str,
        args: &str,
        now: i64,
    ) -> Result<Vec<Message>, StoreError> {
        match name {
            "start" | "help" => Ok(vec![self.welcome()]),
            "location" => Ok(vec![Message::asking_location(
                "Натисніть кнопку нижче, щоб поділитися позицією. Якщо поділитеся «живою» позицією, я оновлюватиму її, поки ви рухаєтесь.",
            )]),
            "kinds" => self.kinds(event, args, now),
            "nearby" => self.nearby(event, args, now),
            "status" => self.status(event),
            "stop" => {
                self.store.delete_recipient(&event.from)?;
                Ok(vec![Message::text(
                    "Готово: усі дані про вас видалено, повідомлень більше не буде. Щоб повернутись, надішліть /start.",
                )])
            }
            _ => Ok(vec![Message::text(
                "Не знаю такої команди. Надішліть /help, щоб побачити, що вмію.",
            )]),
        }
    }

    fn welcome(&self) -> Message {
        let sources = if self.sources.is_empty() {
            "відкриті канали".to_owned()
        } else {
            self.sources.join(", ")
        };
        Message::asking_location(format!(
            "Я повідомляю про повітряні загрози біля вас: дрони, КАБи, ракети. Лише про ті, що стосуються вашого місця.\n\n\
             Як це працює: ви ділитесь позицією, я читаю публічні джерела ({sources}) і надсилаю лише те, що стосується вашої околиці.\n\n\
             {NOT_OFFICIAL}\n\n{PRIVACY}\n\n\
             Команди:\n\
             /location — поділитися позицією\n\
             /kinds — види загроз (дрони, каби, ракети)\n\
             /nearby — попереджати й про загрози поруч\n\
             /status — що я про вас знаю\n\
             /stop — видалити все"
        ))
    }

    fn location(
        &self,
        event: &Event,
        lat: f64,
        lon: f64,
        live: bool,
        now: i64,
    ) -> Result<Vec<Message>, StoreError> {
        let Ok(cell) = Cell::around(lat, lon) else {
            return Ok(vec![Message::text(
                "Не можу прочитати цю позицію. Спробуйте ще раз.",
            )]);
        };
        let mut subscription = self
            .store
            .subscription(&event.from)?
            .unwrap_or_else(|| Subscription::new(event.from.clone()));
        let changed = subscription.cell != Some(cell);
        subscription.cell = Some(cell);
        if changed || !live {
            self.store.save_subscription(&subscription, now)?;
        }
        if live {
            // A live position updates in silence: a message per movement would be spam.
            return Ok(Vec::new());
        }
        Ok(vec![Message::text(format!(
            "Позицію збережено. {}\nТочних координат я не зберігаю, лише клітинку близько 3 км.\n\nЗараз стежу за: {}. Змінити: /kinds",
            self.near(cell),
            describe(&subscription.categories),
        ))])
    }

    fn kinds(&self, event: &Event, args: &str, now: i64) -> Result<Vec<Message>, StoreError> {
        let mut subscription = self
            .store
            .subscription(&event.from)?
            .unwrap_or_else(|| Subscription::new(event.from.clone()));
        let words: Vec<&str> = args
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|word| !word.is_empty())
            .collect();
        if words.is_empty() {
            return Ok(vec![Message::text(format!(
                "Зараз стежу за: {}.\nЩоб змінити, перелічіть потрібне: /kinds дрони каби ракети",
                describe(&subscription.categories)
            ))]);
        }
        let mut chosen = BTreeSet::new();
        for word in &words {
            match Category::from_word(word) {
                Some(category) => {
                    chosen.insert(category);
                }
                None => {
                    return Ok(vec![Message::text(format!(
                        "Не розумію «{word}». Доступні види: дрони, каби, ракети."
                    ))]);
                }
            }
        }
        subscription.categories = chosen;
        self.store.save_subscription(&subscription, now)?;
        Ok(vec![Message::text(format!(
            "Гаразд, стежу за: {}.",
            describe(&subscription.categories)
        ))])
    }

    fn nearby(&self, event: &Event, args: &str, now: i64) -> Result<Vec<Message>, StoreError> {
        let mut subscription = self
            .store
            .subscription(&event.from)?
            .unwrap_or_else(|| Subscription::new(event.from.clone()));
        match args.trim().to_lowercase().as_str() {
            "on" | "так" | "вкл" | "увімк" => subscription.include_nearby = true,
            "off" | "ні" | "викл" | "вимк" => subscription.include_nearby = false,
            _ => {
                let state = if subscription.include_nearby {
                    "увімкнено"
                } else {
                    "вимкнено"
                };
                return Ok(vec![Message::text(format!(
                    "Попередження про загрози поруч: {state}.\nЩоб змінити: /nearby так або /nearby ні\n\
                     «Поруч» — коли джерело пише, що щось летить повз або біля вашого міста, а не на нього."
                ))]);
            }
        }
        self.store.save_subscription(&subscription, now)?;
        let state = if subscription.include_nearby {
            "увімкнено"
        } else {
            "вимкнено"
        };
        Ok(vec![Message::text(format!(
            "Попередження про загрози поруч: {state}."
        ))])
    }

    fn status(&self, event: &Event) -> Result<Vec<Message>, StoreError> {
        let Some(subscription) = self.store.subscription(&event.from)? else {
            return Ok(vec![Message::asking_location(
                "Я ще нічого про вас не знаю. Поділіться позицією, і я почну стежити.",
            )]);
        };
        let place = subscription.cell.map_or(
            "позицію не задано — повідомлень не буде".to_owned(),
            |cell| self.near(cell),
        );
        let sources = if self.sources.is_empty() {
            "—".to_owned()
        } else {
            self.sources.join(", ")
        };
        Ok(vec![Message::text(format!(
            "Позиція: {place}\nВиди: {}\nЗагрози поруч: {}\nДжерела: {sources}\n\n{NOT_OFFICIAL}",
            describe(&subscription.categories),
            if subscription.include_nearby {
                "так"
            } else {
                "ні"
            },
        ))])
    }

    /// `Найближче відоме місце: Київ.`, or nothing useful to say.
    fn near(&self, cell: Cell) -> String {
        match nearest(self.gazetteer, cell) {
            Some(place) => format!("Найближче відоме місце: {}.", place.name),
            None => {
                "Поруч немає місць з мого словника, тож розпізнаю лише згадані міста.".to_owned()
            }
        }
    }
}

fn nearest(gazetteer: &Gazetteer, cell: Cell) -> Option<&Place> {
    gazetteer
        .places()
        .map(|place| (place, cell.distance_km(place.lat, place.lon)))
        .filter(|(_, distance)| *distance <= NEAREST_PLACE_MAX_KM)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(place, _)| place)
}

fn describe(categories: &BTreeSet<Category>) -> String {
    if categories.is_empty() {
        return "нічим".to_owned();
    }
    categories
        .iter()
        .map(|c| c.label())
        .collect::<Vec<_>>()
        .join(", ")
}
