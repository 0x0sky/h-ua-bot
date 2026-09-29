// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! What the bot answers, and what it asks to be kept.

use h_ua_core::category::Category;
use h_ua_core::conversation::Conversation;
use h_ua_core::memory::MemorySubscriptions;
use h_ua_core::message::Message;
use h_ua_core::ports::{Event, EventKind, Subscriptions};
use h_ua_core::subscriber::{Recipient, Subscription};
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
    subscriptions: MemorySubscriptions,
    gazetteer: Gazetteer,
}

impl Chat {
    fn new() -> Self {
        Self {
            subscriptions: MemorySubscriptions::new(),
            gazetteer: Gazetteer::embedded().unwrap(),
        }
    }

    async fn say(&self, event: &Event) -> Vec<Message> {
        Conversation::new(
            &self.subscriptions,
            &self.gazetteer,
            vec!["@vanek_nikolaev".to_owned()],
        )
        .handle(event)
        .await
        .unwrap()
    }

    async fn saved(&self) -> Option<Subscription> {
        self.subscriptions.get(&me()).await.unwrap()
    }
}

#[tokio::test]
async fn the_welcome_says_what_this_is_not_and_what_is_kept() {
    let chat = Chat::new();
    for name in ["start", "help"] {
        let replies = chat.say(&command(name, "")).await;
        assert_eq!(replies.len(), 1);
        let text = &replies[0].text;
        assert!(text.contains("неофіційні"));
        assert!(text.contains("Відсутність повідомлення не означає безпеку"));
        assert!(text.contains("не точні координати"));
        assert!(text.contains("ідентифікатор вашого чату"));
        assert!(text.contains("@vanek_nikolaev"));
        assert!(replies[0].ask_location);
    }
}

#[tokio::test]
async fn a_position_is_kept_as_a_cell_and_confirmed_by_the_nearest_known_place() {
    let chat = Chat::new();
    let replies = chat.say(&location(50.40, 30.60, false)).await;
    assert!(
        replies[0].text.contains("Найближче відоме місце: Київ."),
        "{}",
        replies[0].text
    );

    let saved = chat.saved().await.unwrap();
    // The exact point is gone: only the cell centre is known, and it is not the point.
    assert_ne!(saved.cell.center(), (50.40, 30.60));
    assert_eq!(saved.categories.len(), Category::ALL.len());
    assert!(saved.include_nearby);
}

#[tokio::test]
async fn a_live_position_updates_in_silence() {
    let chat = Chat::new();
    assert!(chat.say(&location(50.40, 30.60, true)).await.is_empty());
    let before = chat.saved().await.unwrap().cell;
    // Moving to Lviv changes the cell, still without a message.
    assert!(chat.say(&location(49.84, 24.03, true)).await.is_empty());
    assert_ne!(chat.saved().await.unwrap().cell, before);
}

#[tokio::test]
async fn moving_keeps_the_choices_already_made() {
    let chat = Chat::new();
    chat.say(&location(50.40, 30.60, false)).await;
    chat.say(&command("kinds", "дрони")).await;
    chat.say(&command("nearby", "ні")).await;

    chat.say(&location(49.84, 24.03, false)).await;

    let saved = chat.saved().await.unwrap();
    assert_eq!(saved.categories, [Category::Drone].into());
    assert!(!saved.include_nearby);
}

#[tokio::test]
async fn an_unreadable_position_is_refused_without_saving_anything() {
    let chat = Chat::new();
    let replies = chat.say(&location(200.0, 0.0, false)).await;
    assert!(replies[0].text.contains("Не можу прочитати"));
    assert!(chat.saved().await.is_none());
}

#[tokio::test]
async fn a_position_far_from_every_known_place_says_so_honestly() {
    let chat = Chat::new();
    // The middle of the Black Sea.
    let replies = chat.say(&location(43.0, 34.0, false)).await;
    assert!(replies[0].text.contains("немає місць з мого словника"));
}

#[tokio::test]
async fn choices_need_a_position_first_and_save_nothing_without_one() {
    let chat = Chat::new();
    for event in [command("kinds", "дрони"), command("nearby", "ні")] {
        let replies = chat.say(&event).await;
        assert!(replies[0].text.contains("Спершу поділіться позицією"));
        assert!(replies[0].ask_location);
    }
    assert!(chat.saved().await.is_none());
    assert!(chat.subscriptions.is_empty());
}

#[tokio::test]
async fn kinds_are_chosen_in_either_language_and_bad_words_change_nothing() {
    let chat = Chat::new();
    chat.say(&location(50.40, 30.60, false)).await;
    let shown = chat.say(&command("kinds", "")).await;
    assert!(shown[0].text.contains("дрони, КАБи, ракети"));

    let replies = chat.say(&command("kinds", "каби, ракеты")).await;
    assert!(replies[0].text.contains("КАБи, ракети"));
    assert_eq!(
        chat.saved().await.unwrap().categories,
        [Category::Bomb, Category::Missile].into()
    );

    let refused = chat.say(&command("kinds", "дрони котики")).await;
    assert!(refused[0].text.contains("Не розумію «котики»"));
    assert_eq!(
        chat.saved().await.unwrap().categories,
        [Category::Bomb, Category::Missile].into()
    );
}

#[tokio::test]
async fn nearby_warnings_toggle() {
    let chat = Chat::new();
    chat.say(&location(50.40, 30.60, false)).await;
    assert!(
        chat.say(&command("nearby", "")).await[0]
            .text
            .contains("увімкнено")
    );
    assert!(
        chat.say(&command("nearby", "ні")).await[0]
            .text
            .contains("вимкнено")
    );
    assert!(!chat.saved().await.unwrap().include_nearby);
    assert!(
        chat.say(&command("nearby", "так")).await[0]
            .text
            .contains("увімкнено")
    );
    assert!(chat.saved().await.unwrap().include_nearby);
}

#[tokio::test]
async fn status_reports_what_is_known_and_repeats_the_disclaimer() {
    let chat = Chat::new();
    assert!(
        chat.say(&command("status", "")).await[0]
            .text
            .contains("ще нічого про вас не знаю")
    );
    chat.say(&location(50.40, 30.60, false)).await;
    let text = &chat.say(&command("status", "")).await[0].text;
    assert!(text.contains("Київ"));
    assert!(text.contains("Джерела: @vanek_nikolaev"));
    assert!(text.contains("неофіційні"));
}

#[tokio::test]
async fn stop_removes_the_subscription_and_says_what_remains() {
    let chat = Chat::new();
    chat.say(&location(50.40, 30.60, false)).await;
    let replies = chat.say(&command("stop", "")).await;
    assert!(replies[0].text.contains("підписку й позицію видалено"));
    assert!(replies[0].text.contains("ідентифікатор чату лишається"));
    assert!(chat.saved().await.is_none());
}

#[tokio::test]
async fn unknown_input_gets_a_pointer_to_help() {
    let chat = Chat::new();
    assert!(
        chat.say(&command("dance", "")).await[0]
            .text
            .contains("/help")
    );
    let other = Event {
        from: me(),
        kind: EventKind::Other,
    };
    assert!(chat.say(&other).await[0].text.contains("/help"));
}
