// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Who receives alerts, and what they asked for.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use crate::category::Category;
use crate::geo::Cell;

/// A person on some messenger: which client reaches them and their address there.
///
/// The address is whatever the messenger needs to send to them, such as a Telegram chat id.
/// It is the only identity the bot keeps.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Recipient {
    /// Client name, such as `telegram`.
    pub client: String,
    /// Address inside that client.
    pub address: String,
}

impl Recipient {
    /// Builds a recipient.
    pub fn new(client: impl Into<String>, address: impl Into<String>) -> Self {
        Self {
            client: client.into(),
            address: address.into(),
        }
    }
}

impl fmt::Display for Recipient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.client, self.address)
    }
}

impl FromStr for Recipient {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.split_once(':') {
            Some((client, address)) if !client.is_empty() && !address.is_empty() => {
                Ok(Self::new(client, address))
            }
            _ => Err(()),
        }
    }
}

/// What one person asked to be told about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subscription {
    /// Who.
    pub recipient: Recipient,
    /// Where they are. Without a position nothing is relevant to them, so nothing is sent.
    pub cell: Option<Cell>,
    /// Which kinds of threat.
    pub categories: BTreeSet<Category>,
    /// Also warn when a threat is reported near, not at, their position.
    pub include_nearby: bool,
}

impl Subscription {
    /// A new subscription: every category, nearby warnings on, and no position yet.
    pub fn new(recipient: Recipient) -> Self {
        Self {
            recipient,
            cell: None,
            categories: Category::ALL.into_iter().collect(),
            include_nearby: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recipient_round_trips_through_text() {
        let recipient = Recipient::new("telegram", "12345");
        assert_eq!(recipient.to_string(), "telegram:12345");
        assert_eq!("telegram:12345".parse(), Ok(recipient));
        assert!("telegram".parse::<Recipient>().is_err());
        assert!(":1".parse::<Recipient>().is_err());
    }

    #[test]
    fn a_new_subscription_wants_everything_and_knows_no_position() {
        let subscription = Subscription::new(Recipient::new("telegram", "1"));
        assert_eq!(subscription.categories.len(), Category::ALL.len());
        assert!(subscription.include_nearby);
        assert!(subscription.cell.is_none());
    }
}
