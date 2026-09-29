// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The hub's HTTP API, one method per call this bot makes.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

const TIMEOUT: Duration = Duration::from_secs(15);

/// How to reach the hub and who this bot is to it.
#[derive(Clone)]
pub struct HubConfig {
    /// The hub's origin, such as `https://hub.example`.
    pub origin: String,
    /// The bearer credential of this bot's service principal.
    pub token: String,
    /// Identity provider name the hub knows people by. `telegram`.
    pub provider: String,
    /// Which bot the provider subjects belong to.
    pub provider_scope: String,
    /// The hub's logical channel this bot's chats are bound to.
    pub alert_channel: String,
}

impl fmt::Debug for HubConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The token is a credential. A `{config:?}` in a log line must not carry it.
        f.debug_struct("HubConfig")
            .field("origin", &self.origin)
            .field("token", &"<redacted>")
            .field("provider", &self.provider)
            .field("provider_scope", &self.provider_scope)
            .field("alert_channel", &self.alert_channel)
            .finish()
    }
}

/// A person as the hub identifies them: provider evidence, never an internal id.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Subject {
    /// Provider name.
    pub provider: String,
    /// Provider scope.
    pub provider_scope: String,
    /// The person's id at the provider.
    pub subject_id: String,
}

/// What a person asked for, as the hub stores it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubscriptionState {
    /// A resolution 6 grid cell, 15 lowercase hexadecimal digits.
    pub cell: String,
    /// `drone`, `bomb`, `missile`.
    pub categories: Vec<String>,
    /// Also warn about threats near the position.
    pub include_nearby: bool,
}

/// A call to the hub failed.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum HubError {
    /// The hub could not be reached, or did not answer in time.
    #[error("hub unreachable: {0}")]
    Unavailable(String),
    /// The hub does not accept this bot's credential.
    #[error("hub refused the credential")]
    Unauthorized,
    /// The hub understood the request and said no.
    #[error("hub rejected the request ({status}, {code})")]
    Rejected {
        /// HTTP status.
        status: u16,
        /// The hub's error code, such as `hub.actor.not_authorized`.
        code: String,
    },
    /// The hub answered with something that is not its contract.
    #[error("hub answered unexpectedly: {0}")]
    Unexpected(String),
}

impl HubError {
    /// The hub's error code, if it gave one.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Rejected { code, .. } => Some(code),
            _ => None,
        }
    }
}

/// The hub, over HTTP.
pub struct HubClient {
    http: reqwest::Client,
    config: HubConfig,
}

impl HubClient {
    /// A client for a hub.
    pub fn new(config: HubConfig) -> Result<Self, HubError> {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| HubError::Unavailable(e.without_url().to_string()))?;
        Ok(Self {
            http,
            config: HubConfig {
                origin: config.origin.trim_end_matches('/').to_owned(),
                ..config
            },
        })
    }

    /// The configuration, for callers that need the channel or provider names.
    pub fn config(&self) -> &HubConfig {
        &self.config
    }

    /// The hub's identity for a person at this provider.
    pub fn subject(&self, subject_id: &str) -> Subject {
        Subject {
            provider: self.config.provider.clone(),
            provider_scope: self.config.provider_scope.clone(),
            subject_id: subject_id.to_owned(),
        }
    }

    /// Makes sure the person has an identity and a personal workspace, and returns the workspace.
    /// Repeating it changes nothing.
    pub async fn onboard(&self, subject: &Subject) -> Result<String, HubError> {
        let answer = self.post("/api/v1/actors/onboard", subject).await?;
        text_at(&answer, &["actor", "workspace_id"])
    }

    /// Reads the bot's status for the person. As a side effect the hub makes sure this bot has an
    /// instance in the person's workspace, which binding a chat needs.
    pub async fn bot_status(&self, subject: &Subject) -> Result<String, HubError> {
        let answer = self
            .post("/api/v1/bot-instances/personal/status", subject)
            .await?;
        text_at(&answer, &["bot_instance", "status"])
    }

    /// Binds a chat to this bot's alert channel in the person's workspace. Repeating it changes
    /// nothing.
    pub async fn bind_surface(
        &self,
        workspace_id: &str,
        chat_id: i64,
        subject: &Subject,
    ) -> Result<(), HubError> {
        let body = json!({
            "workspace_id": workspace_id,
            "logical_channel": self.config.alert_channel,
            "chat_id": chat_id,
            "provider": subject.provider,
            "provider_scope": subject.provider_scope,
            "subject_id": subject.subject_id,
        });
        self.post("/api/v1/telegram/surfaces/bind", &body)
            .await
            .map(|_| ())
    }

    /// The person's subscription. `None` when they have none, or the hub has never heard of them.
    pub async fn subscription(
        &self,
        subject: &Subject,
    ) -> Result<Option<SubscriptionState>, HubError> {
        match self
            .post("/api/v1/alert-subscriptions/personal/status", subject)
            .await
        {
            Ok(answer) => state_of(&answer),
            Err(error) if error.code() == Some("hub.actor.not_authorized") => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Sets the person's subscription, replacing any earlier one.
    pub async fn save_subscription(
        &self,
        subject: &Subject,
        state: &SubscriptionState,
    ) -> Result<(), HubError> {
        let body = json!({
            "provider": subject.provider,
            "provider_scope": subject.provider_scope,
            "subject_id": subject.subject_id,
            "cell": state.cell,
            "categories": state.categories,
            "include_nearby": state.include_nearby,
        });
        self.post("/api/v1/alert-subscriptions/personal/save", &body)
            .await
            .map(|_| ())
    }

    /// Deletes the person's subscription. Succeeds when there is none.
    pub async fn clear_subscription(&self, subject: &Subject) -> Result<(), HubError> {
        match self
            .post("/api/v1/alert-subscriptions/personal/clear", subject)
            .await
        {
            Ok(_) => Ok(()),
            Err(error) if error.code() == Some("hub.actor.not_authorized") => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn post<B: Serialize>(&self, path: &str, body: &B) -> Result<Value, HubError> {
        let response = self
            .http
            .post(format!("{}{path}", self.config.origin))
            .bearer_auth(&self.config.token)
            .json(body)
            .send()
            .await
            // The error is printed without its URL and never with the request, so neither the
            // credential nor a subject can end up in a log line.
            .map_err(|e| HubError::Unavailable(e.without_url().to_string()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| HubError::Unavailable(e.without_url().to_string()))?;
        if status.is_success() {
            return serde_json::from_str(&text)
                .map_err(|_| HubError::Unexpected("the answer is not JSON".to_owned()));
        }
        if status.as_u16() == 401 {
            return Err(HubError::Unauthorized);
        }
        let code = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["error"]["code"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".to_owned());
        Err(HubError::Rejected {
            status: status.as_u16(),
            code,
        })
    }
}

fn text_at(value: &Value, path: &[&str]) -> Result<String, HubError> {
    path.iter()
        .try_fold(value, |node, key| node.get(key))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| HubError::Unexpected(format!("`{}` is missing", path.join("."))))
}

fn state_of(answer: &Value) -> Result<Option<SubscriptionState>, HubError> {
    match answer.get("alert_subscription") {
        Some(Value::Null) => Ok(None),
        Some(state) => serde_json::from_value(state.clone())
            .map(Some)
            .map_err(|_| HubError::Unexpected("the subscription is malformed".to_owned())),
        None => Err(HubError::Unexpected(
            "`alert_subscription` is missing".to_owned(),
        )),
    }
}
