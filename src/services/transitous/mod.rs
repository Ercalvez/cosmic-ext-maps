// SPDX-License-Identifier: MPL-2.0

//! Walking, cycling, driving and public transit routes from Transitous, the
//! community-run MOTIS service GNOME Maps also uses for public transit.
//!
//! Usage policy (transitous.org/api): open-source, non-commercial apps sending a
//! `User-Agent` with a contact, with visible attribution of transitous.org/sources
//! and OpenStreetMap. Maintainers ask to be contacted before heavy `/plan` use.

use std::time::Duration;

use self::raw::Response;
use super::http::{Error, RateLimit, get_json};
use crate::domain::route::{Plan, RouteMode, RouteOptions, Timing, WalkingPace};
use crate::geo::LatLon;

const PLAN_URL: &str = "https://api.transitous.org/api/v6/plan";
/// Transitous gives no numbers; plans are only requested on user actions, so one
/// per second is plenty.
static TRANSITOUS: RateLimit = RateLimit::new(Duration::from_secs(1));

/// Routes from `from` to `to`, both latitude and longitude, as `options` ask, or
/// the earlier or later page given by a cursor from a previous [`Plan`] (which
/// must be asked for with the same options).
pub async fn plan(
    from: LatLon,
    to: LatLon,
    mode: RouteMode,
    options: RouteOptions,
    page: Option<String>,
) -> Result<Plan, Error> {
    let query = query(from, to, mode, options, page);
    let response: Response = get_json(&TRANSITOUS, PLAN_URL, &query).await?;

    Ok(response.into_plan())
}

/// The `/plan` query parameters.
fn query(
    from: LatLon,
    to: LatLon,
    mode: RouteMode,
    options: RouteOptions,
    page: Option<String>,
) -> Vec<(&'static str, String)> {
    // Without transit modes, only the route all the way by street is computed.
    let transit_modes = if mode == RouteMode::Transit {
        "TRANSIT"
    } else {
        ""
    };
    let mut query = vec![
        ("fromPlace", format!("{},{}", from.lat, from.lon)),
        ("toPlace", format!("{},{}", to.lat, to.lon)),
        ("directModes", direct_mode(mode).to_owned()),
        ("transitModes", transit_modes.to_owned()),
        ("maxDirectTime", max_direct_time(mode).to_owned()),
    ];
    if mode == RouteMode::Bike {
        // Flatter streets when the detour is small.
        query.push(("elevationCosts", "LOW".to_owned()));
    }
    if mode.uses_time() {
        match options.timing {
            Timing::Now => {}
            Timing::DepartAt(time) => query.push(("time", time.to_string())),
            Timing::ArriveBy(time) => {
                query.push(("time", time.to_string()));
                query.push(("arriveBy", "true".to_owned()));
            }
        }
    }
    // Normal is the service's own assumption: nothing to say.
    if mode.uses_pace() && options.pace != WalkingPace::Normal {
        query.push((
            "pedestrianSpeed",
            options.pace.meters_per_second().to_string(),
        ));
    }
    if let Some(page) = page {
        query.push(("pageCursor", page));
    }
    query
}

/// The MOTIS mode of the route all the way by street.
fn direct_mode(mode: RouteMode) -> &'static str {
    match mode {
        RouteMode::Walk | RouteMode::Transit => "WALK",
        RouteMode::Bike => "BIKE",
        RouteMode::Car => "CAR",
    }
}

/// Longest route all the way by street searched, in seconds. The server's default
/// of 30 minutes leaves out walks across a city.
fn max_direct_time(mode: RouteMode) -> &'static str {
    match mode {
        RouteMode::Walk | RouteMode::Transit => "7200",
        RouteMode::Bike => "14400",
        RouteMode::Car => "43200",
    }
}

