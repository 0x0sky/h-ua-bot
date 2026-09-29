// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The kinds of threat a person can subscribe to.
//!
//! The reader tells five kinds apart; a person chooses among three. `КАБ` and drones warn of
//! different things, while a cruise, a ballistic, and an unspecified missile all mean the same
//! to someone deciding whether to go to shelter.

use prism_signal_normalize::HazardKind;

/// What a person subscribes to.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Category {
    /// Attack drones.
    Drone,
    /// Guided aerial bombs.
    Bomb,
    /// Any missile.
    Missile,
}

impl Category {
    /// Every category, in display order.
    pub const ALL: [Category; 3] = [Category::Drone, Category::Bomb, Category::Missile];

    /// The category a hazard kind belongs to.
    pub fn of(kind: HazardKind) -> Self {
        match kind {
            HazardKind::Drone => Self::Drone,
            HazardKind::GuidedBomb => Self::Bomb,
            HazardKind::CruiseMissile | HazardKind::BallisticMissile | HazardKind::Missile => {
                Self::Missile
            }
        }
    }

    /// Stable key used in storage.
    pub fn key(self) -> &'static str {
        match self {
            Self::Drone => "drone",
            Self::Bomb => "bomb",
            Self::Missile => "missile",
        }
    }

    /// Reads a storage key.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|category| category.key() == key)
    }

    /// Reads a word a person typed, in Ukrainian or Russian, singular or plural.
    pub fn from_word(word: &str) -> Option<Self> {
        let word = word.trim().to_lowercase();
        let word = word.trim_matches(|c: char| !c.is_alphanumeric());
        match word {
            "дрон" | "дрони" | "бпла" | "шахед" | "шахеди" | "мопед" | "мопеди" | "drone" => {
                Some(Self::Drone)
            }
            "каб" | "каби" | "кабы" | "бомба" | "бомби" | "bomb" => {
                Some(Self::Bomb)
            }
            "ракета" | "ракети" | "ракеты" | "балістика" | "баллистика" | "missile" => {
                Some(Self::Missile)
            }
            _ => None,
        }
    }

    /// Name shown to a person, in the plural.
    pub fn label(self) -> &'static str {
        match self {
            Self::Drone => "дрони",
            Self::Bomb => "КАБи",
            Self::Missile => "ракети",
        }
    }
}

/// Name of a hazard kind in an alert.
pub fn kind_label(kind: HazardKind) -> &'static str {
    match kind {
        HazardKind::Drone => "БпЛА (дрон)",
        HazardKind::GuidedBomb => "КАБ",
        HazardKind::CruiseMissile => "Крилаті ракети",
        HazardKind::BallisticMissile => "Балістика",
        HazardKind::Missile => "Ракети",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_category_and_every_key_round_trips() {
        assert_eq!(Category::of(HazardKind::Drone), Category::Drone);
        assert_eq!(Category::of(HazardKind::GuidedBomb), Category::Bomb);
        for kind in [
            HazardKind::CruiseMissile,
            HazardKind::BallisticMissile,
            HazardKind::Missile,
        ] {
            assert_eq!(Category::of(kind), Category::Missile);
        }
        for category in Category::ALL {
            assert_eq!(Category::from_key(category.key()), Some(category));
        }
        assert_eq!(Category::from_key("nope"), None);
    }

    #[test]
    fn people_type_categories_in_either_language() {
        assert_eq!(Category::from_word("Дрони,"), Some(Category::Drone));
        assert_eq!(Category::from_word("шахеди"), Some(Category::Drone));
        assert_eq!(Category::from_word("КАБи"), Some(Category::Bomb));
        assert_eq!(Category::from_word("баллистика"), Some(Category::Missile));
        assert_eq!(Category::from_word("ракети"), Some(Category::Missile));
        assert_eq!(Category::from_word("сумки"), None);
    }
}
