// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! What the bot does when a person writes to it.

use std::collections::BTreeSet;

use prism_signal_normalize::{Gazetteer, Place};

use crate::category::Category;
use crate::copy::{Copy, fill};
use crate::geo::Cell;
use crate::message::{Choice, Message};
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
        let copy = Copy::of(event.locale);
        let replies = match &event.kind {
            EventKind::Command { name, args } => self.command(event, copy, name, args).await,
            EventKind::Location { lat, lon, live } => {
                self.location(event, copy, *lat, *lon, *live).await
            }
            EventKind::Choice { data, message } => self.choice(event, copy, data, message).await,
            EventKind::Other => Ok(vec![Message::text(copy.other)]),
        }?;
        // The messenger draws its buttons in the same language.
        Ok(replies
            .into_iter()
            .map(|reply| reply.in_locale(event.locale))
            .collect())
    }

    async fn command(
        &self,
        event: &Event,
        copy: &Copy,
        name: &str,
        args: &str,
    ) -> Result<Vec<Message>, SubscriptionError> {
        match name {
            "start" | "help" => Ok(vec![welcome(copy)]),
            "location" => Ok(vec![Message::asking_location(copy.location_prompt)]),
            "kinds" => self.kinds(event, copy, args).await,
            "menu" => Ok(vec![Message::text(copy.menu)]),
            "nearby" => self.nearby(event, copy, args).await,
            "status" => self.status(event, copy).await,
            "stop" => {
                self.subscriptions.delete(&event.from).await?;
                Ok(vec![Message::text(copy.stopped)])
            }
            _ => Ok(vec![Message::text(copy.unknown_command)]),
        }
    }

    async fn location(
        &self,
        event: &Event,
        copy: &Copy,
        lat: f64,
        lon: f64,
        live: bool,
    ) -> Result<Vec<Message>, SubscriptionError> {
        let Ok(cell) = Cell::around(lat, lon) else {
            return Ok(vec![Message::text(copy.bad_location)]);
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
        Ok(vec![Message::text(fill(
            copy.saved,
            &[
                ("near", &self.near(copy, cell)),
                ("kinds", &describe(copy, &subscription.categories)),
                ("settings", copy.button_settings),
            ],
        ))])
    }

    /// Choices are made on a subscription, and a subscription needs a position first.
    async fn subscription_or_ask(
        &self,
        event: &Event,
        copy: &Copy,
    ) -> Result<Result<Subscription, Vec<Message>>, SubscriptionError> {
        Ok(self
            .subscriptions
            .get(&event.from)
            .await?
            .ok_or_else(|| vec![Message::asking_location(copy.position_first)]))
    }

    async fn kinds(
        &self,
        event: &Event,
        copy: &Copy,
        args: &str,
    ) -> Result<Vec<Message>, SubscriptionError> {
        let mut subscription = match self.subscription_or_ask(event, copy).await? {
            Ok(subscription) => subscription,
            Err(reply) => return Ok(reply),
        };
        let words: Vec<&str> = args
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|word| !word.is_empty())
            .collect();
        if words.is_empty() {
            return Ok(vec![settings(copy, &subscription, None)]);
        }
        let mut chosen = BTreeSet::new();
        for word in &words {
            match Category::from_word(word) {
                Some(category) => {
                    chosen.insert(category);
                }
                None => {
                    return Ok(vec![Message::text(fill(
                        copy.unknown_kind,
                        &[("word", word)],
                    ))]);
                }
            }
        }
        subscription.categories = chosen;
        self.subscriptions.save(&subscription).await?;
        Ok(vec![Message::text(fill(
            copy.following,
            &[("kinds", &describe(copy, &subscription.categories))],
        ))])
    }

    async fn nearby(
        &self,
        event: &Event,
        copy: &Copy,
        args: &str,
    ) -> Result<Vec<Message>, SubscriptionError> {
        let mut subscription = match self.subscription_or_ask(event, copy).await? {
            Ok(subscription) => subscription,
            Err(reply) => return Ok(reply),
        };
        match args.trim().to_lowercase().as_str() {
            "on" | "yes" | "так" | "да" | "вкл" | "увімк" => {
                subscription.include_nearby = true
            }
            "off" | "no" | "ні" | "нет" | "викл" | "выкл" | "вимк" => {
                subscription.include_nearby = false
            }
            _ => return Ok(vec![settings(copy, &subscription, None)]),
        }
        self.subscriptions.save(&subscription).await?;
        let state = if subscription.include_nearby {
            copy.on
        } else {
            copy.off
        };
        Ok(vec![Message::text(fill(
            copy.nearby_set,
            &[("state", state)],
        ))])
    }

    async fn status(&self, event: &Event, copy: &Copy) -> Result<Vec<Message>, SubscriptionError> {
        let Some(subscription) = self.subscriptions.get(&event.from).await? else {
            return Ok(vec![Message::asking_location(copy.nothing_known)]);
        };
        let nearby = if subscription.include_nearby {
            copy.yes
        } else {
            copy.no
        };
        Ok(vec![Message::text(fill(
            copy.status,
            &[
                ("near", &self.near(copy, subscription.cell)),
                ("kinds", &describe(copy, &subscription.categories)),
                ("nearby", nearby),
            ],
        ))])
    }

    /// A settings choice: switch one thing and return the updated panel.
    async fn choice(
        &self,
        event: &Event,
        copy: &Copy,
        data: &str,
        message: &str,
    ) -> Result<Vec<Message>, SubscriptionError> {
        let mut subscription = match self.subscription_or_ask(event, copy).await? {
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
                    note = Some(copy.keep_one);
                }
            }
            None => return Ok(Vec::new()),
        }
        if note.is_none() {
            self.subscriptions.save(&subscription).await?;
        }
        Ok(vec![
            // A reply-keyboard tap has no message to replace.
            settings(copy, &subscription, note)
                .replacing(Some(message.to_owned()).filter(|id| !id.is_empty())),
        ])
    }

    /// `Найближче відоме місце: Київ.`, or nothing useful to say.
    fn near(&self, copy: &Copy, cell: Cell) -> String {
        match nearest(self.gazetteer, cell) {
            Some(place) => fill(copy.near_place, &[("place", &place.name)]),
            None => copy.no_place.to_owned(),
        }
    }
}

