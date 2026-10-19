// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Source references displayed by a client.
//!
//! This is metadata, not a source collector or evidence of activation. The hub owns collection,
//! provenance, trust, and fusion; individual deliveries contain their own source attribution.

use serde::Deserialize;

/// The transport or feed format of a configured source.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Public Telegram channel.
    TelegramChannel,
    /// Structured HTTP API, official or third-party.
    Api,
    /// RSS or Atom feed.
    Rss,
    /// Public webpage (collection belongs to Prism).
    Website,
    /// Partner event feed (its inbound endpoint belongs to Prism).
    Webhook,
}

/// Public source metadata for a bot's welcome and status messages.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Stable identity in the source catalog; not a display name.
    pub id: String,
    /// The kind of upstream source.
    pub kind: SourceKind,
    /// Short display name chosen by the operator.
    pub label: String,
}

impl Source {
    /// A legacy Telegram channel, preserving its current public label.
    pub fn telegram_channel(name: &str) -> Self {
        Self {
            id: format!("telegram:{name}"),
            kind: SourceKind::TelegramChannel,
            label: format!("@{name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_have_a_stable_id_separate_from_the_label() {
        let source = Source::telegram_channel("vanek_nikolaev");
        assert_eq!(source.id, "telegram:vanek_nikolaev");
        assert_eq!(source.label, "@vanek_nikolaev");
        assert_eq!(source.kind, SourceKind::TelegramChannel);
    }

    #[test]
    fn structured_catalog_accepts_api_and_feed_kinds() {
        let sources: Vec<Source> = serde_json::from_str(
            r#"[{"id":"api:alarm","kind":"api","label":"Повітряна тривога"},{"id":"feed:dsns","kind":"rss","label":"ДСНС"}]"#,
        )
        .unwrap();
        assert_eq!(sources[0].kind, SourceKind::Api);
        assert_eq!(sources[1].kind, SourceKind::Rss);
    }
}
