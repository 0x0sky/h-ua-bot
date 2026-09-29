// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! SQLite implementation of the core's [`Store`].
//!
//! Kept: a recipient address, a coarse cell, the categories chosen, and the alerts sent. Never
//! kept: coordinates, names, or message text.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use h_ua_core::category::Category;
use h_ua_core::geo::Cell;
use h_ua_core::ports::{Delivery, Store, StoreError};
use h_ua_core::subscriber::{Recipient, Subscription};
use rusqlite::{Connection, OptionalExtension, params};

/// Version of the schema below, kept in `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE subscriptions (
    recipient      TEXT PRIMARY KEY,
    cell           TEXT,
    categories     TEXT NOT NULL,
    include_nearby INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);
CREATE TABLE cursors (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE deliveries (
    recipient   TEXT NOT NULL,
    external_id TEXT NOT NULL,
    category    TEXT NOT NULL,
    place_id    TEXT NOT NULL,
    at          INTEGER NOT NULL,
    PRIMARY KEY (recipient, external_id, category, place_id)
);
CREATE INDEX deliveries_by_time ON deliveries (at);
";

/// A [`Store`] backed by a SQLite database.
pub struct SqliteStore(Mutex<Connection>);

fn failure(error: impl std::fmt::Display) -> StoreError {
    StoreError(error.to_string())
}

impl SqliteStore {
    /// Opens or creates the database at `path`. On Unix a new file is readable by its owner
    /// only.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(failure)?;
        }
        let existed = path.exists();
        let connection = Connection::open(path).map_err(failure)?;
        #[cfg(unix)]
        if !existed {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(failure)?;
        }
        #[cfg(not(unix))]
        let _ = existed;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(failure)?;
        Self::migrate(connection)
    }

    /// An empty database in memory, for tests.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::migrate(Connection::open_in_memory().map_err(failure)?)
    }

    fn migrate(connection: Connection) -> Result<Self, StoreError> {
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(failure)?;
        match version {
            0 => {
                connection.execute_batch(SCHEMA).map_err(failure)?;
                connection
                    .pragma_update(None, "user_version", SCHEMA_VERSION)
                    .map_err(failure)?;
            }
            SCHEMA_VERSION => {}
            other => return Err(failure(format!("unsupported schema version {other}"))),
        }
        Ok(Self(Mutex::new(connection)))
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.0.lock().map_err(|_| failure("database lock poisoned"))
    }
}

fn subscription_from(
    recipient: &str,
    cell: Option<String>,
    categories: &str,
    nearby: i64,
) -> Result<Subscription, StoreError> {
    let recipient = recipient
        .parse::<Recipient>()
        .map_err(|()| failure("bad recipient"))?;
    let cell = cell
        .map(|text| text.parse::<Cell>())
        .transpose()
        .map_err(|_| failure("bad cell"))?;
    let categories: BTreeSet<Category> = categories
        .split(',')
        .filter(|key| !key.is_empty())
        .map(|key| Category::from_key(key).ok_or_else(|| failure(format!("bad category {key}"))))
        .collect::<Result<_, _>>()?;
    Ok(Subscription {
        recipient,
        cell,
        categories,
        include_nearby: nearby != 0,
    })
}

fn category_keys(categories: &BTreeSet<Category>) -> String {
    categories
        .iter()
        .map(|c| c.key())
        .collect::<Vec<_>>()
        .join(",")
}

