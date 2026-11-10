// SPDX-License-Identifier: MPL-2.0

//! The route options popover's state: when to travel and how fast to walk, edited
//! as a draft until the user applies them.

use cosmic::widget::calendar::CalendarModel;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan, Zoned};

use crate::domain::route::{RouteMode, RouteOptions, Timing, WalkingPace};
use crate::fl;

/// How the time of the trip is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingKind {
    Now,
    Depart,
    Arrive,
}

impl TimingKind {
    pub const ALL: [Self; 3] = [Self::Now, Self::Depart, Self::Arrive];

    pub fn label(self) -> String {
        match self {
            Self::Now => fl!("route-leave-now"),
            Self::Depart => fl!("route-depart-at"),
            Self::Arrive => fl!("route-arrive-by"),
        }
    }
}

impl WalkingPace {
    pub fn label(self) -> String {
        match self {
            Self::Slow => fl!("pace-slow"),
            Self::Normal => fl!("pace-normal"),
            Self::Fast => fl!("pace-fast"),
        }
    }
}

/// What the popover shows while the user edits: the options as they would be if
/// applied now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptionsDraft {
    pub kind: TimingKind,
    /// The day, and the month the calendar shows.
    pub calendar: CalendarModel,
    /// The time of day, in the user's time zone.
    pub hour: i8,
    pub minute: i8,
    pub pace: WalkingPace,
}

impl OptionsDraft {
    /// A draft of `options`. Without a time, the time of day shown is in a few
    /// minutes, rounded up to five.
    pub fn new(options: RouteOptions, zone: &TimeZone, now: Timestamp) -> Self {
        let (kind, when) = match options.timing {
            Timing::Now => (TimingKind::Now, soon(now, zone)),
            Timing::DepartAt(time) => (TimingKind::Depart, time.to_zoned(zone.clone())),
            Timing::ArriveBy(time) => (TimingKind::Arrive, time.to_zoned(zone.clone())),
        };
        Self {
            kind,
            calendar: CalendarModel::new(when.date(), when.date()),
            hour: when.hour(),
            minute: when.minute(),
            pace: options.pace,
        }
    }

    /// Chooses how the time is given. Going from "now" to a time shows a time in a
    /// few minutes, rather than whatever was left in the draft.
    pub fn set_kind(&mut self, kind: TimingKind, zone: &TimeZone, now: Timestamp) {
        if self.kind == TimingKind::Now && kind != TimingKind::Now {
            let when = soon(now, zone);
            self.calendar = CalendarModel::new(when.date(), when.date());
            self.hour = when.hour();
            self.minute = when.minute();
        }
        self.kind = kind;
    }

    pub fn select_date(&mut self, date: Date) {
        self.calendar.set_selected_visible(date);
    }

    /// Shows the next month in the calendar, or the previous one.
    pub fn show_month(&mut self, next: bool) {
        if next {
            self.calendar.show_next_month();
        } else {
            self.calendar.show_prev_month();
        }
    }

    /// The time the draft stands for, in `zone`.
    pub fn timing(&self, zone: &TimeZone) -> Timing {
        let time = self
            .calendar
            .selected
            .at(self.hour, self.minute, 0, 0)
            .to_zoned(zone.clone())
            .map(|when| when.timestamp());
        match (self.kind, time) {
            (TimingKind::Depart, Ok(time)) => Timing::DepartAt(time),
            (TimingKind::Arrive, Ok(time)) => Timing::ArriveBy(time),
            // A time that does not exist, which the calendar and spin buttons cannot
            // give, would only be asked for in vain.
            (TimingKind::Now, _) | (_, Err(_)) => Timing::Now,
        }
    }

    pub fn options(&self, zone: &TimeZone) -> RouteOptions {
        RouteOptions {
            timing: self.timing(zone),
            pace: self.pace,
        }
    }
}

/// In about a minute, and on a multiple of five minutes.
fn soon(now: Timestamp, zone: &TimeZone) -> Zoned {
    let now = now.to_zoned(zone.clone());
    let later = now.checked_add(1.minute()).unwrap_or_else(|_| now.clone());
    let wait = i64::from((5 - later.minute() % 5) % 5);
    later.checked_add(wait.minutes()).unwrap_or(later)
}

