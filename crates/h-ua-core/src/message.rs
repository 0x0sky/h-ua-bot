// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! What the bot says in conversation. All copy is here, in Ukrainian, so it can be reviewed in one
//! place. Alerts are not here: the hub composes them and Porter renders them.

/// A message to a person, before any messenger formats it.
///
/// Plain text only. A messenger adapter must not need to escape anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message {
    /// The text.
    pub text: String,
    /// Offer the person a way to share their position with one tap, where the messenger has one.
    pub ask_location: bool,
}

impl Message {
    /// A plain message.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ask_location: false,
        }
    }

    /// A message with a share-position button.
    pub fn asking_location(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ask_location: true,
        }
    }
}

/// The notice that must accompany every welcome: what this is and is not.
pub const NOT_OFFICIAL: &str = "⚠️ Це неофіційні повідомлення з відкритих каналів. Відсутність повідомлення не означає безпеку. Орієнтуйтесь на офіційні сповіщення про повітряну тривогу.";

/// The privacy statement shown with the welcome.
pub const PRIVACY: &str = "🔒 Позицію зберігаю лише як грубу клітинку близько 3 км, а не точні координати. Разом із нею зберігається технічний ідентифікатор вашого чату. /stop видаляє підписку й позицію.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_disclaimers_say_the_two_things_that_matter() {
        assert!(NOT_OFFICIAL.contains("неофіційні"));
        assert!(NOT_OFFICIAL.contains("не означає безпеку"));
        assert!(PRIVACY.contains("не точні координати"));
    }

    #[test]
    fn a_message_can_ask_for_a_position() {
        assert!(!Message::text("x").ask_location);
        assert!(Message::asking_location("x").ask_location);
    }
}
