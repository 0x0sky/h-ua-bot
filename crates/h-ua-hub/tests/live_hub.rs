// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The client against a real `prism-hub`. Not run by default: it needs a hub and a credential.
//!
//! ```text
//! HUA_LIVE_HUB_ORIGIN=http://127.0.0.1:3000 HUA_LIVE_HUB_TOKEN=... \
//!   cargo test -p h-ua-hub --test live_hub -- --ignored
//! ```
//!
//! The credential's principal needs `actors:onboard`, `actors:resolve`, `bot_instances:read`,
//! `bot_instances:manage`, `alert_subscriptions:read` and `alert_subscriptions:manage`. The test
//! makes one person known to the hub (a random chat id), binds a chat, saves, reads, and clears.

use h_ua_core::geo::Cell;
use h_ua_core::ports::Subscriptions;
use h_ua_core::subscriber::{Recipient, Subscription};
use h_ua_hub::{HubClient, HubConfig, HubSubscriptions};

#[tokio::test]
#[ignore = "needs a running prism-hub"]
async fn the_whole_conversation_works_against_a_real_hub() {
    let origin = std::env::var("HUA_LIVE_HUB_ORIGIN").expect("HUA_LIVE_HUB_ORIGIN");
    let token = std::env::var("HUA_LIVE_HUB_TOKEN").expect("HUA_LIVE_HUB_TOKEN");
    let chat: i64 = 7_000_000_000
        + i64::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos(),
        );
    let client = HubClient::new(HubConfig {
        origin,
        token,
        provider: "telegram".to_owned(),
        provider_scope: "h-ua-bot-live-test".to_owned(),
        alert_channel: "alerts".to_owned(),
    })
    .unwrap();
    let subscriptions = HubSubscriptions::new(client);
    let me = Recipient::new("telegram", chat.to_string());

    assert_eq!(subscriptions.get(&me).await.unwrap(), None);

    let mut wanted = Subscription::new(me.clone(), Cell::around(50.45, 30.52).unwrap());
    wanted.include_nearby = false;
    subscriptions.save(&wanted).await.unwrap();
    assert_eq!(subscriptions.get(&me).await.unwrap(), Some(wanted.clone()));

    wanted.cell = Cell::around(49.84, 24.03).unwrap();
    subscriptions.save(&wanted).await.unwrap();
    assert_eq!(subscriptions.get(&me).await.unwrap(), Some(wanted));

    subscriptions.delete(&me).await.unwrap();
    assert_eq!(subscriptions.get(&me).await.unwrap(), None);
    // Clearing again, and for a person who never existed, is not an error.
    subscriptions.delete(&me).await.unwrap();
}
