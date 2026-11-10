// SPDX-License-Identifier: MPL-2.0

//! Routes between two places, with the times they are scheduled and estimated
//! for.

use cosmic::iced::Color;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::geo::WorldPoint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteMode {
    Walk,
    Bike,
    Car,
    /// Public transit, with walking to and between stops.
    Transit,
}

impl RouteMode {
    /// Whether when to travel matters: only public transit follows a timetable.
    pub fn uses_time(self) -> bool {
        self == Self::Transit
    }

    /// Whether the walking pace matters: on foot, and on the walking legs of a
    /// transit route.
    pub fn uses_pace(self) -> bool {
        matches!(self, Self::Walk | Self::Transit)
    }
}

/// When to travel.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    #[default]
    Now,
    /// Leave at this time, or after it.
    DepartAt(Timestamp),
    /// Arrive at this time, or before it.
    ArriveBy(Timestamp),
}

/// How fast the user walks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalkingPace {
    Slow,
    #[default]
    Normal,
    Fast,
}

impl WalkingPace {
    pub const ALL: [Self; 3] = [Self::Slow, Self::Normal, Self::Fast];

    /// The average speed, in meters per second: about 3.2, 4.3 and 5.8 km/h. Normal
    /// is what the routing service assumes without being told.
    pub fn meters_per_second(self) -> f64 {
        match self {
            Self::Slow => 0.9,
            Self::Normal => 1.2,
            Self::Fast => 1.6,
        }
    }
}

/// What the user asks of a route besides where it goes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RouteOptions {
    pub timing: Timing,
    pub pace: WalkingPace,
}

impl RouteOptions {
    /// The options that matter to routes by `mode`, the others left at their
    /// defaults: two sets that give the same routes are then equal.
    pub fn for_mode(self, mode: RouteMode) -> Self {
        Self {
            timing: if mode.uses_time() {
                self.timing
            } else {
                Timing::Now
            },
            pace: if mode.uses_pace() {
                self.pace
            } else {
                WalkingPace::Normal
            },
        }
    }
}

/// The routes found between two places.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// The route all the way by street, in the requested mode (walking for
    /// transit), if possible.
    pub direct: Option<Itinerary>,
    /// Public transit options, empty for other modes.
    pub transit: Vec<Itinerary>,
    /// Cursors for transit options leaving earlier or later, passed back to [`plan`].
    pub earlier: Option<String>,
    pub later: Option<String>,
}

impl Plan {
    /// Whether nothing was found for `mode`.
    pub fn is_empty(&self, mode: RouteMode) -> bool {
        match mode {
            RouteMode::Walk | RouteMode::Bike | RouteMode::Car => self.direct.is_none(),
            RouteMode::Transit => self.direct.is_none() && self.transit.is_empty(),
        }
    }
}

/// Real-time estimates earlier than the schedule by more than this, in seconds,
/// are taken for errors in the feed: vehicles do not run an hour early.
const MAX_EARLY: i64 = 300;

/// A time as estimated with real-time data, and as scheduled. Both are the same
/// without real-time data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Planned {
    pub estimated: Timestamp,
    pub scheduled: Timestamp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Itinerary {
    /// Estimated, in seconds.
    pub duration: u64,
    pub start_time: Timestamp,
    pub end_time: Timestamp,
    /// Of the first and last legs, which MOTIS gives no itinerary times for.
    pub scheduled_start: Timestamp,
    pub scheduled_end: Timestamp,
    pub transfers: u32,
    pub legs: Vec<Leg>,
}

impl Itinerary {
    pub fn start(&self) -> Planned {
        Planned {
            estimated: self.start_time,
            scheduled: self.scheduled_start,
        }
    }

    pub fn end(&self) -> Planned {
        Planned {
            estimated: self.end_time,
            scheduled: self.scheduled_end,
        }
    }

    /// The duration as estimated and as scheduled, in seconds.
    pub fn durations(&self) -> (u64, u64) {
        let scheduled = self.scheduled_end.as_second() - self.scheduled_start.as_second();
        (self.duration, u64::try_from(scheduled).unwrap_or_default())
    }
}

/// How a leg is travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegMode {
    Walk,
    Bike,
    Car,
    Transit(TransitKind),
}

/// The kind of vehicle on a public transit line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitKind {
    Metro,
    Bus,
    Tram,
    Ferry,
    Train,
    /// Cable cars, funiculars, and whatever else feeds describe.
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Leg {
    pub mode: LegMode,
    pub from: Stop,
    pub to: Stop,
    pub start_time: Timestamp,
    pub end_time: Timestamp,
    pub scheduled_start: Timestamp,
    pub scheduled_end: Timestamp,
    /// In seconds.
    pub duration: u64,
    /// In meters, for street legs.
    pub distance: Option<f64>,
    /// The line taken, for transit legs.
    pub line: Option<Line>,
    /// Stops passed between boarding and getting off, for transit legs.
    pub stops: Vec<PassedStop>,
    /// The leg's path, in Web Mercator coordinates spanning 0..1.
    pub path: Vec<WorldPoint>,
    /// Turn-by-turn instructions, for street legs.
    pub steps: Vec<Step>,
}

