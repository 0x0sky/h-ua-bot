// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Settings, read from the environment.

use std::net::SocketAddr;

/// Shortest shared secret accepted, in characters. It is all that stands between the internet and
/// a message to anyone who ever used the bot.
pub const MIN_SECRET_CHARS: usize = 32;

/// Settings could not be read.
#[derive(Debug, Eq, PartialEq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Everything the bot is configured with.
pub struct Config {
    /// Bot API token.
    pub telegram_token: Option<String>,
    /// Bot API base URL, for a local Bot API server.
    pub telegram_api_base: Option<String>,
    /// Channels named to people as where alerts come from. Which sources the hub reads is the
    /// hub's setting; this is only what the bot says.
    pub sources: Vec<String>,
    /// The hub's origin.
    pub hub_origin: Option<String>,
    /// This bot's credential at the hub.
    pub hub_token: Option<String>,
    /// Which bot people are known as at the hub.
    pub hub_provider_scope: String,
    /// The hub's logical channel this bot's chats are bound to.
    pub alert_channel: String,
    /// Where the hub's delivery endpoint listens.
    pub delivery_listen: SocketAddr,
    /// The secret the hub sends with every delivery.
    pub delivery_secret: Option<String>,
}

fn redacted(value: &Option<String>) -> Option<&'static str> {
    value.as_ref().map(|_| "<redacted>")
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Three credentials live here. A `{config:?}` in a log line must carry none of them.
        f.debug_struct("Config")
            .field("telegram_token", &redacted(&self.telegram_token))
            .field("telegram_api_base", &self.telegram_api_base)
            .field("sources", &self.sources)
            .field("hub_origin", &self.hub_origin)
            .field("hub_token", &redacted(&self.hub_token))
            .field("hub_provider_scope", &self.hub_provider_scope)
            .field("alert_channel", &self.alert_channel)
            .field("delivery_listen", &self.delivery_listen)
            .field("delivery_secret", &redacted(&self.delivery_secret))
            .finish()
    }
}

/// A Telegram public channel username, as people write it.
fn channel(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && (4..=32).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Config {
    /// Reads settings from a lookup, so tests need not touch the process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |key: &str| {
            lookup(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };

        let sources_text = get("HUA_SOURCES").unwrap_or_else(|| "vanek_nikolaev".to_owned());
        let sources = sources_text
            .split(',')
            .map(|name| name.trim().trim_start_matches('@'))
            .filter(|name| !name.is_empty())
            .map(|name| {
                if channel(name) {
                    Ok(format!("@{name}"))
                } else {
                    Err(ConfigError(format!(
                        "HUA_SOURCES has an invalid channel `{name}`"
                    )))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        if sources.is_empty() {
            return Err(ConfigError("HUA_SOURCES names no channel".to_owned()));
        }

        let hub_origin = get("HUA_HUB_ORIGIN");
        if let Some(origin) = &hub_origin {
            let local =
                origin.starts_with("http://localhost") || origin.starts_with("http://127.0.0.1");
            if !origin.starts_with("https://") && !local {
                return Err(ConfigError(
                    "HUA_HUB_ORIGIN must be an https:// address".to_owned(),
                ));
            }
        }
        let delivery_secret = get("HUA_DELIVERY_SECRET");
        if let Some(secret) = &delivery_secret {
            if secret.chars().count() < MIN_SECRET_CHARS {
                return Err(ConfigError(format!(
                    "HUA_DELIVERY_SECRET must be at least {MIN_SECRET_CHARS} characters"
                )));
            }
        }
        let delivery_listen = match get("HUA_DELIVERY_LISTEN") {
            None => SocketAddr::from(([0, 0, 0, 0], 8080)),
            Some(text) => text.parse().map_err(|_| {
                ConfigError("HUA_DELIVERY_LISTEN must be an address like 0.0.0.0:8080".to_owned())
            })?,
        };

        Ok(Self {
            telegram_token: get("HUA_TELEGRAM_TOKEN"),
            telegram_api_base: get("HUA_TELEGRAM_API_BASE"),
            sources,
            hub_origin,
            hub_token: get("HUA_HUB_TOKEN"),
            hub_provider_scope: get("HUA_HUB_PROVIDER_SCOPE")
                .unwrap_or_else(|| "h-ua-bot".to_owned()),
            alert_channel: get("HUA_ALERT_CHANNEL").unwrap_or_else(|| "alerts".to_owned()),
            delivery_listen,
            delivery_secret,
        })
    }

    /// Reads settings from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_lookup(|key| map.get(key).cloned())
    }

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn the_defaults_name_the_first_channel_and_need_nothing_secret() {
        let c = config(&[]).unwrap();
        assert_eq!(c.sources, ["@vanek_nikolaev"]);
        assert_eq!(c.hub_provider_scope, "h-ua-bot");
        assert_eq!(c.alert_channel, "alerts");
        assert_eq!(c.delivery_listen, SocketAddr::from(([0, 0, 0, 0], 8080)));
        assert!(c.telegram_token.is_none() && c.hub_token.is_none() && c.delivery_secret.is_none());
    }

    #[test]
    fn several_channels_are_listed_with_or_without_an_at_sign() {
        let c = config(&[("HUA_SOURCES", "vanek_nikolaev, @other_channel ,")]).unwrap();
        assert_eq!(c.sources, ["@vanek_nikolaev", "@other_channel"]);
    }

    #[test]
    fn bad_values_are_named_not_defaulted() {
        let bad = [
            ("HUA_SOURCES", "a b", "HUA_SOURCES"),
            ("HUA_SOURCES", " , ", "names no channel"),
            ("HUA_HUB_ORIGIN", "http://hub.example", "https://"),
            ("HUA_DELIVERY_SECRET", "short", "HUA_DELIVERY_SECRET"),
            ("HUA_DELIVERY_LISTEN", "everywhere", "HUA_DELIVERY_LISTEN"),
        ];
        for (key, value, expected) in bad {
            let error = config(&[(key, value)]).unwrap_err().0;
            assert!(error.contains(expected), "{key}: {error}");
        }
    }

    #[test]
    fn a_local_hub_may_use_plain_http() {
        assert!(config(&[("HUA_HUB_ORIGIN", "http://localhost:3000")]).is_ok());
        assert!(config(&[("HUA_HUB_ORIGIN", "https://hub.example")]).is_ok());
    }

    #[test]
    fn no_credential_appears_when_settings_are_printed() {
        let c = config(&[
            ("HUA_TELEGRAM_TOKEN", "123456:SECRET-TOKEN"),
            ("HUA_HUB_TOKEN", "hub-credential-value"),
            ("HUA_DELIVERY_SECRET", SECRET),
        ])
        .unwrap();
        let shown = format!("{c:?}");
        for secret in ["SECRET-TOKEN", "hub-credential-value", SECRET] {
            assert!(!shown.contains(secret), "{shown}");
        }
        assert_eq!(shown.matches("<redacted>").count(), 3);
        assert!(
            !format!("{:?}", config(&[]).unwrap()).contains("<redacted>"),
            "absent stays absent"
        );
    }
}