mod polyline;
mod raw;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_follow_the_mode() {
        let louvre = LatLon::new(48.8606, 2.3376);
        let gare_de_lyon = LatLon::new(48.8443, 2.3744);
        let param = |mode, key| {
            query(louvre, gare_de_lyon, mode, RouteOptions::default(), None)
                .into_iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value)
        };

        assert_eq!(
            param(RouteMode::Walk, "directModes").as_deref(),
            Some("WALK")
        );
        assert_eq!(param(RouteMode::Walk, "transitModes").as_deref(), Some(""));
        assert_eq!(
            param(RouteMode::Transit, "directModes").as_deref(),
            Some("WALK")
        );
        assert_eq!(
            param(RouteMode::Transit, "transitModes").as_deref(),
            Some("TRANSIT")
        );
        assert_eq!(
            param(RouteMode::Bike, "directModes").as_deref(),
            Some("BIKE")
        );
        assert_eq!(
            param(RouteMode::Bike, "elevationCosts").as_deref(),
            Some("LOW")
        );
        assert_eq!(param(RouteMode::Car, "directModes").as_deref(), Some("CAR"));
        assert_eq!(param(RouteMode::Car, "elevationCosts"), None);
        assert_eq!(
            param(RouteMode::Car, "maxDirectTime").as_deref(),
            Some("43200")
        );
        assert_eq!(
            param(RouteMode::Walk, "fromPlace").as_deref(),
            Some("48.8606,2.3376")
        );

        let page = query(
            louvre,
            gare_de_lyon,
            RouteMode::Transit,
            RouteOptions::default(),
            Some("LATER|1".to_owned()),
        );
        assert!(page.contains(&("pageCursor", "LATER|1".to_owned())));
    }

    const LOUVRE: LatLon = LatLon::new(48.8606, 2.3376);
    const GARE_DE_LYON: LatLon = LatLon::new(48.8443, 2.3744);

    fn names(mode: RouteMode, options: RouteOptions) -> Vec<&'static str> {
        query(LOUVRE, GARE_DE_LYON, mode, options, None)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    fn value(mode: RouteMode, options: RouteOptions, key: &str) -> Option<String> {
        query(LOUVRE, GARE_DE_LYON, mode, options, None)
            .into_iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value)
    }

    fn at(time: &str) -> Timing {
        Timing::DepartAt(time.parse().unwrap())
    }

    #[test]
    fn default_options_add_nothing_to_the_request() {
        for mode in [RouteMode::Walk, RouteMode::Transit] {
            let names = names(mode, RouteOptions::default());
            assert!(!names.contains(&"time") && !names.contains(&"pedestrianSpeed"));
        }
    }

    #[test]
    fn transit_can_depart_after_a_time() {
        let options = RouteOptions {
            timing: at("2026-11-11T07:30:00Z"),
            ..RouteOptions::default()
        };
        assert_eq!(
            value(RouteMode::Transit, options, "time").as_deref(),
            Some("2026-11-11T07:30:00Z")
        );
        assert_eq!(value(RouteMode::Transit, options, "arriveBy"), None);
    }

    #[test]
    fn transit_can_arrive_by_a_time() {
        let options = RouteOptions {
            timing: Timing::ArriveBy("2026-11-11T16:00:00Z".parse().unwrap()),
            ..RouteOptions::default()
        };
        assert_eq!(
            value(RouteMode::Transit, options, "time").as_deref(),
            Some("2026-11-11T16:00:00Z")
        );
        assert_eq!(
            value(RouteMode::Transit, options, "arriveBy").as_deref(),
            Some("true")
        );
    }

    #[test]
    fn only_transit_follows_a_timetable() {
        let options = RouteOptions {
            timing: at("2026-11-11T07:30:00Z"),
            ..RouteOptions::default()
        };
        for mode in [RouteMode::Walk, RouteMode::Bike, RouteMode::Car] {
            assert_eq!(value(mode, options, "time"), None, "{mode:?}");
        }
    }

    #[test]
    fn the_walking_pace_is_a_speed_in_meters_per_second() {
        let pace = |pace| RouteOptions {
            pace,
            ..RouteOptions::default()
        };
        assert_eq!(
            value(RouteMode::Walk, pace(WalkingPace::Fast), "pedestrianSpeed").as_deref(),
            Some("1.6")
        );
        assert_eq!(
            value(
                RouteMode::Transit,
                pace(WalkingPace::Slow),
                "pedestrianSpeed"
            )
            .as_deref(),
            Some("0.9")
        );
        // Cycling and driving do not walk.
        for mode in [RouteMode::Bike, RouteMode::Car] {
            assert_eq!(
                value(mode, pace(WalkingPace::Fast), "pedestrianSpeed"),
                None
            );
        }
    }

    #[test]
    fn a_page_is_asked_for_with_the_same_options() {
        let options = RouteOptions {
            timing: at("2026-11-11T07:30:00Z"),
            pace: WalkingPace::Fast,
        };
        let page = query(
            LOUVRE,
            GARE_DE_LYON,
            RouteMode::Transit,
            options,
            Some("LATER|1".to_owned()),
        );
        let names: Vec<_> = page.iter().map(|(name, _)| *name).collect();
        assert!(
            ["time", "pedestrianSpeed", "pageCursor"]
                .iter()
                .all(|key| names.contains(key))
        );
    }
}
