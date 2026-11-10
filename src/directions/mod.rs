// SPDX-License-Identifier: MPL-2.0

//! The directions form: transport mode, start and destination, and the routes
//! found between them.

mod options;
mod results;
mod view;

use std::collections::HashSet;

use cosmic::widget::segmented_button::{self, SingleSelectModel};
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::domain::place::Poi;
use crate::domain::route::{Plan, RouteMode, RouteOptions, WalkingPace};
use crate::fl;
use crate::geo::{LatLon, Position};
use crate::map::RouteOverlay;
use crate::search_box::{self, SearchBox};
use crate::services;

use options::{OptionsDraft, TimingKind};

pub use results::attribution;

/// Messages of the directions form and its results.
#[derive(Debug, Clone)]
pub enum Message {
    Mode(segmented_button::Entity),
    Swap,
    /// Open or close the route options, dropping what was changed in them.
    ToggleOptions,
    /// How the time is given, in the options.
    TimingKind(segmented_button::Entity),
    /// The day chosen in the options' calendar.
    OptionDate(jiff::civil::Date),
    /// Show the next (`true`) or the previous month in the options' calendar.
    OptionMonth(bool),
    OptionHour(i8),
    OptionMinute(i8),
    /// The walking pace chosen in the options.
    PaceChosen(segmented_button::Entity),
    /// Search again with the options as edited.
    ApplyOptions,
    Field(Field, search_box::Message),
    /// Routes found, for the search with this number.
    RouteFound(u64, Result<Plan, services::Error>),
    /// Transit options leaving later (`true`) or earlier than those shown.
    Page(bool),
    /// Show or hide the details of a route option.
    Choose(RouteChoice),
    /// Show or hide the stops of a leg of the chosen route option.
    ToggleStops(usize),
}

/// Work the directions need the app to do, returned from [`Directions::update`].
#[derive(Debug)]
pub enum Action {
    /// Search places for a field, sending the answer back as [`Message::Field`].
    Search(Field, search_box::Query),
    /// Find routes, sending the answer back as [`Message::RouteFound`].
    FindRoute(RouteRequest),
    /// The routes to draw changed: pass [`Directions::route_overlay`] to the map,
    /// framing them when `fit` is set.
    ShowRoutes { fit: bool },
    /// The walking pace changed, to be remembered for the next time.
    SavePace(WalkingPace),
}

/// One of the form's place fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    From,
    To,
}

/// Which route of a plan is chosen, to show its details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteChoice {
    /// The route all the way by street.
    Direct,
    Transit(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TravelMode {
    Foot,
    Bike,
    Transit,
    Car,
}

impl TravelMode {
    const ALL: [Self; 4] = [Self::Foot, Self::Bike, Self::Transit, Self::Car];

    fn route_mode(self) -> RouteMode {
        match self {
            Self::Foot => RouteMode::Walk,
            Self::Bike => RouteMode::Bike,
            Self::Transit => RouteMode::Transit,
            Self::Car => RouteMode::Car,
        }
    }
}

/// One end of the trip.
#[derive(Debug, Clone)]
pub enum Endpoint {
    Unset,
    UserLocation(Position),
    Place(Poi),
}

impl Endpoint {
    fn lat_lon(&self) -> Option<LatLon> {
        match self {
            Self::Unset => None,
            Self::UserLocation(position) => Some(position.lat_lon),
            Self::Place(poi) => Some(poi.position.to_lat_lon()),
        }
    }
}

/// What is known about routes for the current form.
#[derive(Debug, Clone)]
pub enum RouteState {
    /// The start or destination is missing.
    Idle,
    Loading,
    Found(Plan),
    NotFound,
    Failed,
}

/// The trip a route request is for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RouteQuery {
    from: LatLon,
    to: LatLon,
    mode: RouteMode,
    /// Only what matters to `mode`.
    options: RouteOptions,
}

