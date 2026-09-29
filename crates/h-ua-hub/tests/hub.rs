// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The hub client against a small stand-in for the hub that follows its contract.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use h_ua_core::category::Category;
use h_ua_core::geo::Cell;
use h_ua_core::ports::Subscriptions;
use h_ua_core::subscriber::{Recipient, Subscription};
use h_ua_hub::{HubClient, HubConfig, HubSubscriptions};
use serde_json::{Value, json};

const TOKEN: &str = "hub-credential-value";

#[derive(Default)]
struct FakeHub {
    calls: Vec<(String, Value)>,
    tokens: Vec<String>,
    people: BTreeMap<String, String>,
    subscriptions: BTreeMap<String, Value>,
    fail: Option<(u16, &'static str)>,
}

type Shared = Arc<Mutex<FakeHub>>;

async fn handle(
    State(hub): State<Shared>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let mut hub = hub.lock().unwrap();
    let path = uri.path().to_owned();
    hub.calls.push((path.clone(), body.clone()));
    hub.tokens.push(
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned(),
    );
    if let Some((status, code)) = hub.fail {
        return (
            StatusCode::from_u16(status).unwrap(),
            Json(json!({"status": "error", "error": {"code": code, "message": "secret detail"}})),
        );
    }
    let subject = body["subject_id"].as_str().unwrap_or_default().to_owned();
    let known = hub.people.contains_key(&subject);
    let denied = || {
        (
            StatusCode::FORBIDDEN,
            Json(json!({"status": "error", "error": {"code": "hub.actor.not_authorized"}})),
        )
    };
    match path.as_str() {
        "/api/v1/actors/onboard" => {
            let workspace = format!("personal-{subject}");
            hub.people.insert(subject, workspace.clone());
            (
                StatusCode::OK,
                Json(
                    json!({"actor": {"identity": {}, "workspace_id": workspace, "role": "owner"}}),
                ),
            )
        }
        "/api/v1/bot-instances/personal/status" if known => (
            StatusCode::OK,
            Json(json!({"bot_instance": {"status": "active"}})),
        ),
        "/api/v1/telegram/surfaces/bind" if known => (
            StatusCode::OK,
            Json(json!({"binding": {"status": "active"}})),
        ),
        "/api/v1/alert-subscriptions/personal/save" if known => {
            let state = json!({
                "cell": body["cell"], "categories": body["categories"], "include_nearby": body["include_nearby"]
            });
            hub.subscriptions.insert(subject, state.clone());
            (StatusCode::OK, Json(json!({"alert_subscription": state})))
        }
        "/api/v1/alert-subscriptions/personal/status" if known => {
            let state = hub
                .subscriptions
                .get(&subject)
                .cloned()
                .unwrap_or(Value::Null);
            (StatusCode::OK, Json(json!({"alert_subscription": state})))
        }
        "/api/v1/alert-subscriptions/personal/clear" if known => {
            hub.subscriptions.remove(&subject);
            (StatusCode::OK, Json(json!({"alert_subscription": null})))
        }
        _ => denied(),
    }
}

async fn start() -> (Shared, HubSubscriptions) {
    let hub: Shared = Arc::default();
    let app = Router::new().fallback(post(handle)).with_state(hub.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = HubClient::new(HubConfig {
        origin: format!("http://{address}"),
        token: TOKEN.to_owned(),
        provider: "telegram".to_owned(),
        provider_scope: "h-ua-bot".to_owned(),
        alert_channel: "alerts".to_owned(),
    })
    .unwrap();
    (hub, HubSubscriptions::new(client))
}

fn me() -> Recipient {
    Recipient::new("telegram", "42")
}

fn subscription() -> Subscription {
    Subscription::new(me(), Cell::around(50.45, 30.52).unwrap())
}

fn paths(hub: &Shared) -> Vec<String> {
    hub.lock()
        .unwrap()
        .calls
        .iter()
        .map(|(path, _)| path.trim_start_matches("/api/v1/").to_owned())
        .collect()
}

#[tokio::test]
async fn the_first_save_makes_the_person_known_binds_the_chat_and_saves() {
    let (hub, subscriptions) = start().await;

    subscriptions.save(&subscription()).await.unwrap();

    assert_eq!(
        paths(&hub),
        [
            "actors/onboard",
            "bot-instances/personal/status",
            "telegram/surfaces/bind",
            "alert-subscriptions/personal/save"
        ]
    );
    let hub = hub.lock().unwrap();
    let subject = json!({"provider": "telegram", "provider_scope": "h-ua-bot", "subject_id": "42"});
    assert_eq!(hub.calls[0].1, subject);
    assert_eq!(hub.calls[1].1, subject);
    assert_eq!(
        hub.calls[2].1,
        json!({
            "workspace_id": "personal-42", "logical_channel": "alerts", "chat_id": 42,
            "provider": "telegram", "provider_scope": "h-ua-bot", "subject_id": "42"
        })
    );
    assert!(hub.tokens.iter().all(|t| t == &format!("Bearer {TOKEN}")));
}

#[tokio::test]
async fn later_saves_are_one_call() {
    let (hub, subscriptions) = start().await;
    subscriptions.save(&subscription()).await.unwrap();
    let mut moved = subscription();
    moved.include_nearby = false;

    subscriptions.save(&moved).await.unwrap();

    assert_eq!(paths(&hub).len(), 5);
    assert_eq!(paths(&hub)[4], "alert-subscriptions/personal/save");
}

#[tokio::test]
async fn only_a_cell_ever_leaves_the_bot() {
    let (hub, subscriptions) = start().await;
    subscriptions.save(&subscription()).await.unwrap();

    let hub = hub.lock().unwrap();
    let saved = &hub.calls.last().unwrap().1;
    let cell = saved["cell"].as_str().unwrap();
    assert!(cell.starts_with("86") && cell.len() == 15, "{cell}");
    assert_eq!(saved["categories"], json!(["drone", "bomb", "missile"]));
    let everything = serde_json::to_string(&hub.calls).unwrap();
    for name in ["lat", "lon", "latitude", "longitude", "coordinates"] {
        assert!(
            !everything.contains(&format!("\"{name}\"")),
            "{name} was sent"
        );
    }
    assert!(!everything.contains("50.45") && !everything.contains("30.52"));
}

#[tokio::test]
async fn what_was_saved_reads_back_the_same() {
    let (_hub, subscriptions) = start().await;
    let mut wanted = subscription();
    wanted.categories = [Category::Bomb, Category::Missile].into();
    wanted.include_nearby = false;
    subscriptions.save(&wanted).await.unwrap();

    assert_eq!(subscriptions.get(&me()).await.unwrap(), Some(wanted));
}

#[tokio::test]
async fn a_stranger_has_no_subscription_instead_of_an_error() {
    let (_hub, subscriptions) = start().await;

    assert_eq!(subscriptions.get(&me()).await.unwrap(), None);
    subscriptions.delete(&me()).await.unwrap();
}

#[tokio::test]
async fn deleting_clears_the_subscription_at_the_hub() {
    let (hub, subscriptions) = start().await;
    subscriptions.save(&subscription()).await.unwrap();

    subscriptions.delete(&me()).await.unwrap();

    assert_eq!(subscriptions.get(&me()).await.unwrap(), None);
    assert!(hub.lock().unwrap().subscriptions.is_empty());
    // The person is prepared again if they come back.
    subscriptions.save(&subscription()).await.unwrap();
    assert!(
        paths(&hub)
            .iter()
            .filter(|p| *p == "actors/onboard")
            .count()
            >= 2
    );
}

#[tokio::test]
async fn hub_failures_are_reported_without_the_hubs_text_or_the_credential() {
    let (hub, subscriptions) = start().await;
    for (status, code, expected) in [
        (401, "x", "refused the credential"),
        (
            409,
            "hub.telegram_surface_binding.conflict",
            "hub.telegram_surface_binding.conflict",
        ),
        (500, "hub.internal", "hub.internal"),
    ] {
        hub.lock().unwrap().fail = Some((status, code));
        let error = subscriptions
            .save(&subscription())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
        assert!(
            !error.contains("secret detail") && !error.contains(TOKEN),
            "{error}"
        );
    }
}

#[tokio::test]
async fn an_unreachable_hub_is_an_error_not_a_lost_save() {
    let client = HubClient::new(HubConfig {
        origin: "http://127.0.0.1:1".to_owned(),
        token: TOKEN.to_owned(),
        provider: "telegram".to_owned(),
        provider_scope: "h-ua-bot".to_owned(),
        alert_channel: "alerts".to_owned(),
    })
    .unwrap();
    let subscriptions = HubSubscriptions::new(client);

    let error = subscriptions
        .save(&subscription())
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("unreachable"), "{error}");
    assert!(
        !error.contains(TOKEN) && !error.contains("127.0.0.1"),
        "{error}"
    );
}

#[tokio::test]
async fn only_telegram_chats_are_served() {
    let (hub, subscriptions) = start().await;

    assert!(
        subscriptions
            .get(&Recipient::new("signal", "1"))
            .await
            .is_err()
    );
    assert!(
        subscriptions
            .get(&Recipient::new("telegram", "not-a-number"))
            .await
            .is_err()
    );
    assert!(paths(&hub).is_empty());
}

#[test]
fn the_credential_is_not_in_the_debug_output() {
    let config = HubConfig {
        origin: "https://hub.example".to_owned(),
        token: TOKEN.to_owned(),
        provider: "telegram".to_owned(),
        provider_scope: "h-ua-bot".to_owned(),
        alert_channel: "alerts".to_owned(),
    };
    let shown = format!("{config:?}");
    assert!(!shown.contains(TOKEN) && shown.contains("<redacted>"));
}
