// SPDX-License-Identifier: MPL-2.0

//! What the route options inputs show: when to travel and how fast to walk.

use cosmic::widget::calendar::CalendarModel;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan, Zoned};

use crate::domain::route::{RouteOptions, Timing, WalkingPace};
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

/// Minutes are chosen in steps of this many.
const MINUTE_STEP: i8 = 5;

/// "00" to "23".
pub fn hour_labels() -> Vec<String> {
    (0..24).map(|hour| format!("{hour:02}")).collect()
}

/// "00", "05", up to "55".
pub fn minute_labels() -> Vec<String> {
    (0..60)
        .step_by(usize::from(MINUTE_STEP.unsigned_abs()))
        .map(|minute| format!("{minute:02}"))
        .collect()
}

/// What the inputs show, which the options follow: the time of day is kept even
/// while it is not used, so that choosing "depart at" again finds it as it was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptionsInput {
    pub kind: TimingKind,
    /// The day, and the month the calendar shows.
    pub calendar: CalendarModel,
    /// The time of day, in the user's time zone.
    pub hour: i8,
    pub minute: i8,
    pub pace: WalkingPace,
}

impl OptionsInput {
    /// The inputs for `options`. Without a time, the time of day shown is in a few
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
    /// few minutes, rather than whatever was left from before.
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

    /// Sets the hour from its place in [`hour_labels`].
    pub fn set_hour(&mut self, index: usize) {
        self.hour = i8::try_from(index).unwrap_or(0).clamp(0, 23);
    }

    /// Sets the minute from its place in [`minute_labels`].
    pub fn set_minute(&mut self, index: usize) {
        self.minute = (i8::try_from(index).unwrap_or(0) * MINUTE_STEP).clamp(0, 59);
    }

    /// The place of the hour in [`hour_labels`].
    pub fn hour_index(&self) -> usize {
        usize::try_from(self.hour).unwrap_or(0)
    }

    /// The place of the minute in [`minute_labels`], a minute between two steps
    /// showing the step before it.
    pub fn minute_index(&self) -> usize {
        usize::try_from(self.minute / MINUTE_STEP).unwrap_or(0)
    }

    /// The time the inputs stand for, in `zone`.
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
            // A time that does not exist, which the calendar and dropdowns cannot
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

    /// The day, as written in the date button.
    pub fn date_label(&self) -> String {
        self.calendar.selected.strftime("%Y-%m-%d").to_string()
    }
}

/// In about a minute, and on a multiple of five minutes.
fn soon(now: Timestamp, zone: &TimeZone) -> Zoned {
    let now = now.to_zoned(zone.clone());
    let later = now.checked_add(1.minute()).unwrap_or_else(|_| now.clone());
    let wait = i64::from((MINUTE_STEP - later.minute() % MINUTE_STEP) % MINUTE_STEP);
    later.checked_add(wait.minutes()).unwrap_or(later)
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

    fn input(options: RouteOptions) -> OptionsInput {
        OptionsInput::new(options, &paris(), at("2026-11-10T14:32:10Z"))
    }

    #[test]
    fn without_a_time_the_inputs_show_the_next_multiple_of_five_minutes() {
        // 14:32:10 UTC is 15:32:10 in Paris: in a minute that is 15:33, then 15:35.
        let input = input(RouteOptions::default());
        assert_eq!(input.kind, TimingKind::Now);
        assert_eq!((input.hour, input.minute), (15, 35));
        assert_eq!(input.date_label(), "2026-11-10");

        // Already on a multiple of five a minute from now.
        let now = at("2026-11-10T14:34:00Z");
        let input = OptionsInput::new(RouteOptions::default(), &paris(), now);
        assert_eq!((input.hour, input.minute), (15, 35));
    }

    #[test]
    fn the_time_wraps_to_the_next_day() {
        let now = at("2026-11-10T22:58:00Z");
        let input = OptionsInput::new(RouteOptions::default(), &paris(), now);
        // 23:58 in Paris, so 00:00 on the 11th.
        assert_eq!((input.hour, input.minute), (0, 0));
        assert_eq!(input.date_label(), "2026-11-11");
    }

    #[test]
    fn leave_now_asks_for_no_time() {
        assert_eq!(input(RouteOptions::default()).timing(&paris()), Timing::Now);
    }

    #[test]
    fn a_local_day_and_time_become_the_right_instant() {
        let mut input = input(RouteOptions::default());
        input.set_kind(TimingKind::Depart, &paris(), at("2026-11-10T14:32:10Z"));
        input.select_date("2026-11-11".parse().unwrap());
        input.set_hour(8);
        input.set_minute(6); // The sixth step: 30 minutes.

        // 08:30 in Paris is 07:30 UTC.
        assert_eq!(
            input.timing(&paris()),
            Timing::DepartAt(at("2026-11-11T07:30:00Z"))
        );
        input.kind = TimingKind::Arrive;
        assert_eq!(
            input.timing(&paris()),
            Timing::ArriveBy(at("2026-11-11T07:30:00Z"))
        );
    }

    #[test]
    fn inputs_for_a_time_show_that_time() {
        let options = RouteOptions {
            timing: Timing::ArriveBy(at("2026-11-11T16:00:00Z")),
            pace: WalkingPace::Slow,
        };
        let input = input(options);
        assert_eq!(input.kind, TimingKind::Arrive);
        assert_eq!((input.hour, input.minute), (17, 0));
        assert_eq!(input.pace, WalkingPace::Slow);
        // And they give back the same options.
        assert_eq!(input.options(&paris()), options);
    }

    #[test]
    fn going_from_now_to_a_time_shows_a_time_in_a_few_minutes() {
        let mut input = input(RouteOptions::default());
        input.set_hour(3);
        input.select_date("2026-12-24".parse().unwrap());

        input.set_kind(TimingKind::Arrive, &paris(), at("2026-11-10T14:32:10Z"));
        assert_eq!((input.hour, input.minute), (15, 35));
        assert_eq!(input.date_label(), "2026-11-10");

        // Between two times, what was chosen stays.
        input.set_hour(9);
        input.set_kind(TimingKind::Depart, &paris(), at("2026-11-10T14:32:10Z"));
        assert_eq!(input.hour, 9);
    }

    #[test]
    fn the_dropdowns_list_hours_and_steps_of_five_minutes() {
        let hours = hour_labels();
        assert_eq!(
            (hours.len(), hours[0].as_str(), hours[23].as_str()),
            (24, "00", "23")
        );
        let minutes = minute_labels();
        assert_eq!(
            (minutes.len(), minutes[1].as_str(), minutes[11].as_str()),
            (12, "05", "55")
        );

        let mut input = input(RouteOptions::default());
        input.set_hour(17);
        input.set_minute(11);
        assert_eq!((input.hour, input.minute), (17, 55));
        assert_eq!((input.hour_index(), input.minute_index()), (17, 11));
        // A minute between two steps shows the step before it.
        input.minute = 47;
        assert_eq!(input.minute_index(), 9);
    }

    #[test]
    fn the_calendar_moves_by_month_without_changing_the_day() {
        let mut input = input(RouteOptions::default());
        input.show_month(true);
        assert_eq!(input.calendar.visible.to_string(), "2026-12-10");
        assert_eq!(input.date_label(), "2026-11-10");
        input.show_month(false);
        input.show_month(false);
        assert_eq!(input.calendar.visible.to_string(), "2026-10-10");
    }
}