impl Leg {
    pub fn is_walk(&self) -> bool {
        self.mode == LegMode::Walk
    }

    pub fn start(&self) -> Planned {
        Planned {
            estimated: self.start_time,
            scheduled: self.scheduled_start,
        }
    }

    pub fn end(&self) -> Planned {
        Planned {
            estimated: self.end_time,
            scheduled: self.scheduled_end,
        }
    }

    /// On a public transit line, rather than walking, cycling or driving.
    pub fn is_transit(&self) -> bool {
        matches!(self.mode, LegMode::Transit(_))
    }
}

/// What a stop is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopName {
    /// Where the trip starts, which has no name of its own.
    Start,
    /// Where the trip ends.
    End,
    Named(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    pub name: StopName,
    pub position: WorldPoint,
}

/// A stop the vehicle calls at between boarding and getting off.
#[derive(Debug, Clone, PartialEq)]
pub struct PassedStop {
    pub name: String,
    pub arrival: Option<Timestamp>,
    pub scheduled_arrival: Option<Timestamp>,
    pub position: WorldPoint,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Such as "3" or "A".
    pub name: Option<String>,
    /// The line's colours, when the operator publishes them.
    pub color: Option<Color>,
    pub text_color: Option<Color>,
    /// Where the vehicle is heading.
    pub headsign: Option<String>,
    pub agency: Option<String>,
}

/// What to do at the start of a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Depart,
    Continue,
    Left,
    Right,
    SlightlyLeft,
    SlightlyRight,
    HardLeft,
    HardRight,
    Roundabout,
    UTurn,
    Elevator,
    Stairs,
    EnterStation,
    ExitStation,
    FollowSigns,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub direction: Direction,
    /// Empty when the way has no name.
    pub street: String,
    /// In meters.
    pub distance: f64,
}

impl Itinerary {
    /// The itinerary with its times on schedule if some leg is estimated to run much
    /// earlier than scheduled, which feeds get wrong.
    pub fn without_implausible_estimates(mut self) -> Self {
        if self.has_implausible_estimates() {
            self.use_schedule();
        }
        self
    }

    /// Some leg is estimated to run much earlier than scheduled.
    fn has_implausible_estimates(&self) -> bool {
        let too_early = |planned: Planned| {
            planned.scheduled.as_second() - planned.estimated.as_second() > MAX_EARLY
        };
        self.legs
            .iter()
            .any(|leg| too_early(leg.start()) || too_early(leg.end()))
    }

    /// Drops all the real-time estimates, so the times shown stay consistent.
    fn use_schedule(&mut self) {
        self.start_time = self.scheduled_start;
        self.end_time = self.scheduled_end;
        self.duration = self.durations().1;
        for leg in &mut self.legs {
            leg.start_time = leg.scheduled_start;
            leg.end_time = leg.scheduled_end;
            for stop in &mut leg.stops {
                stop.arrival = stop.scheduled_arrival.or(stop.arrival);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faster_walkers_have_higher_speeds_and_normal_is_the_default() {
        let [slow, normal, fast] = WalkingPace::ALL.map(WalkingPace::meters_per_second);
        assert!(slow < normal && normal < fast);
        assert_eq!(normal, 1.2);
        assert_eq!(WalkingPace::default(), WalkingPace::Normal);
    }

    #[test]
    fn options_that_do_not_apply_to_a_mode_are_dropped() {
        let options = RouteOptions {
            timing: Timing::DepartAt("2026-11-11T07:30:00Z".parse().unwrap()),
            pace: WalkingPace::Fast,
        };
        assert_eq!(options.for_mode(RouteMode::Transit), options);
        assert_eq!(
            options.for_mode(RouteMode::Walk),
            RouteOptions {
                pace: WalkingPace::Fast,
                ..RouteOptions::default()
            }
        );
        assert_eq!(options.for_mode(RouteMode::Bike), RouteOptions::default());
    }

    #[test]
    fn the_pace_survives_being_saved() {
        for pace in WalkingPace::ALL {
            let json = serde_json::to_string(&pace).unwrap();
            assert_eq!(serde_json::from_str::<WalkingPace>(&json).unwrap(), pace);
        }
    }

    #[test]
    fn only_some_modes_use_time_and_pace() {
        assert!(RouteMode::Transit.uses_time());
        assert!(
            ![RouteMode::Walk, RouteMode::Bike, RouteMode::Car]
                .into_iter()
                .any(RouteMode::uses_time)
        );
        assert!(RouteMode::Walk.uses_pace() && RouteMode::Transit.uses_pace());
        assert!(!RouteMode::Bike.uses_pace() && !RouteMode::Car.uses_pace());
    }
}