/// A route search to run with [`transitous::plan`](crate::services::transitous::plan), then pass to
/// [`Directions::insert_route`].
#[derive(Debug, Clone)]
pub struct RouteRequest {
    pub id: u64,
    pub from: LatLon,
    pub to: LatLon,
    pub mode: RouteMode,
    /// Only what matters to `mode`.
    pub options: RouteOptions,
    /// Earlier or later options, as a cursor from a previous plan. It is asked for
    /// with the options of the plan it came from.
    pub page: Option<String>,
}

pub struct Directions {
    modes: SingleSelectModel,
    from: Endpoint,
    to: Endpoint,
    /// The user changed the start, so a position arriving must not replace it.
    from_edited: bool,
    from_search: SearchBox,
    to_search: SearchBox,
    route: RouteState,
    /// The trip shown or being searched, so an unchanged form is not searched again.
    query: Option<RouteQuery>,
    /// Number of the latest request: answers to older ones are stale.
    latest_request: u64,
    /// The route option whose details are shown.
    chosen: Option<RouteChoice>,
    /// Legs of the chosen option whose stops are listed.
    open_stops: HashSet<usize>,
    /// When to travel and how fast to walk, as last applied.
    options: RouteOptions,
    /// The options as being edited in the popover, until applied.
    draft: OptionsDraft,
    options_open: bool,
    /// The choices in the popover, mirroring `draft`.
    timing_kinds: SingleSelectModel,
    paces: SingleSelectModel,
}

impl Directions {
    /// From the user's position, if known, to `destination`, leaving now at the
    /// walking pace given.
    pub fn new(destination: &Poi, user: Option<Position>, pace: WalkingPace) -> Self {
        let mut modes = SingleSelectModel::default();
        for mode in TravelMode::ALL {
            let entity = modes
                .insert()
                .text(mode.label())
                .icon(mode.icon())
                .data(mode);
            if mode == TravelMode::Foot {
                entity.activate();
            }
        }
        let options = RouteOptions {
            pace,
            ..RouteOptions::default()
        };
        let draft = OptionsDraft::new(options, &TimeZone::system(), Timestamp::now());

        let mut directions = Self {
            modes,
            from: Endpoint::Unset,
            to: Endpoint::Unset,
            from_edited: false,
            from_search: SearchBox::default(),
            to_search: SearchBox::default(),
            route: RouteState::Idle,
            query: None,
            latest_request: 0,
            chosen: None,
            open_stops: HashSet::new(),
            options,
            draft,
            options_open: false,
            timing_kinds: choices(TimingKind::ALL.map(|kind| (kind.label(), kind))),
            paces: choices(WalkingPace::ALL.map(|pace| (pace.label(), pace))),
        };
        directions.sync_choices();
        if let Some(position) = user {
            directions.set(Field::From, Endpoint::UserLocation(position));
        }
        directions.set(Field::To, Endpoint::Place(destination.clone()));
        directions
    }

    pub fn update(&mut self, message: Message) -> Vec<Action> {
        match message {
            Message::Mode(mode) => {
                self.modes.activate(mode);
                // What the options show depends on the mode.
                self.options_open = false;
                self.refresh()
            }
            Message::ToggleOptions => {
                self.toggle_options();
                Vec::new()
            }
            Message::TimingKind(entity) => {
                if let Some(&kind) = self.timing_kinds.data::<TimingKind>(entity) {
                    self.draft
                        .set_kind(kind, &TimeZone::system(), Timestamp::now());
                    self.sync_choices();
                }
                Vec::new()
            }
            Message::OptionDate(date) => {
                self.draft.select_date(date);
                Vec::new()
            }
            Message::OptionMonth(next) => {
                self.draft.show_month(next);
                Vec::new()
            }
            Message::OptionHour(hour) => {
                self.draft.hour = hour;
                Vec::new()
            }
            Message::OptionMinute(minute) => {
                self.draft.minute = minute;
                Vec::new()
            }
            Message::PaceChosen(entity) => {
                if let Some(&pace) = self.paces.data::<WalkingPace>(entity) {
                    self.draft.pace = pace;
                    self.sync_choices();
                }
                Vec::new()
            }
            Message::ApplyOptions => self.apply_options(),
            Message::Swap => {
                self.swap();
                self.refresh()
            }
            Message::Field(field, message) => {
                // A typed start or destination is unset until picked.
                let typed = matches!(message, search_box::Message::Input(_));
                if typed {
                    self.unset(field);
                }
                let mut actions = match self.search_mut(field).update(message) {
                    Some(search_box::Action::Query(query)) => vec![Action::Search(field, query)],
                    Some(search_box::Action::Pick(result)) => {
                        self.pick(field, result.poi);
                        self.refresh()
                    }
                    None => Vec::new(),
                };
                if typed {
                    actions.extend(self.refresh());
                }
                actions
            }
            Message::RouteFound(id, result) => {
                if self.insert_route(id, result) {
                    vec![Action::ShowRoutes { fit: true }]
                } else {
                    Vec::new()
                }
            }
            Message::Page(later) => match self.page_request(later) {
                Some(request) => vec![
                    Action::ShowRoutes { fit: false },
                    Action::FindRoute(request),
                ],
                None => Vec::new(),
            },
            Message::Choose(choice) => {
                self.choose(choice);
                // The map already shows every option: only the highlight changes.
                vec![Action::ShowRoutes { fit: false }]
            }
            Message::ToggleStops(leg) => {
                self.toggle_stops(leg);
                Vec::new()
            }
        }
    }

