// SPDX-License-Identifier: MPL-2.0

//! The directions form, and the status or the routes under it.

use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget::{self, icon};
use jiff::civil::Weekday;
use jiff::tz::TimeZone;

use super::options::{self, TimingKind};
use super::results::{self, Ends};
use super::{Directions, Field, Message, RouteState, TravelMode};
use crate::domain::route::RouteMode;
use crate::fl;
use crate::search_box::{self, SearchBox};

/// Width of the options popover.
const OPTIONS_WIDTH: f32 = 340.0;
/// What the popover needs besides its scrolling controls, and above and below it:
/// the Apply button, the padding, and the header bar.
const OPTIONS_CHROME: f32 = 160.0;
/// The least height given to the scrolling controls.
const OPTIONS_MIN_HEIGHT: f32 = 120.0;

impl TravelMode {
    pub(super) fn label(self) -> String {
        match self {
            Self::Foot => fl!("travel-mode-foot"),
            Self::Bike => fl!("travel-mode-bike"),
            Self::Transit => fl!("travel-mode-transit"),
            Self::Car => fl!("travel-mode-car"),
        }
    }

    pub(super) fn icon(self) -> icon::Icon {
        let svg: &'static [u8] = match self {
            Self::Foot => include_bytes!("../../resources/icons/transport/walking-symbolic.svg"),
            Self::Bike => include_bytes!("../../resources/icons/transport/cycling-symbolic.svg"),
            Self::Transit => include_bytes!("../../resources/icons/transport/transit-symbolic.svg"),
            Self::Car => include_bytes!("../../resources/icons/transport/driving-symbolic.svg"),
        };
        let mut handle = icon::from_svg_bytes(svg);
        // Drawn in one colour, so the theme can recolour it like its own icons.
        handle.symbolic = true;
        handle.icon()
    }
}

impl Directions {
    /// The form, kept above the scrolling results: modes, start and destination,
    /// the options, and earlier/later buttons for transit. Popovers must fit in a
    /// window `window_height` high.
    pub fn form_view(&self, window_height: f32) -> Element<'_, Message> {
        let spacing = cosmic::theme::spacing();

        let modes = widget::segmented_control::horizontal(&self.modes).on_activate(Message::Mode);

        let swap = widget::tooltip(
            widget::button::icon(icon::from_name("object-flip-vertical-symbolic"))
                .on_press(Message::Swap),
            widget::text::body(fl!("directions-swap")),
            widget::tooltip::Position::Bottom,
        );