/// The options as one line, for the button that opens the popover: only what
/// applies to `mode`.
pub fn summary(options: RouteOptions, mode: RouteMode, zone: &TimeZone) -> String {
    let mut parts = Vec::new();

    if mode.uses_time() {
        let time = |time: Timestamp| {
            time.to_zoned(zone.clone())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        };
        parts.push(match options.timing {
            Timing::Now => fl!("route-leave-now"),
            Timing::DepartAt(at) => {
                let time = time(at);
                fl!("route-summary-depart", time = time)
            }
            Timing::ArriveBy(at) => {
                let time = time(at);
                fl!("route-summary-arrive", time = time)
            }
        });
    }
    if mode.uses_pace() {
        parts.push(match options.pace {
            WalkingPace::Slow => fl!("route-summary-pace-slow"),
            WalkingPace::Normal => fl!("route-summary-pace-normal"),
            WalkingPace::Fast => fl!("route-summary-pace-fast"),
        });
    }

    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paris in winter: one hour ahead of UTC.
    fn paris() -> TimeZone {
        TimeZone::fixed(jiff::tz::offset(1))
    }

    fn at(time: &str) -> Timestamp {
        time.parse().unwrap()
    }

    fn draft(options: RouteOptions) -> OptionsDraft {
        OptionsDraft::new(options, &paris(), at("2026-11-10T14:32:10Z"))
    }

    #[test]
    fn without_a_time_the_draft_shows_the_next_multiple_of_five_minutes() {
        // 14:32:10 UTC is 15:32:10 in Paris: in a minute that is 15:33, then 15:35.
        let draft = draft(RouteOptions::default());
        assert_eq!(draft.kind, TimingKind::Now);
        assert_eq!((draft.hour, draft.minute), (15, 35));
        assert_eq!(draft.calendar.selected.to_string(), "2026-11-10");

        // Already on a multiple of five a minute from now.
        let now = at("2026-11-10T14:34:00Z");
        let draft = OptionsDraft::new(RouteOptions::default(), &paris(), now);
        assert_eq!((draft.hour, draft.minute), (15, 35));
    }

    #[test]
    fn the_time_wraps_to_the_next_day() {
        let now = at("2026-11-10T22:58:00Z");
        let draft = OptionsDraft::new(RouteOptions::default(), &paris(), now);
        // 23:58 in Paris, so 00:00 on the 11th.
        assert_eq!((draft.hour, draft.minute), (0, 0));
        assert_eq!(draft.calendar.selected.to_string(), "2026-11-11");
    }

    #[test]
    fn leave_now_asks_for_no_time() {
        assert_eq!(draft(RouteOptions::default()).timing(&paris()), Timing::Now);
    }

    #[test]
    fn a_local_day_and_time_become_the_right_instant() {
        let mut draft = draft(RouteOptions::default());
        draft.set_kind(TimingKind::Depart, &paris(), at("2026-11-10T14:32:10Z"));
        draft.select_date("2026-11-11".parse().unwrap());
        draft.hour = 8;
        draft.minute = 30;

        // 08:30 in Paris is 07:30 UTC.
        assert_eq!(
            draft.timing(&paris()),
            Timing::DepartAt(at("2026-11-11T07:30:00Z"))
        );
        draft.kind = TimingKind::Arrive;
        assert_eq!(
            draft.timing(&paris()),
            Timing::ArriveBy(at("2026-11-11T07:30:00Z"))
        );
    }

    #[test]
    fn a_draft_of_a_time_shows_that_time() {
        let options = RouteOptions {
            timing: Timing::ArriveBy(at("2026-11-11T16:00:00Z")),
            pace: WalkingPace::Slow,
        };
        let draft = draft(options);
        assert_eq!(draft.kind, TimingKind::Arrive);
        assert_eq!((draft.hour, draft.minute), (17, 0));
        assert_eq!(draft.pace, WalkingPace::Slow);
        // And it applies to the same options.
        assert_eq!(draft.options(&paris()), options);
    }

    #[test]
    fn going_from_now_to_a_time_shows_a_time_in_a_few_minutes() {
        let mut draft = draft(RouteOptions::default());
        draft.hour = 3;
        draft.select_date("2026-12-24".parse().unwrap());

        draft.set_kind(TimingKind::Arrive, &paris(), at("2026-11-10T14:32:10Z"));
        assert_eq!((draft.hour, draft.minute), (15, 35));
        assert_eq!(draft.calendar.selected.to_string(), "2026-11-10");

        // Between two times, what was chosen stays.
        draft.hour = 9;
        draft.set_kind(TimingKind::Depart, &paris(), at("2026-11-10T14:32:10Z"));
        assert_eq!(draft.hour, 9);
    }

    #[test]
    fn the_calendar_moves_by_month_without_changing_the_day() {
        let mut draft = draft(RouteOptions::default());
        draft.show_month(true);
        assert_eq!(draft.calendar.visible.to_string(), "2026-12-10");
        assert_eq!(draft.calendar.selected.to_string(), "2026-11-10");
        draft.show_month(false);
        draft.show_month(false);
        assert_eq!(draft.calendar.visible.to_string(), "2026-10-10");
    }

    #[test]
    fn the_summary_only_mentions_what_applies() {
        let options = RouteOptions {
            timing: Timing::DepartAt(at("2026-11-11T07:30:00Z")),
            pace: WalkingPace::Fast,
        };
        let pace = fl!("route-summary-pace-fast");
        let leave = fl!("route-summary-depart", time = "2026-11-11 08:30");

        assert_eq!(
            summary(options, RouteMode::Transit, &paris()),
            format!("{leave} · {pace}")
        );
        assert_eq!(summary(options, RouteMode::Walk, &paris()), pace);
        assert_eq!(summary(options, RouteMode::Car, &paris()), "");
        assert_eq!(
            summary(RouteOptions::default(), RouteMode::Transit, &paris()),
            format!(
                "{} · {}",
                fl!("route-leave-now"),
                fl!("route-summary-pace-normal")
            )
        );
    }
}
