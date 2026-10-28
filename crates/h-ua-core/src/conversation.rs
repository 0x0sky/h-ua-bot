// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! What the bot does when a person writes to it.

use std::collections::BTreeSet;

use prism_signal_normalize::{Gazetteer, Place};

use crate::category::Category;
use crate::geo::Cell;
use crate::message::{Choice, Message, NOT_OFFICIAL, PRIVACY};
use crate::ports::{Event, EventKind, SubscriptionError, Subscriptions};
use crate::subscriber::Subscription;

/// A place from the gazetteer is named to a person only if it is this close to their cell, in
/// km. Further away it would say nothing about where they are.
const NEAREST_PLACE_MAX_KM: f64 = 40.0;

/// Handles what people send and answers them.
pub struct Conversation<'a> {
    subscriptions: &'a dyn Subscriptions,
    gazetteer: &'a Gazetteer,
}

impl<'a> Conversation<'a> {
    pub fn new(subscriptions: &'a dyn Subscriptions, gazetteer: &'a Gazetteer) -> Self {
        Self {
            subscriptions,
            gazetteer,
        }
    }

    /// Handles one event and returns the replies, in order.
    pub async fn handle(&self, event: &Event) -> Result<Vec<Message>, SubscriptionError> {
        match &event.kind {
            EventKind::Command { name, args } => self.command(event, name, args).await,
            EventKind::Location { lat, lon, live } => self.location(event, *lat, *lon, *live).await,
            EventKind::Choice { data, message } => self.choice(event, data, message).await,
            EventKind::Other => Ok(vec![Message::text(
                "Я розумію команди та позицію. Надішліть /help, щоб побачити, що вмію.",
            )]),
        }
    }

    async fn command(
        &self,
        event: &Event,
        name: &str,
        args: &str,
    ) -> Result<Vec<Message>, SubscriptionError> {
        match name {
            "start" | "help" => Ok(vec![self.welcome()]),
            "location" => Ok(vec![Message::asking_location(
                "Натисніть кнопку нижче, щоб поділитися позицією. Якщо поділитеся «живою» позицією, я оновлюватиму її, поки ви рухаєтесь.",
            )]),
            "kinds" => self.kinds(event, args).await,
            "nearby" => self.nearby(event, args).await,
            "status" => self.status(event).await,
            "stop" => {
                self.subscriptions.delete(&event.from).await?;
                Ok(vec![Message::text(
                    "Готово: підписку й позицію видалено, повідомлень більше не буде. Технічний ідентифікатор чату лишається в системі, але без підписки нічого не надсилається. Щоб повернутись, надішліть /start і поділіться позицією.",
                )])
            }
            _ => Ok(vec![Message::text(
                "Не знаю такої команди. Надішліть /help, щоб побачити, що вмію.",
            )]),
        }
    }

    fn welcome(&self) -> Message {
        Message::asking_location(format!(
            "Я повідомляю про повітряні загрози біля вас: дрони, КАБи, ракети. Лише про ті, що стосуються вашого місця.\n\n\
             Як це працює: ви ділитесь позицією, система обробляє повідомлення про загрози, а я надсилаю лише те, що стосується вашої околиці.\n\n\
             {NOT_OFFICIAL}\n\n{PRIVACY}\n\n\
             Кнопки під полем вводу:\n\
             📍 Оновити позицію\n\
             🛡 Стан — що я про вас знаю\n\
             ⚙️ Що стежити — дрони, КАБи, ракети, загрози поруч\n\
             /stop — видалити все"
        ))
    }

    async fn location(
        &self,
        event: &Event,
        lat: f64,
        lon: f64,
        live: bool,
    ) -> Result<Vec<Message>, SubscriptionError> {
        let Ok(cell) = Cell::around(lat, lon) else {
            return Ok(vec![Message::text(
                "Не можу прочитати цю позицію. Спробуйте ще раз.",
            )]);
        };
        // The choices already made are kept when the position moves.
        let existing = self.subscriptions.get(&event.from).await?;
        let known = existing.is_some();
        let mut subscription =
            existing.unwrap_or_else(|| Subscription::new(event.from.clone(), cell));
        let changed = subscription.cell != cell;
        subscription.cell = cell;
        if !known || changed || !live {
            self.subscriptions.save(&subscription).await?;
        }
        if live {
            // A live position updates in silence: a message per movement would be spam.
            return Ok(Vec::new());
        }
        Ok(vec![Message::text(format!(
            "Позицію збережено. {}\nТочних координат я не зберігаю, лише клітинку близько 3 км.\n\nЗараз стежу за: {}. Змінити: ⚙️ Що стежити.",
            self.near(cell),
            describe(&subscription.categories),
        ))])
    }

