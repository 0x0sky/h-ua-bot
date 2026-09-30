// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Coarse positions.
//!
//! A person's exact coordinates are used once, to find the grid cell they stand in, and are
//! never kept. Everything after that works on the cell.

use std::fmt;
use std::str::FromStr;

use h3o::{CellIndex, LatLng, Resolution};
use thiserror::Error;

/// Grid resolution of a stored position: H3 resolution 6, about 3.2 km edge.
///
/// A place's reach is 5 to 20 km, so this resolves it without tying a chat to a street.
pub const CELL_RESOLUTION: Resolution = Resolution::Six;

/// Distance a person may be from their cell's centre and still be inside the cell, in km.
///
/// Added to a place's reach when matching, so rounding a position to its cell never hides a
/// threat that is inside the reach. The longest centre-to-vertex distance at resolution 6 is
/// about 3.7 km.
pub const SNAP_TOLERANCE_KM: f64 = 4.0;

const EARTH_RADIUS_KM: f64 = 6371.0088;

/// A position could not be turned into a cell.
#[derive(Debug, Eq, Error, PartialEq)]
pub enum GeoError {
    /// Latitude or longitude is not a finite value inside its range.
    #[error("invalid coordinates")]
    InvalidCoordinates,
    /// A stored cell identifier does not parse.
    #[error("invalid cell")]
    InvalidCell,
}

/// The grid cell a person stands in. This is all that is stored about where they are.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Cell(CellIndex);

impl Cell {
    /// Finds the cell around a position. The coordinates are not kept.
    pub fn around(lat: f64, lon: f64) -> Result<Self, GeoError> {
        // The grid library only rejects non-finite values and wraps everything else, so a
        // latitude of 200 would become a cell somewhere. The range is checked here.
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            return Err(GeoError::InvalidCoordinates);
        }
        let point = LatLng::new(lat, lon).map_err(|_| GeoError::InvalidCoordinates)?;
        Ok(Self(point.to_cell(CELL_RESOLUTION)))
    }

    /// The centre of the cell as `(lat, lon)` in degrees.
    pub fn center(&self) -> (f64, f64) {
        let centre = LatLng::from(self.0);
        (centre.lat(), centre.lng())
    }

    /// Distance from the cell's centre to a point, in km.
    pub fn distance_km(&self, lat: f64, lon: f64) -> f64 {
        let (own_lat, own_lon) = self.center();
        haversine_km(own_lat, own_lon, lat, lon)
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for Cell {
    type Err = GeoError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let cell: CellIndex = value.parse().map_err(|_| GeoError::InvalidCell)?;
        if cell.resolution() == CELL_RESOLUTION {
            Ok(Self(cell))
        } else {
            Err(GeoError::InvalidCell)
        }
    }
}

/// Great-circle distance between two points in km.
pub fn haversine_km(lat_a: f64, lon_a: f64, lat_b: f64, lon_b: f64) -> f64 {
    let (phi_a, phi_b) = (lat_a.to_radians(), lat_b.to_radians());
    let d_phi = phi_b - phi_a;
    let d_lambda = (lon_b - lon_a).to_radians();
    let a =
        (d_phi / 2.0).sin().powi(2) + phi_a.cos() * phi_b.cos() * (d_lambda / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * a.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KYIV: (f64, f64) = (50.4501, 30.5234);

    #[test]
    fn a_cell_holds_its_position_and_forgets_the_exact_point() {
        let cell = Cell::around(KYIV.0, KYIV.1).unwrap();
        let (lat, lon) = cell.center();
        assert!(haversine_km(lat, lon, KYIV.0, KYIV.1) < SNAP_TOLERANCE_KM);
        assert_ne!((lat, lon), KYIV);
    }

    #[test]
    fn a_cell_round_trips_through_text() {
        let cell = Cell::around(KYIV.0, KYIV.1).unwrap();
        assert_eq!(cell.to_string().parse::<Cell>(), Ok(cell));
    }

    #[test]
    fn only_cells_of_the_stored_resolution_parse() {
        assert_eq!("not a cell".parse::<Cell>(), Err(GeoError::InvalidCell));
        let coarse = LatLng::new(KYIV.0, KYIV.1)
            .unwrap()
            .to_cell(Resolution::Three);
        assert_eq!(
            coarse.to_string().parse::<Cell>(),
            Err(GeoError::InvalidCell)
        );
    }

    #[test]
    fn invalid_coordinates_are_rejected() {
        for (lat, lon) in [
            (91.0, 0.0),
            (0.0, 181.0),
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
        ] {
            assert_eq!(Cell::around(lat, lon), Err(GeoError::InvalidCoordinates));
        }
    }

    #[test]
    fn haversine_matches_a_known_distance() {
        // Kyiv to Lviv is about 470 km.
        let d = haversine_km(50.4501, 30.5234, 49.8397, 24.0297);
        assert!((465.0..480.0).contains(&d), "{d}");
        assert!(haversine_km(1.0, 2.0, 1.0, 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_snap_tolerance_covers_the_whole_cell() {
        // Every vertex of the cell is within the tolerance of its centre.
        let cell = Cell::around(KYIV.0, KYIV.1).unwrap();
        let index: CellIndex = cell.to_string().parse().unwrap();
        for vertex in index.boundary().iter() {
            assert!(cell.distance_km(vertex.lat(), vertex.lng()) <= SNAP_TOLERANCE_KM);
        }
    }
}