    /// Updates the routes after a change to the form, searching when the trip
    /// changed.
    pub fn refresh(&mut self) -> Vec<Action> {
        // The routes shown may no longer match the form.
        let mut actions = vec![Action::ShowRoutes { fit: false }];
        actions.extend(self.route_request().map(Action::FindRoute));
        actions
    }

    /// Opens the options with the ones applied, or closes them, dropping any change.
    fn toggle_options(&mut self) {
        self.options_open = !self.options_open;
        if self.options_open {
            self.draft = OptionsDraft::new(self.options, &TimeZone::system(), Timestamp::now());
            self.sync_choices();
        }
    }

    /// Makes the popover's choices show the draft.
    fn sync_choices(&mut self) {
        activate(&mut self.timing_kinds, &self.draft.kind);
        activate(&mut self.paces, &self.draft.pace);
    }

    /// Applies the options as edited: one search, or none if they changed nothing.
    fn apply_options(&mut self) -> Vec<Action> {
        let options = self.draft.options(&TimeZone::system());
        self.options_open = false;

        let mut actions = Vec::new();
        if options.pace != self.options.pace {
            actions.push(Action::SavePace(options.pace));
        }
        self.options = options;
        actions.extend(self.refresh());
        actions
    }

    /// The user's position is known or moved.
    pub fn set_user_position(&mut self, position: Position) -> Vec<Action> {
        if self.user_position_known(position) {
            self.refresh()
        } else {
            Vec::new()
        }
    }

    /// A place selected on the map becomes the destination.
    pub fn set_destination(&mut self, place: Poi) -> Vec<Action> {
        self.set(Field::To, Endpoint::Place(place));
        self.refresh()
    }

    /// Moves the highlight through the search results shown, if any. Returns
    /// `false` when none are shown.
    pub fn move_highlight(&mut self, down: bool) -> bool {
        [&mut self.from_search, &mut self.to_search]
            .into_iter()
            .find(|search| !search.results.is_empty())
            .is_some_and(|search| search.move_highlight(down))
    }

    fn mode(&self) -> TravelMode {
        self.modes
            .active_data::<TravelMode>()
            .copied()
            .unwrap_or(TravelMode::Foot)
    }

    /// The user's position became known. It becomes the start unless the user
    /// already chose or cleared one. Returns whether the start changed this way.
    fn user_position_known(&mut self, position: Position) -> bool {
        match self.from {
            Endpoint::Unset if !self.from_edited => {
                self.set(Field::From, Endpoint::UserLocation(position));
                true
            }
            // Keep the latest position, without searching again for every move.
            Endpoint::UserLocation(_) => {
                self.from = Endpoint::UserLocation(position);
                false
            }
            _ => false,
        }
    }

    fn swap(&mut self) {
        std::mem::swap(&mut self.from, &mut self.to);
        std::mem::swap(&mut self.from_search, &mut self.to_search);
        self.from_edited = true;
    }

