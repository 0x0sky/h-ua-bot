// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! What the bot says in conversation, before a messenger formats it. The copy itself is in
//! [`copy`](crate::copy). Alerts are not here: the hub composes them and Porter renders them.

use crate::locale::Locale;

/// A message to a person, before any messenger formats it.
///
/// Plain text only. A messenger adapter must not need to escape anything.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Message {
    /// The text.
    pub text: String,
    /// Offer the person a way to share their position with one tap, where the messenger has one.
    pub ask_location: bool,
    /// Choices in rows. A messenger may render these as inline or reply-keyboard buttons.
    /// Tapping one comes back as
    /// [`EventKind::Choice`](crate::ports::EventKind::Choice) with its `data`.
    pub choices: Vec<Vec<Choice>>,
    /// Replace this earlier message when the messenger supports editing its choice markup.
    /// Telegram reply keyboards require a new message instead.
    pub replaces: Option<String>,
    /// The language of the text, so a messenger draws its own buttons in the same one.
    pub locale: Locale,
}

/// A button under a message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Choice {
    /// What the button says.
    pub label: String,
    /// What comes back when it is tapped. Short: messengers limit it.
    pub data: String,
}

impl Choice {
    /// A button.
    pub fn new(label: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            data: data.into(),
        }
    }
}

impl Message {
    /// A plain message.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// A message with a share-position button.
    pub fn asking_location(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ask_location: true,
            ..Self::default()
        }
    }

    /// The same message with buttons under it.
    pub fn with_choices(mut self, choices: Vec<Vec<Choice>>) -> Self {
        self.choices = choices;
        self
    }

    /// The same message, replacing an earlier one.
    pub fn replacing(mut self, message: Option<String>) -> Self {
        self.replaces = message;
        self
    }

    /// The same message, marked as written in a language.
    pub fn in_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_can_ask_for_a_position() {
        assert!(!Message::text("x").ask_location);
        assert!(Message::asking_location("x").ask_location);
    }
}
