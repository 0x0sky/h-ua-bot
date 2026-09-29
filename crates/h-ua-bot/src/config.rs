// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Settings, read from the environment.

use std::path::PathBuf;
use std::time::Duration;

use h_ua_core::relay::RelayPolicy;
use prism_signal_source_telegram::ChannelName;

/// Fewest seconds between two reads of a source. The channel preview is a public courtesy, not
/// an API with a quota, so it is not polled faster than this.
pub const MIN_POLL_SECS: u64 = 15;

/// Settings could not be read.
#[derive(Debug, Eq, PartialEq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Everything the bot is configured with.
#[derive(Debug)]
pub struct Config {
    /// Bot API token. Absent for commands that do not talk to Telegram.
    pub telegram_token: Option<String>,
    /// Bot API base URL, for a local Bot API server.
    pub telegram_api_base: Option<String>,
    /// Telegram channels to read, in order.
    pub sources: Vec<ChannelName>,
    /// Where the database lives.
    pub db_path: PathBuf,
    /// Time between two reads of a source.
    pub poll_interval: Duration,
    /// How the relay behaves.
    pub policy: RelayPolicy,
}

impl Config {
    /// Reads settings from a lookup, so tests need not touch the process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |key: &str| {
            lookup(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let number = |key: &str, default: i64, min: i64| -> Result<i64, ConfigError> {
            match get(key) {
                None => Ok(default),
                Some(text) => text
                    .parse::<i64>()
                    .ok()
                    .filter(|n| *n >= min)
                    .ok_or_else(|| {
                        ConfigError(format!("{key} must be a whole number of at least {min}"))
                    }),
            }
        };

        let sources_text = get("HUA_SOURCES").unwrap_or_else(|| "vanek_nikolaev".to_owned());
        let sources = sources_text
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| {
                ChannelName::parse(name.trim_start_matches('@')).map_err(|_| {
                    ConfigError(format!("HUA_SOURCES has an invalid channel `{name}`"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if sources.is_empty() {
            return Err(ConfigError("HUA_SOURCES names no channel".to_owned()));
        }

        let defaults = RelayPolicy::default();
        let include_text = match get("HUA_INCLUDE_TEXT").as_deref() {
            None => defaults.include_text,
            Some("true" | "1" | "yes") => true,
            Some("false" | "0" | "no") => false,
            Some(other) => {
                return Err(ConfigError(format!(
                    "HUA_INCLUDE_TEXT must be true or false, not `{other}`"
                )));
            }
        };

        Ok(Self {
            telegram_token: get("HUA_TELEGRAM_TOKEN"),
            telegram_api_base: get("HUA_TELEGRAM_API_BASE"),
            sources,
            db_path: PathBuf::from(
                get("HUA_DB_PATH").unwrap_or_else(|| "var/h-ua.sqlite".to_owned()),
            ),
            poll_interval: Duration::from_secs(
                number("HUA_POLL_SECS", 30, MIN_POLL_SECS as i64)? as u64
            ),
            policy: RelayPolicy {
                max_age_secs: number("HUA_MAX_AGE_SECS", defaults.max_age_secs, 1)?,
                cooldown_secs: number("HUA_COOLDOWN_SECS", defaults.cooldown_secs, 0)?,
                cleared_window_secs: number(
                    "HUA_CLEARED_WINDOW_SECS",
                    defaults.cleared_window_secs,
                    1,
                )?,
                include_text,
            },
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

    #[test]
    fn the_defaults_read_the_first_channel_and_need_no_token() {
        let c = config(&[]).unwrap();
        assert_eq!(c.sources.len(), 1);
        assert_eq!(c.sources[0].as_str(), "vanek_nikolaev");
        assert!(c.telegram_token.is_none());
        assert_eq!(c.poll_interval, Duration::from_secs(30));
        assert_eq!(c.policy, RelayPolicy::default());
    }

    #[test]
    fn several_channels_are_listed_with_or_without_an_at_sign() {
        let c = config(&[("HUA_SOURCES", "vanek_nikolaev, @other_channel ,")]).unwrap();
        let names: Vec<_> = c.sources.iter().map(|s| s.as_str().to_owned()).collect();
        assert_eq!(names, ["vanek_nikolaev", "other_channel"]);
    }

    #[test]
    fn bad_values_are_named_not_defaulted() {
        assert!(
            config(&[("HUA_SOURCES", "a b")])
                .unwrap_err()
                .0
                .contains("HUA_SOURCES")
        );
        assert!(
            config(&[("HUA_SOURCES", " , ")])
                .unwrap_err()
                .0
                .contains("names no channel")
        );
        assert!(
            config(&[("HUA_POLL_SECS", "5")])
                .unwrap_err()
                .0
                .contains("HUA_POLL_SECS")
        );
        assert!(
            config(&[("HUA_POLL_SECS", "fast")])
                .unwrap_err()
                .0
                .contains("HUA_POLL_SECS")
        );
        assert!(
            config(&[("HUA_MAX_AGE_SECS", "0")])
                .unwrap_err()
                .0
                .contains("HUA_MAX_AGE_SECS")
        );
        assert!(
            config(&[("HUA_INCLUDE_TEXT", "maybe")])
                .unwrap_err()
                .0
                .contains("HUA_INCLUDE_TEXT")
        );
    }

    #[test]
    fn the_relay_policy_can_be_tuned() {
        let c = config(&[
            ("HUA_TELEGRAM_TOKEN", " 1:abc "),
            ("HUA_MAX_AGE_SECS", "600"),
            ("HUA_COOLDOWN_SECS", "0"),
            ("HUA_CLEARED_WINDOW_SECS", "1800"),
            ("HUA_INCLUDE_TEXT", "false"),
        ])
        .unwrap();
        assert_eq!(c.telegram_token.as_deref(), Some("1:abc"));
        assert_eq!(
            c.policy,
            RelayPolicy {
                max_age_secs: 600,
                cooldown_secs: 0,
                cleared_window_secs: 1800,
                include_text: false
            }
        );
    }
}
