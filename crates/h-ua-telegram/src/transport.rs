// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! How requests reach the Bot API.

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

/// A request never got an answer from the Bot API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportError(pub String);

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Calls a Bot API method and returns Telegram's JSON answer, whether it says `ok` or not.
#[async_trait]
pub trait Transport: Send + Sync {
    /// `POST /bot<token>/<method>` with a JSON body.
    async fn call(&self, method: &str, body: &Value) -> Result<Value, TransportError>;
}

/// The default Bot API address.
pub const DEFAULT_BASE_URL: &str = "https://api.telegram.org";

/// A longer wait than the long poll itself, so a healthy poll is never cut off.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

/// [`Transport`] over HTTPS.
pub struct ReqwestTransport {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

impl ReqwestTransport {
    /// A transport for a bot token. `base_url` is `None` for the public Bot API.
    pub fn new(token: impl Into<String>, base_url: Option<&str>) -> Result<Self, TransportError> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| TransportError(e.without_url().to_string()))?;
        Ok(Self {
            client,
            base_url: base_url
                .unwrap_or(DEFAULT_BASE_URL)
                .trim_end_matches('/')
                .to_owned(),
            token: token.into(),
        })
    }
}

impl fmt::Debug for ReqwestTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The token is a credential and must not reach a log.
        f.debug_struct("ReqwestTransport")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl Transport for ReqwestTransport {
    async fn call(&self, method: &str, body: &Value) -> Result<Value, TransportError> {
        let url = format!("{}/bot{}/{method}", self.base_url, self.token);
        // The URL carries the token, and reqwest prints the URL in its errors, so it is removed.
        let response = self
            .client
            .post(url)
            .json(body)
            .send()
            .await
            .map_err(|e| TransportError(e.without_url().to_string()))?;
        response
            .json::<Value>()
            .await
            .map_err(|e| TransportError(e.without_url().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_token_never_appears_in_an_error_or_in_debug_output() {
        let transport =
            ReqwestTransport::new("123:SECRET-TOKEN", Some("http://127.0.0.1:1")).unwrap();
        let error = transport.call("getMe", &Value::Null).await.unwrap_err();
        assert!(!error.to_string().contains("SECRET-TOKEN"), "{error}");
        assert!(!format!("{transport:?}").contains("SECRET-TOKEN"));
    }
}
