// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The language the bot speaks to a person.
//!
//! A messenger reports a language tag such as `uk`, `ru-RU` or `en-GB`, usually without a region.
//! The bot serves Ukraine, so Ukrainian, Russian and Belarusian speakers are answered in
//! Ukrainian unless the tag places them in Russia, where they are answered in Russian. Everyone
//! else is answered in English.

/// A language the bot has copy for.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Locale {
    /// Ukrainian, also the language when nothing is known.
    #[default]
    Uk,
    /// Russian.
    Ru,
    /// English, for everyone else.
    En,
}

impl Locale {
    /// Every locale, Ukrainian first.
    pub const ALL: [Locale; 3] = [Locale::Uk, Locale::Ru, Locale::En];

    /// Resolves a language tag: `uk`, `ru-UA`, `be-BY` → Ukrainian; `ru-RU`, `uk-RU`, `be-RU` →
    /// Russian; any other language → English. An empty tag means nothing is known: Ukrainian.
    pub fn from_tag(tag: &str) -> Self {
        let tag = tag.trim().replace('_', "-");
        if tag.is_empty() {
            return Self::default();
        }
        let mut parts = tag.split('-');
        let language = parts.next().unwrap_or_default().to_lowercase();
        // A region is the first two-letter subtag; script subtags such as `Cyrl` are skipped.
        let region = parts
            .find(|part| part.len() == 2)
            .map(str::to_uppercase)
            .unwrap_or_default();
        match language.as_str() {
            "uk" | "ru" | "be" if region == "RU" => Self::Ru,
            "uk" | "ru" | "be" => Self::Uk,
            _ => Self::En,
        }
    }

    /// The BCP 47 language subtag: `uk`, `ru`, `en`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Uk => "uk",
            Self::Ru => "ru",
            Self::En => "en",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ukrainian_for_ukraine_and_for_a_bare_tag() {
        for tag in [
            "uk", "uk-UA", "ru-UA", "be-UA", "be-BY", "ru-BY", "ru", "be", "UK_ua", " uk ",
        ] {
            assert_eq!(Locale::from_tag(tag), Locale::Uk, "{tag}");
        }
    }

    #[test]
    fn russian_for_russia() {
        for tag in ["ru-RU", "uk-RU", "be-RU", "ru_ru", "ru-Cyrl-RU"] {
            assert_eq!(Locale::from_tag(tag), Locale::Ru, "{tag}");
        }
    }

    #[test]
    fn english_for_everyone_else() {
        for tag in ["en", "en-GB", "de-DE", "pl", "ja-JP", "kk-KZ"] {
            assert_eq!(Locale::from_tag(tag), Locale::En, "{tag}");
        }
    }

    #[test]
    fn nothing_known_is_ukrainian() {
        assert_eq!(Locale::from_tag(""), Locale::Uk);
        assert_eq!(Locale::from_tag("  "), Locale::Uk);
        assert_eq!(Locale::default(), Locale::Uk);
    }

    #[test]
    fn codes() {
        let codes: Vec<_> = Locale::ALL.into_iter().map(Locale::code).collect();
        assert_eq!(codes, ["uk", "ru", "en"]);
    }
}