impl Store for SqliteStore {
    fn subscription(&self, recipient: &Recipient) -> Result<Option<Subscription>, StoreError> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT cell, categories, include_nearby FROM subscriptions WHERE recipient = ?1",
                params![recipient.to_string()],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(failure)?;
        row.map(|(cell, categories, nearby)| {
            subscription_from(&recipient.to_string(), cell, &categories, nearby)
        })
        .transpose()
    }

    fn save_subscription(&self, subscription: &Subscription, now: i64) -> Result<(), StoreError> {
        self.connection()?
            .execute(
                "INSERT INTO subscriptions (recipient, cell, categories, include_nearby, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(recipient) DO UPDATE SET
                     cell = excluded.cell,
                     categories = excluded.categories,
                     include_nearby = excluded.include_nearby,
                     updated_at = excluded.updated_at",
                params![
                    subscription.recipient.to_string(),
                    subscription.cell.map(|cell| cell.to_string()),
                    category_keys(&subscription.categories),
                    i64::from(subscription.include_nearby),
                    now,
                ],
            )
            .map_err(failure)?;
        Ok(())
    }

    fn delete_recipient(&self, recipient: &Recipient) -> Result<(), StoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(failure)?;
        let key = recipient.to_string();
        transaction
            .execute(
                "DELETE FROM subscriptions WHERE recipient = ?1",
                params![key],
            )
            .map_err(failure)?;
        transaction
            .execute("DELETE FROM deliveries WHERE recipient = ?1", params![key])
            .map_err(failure)?;
        transaction.commit().map_err(failure)
    }

    fn located_subscriptions(&self) -> Result<Vec<Subscription>, StoreError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT recipient, cell, categories, include_nearby FROM subscriptions
                 WHERE cell IS NOT NULL ORDER BY recipient",
            )
            .map_err(failure)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(failure)?;
        rows.map(|row| {
            let (recipient, cell, categories, nearby) = row.map_err(failure)?;
            subscription_from(&recipient, cell, &categories, nearby)
        })
        .collect()
    }

    fn cursor(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.connection()?
            .query_row(
                "SELECT value FROM cursors WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(failure)
    }

    fn set_cursor(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.connection()?
            .execute(
                "INSERT INTO cursors (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(failure)?;
        Ok(())
    }

    fn was_delivered(
        &self,
        recipient: &Recipient,
        external_id: &str,
        category: Category,
        place_id: &str,
    ) -> Result<bool, StoreError> {
        self.connection()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM deliveries
                 WHERE recipient = ?1 AND external_id = ?2 AND category = ?3 AND place_id = ?4)",
                params![recipient.to_string(), external_id, category.key(), place_id],
                |row| row.get(0),
            )
            .map_err(failure)
    }

    fn delivered_since(
        &self,
        recipient: &Recipient,
        category: Category,
        place_id: &str,
        since: i64,
    ) -> Result<bool, StoreError> {
        self.connection()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM deliveries
                 WHERE recipient = ?1 AND category = ?2 AND place_id = ?3 AND at >= ?4)",
                params![recipient.to_string(), category.key(), place_id, since],
                |row| row.get(0),
            )
            .map_err(failure)
    }

    fn record_delivery(&self, delivery: &Delivery) -> Result<(), StoreError> {
        self.connection()?
            .execute(
                "INSERT OR IGNORE INTO deliveries (recipient, external_id, category, place_id, at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    delivery.recipient.to_string(),
                    delivery.external_id,
                    delivery.category.key(),
                    delivery.place_id,
                    delivery.at
                ],
            )
            .map_err(failure)?;
        Ok(())
    }

    fn take_deliveries(
        &self,
        since: i64,
        category: Option<Category>,
        place_ids: Option<&[String]>,
    ) -> Result<Vec<Delivery>, StoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(failure)?;
        let mut taken = Vec::new();
        {
            let mut statement = transaction
                .prepare("SELECT recipient, external_id, category, place_id, at FROM deliveries WHERE at >= ?1")
                .map_err(failure)?;
            let rows = statement
                .query_map(params![since], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .map_err(failure)?;
            for row in rows {
                let (recipient, external_id, category_key, place_id, at) = row.map_err(failure)?;
                let row_category = Category::from_key(&category_key)
                    .ok_or_else(|| failure(format!("bad category {category_key}")))?;
                let wanted = category.is_none_or(|c| c == row_category)
                    && place_ids.is_none_or(|ids| ids.contains(&place_id));
                if wanted {
                    taken.push(Delivery {
                        recipient: recipient.parse().map_err(|()| failure("bad recipient"))?,
                        external_id,
                        category: row_category,
                        place_id,
                        at,
                    });
                }
            }
        }
        for delivery in &taken {
            transaction
                .execute(
                    "DELETE FROM deliveries
                     WHERE recipient = ?1 AND external_id = ?2 AND category = ?3 AND place_id = ?4",
                    params![
                        delivery.recipient.to_string(),
                        delivery.external_id,
                        delivery.category.key(),
                        delivery.place_id
                    ],
                )
                .map_err(failure)?;
        }
        transaction.commit().map_err(failure)?;
        Ok(taken)
    }

    fn purge_deliveries_before(&self, before: i64) -> Result<usize, StoreError> {
        self.connection()?
            .execute("DELETE FROM deliveries WHERE at < ?1", params![before])
            .map_err(failure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sqlite_store_meets_the_store_contract() {
        h_ua_core::conformance::check_store(&SqliteStore::open_in_memory().unwrap());
    }

    #[test]
    fn a_file_database_survives_a_restart_and_is_private() {
        let dir = std::env::temp_dir().join(format!("h-ua-store-{}", std::process::id()));
        let path = dir.join("nested").join("h-ua.sqlite");
        let me = Recipient::new("telegram", "7");
        {
            let store = SqliteStore::open(&path).unwrap();
            let mut sub = Subscription::new(me.clone());
            sub.cell = Some(Cell::around(50.45, 30.52).unwrap());
            store.save_subscription(&sub, 1).unwrap();
            store.set_cursor("telegram.channel:x", "43231").unwrap();
        }
        let store = SqliteStore::open(&path).unwrap();
        assert!(store.subscription(&me).unwrap().unwrap().cell.is_some());
        assert_eq!(
            store.cursor("telegram.channel:x").unwrap(),
            Some("43231".to_owned())
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_database_from_a_newer_version_is_refused() {
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        let error = SqliteStore::migrate(connection).err().unwrap();
        assert!(error.0.contains("unsupported schema version 99"));
    }

    #[test]
    fn the_stored_subscription_holds_no_coordinates() {
        let store = SqliteStore::open_in_memory().unwrap();
        let mut sub = Subscription::new(Recipient::new("telegram", "9"));
        sub.cell = Some(Cell::around(50.4501, 30.5234).unwrap());
        store.save_subscription(&sub, 1).unwrap();
        let connection = store.connection().unwrap();
        let columns: Vec<String> = connection
            .prepare("SELECT name FROM pragma_table_info('subscriptions')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            !columns
                .iter()
                .any(|c| c.contains("lat") || c.contains("lon") || c.contains("coord"))
        );
    }
}
