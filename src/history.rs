// SPDX-License-Identifier: MPL-2.0

//! The places the user picked from search, kept between runs so that they can be
//! picked again without searching.

use serde::{Deserialize, Serialize};

use crate::domain::place::{OsmId, Poi, SearchResult};
use crate::geo::LatLon;

/// Most places kept; the oldest go first.
pub const HISTORY_LIMIT: usize = 20;

/// Two places closer than this, in degrees, are the same one when they have no
/// OpenStreetMap id to tell.
const SAME_PLACE: f64 = 1e-6;

/// A search result as saved on disk: what is needed to show it again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredPlace {
    pub name: String,
    pub translated_name: Option<String>,
    pub class: Option<String>,
    pub subclass: Option<String>,
    pub osm_id: Option<OsmId>,
    pub lat_lon: LatLon,
    /// Where the place is, such as "Rue de Rivoli, Paris, France".
    pub area: Option<String>,
}

impl From<&SearchResult> for StoredPlace {
    fn from(result: &SearchResult) -> Self {
        let poi = &result.poi;
        Self {
            name: poi.name.clone(),
            translated_name: poi.translated_name.clone(),
            class: poi.class.clone(),
            subclass: poi.subclass.clone(),
            osm_id: poi.osm_id,
            lat_lon: poi.position.to_lat_lon(),
            area: result.area.clone(),
        }
    }
}

impl From<StoredPlace> for SearchResult {
    fn from(place: StoredPlace) -> Self {
        Self {
            poi: Poi {
                translated_name: place.translated_name,
                class: place.class,
                subclass: place.subclass,
                osm_id: place.osm_id,
                ..Poi::new(place.name, place.lat_lon.to_world())
            },
            area: place.area,
        }
    }
}

impl StoredPlace {
    /// Whether both are the same place: the same OpenStreetMap object, or without
    /// ids, the same name at the same spot.
    fn is_same_as(&self, other: &Self) -> bool {
        match (self.osm_id, other.osm_id) {
            (Some(a), Some(b)) => a == b,
            _ => {
                self.name == other.name
                    && (self.lat_lon.lat - other.lat_lon.lat).abs() < SAME_PLACE
                    && (self.lat_lon.lon - other.lat_lon.lon).abs() < SAME_PLACE
            }
        }
    }
}

/// The history after picking `place`: it comes first, once, and only
/// [`HISTORY_LIMIT`] places are kept.
pub fn remember(entries: &[StoredPlace], place: StoredPlace) -> Vec<StoredPlace> {
    let others = entries.iter().filter(|entry| !entry.is_same_as(&place));
    let mut history: Vec<StoredPlace> = others.cloned().collect();
    history.insert(0, place);
    history.truncate(HISTORY_LIMIT);
    history
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(name: &str, lat: f64, lon: f64, osm_id: Option<OsmId>) -> StoredPlace {
        StoredPlace {
            name: name.to_owned(),
            translated_name: None,
            class: None,
            subclass: None,
            osm_id,
            lat_lon: LatLon::new(lat, lon),
            area: None,
        }
    }

    fn louvre() -> StoredPlace {
        place("Musée du Louvre", 48.8606, 2.3376, Some(OsmId::Way(1)))
    }

    fn gare_de_lyon() -> StoredPlace {
        place("Gare de Lyon", 48.8443, 2.3744, Some(OsmId::Node(2)))
    }

    fn hotel_de_ville() -> StoredPlace {
        place("Hôtel de Ville", 48.8564, 2.3524, None)
    }

    fn names(history: &[StoredPlace]) -> Vec<&str> {
        history.iter().map(|place| place.name.as_str()).collect()
    }

    #[test]
    fn the_latest_place_comes_first() {
        let history = remember(&[], louvre());
        let history = remember(&history, gare_de_lyon());
        assert_eq!(names(&history), ["Gare de Lyon", "Musée du Louvre"]);
    }

    #[test]
    fn a_place_picked_again_moves_to_the_front_without_repeating() {
        let history = remember(&remember(&[], louvre()), gare_de_lyon());

        // Same OpenStreetMap object, even if its name or spot differ a little.
        let renamed = StoredPlace {
            name: "Louvre".to_owned(),
            lat_lon: LatLon::new(48.861, 2.338),
            ..louvre()
        };
        let history = remember(&history, renamed);
        assert_eq!(names(&history), ["Louvre", "Gare de Lyon"]);

        // Without ids, the same name at the same spot.
        let history = remember(&remember(&history, hotel_de_ville()), hotel_de_ville());
        assert_eq!(
            names(&history),
            ["Hôtel de Ville", "Louvre", "Gare de Lyon"]
        );
        // The same name elsewhere is another place.
        let elsewhere = place("Hôtel de Ville", 45.7676, 4.8344, None);
        assert_eq!(remember(&history, elsewhere).len(), 4);
    }

    #[test]
    fn only_the_latest_places_are_kept() {
        let mut history = Vec::new();
        for index in 0..HISTORY_LIMIT + 5 {
            let place = place(
                &format!("Place {index}"),
                48.8,
                2.3,
                Some(OsmId::Node(index as u64)),
            );
            history = remember(&history, place);
        }

        assert_eq!(history.len(), HISTORY_LIMIT);
        assert_eq!(history[0].name, format!("Place {}", HISTORY_LIMIT + 4));
        assert_eq!(history[HISTORY_LIMIT - 1].name, "Place 5");
    }

    #[test]
    fn a_search_result_survives_being_stored() {
        let result = SearchResult {
            poi: Poi {
                translated_name: Some("Louvre Museum".to_owned()),
                class: Some("tourism".to_owned()),
                subclass: Some("museum".to_owned()),
                osm_id: Some(OsmId::Way(1)),
                ..Poi::new("Musée du Louvre", LatLon::new(48.8606, 2.3376).to_world())
            },
            area: Some("Rue de Rivoli, Paris, France".to_owned()),
        };

        let stored = StoredPlace::from(&result);
        let json = serde_json::to_string(&vec![stored.clone()]).unwrap();
        let back: Vec<StoredPlace> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.as_slice(), std::slice::from_ref(&stored));

        let restored = SearchResult::from(back.into_iter().next().unwrap());
        assert_eq!(restored.poi.name, "Musée du Louvre");
        assert_eq!(
            restored.poi.translated_name.as_deref(),
            Some("Louvre Museum")
        );
        assert_eq!(restored.poi.osm_id, Some(OsmId::Way(1)));
        assert_eq!(
            restored.area.as_deref(),
            Some("Rue de Rivoli, Paris, France")
        );
        // The position comes back to within a hair of where it was.
        let back = restored.poi.position.to_lat_lon();
        assert!((back.lat - 48.8606).abs() < 1e-9 && (back.lon - 2.3376).abs() < 1e-9);
        assert_eq!(StoredPlace::from(&restored).osm_id, stored.osm_id);
    }
}
