// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The running bot: the Telegram conversation and the hub's delivery endpoint side by side.

use std::sync::Arc;
use std::time::Duration;

use h_ua_core::conversation::Conversation;
use h_ua_core::message::Message;
use h_ua_core::ports::{EventKind, Messenger, Subscriptions};
use h_ua_hub::{HubClient, HubConfig, HubSubscriptions};
use h_ua_telegram::{CLIENT_NAME, ReqwestTransport, TelegramClient};
use prism_signal_normalize::Gazetteer;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::delivery::{self, DeliveryState};

/// Said when the hub could not be reached, so nobody believes a choice was saved when it was not.
const NOT_SAVED: &str = "Не вдалося звʼязатися зі службою підписок. Нічого не збережено — спробуйте ще раз трохи згодом.";

fn required(value: Option<String>, name: &str) -> Result<String, String> {
    value.ok_or_else(|| format!("{name} is not set"))
}

/// Runs the bot until interrupted.
pub async fn run(config: Config) -> Result<(), String> {
    let token = required(config.telegram_token.clone(), "HUA_TELEGRAM_TOKEN")?;
    let hub = HubClient::new(HubConfig {
        origin: required(config.hub_origin.clone(), "HUA_HUB_ORIGIN")?,
        token: required(config.hub_token.clone(), "HUA_HUB_TOKEN")?,
        provider: CLIENT_NAME.to_owned(),
        provider_scope: config.hub_provider_scope.clone(),
        alert_channel: config.alert_channel.clone(),
    })
    .map_err(|e| e.to_string())?;
    let secret = required(config.delivery_secret.clone(), "HUA_DELIVERY_SECRET")?;

    let transport = ReqwestTransport::new(token, config.telegram_api_base.as_deref())
        .map_err(|e| e.to_string())?;
    let telegram = Arc::new(TelegramClient::new(transport));
    let username = telegram
        .username()
        .await
        .map_err(|e| format!("Telegram refused the token: {e}"))?;
    info!(%username, "connected to Telegram");

    let gazetteer = Arc::new(Gazetteer::embedded().map_err(|e| e.to_string())?);
    let subscriptions: Arc<dyn Subscriptions> = Arc::new(HubSubscriptions::new(hub));
    let messenger: Arc<dyn Messenger> = telegram.clone();

    let listener = tokio::net::TcpListener::bind(config.delivery_listen)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", config.delivery_listen))?;
    info!(address = %config.delivery_listen, "hub deliveries are accepted");
    let app = delivery::router(DeliveryState::new(secret, telegram));

    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(client_loop(
        messenger,
        subscriptions,
        gazetteer,
        config.sources.clone(),
    ));
    tasks.spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            error!(%error, "the delivery endpoint stopped");
        }
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("interrupted, stopping"),
        finished = tasks.join_next() => error!(?finished, "a loop ended, which it should never do"),
    }
    Ok(())
}

async fn client_loop(
    messenger: Arc<dyn Messenger>,
    subscriptions: Arc<dyn Subscriptions>,
    gazetteer: Arc<Gazetteer>,
    labels: Vec<String>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        match messenger.poll().await {
            Ok(events) => {
                backoff = Duration::from_secs(1);
                for event in events {
                    let live = matches!(event.kind, EventKind::Location { live: true, .. });
                    let replies = Conversation::new(&*subscriptions, &gazetteer, labels.clone())
                        .handle(&event)
                        .await;
                    let replies = match replies {
                        Ok(replies) => replies,
                        Err(error) => {
                            warn!(%error, "cannot handle an event");
                            // A live position updates in silence, and a failed update is
                            // retried by the next one. Anything else a person is waiting on.
                            if live {
                                Vec::new()
                            } else {
                                vec![Message::text(NOT_SAVED)]
                            }
                        }
                    };
                    for reply in replies {
                        if let Err(error) = messenger.send(&event.from, &reply).await {
                            warn!(%error, to = %event.from, "reply not sent");
                        }
                    }
                }
            }
            Err(error) => {
                warn!(%error, "cannot read from the messenger");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
}
