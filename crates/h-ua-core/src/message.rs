// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! What the bot says. All copy is here, in Ukrainian, so it can be reviewed in one place.

use prism_signal_core::SourceId;
use prism_signal_normalize::{PlaceRole, Reading};

use crate::category::kind_label;
use crate::relevance::Affected;

/// Longest quote of a source post inside an alert, in characters.
const MAX_QUOTE_CHARS: usize = 400;

/// A message to a person, before any messenger formats it.
///
/// Plain text only. A messenger adapter must not need to escape anything, and an alert must not
/// break because a post contained a stray `_` or `*`.
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

/// How a source is named to a person, such as `@vanek_nikolaev`.
pub fn source_label(source: &SourceId) -> String {
    match source.as_str().split_once(':') {
        Some(("telegram.channel", name)) => format!("@{name}"),
        _ => source.as_str().to_owned(),
    }
}

fn role_label(role: PlaceRole) -> &'static str {
    match role {
        PlaceRole::Target => "ціль",
        PlaceRole::Via => "поруч",
        PlaceRole::Origin | PlaceRole::Mention => "згадано",
    }
}

fn age_label(age_secs: i64) -> String {
    match age_secs {
        s if s < 60 => "щойно".to_owned(),
        s if s < 3600 => format!("{} хв тому", s / 60),
        s => format!("{} год тому", s / 3600),
    }
}

fn quote(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_QUOTE_CHARS {
        return format!("«{flat}»");
    }
    let cut: String = flat.chars().take(MAX_QUOTE_CHARS).collect();
    format!("«{}…»", cut.trim_end())
}

fn source_line(reading: &Reading, age_secs: i64) -> String {
    format!(
        "Джерело: {}, неофіційне · {}\n{}",
        source_label(&reading.source_id),
        age_label(age_secs),
        reading.url
    )
}

/// The alert for a threat that concerns a person.
///
/// `post_text` is the source's own words, quoted when given. `age_secs` is how long ago the
/// source published the post.
pub fn threat_alert(
    reading: &Reading,
    affected: &[Affected],
    post_text: Option<&str>,
    age_secs: i64,
) -> Message {
    let kind = reading.kind.map_or("Загроза", kind_label);
    let places: Vec<String> = affected
        .iter()
        .map(|a| format!("{} ({})", a.place.name, role_label(a.place.role)))
        .collect();
    let mut lines = vec![format!("⚠️ {kind} — {}", places.join(", "))];
    if let Some(text) = post_text {
        lines.push(quote(text));
    }
    if reading.kind_inherited {
        lines.push("Вид загрози взято з попереднього рядка допису.".to_owned());
    }
    lines.push(source_line(reading, age_secs));
    Message::text(lines.join("\n"))
}

/// The correction for people who were alerted, when the source calls a threat off.
pub fn all_clear(reading: &Reading, post_text: Option<&str>, age_secs: i64) -> Message {
    // An all-clear that names no kind is about places: `минус по всему на Маяки`.
    let subject = match reading.kind {
        Some(kind) => kind_label(kind).to_owned(),
        None => {
            let names: Vec<&str> = reading.places.iter().map(|p| p.name.as_str()).collect();
            if names.is_empty() {
                "загроза".to_owned()
            } else {
                names.join(", ")
            }
        }
    };
    let mut lines = vec![format!(
        "✅ Джерело повідомляє про відбій: {subject}. Це не офіційний відбій тривоги."
    )];
    if let Some(text) = post_text {
        lines.push(quote(text));
    }
    lines.push(source_line(reading, age_secs));
    Message::text(lines.join("\n"))
}

/// The notice that must accompany every welcome: what this is and is not.
pub const NOT_OFFICIAL: &str = "⚠️ Це неофіційні повідомлення з відкритих каналів. Відсутність повідомлення не означає безпеку. Орієнтуйтесь на офіційні сповіщення про повітряну тривогу.";

/// The privacy statement shown with the welcome.
pub const PRIVACY: &str = "🔒 Зберігаю лише ідентифікатор чату та грубу клітинку близько 3 км навколо вашої позиції, а не точні координати. /stop видаляє все.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relevance::affected_places;
    use crate::subscriber::{Recipient, Subscription};
    use prism_signal_core::{ExternalId, Timestamp};
    use prism_signal_normalize::{HazardKind, Phase, PlaceMention};

    fn reading() -> Reading {
        Reading {
            source_id: SourceId::new("telegram.channel", "vanek_nikolaev").unwrap(),
            external_id: ExternalId::try_from("vanek_nikolaev/43231".to_owned()).unwrap(),
            published_at: Timestamp::parse("2026-09-28T22:19:42Z").unwrap(),
            url: "https://t.me/vanek_nikolaev/43231".to_owned(),
            forwarded: false,
            kind: Some(HazardKind::BallisticMissile),
            kind_inherited: false,
            phase: Phase::Threat,
            places: vec![PlaceMention {
                place_id: "geonames:703448".to_owned(),
                name: "Київ".to_owned(),
                lat: 50.4547,
                lon: 30.5238,
                reach_km: 20,
                role: PlaceRole::Target,
            }],
            unresolved: Vec::new(),
        }
    }

    fn affected(reading: &Reading) -> Vec<Affected> {
        let mut subscription = Subscription::new(Recipient::new("telegram", "1"));
        subscription.cell = Some(crate::geo::Cell::around(50.45, 30.52).unwrap());
        affected_places(&subscription, reading)
    }

    #[test]
    fn a_threat_alert_names_kind_place_source_age_and_link() {
        let r = reading();
        let message = threat_alert(&r, &affected(&r), Some("2 баллистики на Киев !"), 130);
        assert_eq!(
            message.text,
            "⚠️ Балістика — Київ (ціль)\n«2 баллистики на Киев !»\n\
             Джерело: @vanek_nikolaev, неофіційне · 2 хв тому\nhttps://t.me/vanek_nikolaev/43231"
        );
        assert!(!message.ask_location);
    }

    #[test]
    fn the_quote_is_optional_and_bounded() {
        let r = reading();
        let bare = threat_alert(&r, &affected(&r), None, 5);
        assert!(!bare.text.contains('«'));
        assert!(bare.text.contains("щойно"));

        let long = "слово ".repeat(200);
        let bounded = threat_alert(&r, &affected(&r), Some(&long), 5);
        let quoted = bounded.text.lines().nth(1).unwrap();
        assert!(quoted.chars().count() <= MAX_QUOTE_CHARS + 3);
        assert!(quoted.ends_with("…»"));
    }

    #[test]
    fn a_kind_taken_from_context_says_so() {
        let mut r = reading();
        r.kind_inherited = true;
        let message = threat_alert(&r, &affected(&r), None, 5);
        assert!(message.text.contains("з попереднього рядка"));
    }

    #[test]
    fn the_all_clear_is_never_presented_as_official() {
        let mut r = reading();
        r.phase = Phase::Cleared;
        let message = all_clear(&r, Some("мінус по балістиці"), 60);
        assert!(message.text.starts_with("✅"));
        assert!(message.text.contains("не офіційний відбій"));
        assert!(message.text.contains("1 хв тому"));
    }

    #[test]
    fn ages_read_naturally() {
        assert_eq!(age_label(10), "щойно");
        assert_eq!(age_label(59 * 60), "59 хв тому");
        assert_eq!(age_label(2 * 3600 + 5), "2 год тому");
    }

    #[test]
    fn a_non_telegram_source_keeps_its_full_id() {
        let other = SourceId::new("rss.feed", "example").unwrap();
        assert_eq!(source_label(&other), "rss.feed:example");
    }
}