    /// Choices are made on a subscription, and a subscription needs a position first.
    async fn subscription_or_ask(
        &self,
        event: &Event,
    ) -> Result<Result<Subscription, Vec<Message>>, SubscriptionError> {
        Ok(self.subscriptions.get(&event.from).await?.ok_or_else(|| {
            vec![Message::asking_location(
                "Спершу поділіться позицією: без неї я не знаю, що вас стосується.",
            )]
        }))
    }

    async fn kinds(&self, event: &Event, args: &str) -> Result<Vec<Message>, SubscriptionError> {
        let mut subscription = match self.subscription_or_ask(event).await? {
            Ok(subscription) => subscription,
            Err(reply) => return Ok(reply),
        };
        let words: Vec<&str> = args
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|word| !word.is_empty())
            .collect();
        if words.is_empty() {
            return Ok(vec![settings(&subscription, None)]);
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
        self.subscriptions.save(&subscription).await?;
        Ok(vec![Message::text(format!(
            "Гаразд, стежу за: {}.",
            describe(&subscription.categories)
        ))])
    }

    async fn nearby(&self, event: &Event, args: &str) -> Result<Vec<Message>, SubscriptionError> {
        let mut subscription = match self.subscription_or_ask(event).await? {
            Ok(subscription) => subscription,
            Err(reply) => return Ok(reply),
        };
        match args.trim().to_lowercase().as_str() {
            "on" | "так" | "вкл" | "увімк" => subscription.include_nearby = true,
            "off" | "ні" | "викл" | "вимк" => subscription.include_nearby = false,
            _ => return Ok(vec![settings(&subscription, None)]),
        }
        self.subscriptions.save(&subscription).await?;
        Ok(vec![Message::text(format!(
            "Попередження про загрози поруч: {}.",
            on_off(subscription.include_nearby)
        ))])
    }

    async fn status(&self, event: &Event) -> Result<Vec<Message>, SubscriptionError> {
        let Some(subscription) = self.subscriptions.get(&event.from).await? else {
            return Ok(vec![Message::asking_location(
                "Я ще нічого про вас не знаю. Поділіться позицією, і я почну стежити.",
            )]);
        };
        Ok(vec![Message::text(format!(
            "Позиція: {}\nВиди: {}\nЗагрози поруч: {}",
            self.near(subscription.cell),
            describe(&subscription.categories),
            if subscription.include_nearby {
                "так"
            } else {
                "ні"
            },
        ))])
    }

    /// A tap on the settings panel: switch one thing and redraw the panel where it is.
    async fn choice(
        &self,
        event: &Event,
        data: &str,
        message: &str,
    ) -> Result<Vec<Message>, SubscriptionError> {
        let mut subscription = match self.subscription_or_ask(event).await? {
            Ok(subscription) => subscription,
            Err(reply) => return Ok(reply),
        };
        let mut note = None;
        match data.strip_prefix(TOGGLE) {
            Some(NEARBY) => subscription.include_nearby = !subscription.include_nearby,
            Some(key) => {
                let Some(category) = Category::from_key(key) else {
                    return Ok(Vec::new());
                };
                if !subscription.categories.remove(&category) {
                    subscription.categories.insert(category);
                } else if subscription.categories.is_empty() {
                    // Following nothing is a subscription that never speaks; /stop is for that.
                    subscription.categories.insert(category);
                    note = Some("Хоча б один вид лишається. Щоб не отримувати нічого: /stop");
                }
            }
            None => return Ok(Vec::new()),
        }
        if note.is_none() {
            self.subscriptions.save(&subscription).await?;
        }
        Ok(vec![
            settings(&subscription, note).replacing(Some(message.to_owned())),
        ])
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

/// What a settings button sends back: `toggle:drone`, `toggle:nearby`.
const TOGGLE: &str = "toggle:";
const NEARBY: &str = "nearby";

/// The settings panel: one button per thing to follow, each showing whether it is on.
fn settings(subscription: &Subscription, note: Option<&str>) -> Message {
    let mark = |on: bool| if on { "✅" } else { "▫️" };
    let kinds = Category::ALL
        .into_iter()
        .map(|category| {
            let on = subscription.categories.contains(&category);
            Choice::new(
                format!("{} {}", mark(on), capitalised(category.label())),
                format!("{TOGGLE}{}", category.key()),
            )
        })
        .collect();
    let nearby = vec![Choice::new(
        format!("{} Загрози поруч", mark(subscription.include_nearby)),
        format!("{TOGGLE}{NEARBY}"),
    )];
    let mut text = "⚙️ Що стежити\n\nНатисніть, щоб увімкнути чи вимкнути.\n\
                    «Поруч» — коли щось летить повз ваше місто або біля нього, а не на нього."
        .to_owned();
    if let Some(note) = note {
        text.push_str("\n\n");
        text.push_str(note);
    }
    Message::text(text).with_choices(vec![kinds, nearby])
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn on_off(on: bool) -> &'static str {
    if on {
        "увімкнено"
    } else {
        "вимкнено"
    }
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