    fn search_mut(&mut self, field: Field) -> &mut SearchBox {
        match field {
            Field::From => &mut self.from_search,
            Field::To => &mut self.to_search,
        }
    }

    /// Text was typed in a field: its place is no longer set until one is picked.
    fn unset(&mut self, field: Field) {
        match field {
            Field::From => {
                self.from = Endpoint::Unset;
                self.from_edited = true;
            }
            Field::To => self.to = Endpoint::Unset,
        }
    }

    /// A search result was picked for a field.
    fn pick(&mut self, field: Field, place: Poi) {
        if field == Field::From {
            self.from_edited = true;
        }
        self.set(field, Endpoint::Place(place));
    }

    pub fn hide_results(&mut self) {
        self.from_search.hide_results();
        self.to_search.hide_results();
    }

    /// Updates the routes after a change to the form. Returns the search to run
    /// when the trip changed and has both ends.
    fn route_request(&mut self) -> Option<RouteRequest> {
        let mode = self.mode().route_mode();
        let (Some(from), Some(to)) = (self.from.lat_lon(), self.to.lat_lon()) else {
            self.query = None;
            self.route = RouteState::Idle;
            return None;
        };

        let options = self.options.for_mode(mode);
        let query = RouteQuery {
            from,
            to,
            mode,
            options,
        };
        if self.query == Some(query) {
            return None;
        }
        self.query = Some(query);
        Some(self.start(query, None))
    }

    /// The search for transit options leaving earlier or later than those shown.
    fn page_request(&mut self, later: bool) -> Option<RouteRequest> {
        let RouteState::Found(plan) = &self.route else {
            return None;
        };
        let cursor = if later {
            plan.later.clone()
        } else {
            plan.earlier.clone()
        }?;
        let query = self.query?;
        Some(self.start(query, Some(cursor)))
    }

    /// Stores the answer to a search, unless a newer one was started since.
    /// Returns whether it was stored.
    fn insert_route(&mut self, id: u64, result: Result<Plan, services::Error>) -> bool {
        if id != self.latest_request {
            return false;
        }
        let Some(query) = self.query else {
            return false;
        };
        self.chosen = None;
        self.open_stops.clear();
        self.route = match result {
            Ok(plan) if plan.is_empty(query.mode) => RouteState::NotFound,
            Ok(plan) => RouteState::Found(plan),
            Err(err) => {
                tracing::warn!("failed to find routes: {err}");
                RouteState::Failed
            }
        };
        true
    }

    /// The routes to draw on the map, in the order listed: the route all the way
    /// by street first, then transit options. The chosen one is highlighted;
    /// before any choice, the first transit option, or else the street route.
    pub fn route_overlay(&self) -> Option<RouteOverlay> {
        let RouteState::Found(plan) = &self.route else {
            return None;
        };
        let mode = self.query?.mode;

        let mut itineraries: Vec<_> = plan.direct.iter().cloned().collect();
        if mode == RouteMode::Transit {
            itineraries.extend(plan.transit.iter().cloned());
        }
        let directs = usize::from(plan.direct.is_some());
        let chosen = match self.chosen {
            Some(RouteChoice::Transit(index)) => directs + index,
            None if mode == RouteMode::Transit && !plan.transit.is_empty() => directs,
            Some(RouteChoice::Direct) | None => 0,
        };

        Some(RouteOverlay {
            itineraries,
            chosen,
        })
    }

    /// Shows the details of a route option, or hides them if already shown.
    fn choose(&mut self, choice: RouteChoice) {
        self.chosen = (self.chosen != Some(choice)).then_some(choice);
        self.open_stops.clear();
    }

    /// Lists the stops of a leg of the chosen option, or hides them.
    fn toggle_stops(&mut self, leg: usize) {
        if !self.open_stops.remove(&leg) {
            self.open_stops.insert(leg);
        }
    }

    fn start(&mut self, query: RouteQuery, page: Option<String>) -> RouteRequest {
        self.latest_request += 1;
        self.route = RouteState::Loading;
        RouteRequest {
            id: self.latest_request,
            from: query.from,
            to: query.to,
            mode: query.mode,
            options: query.options,
            page,
        }
    }