/// The welcome: what the bot does, the two notices, and the buttons.
fn welcome(copy: &Copy) -> Message {
    let buttons = fill(
        copy.welcome_buttons,
        &[
            ("location", copy.button_location),
            ("status", copy.button_status),
            ("settings", copy.button_settings),
        ],
    );
    Message::asking_location(format!(
        "{}\n\n{}\n\n{}\n\n{buttons}",
        copy.welcome, copy.not_official, copy.privacy
    ))
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
fn settings(copy: &Copy, subscription: &Subscription, note: Option<&str>) -> Message {
    let mark = |on: bool| if on { "✅" } else { "▫️" };
    let kinds = Category::ALL
        .into_iter()
        .map(|category| {
            let on = subscription.categories.contains(&category);
            Choice::new(
                format!("{} {}", mark(on), capitalised(copy.category(category))),
                format!("{TOGGLE}{}", category.key()),
            )
        })
        .collect();
    let nearby = vec![Choice::new(
        format!(
            "{} {}",
            mark(subscription.include_nearby),
            copy.nearby_choice
        ),
        format!("{TOGGLE}{NEARBY}"),
    )];
    let mut text = copy.settings.to_owned();
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

fn describe(copy: &Copy, categories: &BTreeSet<Category>) -> String {
    if categories.is_empty() {
        return copy.nothing.to_owned();
    }
    categories
        .iter()
        .map(|c| copy.category(*c))
        .collect::<Vec<_>>()
        .join(", ")
}
