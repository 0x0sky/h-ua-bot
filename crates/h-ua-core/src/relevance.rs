// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Whether a reading concerns a person.

use prism_signal_normalize::{Phase, PlaceMention, PlaceRole, Reading};

use crate::category::Category;
use crate::geo::SNAP_TOLERANCE_KM;
use crate::subscriber::Subscription;

/// A place in a reading that concerns a person, and how far it is from them.
#[derive(Clone, Debug, PartialEq)]
pub struct Affected {
    /// The place, with its role in the report.
    pub place: PlaceMention,
    /// Distance from the centre of the person's cell to the place's centre, in km.
    pub distance_km: f64,
}

/// The places of a threat reading that concern a person.
///
/// A place concerns them when all of these hold:
///
/// - the reading reports a threat of a kind they subscribed to;
/// - the place is a `target`, or a `via` place and they want nearby warnings;
/// - their cell is within the place's reach, plus a tolerance for having rounded their position
///   to a cell.
///
/// `origin` and `mention` places never concern anyone: a launch site is not a danger to the
/// people near it, and a place named in passing is not the subject of the report.
pub fn affected_places(subscription: &Subscription, reading: &Reading) -> Vec<Affected> {
    let Some(cell) = subscription.cell else {
        return Vec::new();
    };
    let Some(kind) = reading.kind else {
        return Vec::new();
    };
    if reading.phase != Phase::Threat || !subscription.categories.contains(&Category::of(kind)) {
        return Vec::new();
    }
    reading
        .places
        .iter()
        .filter(|place| match place.role {
            PlaceRole::Target => true,
            PlaceRole::Via => subscription.include_nearby,
            PlaceRole::Origin | PlaceRole::Mention => false,
        })
        .filter_map(|place| {
            let distance_km = cell.distance_km(place.lat, place.lon);
            (distance_km <= f64::from(place.reach_km) + SNAP_TOLERANCE_KM).then(|| Affected {
                place: place.clone(),
                distance_km,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::Cell;
    use crate::subscriber::Recipient;
    use prism_signal_core::{ExternalId, SourceId, Timestamp};
    use prism_signal_normalize::HazardKind;

    fn place(name: &str, lat: f64, lon: f64, reach_km: u32, role: PlaceRole) -> PlaceMention {
        PlaceMention {
            place_id: format!("geonames:{name}"),
            name: name.to_owned(),
            lat,
            lon,
            reach_km,
            role,
        }
    }

    fn reading(kind: HazardKind, phase: Phase, places: Vec<PlaceMention>) -> Reading {
        Reading {
            source_id: SourceId::new("telegram.channel", "vanek_nikolaev").unwrap(),
            external_id: ExternalId::try_from("vanek_nikolaev/1".to_owned()).unwrap(),
            published_at: Timestamp::parse("2026-09-29T12:00:00Z").unwrap(),
            url: "https://t.me/vanek_nikolaev/1".to_owned(),
            forwarded: false,
            kind: Some(kind),
            kind_inherited: false,
            phase,
            places,
            unresolved: Vec::new(),
        }
    }

    fn person(lat: f64, lon: f64) -> Subscription {
        let mut subscription = Subscription::new(Recipient::new("telegram", "1"));
        subscription.cell = Some(Cell::around(lat, lon).unwrap());
        subscription
    }

    const KYIV: (f64, f64) = (50.4547, 30.5238);

    fn kyiv(role: PlaceRole) -> PlaceMention {
        place("Київ", KYIV.0, KYIV.1, 20, role)
    }

    #[test]
    fn a_person_in_the_reach_of_a_target_is_affected() {
        let r = reading(
            HazardKind::Drone,
            Phase::Threat,
            vec![kyiv(PlaceRole::Target)],
        );
        let hits = affected_places(&person(50.40, 30.60), &r);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].distance_km < 20.0);
    }

    #[test]
    fn a_person_far_away_is_not() {
        let r = reading(
            HazardKind::Drone,
            Phase::Threat,
            vec![kyiv(PlaceRole::Target)],
        );
        // Lviv.
        assert!(affected_places(&person(49.8397, 24.0297), &r).is_empty());
        // Just outside the 20 km reach and the rounding tolerance.
        assert!(affected_places(&person(50.4547, 30.5238 + 0.5), &r).is_empty());
    }

    #[test]
    fn rounding_a_position_to_a_cell_never_hides_a_threat_inside_the_reach() {
        // People standing 0.99 x reach from the place, in every direction. Their cell centre
        // may be several km further away, and the tolerance must still count them.
        let reach_km = 12.0_f64;
        let r = reading(
            HazardKind::Drone,
            Phase::Threat,
            vec![place("Місто", KYIV.0, KYIV.1, 12, PlaceRole::Target)],
        );
        let d = reach_km * 0.99;
        for bearing in (0..360).step_by(5) {
            let b = f64::from(bearing).to_radians();
            let lat = KYIV.0 + d * b.cos() / 111.19;
            let lon = KYIV.1 + d * b.sin() / (111.32 * KYIV.0.to_radians().cos());
            assert_eq!(
                affected_places(&person(lat, lon), &r).len(),
                1,
                "bearing {bearing}"
            );
        }
    }

    #[test]
    fn nearby_places_count_only_when_asked_for() {
        let r = reading(HazardKind::Drone, Phase::Threat, vec![kyiv(PlaceRole::Via)]);
        let mut subscription = person(50.40, 30.60);
        assert_eq!(affected_places(&subscription, &r).len(), 1);
        subscription.include_nearby = false;
        assert!(affected_places(&subscription, &r).is_empty());
    }

    #[test]
    fn an_origin_or_a_passing_mention_never_concerns_anyone() {
        for role in [PlaceRole::Origin, PlaceRole::Mention] {
            let r = reading(HazardKind::Drone, Phase::Threat, vec![kyiv(role)]);
            assert!(
                affected_places(&person(50.45, 30.52), &r).is_empty(),
                "{role:?}"
            );
        }
    }

    #[test]
    fn only_subscribed_categories_count() {
        let r = reading(
            HazardKind::GuidedBomb,
            Phase::Threat,
            vec![kyiv(PlaceRole::Target)],
        );
        let mut subscription = person(50.45, 30.52);
        assert_eq!(affected_places(&subscription, &r).len(), 1);
        subscription.categories.remove(&Category::Bomb);
        assert!(affected_places(&subscription, &r).is_empty());

        let missile = reading(
            HazardKind::BallisticMissile,
            Phase::Threat,
            vec![kyiv(PlaceRole::Target)],
        );
        let mut subscription = person(50.45, 30.52);
        subscription.categories = [Category::Missile].into();
        assert_eq!(affected_places(&subscription, &missile).len(), 1);
    }

    #[test]
    fn a_person_without_a_position_and_an_all_clear_are_never_affected() {
        let r = reading(
            HazardKind::Drone,
            Phase::Threat,
            vec![kyiv(PlaceRole::Target)],
        );
        let nowhere = Subscription::new(Recipient::new("telegram", "2"));
        assert!(affected_places(&nowhere, &r).is_empty());

        let cleared = reading(
            HazardKind::Drone,
            Phase::Cleared,
            vec![kyiv(PlaceRole::Target)],
        );
        assert!(affected_places(&person(50.45, 30.52), &cleared).is_empty());
    }
}