        widget::column::with_capacity(5)
            .width(Length::Fill)
            .push(modes)
            .push(field(
                Field::From,
                fl!("directions-from"),
                fl!("directions-choose-start"),
                &self.from_search,
            ))
            .push(
                widget::row::with_capacity(1)
                    .push(widget::space::horizontal())
                    .push(swap),
            )
            .push(field(
                Field::To,
                fl!("directions-to"),
                fl!("directions-choose-destination"),
                &self.to_search,
            ))
            .push_maybe(self.options_view(window_height))
            .push_maybe(self.pages_view())
            .spacing(spacing.space_s)
            .into()
    }

    /// What is asked of the route besides its ends, as a button that opens the
    /// options. Cycling and driving have none.
    fn options_view(&self, window_height: f32) -> Option<Element<'_, Message>> {
        let mode = self.mode().route_mode();
        if !mode.uses_time() && !mode.uses_pace() {
            return None;
        }

        let summary = options::summary(self.options, mode, &TimeZone::system());
        let button = widget::button::text(summary)
            .leading_icon(icon::from_name("preferences-system-time-symbolic"))
            .on_press(Message::ToggleOptions);

        let mut options = widget::popover(button);
        if self.options_open {
            options = options
                .popup(self.options_popup(mode, window_height))
                .position(widget::popover::Position::Bottom);
        }
        Some(options.into())
    }

    /// The options being edited: when to travel and how fast to walk, applied
    /// together by the button at the bottom. The controls scroll if the window is
    /// too short for them, and the button stays in view.
    fn options_popup(&self, mode: RouteMode, window_height: f32) -> Element<'_, Message> {
        let spacing = cosmic::theme::spacing();
        let mut content = widget::column::with_capacity(7).spacing(spacing.space_s);

        if mode.uses_time() {
            content = content
                .push(widget::text::caption_heading(fl!("route-when")))
                .push(
                    widget::segmented_control::horizontal(&self.timing_kinds)
                        .on_activate(Message::TimingKind),
                );
            if self.draft.kind != TimingKind::Now {
                content = content
                    .push(widget::calendar(
                        &self.draft.calendar,
                        Message::OptionDate,
                        || Message::OptionMonth(false),
                        || Message::OptionMonth(true),
                        Weekday::Monday,
                    ))
                    .push(self.time_of_day());
            }
        }

        if mode.uses_pace() {
            content = content
                .push(widget::text::caption_heading(fl!("route-walking-pace")))
                .push(
                    widget::segmented_control::horizontal(&self.paces)
                        .on_activate(Message::PaceChosen),
                );
        }

        let controls = widget::container(widget::scrollable(content))
            .max_height((window_height - OPTIONS_CHROME).max(OPTIONS_MIN_HEIGHT));
        let apply =
            widget::button::suggested(fl!("route-options-apply")).on_press(Message::ApplyOptions);

        widget::container(
            widget::column::with_capacity(2)
                .push(controls)
                .push(apply)
                .spacing(spacing.space_s),
        )
        .padding(spacing.space_s)
        .width(Length::Fixed(OPTIONS_WIDTH))
        .class(cosmic::theme::Container::Dropdown)
        .apply(widget::container)
        // A small gap, so the popover does not touch the button.
        .padding([spacing.space_xxs, 0, 0, 0])
        .into()
    }

    /// Hour and minute, to be changed with the arrows beside them.
    fn time_of_day(&self) -> Element<'_, Message> {
        let hour = widget::spin_button(
            format!("{:02}", self.draft.hour),
            fl!("route-hour"),
            self.draft.hour,
            1,
            0,
            23,
            Message::OptionHour,
        );
        let minute = widget::spin_button(
            format!("{:02}", self.draft.minute),
            fl!("route-minute"),
            self.draft.minute,
            5,
            0,
            55,
            Message::OptionMinute,
        );

        widget::row::with_capacity(3)
            .push(hour)
            .push(widget::text::title4(":"))
            .push(minute)
            .spacing(cosmic::theme::spacing().space_xs)
            .align_y(Alignment::Center)
            .into()
    }

    /// Earlier and later transit options, when routes were found by transit.
    fn pages_view(&self) -> Option<Element<'_, Message>> {
        let RouteState::Found(plan) = &self.route else {
            return None;
        };
        let is_transit = self
            .query
            .is_some_and(|query| query.mode == RouteMode::Transit);
        if !is_transit || plan.transit.is_empty() {
            return None;
        }

        let pages = widget::row::with_capacity(3)
            .push(
                widget::button::text(fl!("route-earlier"))
                    .on_press_maybe(plan.earlier.is_some().then_some(Message::Page(false))),
            )
            .push(widget::space::horizontal())
            .push(
                widget::button::text(fl!("route-later"))
                    .on_press_maybe(plan.later.is_some().then_some(Message::Page(true))),
            );
        Some(pages.into())
    }

    /// The routes found, or the search's status: the part that scrolls.
    pub fn results_view(&self) -> Element<'_, Message> {
        let status = |text: String| widget::text::caption(text).into();
        let plan = match &self.route {
            RouteState::Idle => return widget::column::with_capacity(0).into(),
            RouteState::Loading => return status(fl!("route-loading")),
            RouteState::NotFound => return status(fl!("route-not-found")),
            RouteState::Failed => return status(fl!("route-failed")),
            RouteState::Found(plan) => plan,
        };

        let Some(query) = self.query else {
            return widget::column::with_capacity(0).into();
        };
        let ends = Ends {
            start: &self.from_search.input,
            destination: &self.to_search.input,
        };
        results::plan_view(plan, query.mode, self.chosen, &self.open_stops, ends)
    }
}

/// A labelled place field, with its search results under it while shown.
fn field(field: Field, label: String, hint: String, search: &SearchBox) -> Element<'_, Message> {
    let input = widget::text_input::text_input(hint, &search.input)
        .on_input(move |text| Message::Field(field, search_box::Message::Input(text)))
        .on_submit(move |_| Message::Field(field, search_box::Message::Submit));

    widget::column::with_capacity(3)
        .push(widget::text::caption_heading(label))
        .push(input)
        .push_maybe((!search.results.is_empty()).then(|| {
            search.results_view(move |index| {
                Message::Field(field, search_box::Message::Select(index))
            })
        }))
        .spacing(cosmic::theme::spacing().space_xxs)
        .into()
}
