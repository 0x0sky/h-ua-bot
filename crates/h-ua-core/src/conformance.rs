// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The behaviour every [`Store`] must have, written once and run against each implementation.

use crate::category::Category;
use crate::geo::Cell;
use crate::ports::{Delivery, Store};
use crate::subscriber::{Recipient, Subscription};

fn delivery(who: &Recipient, post: &str, category: Category, place: &str, at: i64) -> Delivery {
    Delivery {
        recipient: who.clone(),
        external_id: post.to_owned(),
        category,
        place_id: place.to_owned(),
        at,
    }
}

/// Runs the contract against an empty store. Panics, with a message, on the first violation.
pub fn check_store(store: &dyn Store) {
    let ann = Recipient::new("telegram", "1");
    let bob = Recipient::new("telegram", "2");

    // Subscriptions round-trip, are replaced rather than duplicated, and only located ones list.
    assert_eq!(store.subscription(&ann).unwrap(), None);
    let mut sub = Subscription::new(ann.clone());
    store.save_subscription(&sub, 10).unwrap();
    assert_eq!(store.subscription(&ann).unwrap(), Some(sub.clone()));
    assert!(
        store.located_subscriptions().unwrap().is_empty(),
        "no position, not located"
    );
    sub.cell = Some(Cell::around(50.45, 30.52).unwrap());
    sub.categories = [Category::Drone, Category::Missile].into();
    sub.include_nearby = false;
    store.save_subscription(&sub, 20).unwrap();
    assert_eq!(store.subscription(&ann).unwrap(), Some(sub.clone()));
    assert_eq!(store.located_subscriptions().unwrap(), vec![sub.clone()]);
    let mut none = Subscription::new(bob.clone());
    none.categories.clear();
    store.save_subscription(&none, 20).unwrap();
    assert_eq!(store.subscription(&bob).unwrap(), Some(none));

    // Cursors.
    assert_eq!(store.cursor("k").unwrap(), None);
    store.set_cursor("k", "1").unwrap();
    store.set_cursor("k", "2").unwrap();
    assert_eq!(store.cursor("k").unwrap(), Some("2".to_owned()));

    // Deliveries: exact lookups, cool-down lookups, and idempotent recording.
    store
        .record_delivery(&delivery(&ann, "p/1", Category::Drone, "geo:1", 100))
        .unwrap();
    store
        .record_delivery(&delivery(&ann, "p/1", Category::Drone, "geo:1", 100))
        .unwrap();
    assert!(
        store
            .was_delivered(&ann, "p/1", Category::Drone, "geo:1")
            .unwrap()
    );
    assert!(
        !store
            .was_delivered(&ann, "p/2", Category::Drone, "geo:1")
            .unwrap()
    );
    assert!(
        !store
            .was_delivered(&bob, "p/1", Category::Drone, "geo:1")
            .unwrap()
    );
    assert!(
        store
            .delivered_since(&ann, Category::Drone, "geo:1", 100)
            .unwrap()
    );
    assert!(
        !store
            .delivered_since(&ann, Category::Drone, "geo:1", 101)
            .unwrap()
    );
    assert!(
        !store
            .delivered_since(&ann, Category::Bomb, "geo:1", 0)
            .unwrap()
    );
    assert!(
        !store
            .delivered_since(&ann, Category::Drone, "geo:2", 0)
            .unwrap()
    );

    // Taking deliveries filters by time, category, and place, and removes what it returns.
    store
        .record_delivery(&delivery(&ann, "p/2", Category::Bomb, "geo:2", 200))
        .unwrap();
    store
        .record_delivery(&delivery(&bob, "p/3", Category::Drone, "geo:3", 300))
        .unwrap();
    let places = ["geo:1".to_owned(), "geo:3".to_owned()];
    let taken = store
        .take_deliveries(0, Some(Category::Drone), Some(&places))
        .unwrap();
    assert_eq!(taken.len(), 2);
    assert!(taken.iter().all(|d| d.category == Category::Drone));
    assert!(
        !store
            .was_delivered(&ann, "p/1", Category::Drone, "geo:1")
            .unwrap(),
        "taken means gone"
    );
    assert!(
        store
            .was_delivered(&ann, "p/2", Category::Bomb, "geo:2")
            .unwrap(),
        "others stay"
    );
    assert!(
        store.take_deliveries(250, None, None).unwrap().is_empty(),
        "too old to take"
    );
    assert_eq!(store.take_deliveries(0, None, None).unwrap().len(), 1);

    // Purging and deleting a person.
    store
        .record_delivery(&delivery(&ann, "p/4", Category::Drone, "geo:1", 50))
        .unwrap();
    store
        .record_delivery(&delivery(&ann, "p/5", Category::Drone, "geo:1", 500))
        .unwrap();
    assert_eq!(store.purge_deliveries_before(100).unwrap(), 1);
    store
        .record_delivery(&delivery(&bob, "p/6", Category::Drone, "geo:1", 500))
        .unwrap();
    store.delete_recipient(&ann).unwrap();
    assert_eq!(store.subscription(&ann).unwrap(), None);
    assert!(
        !store
            .was_delivered(&ann, "p/5", Category::Drone, "geo:1")
            .unwrap()
    );
    assert!(
        store
            .was_delivered(&bob, "p/6", Category::Drone, "geo:1")
            .unwrap(),
        "others untouched"
    );
    assert!(store.subscription(&bob).unwrap().is_some());
    assert_eq!(
        store.cursor("k").unwrap(),
        Some("2".to_owned()),
        "cursors are not personal"
    );
}