    /// Sets a field's place and shows its name in the field.
    fn set(&mut self, field: Field, endpoint: Endpoint) {
        let name = match &endpoint {
            Endpoint::Unset => String::new(),
            Endpoint::UserLocation(_) => fl!("your-location"),
            Endpoint::Place(poi) => poi
                .translated_name
                .clone()
                .unwrap_or_else(|| poi.name.clone()),
        };
        match field {
            Field::From => self.from = endpoint,
            Field::To => self.to = endpoint,
        }
        self.search_mut(field).set_text(name);
    }
}

/// A set of choices, in the order given, each carrying its value.
fn choices<T: Copy + 'static, const N: usize>(items: [(String, T); N]) -> SingleSelectModel {
    let mut model = SingleSelectModel::default();
    for (label, value) in items {
        model.insert().text(label).data(value);
    }
    model
}

/// Selects the choice that carries `value`.
fn activate<T: PartialEq + 'static>(model: &mut SingleSelectModel, value: &T) {
    let entity = model
        .iter()
        .find(|&entity| model.data::<T>(entity) == Some(value));
    if let Some(entity) = entity {
        model.activate(entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::place::SearchResult;
    use crate::domain::route::Timing;
    use crate::geo::WorldPoint;

    fn place(name: &str) -> Poi {
        Poi::new(name, WorldPoint::new(0.5, 0.5))
    }

    /// Near the Musée du Louvre, Paris.
    fn here() -> Position {
        Position {
            lat_lon: LatLon::new(48.8606, 2.3376),
            accuracy: 50.0,
        }
    }

    /// The choice of a popover that carries `value`.
    fn choice<T: PartialEq + 'static>(
        model: &SingleSelectModel,
        value: &T,
    ) -> segmented_button::Entity {
        model
            .iter()
            .find(|&entity| model.data::<T>(entity) == Some(value))
            .unwrap()
    }

    /// A form from the user's position to the Hôtel de Ville, searched once in `mode`.
    fn searched(mode: TravelMode) -> Directions {
        let mut directions =
            Directions::new(&place("Hôtel de Ville"), Some(here()), WalkingPace::Normal);
        select(&mut directions, mode);
        assert!(directions.route_request().is_some());
        directions
    }

    fn find_route(actions: &[Action]) -> Option<&RouteRequest> {
        actions.iter().find_map(|action| match action {
            Action::FindRoute(request) => Some(request),
            _ => None,
        })
    }

    #[test]
    fn editing_the_options_starts_no_search_until_they_are_applied() {
        let mut directions = searched(TravelMode::Transit);
        directions.update(Message::ToggleOptions);
        assert!(directions.options_open);

        let fast = choice(&directions.paces, &WalkingPace::Fast);
        let depart = choice(&directions.timing_kinds, &TimingKind::Depart);
        for message in [
            Message::PaceChosen(fast),
            Message::TimingKind(depart),
            Message::OptionHour(9),
            Message::OptionMinute(30),
            Message::OptionMonth(true),
        ] {
            let actions = directions.update(message);
            assert!(find_route(&actions).is_none() && actions.is_empty());
        }
        // Nothing is asked of the routes yet.
        assert!(find_route(&directions.refresh()).is_none());
        assert_eq!(directions.options, RouteOptions::default());
    }

    #[test]
    fn applying_a_pace_searches_again_and_saves_it() {
        let mut directions = searched(TravelMode::Transit);
        directions.update(Message::ToggleOptions);
        let fast = choice(&directions.paces, &WalkingPace::Fast);
        directions.update(Message::PaceChosen(fast));

        let actions = directions.update(Message::ApplyOptions);
        assert!(!directions.options_open);
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, Action::SavePace(WalkingPace::Fast)))
        );
        let request = find_route(&actions).expect("a new search");
        assert_eq!(request.options.pace, WalkingPace::Fast);
        assert_eq!(request.mode, RouteMode::Transit);
    }

    #[test]
    fn applying_a_time_searches_for_it() {
        let mut directions = searched(TravelMode::Transit);
        directions.update(Message::ToggleOptions);
        let arrive = choice(&directions.timing_kinds, &TimingKind::Arrive);
        directions.update(Message::TimingKind(arrive));
        directions.update(Message::OptionDate("2026-11-11".parse().unwrap()));
        directions.update(Message::OptionHour(17));
        directions.update(Message::OptionMinute(0));
        let expected = directions.draft.timing(&TimeZone::system());

        let actions = directions.update(Message::ApplyOptions);
        let request = find_route(&actions).expect("a new search");
        assert_eq!(request.options.timing, expected);
        assert!(matches!(request.options.timing, Timing::ArriveBy(_)));
        // The pace did not change: nothing to save.
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, Action::SavePace(_)))
        );
    }

    #[test]
    fn applying_nothing_new_searches_nothing() {
        let mut directions = searched(TravelMode::Transit);
        directions.update(Message::ToggleOptions);
        let actions = directions.update(Message::ApplyOptions);
        assert!(find_route(&actions).is_none());
        assert!(!directions.options_open);
    }

    #[test]
    fn options_that_do_not_apply_to_the_mode_search_nothing() {
        // On foot, a time means nothing.
        let mut directions = searched(TravelMode::Foot);
        directions.update(Message::ToggleOptions);
        let depart = choice(&directions.timing_kinds, &TimingKind::Depart);
        directions.update(Message::TimingKind(depart));
        let actions = directions.update(Message::ApplyOptions);
        assert!(find_route(&actions).is_none());

        // But a pace does.
        directions.update(Message::ToggleOptions);
        let slow = choice(&directions.paces, &WalkingPace::Slow);
        directions.update(Message::PaceChosen(slow));
        let actions = directions.update(Message::ApplyOptions);
        let request = find_route(&actions).expect("a new search");
        assert_eq!(request.options.pace, WalkingPace::Slow);
        assert_eq!(request.options.timing, Timing::Now);

        // Cycling has neither.
        select(&mut directions, TravelMode::Bike);
        let request = directions.route_request().unwrap();
        assert_eq!(request.options, RouteOptions::default());
    }

    #[test]
    fn closing_the_options_drops_what_was_edited() {
        let mut directions = searched(TravelMode::Transit);
        directions.update(Message::ToggleOptions);
        let fast = choice(&directions.paces, &WalkingPace::Fast);
        directions.update(Message::PaceChosen(fast));
        directions.update(Message::ToggleOptions);
        assert!(!directions.options_open);

        directions.update(Message::ToggleOptions);
        assert_eq!(directions.draft.pace, WalkingPace::Normal);
        let normal = choice(&directions.paces, &WalkingPace::Normal);
        assert_eq!(directions.paces.active(), normal);
    }

    #[test]
    fn changing_the_mode_closes_the_options() {
        let mut directions = searched(TravelMode::Transit);
        directions.update(Message::ToggleOptions);
        let foot = choice(&directions.modes, &TravelMode::Foot);
        directions.update(Message::Mode(foot));
        assert!(!directions.options_open);
    }

    #[test]
    fn earlier_and_later_are_asked_for_with_the_options_of_the_plan() {
        let mut directions = searched(TravelMode::Transit);
        directions.update(Message::ToggleOptions);
        let fast = choice(&directions.paces, &WalkingPace::Fast);
        directions.update(Message::PaceChosen(fast));
        let actions = directions.update(Message::ApplyOptions);
        let id = find_route(&actions).unwrap().id;

        let plan = Plan {
            transit: vec![itinerary()],
            later: Some("LATER|1".to_owned()),
            ..Plan::default()
        };
        directions.insert_route(id, Ok(plan));
        let page = directions.page_request(true).unwrap();
        assert_eq!(page.page.as_deref(), Some("LATER|1"));
        assert_eq!(page.options.pace, WalkingPace::Fast);
    }

    #[test]
    fn a_new_form_starts_at_the_saved_pace_and_leaving_now() {
        let directions = Directions::new(&place("Gare de Lyon"), None, WalkingPace::Slow);
        assert_eq!(directions.options.pace, WalkingPace::Slow);
        assert_eq!(directions.options.timing, Timing::Now);
        assert_eq!(directions.draft.pace, WalkingPace::Slow);
        let slow = choice(&directions.paces, &WalkingPace::Slow);
        assert_eq!(directions.paces.active(), slow);
    }

    fn select(directions: &mut Directions, mode: TravelMode) {
        let entity = directions
            .modes
            .iter()
            .find(|&entity| directions.modes.data::<TravelMode>(entity) == Some(&mode))
            .unwrap();
        directions.modes.activate(entity);
    }

    #[test]
    fn defaults_with_and_without_location() {
        let directions =
            Directions::new(&place("Hôtel de Ville"), Some(here()), WalkingPace::Normal);
        assert!(matches!(directions.from, Endpoint::UserLocation(_)));
        assert_eq!(directions.from_search.input, fl!("your-location"));
        assert!(matches!(&directions.to, Endpoint::Place(poi) if poi.name == "Hôtel de Ville"));
        assert_eq!(directions.to_search.input, "Hôtel de Ville");

        let directions = Directions::new(&place("Hôtel de Ville"), None, WalkingPace::Normal);
        assert!(matches!(directions.from, Endpoint::Unset));
        assert_eq!(directions.from_search.input, "");
    }

    #[test]
    fn location_fills_an_untouched_empty_start_only() {
        let mut directions = Directions::new(&place("Gare de Lyon"), None, WalkingPace::Normal);
        assert!(directions.user_position_known(here()));
        assert!(matches!(directions.from, Endpoint::UserLocation(_)));
        // Later positions update the start without asking for a new search.
        assert!(!directions.user_position_known(here()));

        let mut directions = Directions::new(&place("Gare de Lyon"), None, WalkingPace::Normal);
        let input =
            |text: &str| Message::Field(Field::From, search_box::Message::Input(text.to_owned()));
        directions.update(input("Châtelet"));
        directions.update(input(""));
        assert!(!directions.user_position_known(here()));
        assert!(matches!(directions.from, Endpoint::Unset));
    }

    #[test]
    fn swap_exchanges_places_and_texts() {
        let mut directions =
            Directions::new(&place("Gare de Lyon"), Some(here()), WalkingPace::Normal);
        directions.swap();
        assert!(matches!(&directions.from, Endpoint::Place(poi) if poi.name == "Gare de Lyon"));
        assert!(matches!(directions.to, Endpoint::UserLocation(_)));
        assert_eq!(directions.from_search.input, "Gare de Lyon");
        assert_eq!(directions.to_search.input, fl!("your-location"));
    }

    #[test]
    fn requests_follow_the_form() {
        let mut directions = Directions::new(&place("Hôtel de Ville"), None, WalkingPace::Normal);
        // No start yet.
        assert!(directions.route_request().is_none());
        assert!(matches!(directions.route, RouteState::Idle));

        directions.user_position_known(here());
        let first = directions.route_request().unwrap();
        assert_eq!(first.mode, RouteMode::Walk);
        assert!(matches!(directions.route, RouteState::Loading));
        // An unchanged form is not searched again.
        assert!(directions.route_request().is_none());

        select(&mut directions, TravelMode::Transit);
        let second = directions.route_request().unwrap();
        assert_eq!(second.mode, RouteMode::Transit);
        assert!(second.id > first.id);

        select(&mut directions, TravelMode::Bike);
        assert_eq!(directions.route_request().unwrap().mode, RouteMode::Bike);
        select(&mut directions, TravelMode::Car);
        assert_eq!(directions.route_request().unwrap().mode, RouteMode::Car);
    }

    #[test]
    fn stop_lists_open_and_reset() {
        let mut directions =
            Directions::new(&place("Hôtel de Ville"), Some(here()), WalkingPace::Normal);
        directions.choose(RouteChoice::Transit(0));
        directions.toggle_stops(1);
        assert!(directions.open_stops.contains(&1));
        directions.toggle_stops(1);
        assert!(!directions.open_stops.contains(&1));

        // Another option starts with its stop lists closed.
        directions.toggle_stops(1);
        directions.choose(RouteChoice::Transit(1));
        assert!(directions.open_stops.is_empty());
    }

    fn itinerary() -> crate::domain::route::Itinerary {
        crate::domain::route::Itinerary {
            duration: 600,
            start_time: jiff::Timestamp::UNIX_EPOCH,
            end_time: jiff::Timestamp::UNIX_EPOCH,
            scheduled_start: jiff::Timestamp::UNIX_EPOCH,
            scheduled_end: jiff::Timestamp::UNIX_EPOCH,
            transfers: 0,
            legs: Vec::new(),
        }
    }

    #[test]
    fn overlay_highlights_the_chosen_route() {
        let mut directions =
            Directions::new(&place("Gare de Lyon"), Some(here()), WalkingPace::Normal);
        select(&mut directions, TravelMode::Transit);
        let request = directions.route_request().unwrap();
        let plan = Plan {
            direct: Some(itinerary()),
            transit: vec![itinerary(), itinerary()],
            ..Plan::default()
        };
        directions.insert_route(request.id, Ok(plan));

        // The walking route first, then the transit options; the first of those
        // is highlighted until one is chosen.
        let overlay = directions.route_overlay().unwrap();
        assert_eq!(overlay.itineraries.len(), 3);
        assert_eq!(overlay.chosen, 1);

        directions.choose(RouteChoice::Transit(1));
        assert_eq!(directions.route_overlay().unwrap().chosen, 2);
        directions.choose(RouteChoice::Direct);
        assert_eq!(directions.route_overlay().unwrap().chosen, 0);

        // Nothing to draw while searching again.
        select(&mut directions, TravelMode::Foot);
        directions.route_request();
        assert!(directions.route_overlay().is_none());
    }

    #[test]
    fn picking_a_start_searches_routes() {
        let mut directions = Directions::new(&place("Gare de Lyon"), None, WalkingPace::Normal);
        let field = |message| Message::Field(Field::From, message);

        let actions = directions.update(field(search_box::Message::Input("Châtelet".to_owned())));
        assert!(matches!(
            &actions[..],
            [
                Action::Search(Field::From, search_box::Query::Later(_)),
                Action::ShowRoutes { fit: false },
            ]
        ));

        let found = vec![SearchResult {
            poi: place("Châtelet"),
            area: None,
        }];
        directions.update(field(search_box::Message::Results(
            "Châtelet".to_owned(),
            Ok(found),
        )));
        let actions = directions.update(field(search_box::Message::Select(0)));
        assert!(matches!(
            &actions[..],
            [Action::ShowRoutes { .. }, Action::FindRoute(_)]
        ));
        assert!(matches!(&directions.from, Endpoint::Place(poi) if poi.name == "Châtelet"));
    }

    #[test]
    fn only_the_latest_answer_frames_the_map() {
        let mut directions =
            Directions::new(&place("Hôtel de Ville"), Some(here()), WalkingPace::Normal);
        let [_, Action::FindRoute(first)] = &directions.refresh()[..] else {
            panic!()
        };
        let first = first.id;
        let actions = directions.update(Message::Swap);
        let [_, Action::FindRoute(second)] = &actions[..] else {
            panic!()
        };

        let plan = Plan {
            direct: Some(itinerary()),
            ..Plan::default()
        };
        assert!(
            directions
                .update(Message::RouteFound(first, Ok(plan.clone())))
                .is_empty()
        );
        let actions = directions.update(Message::RouteFound(second.id, Ok(plan)));
        assert!(matches!(&actions[..], [Action::ShowRoutes { fit: true }]));
    }

    #[test]
    fn stale_answers_are_dropped() {
        let mut directions =
            Directions::new(&place("Hôtel de Ville"), Some(here()), WalkingPace::Normal);
        let first = directions.route_request().unwrap();
        select(&mut directions, TravelMode::Transit);
        let second = directions.route_request().unwrap();

        directions.insert_route(first.id, Ok(Plan::default()));
        assert!(matches!(directions.route, RouteState::Loading));

        directions.insert_route(second.id, Ok(Plan::default()));
        assert!(matches!(directions.route, RouteState::NotFound));
    }
}
