// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! What the bot answers, and what it keeps.

mod support;

use h_ua_core::category::Category;
use h_ua_core::conversation::Conversation;
use h_ua_core::memory::MemoryStore;
use h_ua_core::ports::{Event, EventKind, Store};
use h_ua_core::subscriber::Recipient;
use prism_signal_normalize::Gazetteer;

fn me() -> Recipient {
    Recipient::new("telegram", "42")
}

fn command(name: &str, args: &str) -> Event {
    Event {
        from: me(),
        kind: EventKind::Command {
            name: name.to_owned(),
            args: args.to_owned(),
        },
    }
}

fn location(lat: f64, lon: f64, live: bool) -> Event {
    Event {
        from: me(),
        kind: EventKind::Location { lat, lon, live },
    }
}

struct Chat {
    store: MemoryStore,
    gazetteer: Gazetteer,
}

impl Chat {
    fn new() -> Self {
        Self {
            store: MemoryStore::new(),
            gazetteer: Gazetteer::embedded().unwrap(),
        }
    }

    fn say(&self, event: &Event) -> Vec<h_ua_core::message::Message> {
        Conversation::new(
            &self.store,
            &self.gazetteer,
            vec!["@vanek_nikolaev".to_owned()],
        )
        .handle(event, 1_000)
        .unwrap()
    }
}

#[test]
fn the_welcome_says_what_this_is_not_and_what_is_kept() {
    let chat = Chat::new();
    for name in ["start", "help"] {
        let replies = chat.say(&command(name, ""));
        assert_eq!(replies.len(), 1);
        let text = &replies[0].text;
        assert!(text.contains("неофіційні"));
        assert!(text.contains("Відсутність повідомлення не означає безпеку"));
        assert!(text.contains("не точні координати"));
        assert!(text.contains("@vanek_nikolaev"));
        assert!(replies[0].ask_location);
    }
}

#[test]
fn a_position_is_kept_as_a_cell_and_confirmed_by_the_nearest_known_place() {
    let chat = Chat::new();
    let replies = chat.say(&location(50.40, 30.60, false));
    assert!(
        replies[0].text.contains("Найближче відоме місце: Київ."),
        "{}",
        replies[0].text
    );

    let saved = chat.store.subscription(&me()).unwrap().unwrap();
    let cell = saved.cell.unwrap();
    // The exact point is gone: only the cell centre is known, and it is not the point.
    assert_ne!(cell.center(), (50.40, 30.60));
    assert_eq!(saved.categories.len(), Category::ALL.len());
}

#[test]
fn a_live_position_updates_in_silence() {
    let chat = Chat::new();
    assert!(chat.say(&location(50.40, 30.60, true)).is_empty());
    assert!(
        chat.store
            .subscription(&me())
            .unwrap()
            .unwrap()
            .cell
            .is_some()
    );
    let before = chat.store.subscription(&me()).unwrap().unwrap().cell;
    // Moving to Lviv changes the cell, still without a message.
    assert!(chat.say(&location(49.84, 24.03, true)).is_empty());
    assert_ne!(
        chat.store.subscription(&me()).unwrap().unwrap().cell,
        before
    );
}

#[test]
fn a_position_keeps_the_choices_already_made() {
    let chat = Chat::new();
    chat.say(&command("kinds", "дрони"));
    chat.say(&location(50.40, 30.60, false));
    let saved = chat.store.subscription(&me()).unwrap().unwrap();
    assert_eq!(saved.categories, [Category::Drone].into());
}

#[test]
fn an_unreadable_position_is_refused_without_saving_anything() {
    let chat = Chat::new();
    let replies = chat.say(&location(200.0, 0.0, false));
    assert!(replies[0].text.contains("Не можу прочитати"));
    assert!(chat.store.subscription(&me()).unwrap().is_none());
}

#[test]
fn a_position_far_from_every_known_place_says_so_honestly() {
    let chat = Chat::new();
    // The middle of the Black Sea.
    let replies = chat.say(&location(43.0, 34.0, false));
    assert!(replies[0].text.contains("немає місць з мого словника"));
}

#[test]
fn kinds_are_chosen_in_either_language_and_bad_words_change_nothing() {
    let chat = Chat::new();
    let shown = chat.say(&command("kinds", ""));
    assert!(shown[0].text.contains("дрони, КАБи, ракети"));

    let replies = chat.say(&command("kinds", "каби, ракеты"));
    assert!(replies[0].text.contains("КАБи, ракети"));
    assert_eq!(
        chat.store.subscription(&me()).unwrap().unwrap().categories,
        [Category::Bomb, Category::Missile].into()
    );

    let refused = chat.say(&command("kinds", "дрони котики"));
    assert!(refused[0].text.contains("Не розумію «котики»"));
    assert_eq!(
        chat.store.subscription(&me()).unwrap().unwrap().categories,
        [Category::Bomb, Category::Missile].into()
    );
}

#[test]
fn nearby_warnings_toggle() {
    let chat = Chat::new();
    assert!(
        chat.say(&command("nearby", ""))[0]
            .text
            .contains("увімкнено")
    );
    assert!(
        chat.say(&command("nearby", "ні"))[0]
            .text
            .contains("вимкнено")
    );
    assert!(
        !chat
            .store
            .subscription(&me())
            .unwrap()
            .unwrap()
            .include_nearby
    );
    assert!(
        chat.say(&command("nearby", "так"))[0]
            .text
            .contains("увімкнено")
    );
    assert!(
        chat.store
            .subscription(&me())
            .unwrap()
            .unwrap()
            .include_nearby
    );
}

#[test]
fn status_reports_what_is_known_and_repeats_the_disclaimer() {
    let chat = Chat::new();
    assert!(
        chat.say(&command("status", ""))[0]
            .text
            .contains("ще нічого про вас не знаю")
    );
    chat.say(&location(50.40, 30.60, false));
    let text = &chat.say(&command("status", ""))[0].text;
    assert!(text.contains("Київ"));
    assert!(text.contains("Джерела: @vanek_nikolaev"));
    assert!(text.contains("неофіційні"));
}

#[test]
fn stop_deletes_everything_about_the_person() {
    let chat = Chat::new();
    chat.say(&location(50.40, 30.60, false));
    let replies = chat.say(&command("stop", ""));
    assert!(replies[0].text.contains("видалено"));
    assert!(chat.store.subscription(&me()).unwrap().is_none());
}

#[test]
fn unknown_input_gets_a_pointer_to_help() {
    let chat = Chat::new();
    assert!(chat.say(&command("dance", ""))[0].text.contains("/help"));
    let other = Event {
        from: me(),
        kind: EventKind::Other,
    };
    assert!(chat.say(&other)[0].text.contains("/help"));
}
