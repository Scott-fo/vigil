use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use tokio::task;

use crate::{
    event::Event,
    forge::{
        ForgeError, PullRequestList, PullRequestListFilter, PullRequestSummary, RepositoryRef,
        Snapshot, Timestamp,
    },
    ui::{self, PullRequestListTarget},
};

#[cfg(test)]
use super::gateway::ForgeCall;
use super::{
    super::{App, Screen, SnackbarVariant, input::is_plain_text_key},
    PullRequestEvent, PullRequestTimer,
    gateway::ForgeGateway,
    list::{LIST_POLL_INTERVAL, PULL_REQUEST_LIST_FILTERS, QueryInput, RowCurrency},
    saved::{Freshness, request_time},
    state::{ConnectReason, ForgeConnection, ReviewOrigin},
    task::spawn_ticker,
};

/// Why the list has no rows to show, or that it has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestListStatus<'a> {
    Connecting,
    /// GitHub is unreachable for this repository.
    Unavailable(Option<&'a ForgeError>),
    /// The first load of this tab is running.
    Loading,
    /// The last load of this tab failed and nothing was loaded before.
    Failed(&'a ForgeError),
    Ready,
}

/// Everything the pull request list screen draws.
#[derive(Debug)]
pub struct PullRequestListView<'a> {
    pub repository: Option<&'a RepositoryRef>,
    pub filter: PullRequestListFilter,
    /// Every tab with the match count of its last load.
    pub tabs: Vec<(PullRequestListFilter, Option<u64>)>,
    pub rows: Vec<&'a PullRequestSummary>,
    pub selected: usize,
    pub scroll: usize,
    pub status: PullRequestListStatus<'a>,
    /// A reload is running while older rows show.
    pub refreshing: bool,
    /// When GitHub reported the rows shown, if they are a snapshot saved by
    /// an earlier session that no load has confirmed yet.
    pub saved_at: Option<&'a Timestamp>,
    /// The last reload failed; the rows shown are older.
    pub stale_error: Option<&'a ForgeError>,
    /// Rows shown and the total GitHub matched, when capped.
    pub truncated: Option<(usize, u64)>,
    pub query: &'a str,
    pub query_input: QueryInput,
    /// The pull request being fetched to open.
    pub opening: Option<u64>,
}

impl App {
    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// The pull request list as the screen draws it.
    pub fn pull_request_list_view(&self) -> PullRequestListView<'_> {
        let list = self.pull_requests.list();
        let page = list.page();
        let status = match (self.pull_requests.connection(), page, list.error()) {
            (ForgeConnection::Idle | ForgeConnection::Connecting, _, _) => {
                PullRequestListStatus::Connecting
            }
            (ForgeConnection::Disabled, _, _) => PullRequestListStatus::Unavailable(None),
            (ForgeConnection::Unavailable(error), _, _) => {
                PullRequestListStatus::Unavailable(Some(error))
            }
            (ForgeConnection::Connected(_), Some(_), _) => PullRequestListStatus::Ready,
            (ForgeConnection::Connected(_), None, Some(error)) => {
                PullRequestListStatus::Failed(error)
            }
            (ForgeConnection::Connected(_), None, None) => PullRequestListStatus::Loading,
        };
        let rows = list.visible_rows();
        PullRequestListView {
            repository: self
                .pull_requests
                .github()
                .map(|github| github.repository()),
            filter: list.filter(),
            tabs: PULL_REQUEST_LIST_FILTERS
                .iter()
                .map(|filter| (*filter, list.total_count(*filter)))
                .collect(),
            selected: list.selected(),
            scroll: list.scroll(),
            status,
            refreshing: list.loading() && page.is_some(),
            // Only rows on screen have an age: when GitHub is unavailable
            // the list explains that instead of drawing its saved rows.
            saved_at: list
                .freshness()
                .and_then(|freshness| freshness.saved_at())
                .filter(|_| status == PullRequestListStatus::Ready),
            stale_error: page.and(list.error()),
            truncated: page
                .filter(|page| page.is_truncated())
                .map(|page| (page.pull_requests.len(), page.total_count)),
            query: list.query(),
            query_input: list.query_input(),
            opening: self.pull_requests.opening_number(),
            rows,
        }
    }

    /// Stores the list scroll after the renderer clamps it.
    pub fn set_pull_request_list_scroll(&mut self, scroll: usize) {
        self.pull_requests.list_mut().set_scroll(scroll);
    }

    /// Shows the pull request list (the `L` key), connecting to GitHub
    /// first if needed.
    pub(in crate::app) fn open_pull_request_list(&mut self) {
        self.screen = Screen::PullRequestList;
        self.clear_diff_text_selection();
        self.find_prefix_pending = false;
        if self.ensure_forge_connection(ConnectReason::UserRequest) {
            self.retry_forge_repository_confirm();
            self.load_pull_request_list();
        }
        let ticker = spawn_ticker(self.events.sender(), LIST_POLL_INTERVAL, || {
            Event::PullRequest(PullRequestEvent::Tick(PullRequestTimer::List))
        });
        self.pull_requests.list_mut().set_ticker(ticker);
    }

    /// Returns to the review screen and stops refreshing the list. A lookup
    /// started from the list is dropped: leaving means the user no longer
    /// wants it opened.
    pub(in crate::app) fn close_pull_request_list(&mut self) {
        self.screen = Screen::Review;
        self.pull_requests.list_mut().stop();
        if self.pull_requests.cancel_lookup() {
            self.status_message = Some(self.current_status_message());
        }
    }

    /// Loads the shown tab from GitHub, superseding a running load. A tab
    /// with nothing to show yet shows its saved page, if any, meanwhile.
    pub(in crate::app) fn load_pull_request_list(&mut self) {
        let Some(github) = self.pull_requests.github().cloned() else {
            return;
        };
        let (request_id, filter) = self.pull_requests.list_mut().begin_load(Instant::now());
        let sender = self.events.sender();
        let handle = task::spawn(async move {
            let result = github.list_pull_requests(filter).await;
            let _ = sender.send(Event::PullRequest(PullRequestEvent::ListLoaded {
                request_id,
                filter,
                result,
            }));
        });
        self.pull_requests
            .list_mut()
            .attach_load(request_id, handle);
        self.read_saved_pull_request_list(filter);
    }

    /// Follow-up once GitHub connects: the list loads if it is waiting.
    pub(super) fn load_pull_request_list_if_shown(&mut self) {
        if self.screen == Screen::PullRequestList {
            self.load_pull_request_list();
        }
    }

    pub(super) fn handle_pull_request_list_loaded(
        &mut self,
        request_id: u64,
        filter: PullRequestListFilter,
        result: Result<PullRequestList, ForgeError>,
    ) -> bool {
        let live = result.as_ref().ok().cloned();
        let list = self.pull_requests.list_mut();
        if !list.finish_load(request_id, filter, result) {
            return false;
        }
        // A live page was just accepted: prefetch its likeliest opens.
        if let Some(page) = &live {
            self.prefetch_listed_pull_requests(page);
        }
        let list = self.pull_requests.list_mut();
        if let (Some(page), Some(Freshness::Live { requested_at })) =
            (live, list.page_freshness(filter))
        {
            let snapshot = Snapshot::new(page, request_time(*requested_at));
            self.save_pull_request_list(filter, snapshot);
        }
        self.screen == Screen::PullRequestList
    }

    pub(super) fn handle_pull_request_list_tick(&mut self) -> bool {
        if self.screen == Screen::PullRequestList && !self.pull_requests.list().loading() {
            self.retry_forge_repository_confirm();
            self.load_pull_request_list();
            return true;
        }
        false
    }

    pub(in crate::app) fn set_pull_request_list_filter(&mut self, filter: PullRequestListFilter) {
        if self.pull_requests.list_mut().set_filter(filter) {
            self.load_pull_request_list();
        }
    }

    fn cycle_pull_request_list_filter(&mut self, delta: i32) {
        if self.pull_requests.list_mut().cycle_filter(delta) {
            self.load_pull_request_list();
        }
    }

    /// Opens the selected row, or the pull request the query names by
    /// number when no row matches. An outdated row (see [`RowCurrency`]) is
    /// looked up first, so the review opens on the live head and base.
    pub(in crate::app) fn open_selected_pull_request(&mut self) {
        if let Some((summary, currency)) = self.pull_requests.selected_list_row(Instant::now()) {
            match currency {
                RowCurrency::Current => {
                    let summary = summary.clone();
                    self.open_pull_request(summary, ReviewOrigin::PullRequestList);
                }
                RowCurrency::Outdated => {
                    let number = summary.number;
                    self.look_up_pull_request(number);
                }
            }
            return;
        }
        if let Some(number) = self.pull_requests.list().query_number() {
            self.look_up_pull_request(number);
        }
    }

    /// Loads pull request `number`'s summary, then opens it. Reaches pull
    /// requests the list does not show, such as closed or merged ones.
    fn look_up_pull_request(&mut self, number: u64) {
        if !self.request_forge_connection() {
            return;
        }
        let Some(github) = self.pull_requests.github().cloned() else {
            return;
        };
        let request_id = self.pull_requests.begin_lookup(number);
        self.status_message = Some(format!("looking up pull request #{number}…"));
        match self.pull_requests.gateway_mut() {
            ForgeGateway::Live => {}
            #[cfg(test)]
            ForgeGateway::Recording(log) => {
                log.push(ForgeCall::LookUp(number));
                return;
            }
        }
        let sender = self.events.sender();
        let handle = task::spawn(async move {
            let result = github.load_summary(number).await;
            let _ = sender.send(Event::PullRequest(PullRequestEvent::LookedUp {
                request_id,
                result,
            }));
        });
        self.pull_requests.attach_lookup(request_id, handle);
    }

    pub(super) fn handle_pull_request_looked_up(
        &mut self,
        request_id: u64,
        result: Result<PullRequestSummary, ForgeError>,
    ) -> bool {
        if !self.pull_requests.finish_lookup(request_id) {
            return false;
        }
        self.status_message = Some(self.current_status_message());
        match result {
            Ok(summary) => self.open_pull_request(summary, ReviewOrigin::PullRequestList),
            Err(error) => {
                self.show_snackbar(
                    format!("could not find that pull request: {error}"),
                    SnackbarVariant::Error,
                );
            }
        }
        true
    }

    /// Keys on the pull request list screen. Every key is consumed so
    /// review-screen shortcuts never act on a screen that is not visible.
    pub(in crate::app) fn handle_pull_request_list_key(&mut self, key_event: KeyEvent) {
        if self.pull_requests.list().query_input() == QueryInput::Editing {
            self.handle_pull_request_list_query_key(key_event);
            return;
        }
        let control = key_event.modifiers.contains(KeyModifiers::CONTROL);
        let list = self.pull_requests.list_mut();
        match key_event.code {
            KeyCode::Char('c') if control => self.quit(),
            KeyCode::Char('d') if control => list.move_selection(10),
            KeyCode::Char('u') if control => list.move_selection(-10),
            KeyCode::Char('q') => self.quit(),
            KeyCode::Char('?') => self.help_modal_open = true,
            KeyCode::Esc => {
                if list.query().is_empty() {
                    self.close_pull_request_list();
                } else {
                    list.clear_query();
                }
            }
            KeyCode::Char('L') => self.close_pull_request_list(),
            KeyCode::Down | KeyCode::Char('j') => list.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => list.move_selection(-1),
            KeyCode::PageDown => list.move_selection(10),
            KeyCode::PageUp => list.move_selection(-10),
            KeyCode::Home | KeyCode::Char('g') => list.select(0),
            KeyCode::End | KeyCode::Char('G') => list.select_last(),
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                self.cycle_pull_request_list_filter(1)
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                self.cycle_pull_request_list_filter(-1)
            }
            KeyCode::Char(digit @ '1'..='3') => {
                let index = digit as usize - '1' as usize;
                self.set_pull_request_list_filter(PULL_REQUEST_LIST_FILTERS[index]);
            }
            KeyCode::Char('/') => list.start_query(),
            KeyCode::Char('r') => {
                if self.ensure_forge_connection(ConnectReason::UserRequest) {
                    self.load_pull_request_list();
                }
            }
            KeyCode::Char('t') => self.open_theme_modal(),
            KeyCode::Enter => self.open_selected_pull_request(),
            KeyCode::Char('y') => self.copy_pull_request_branch(),
            _ => {}
        }
    }

    fn handle_pull_request_list_query_key(&mut self, key_event: KeyEvent) {
        let list = self.pull_requests.list_mut();
        match key_event.code {
            KeyCode::Esc => list.clear_query(),
            KeyCode::Enter => {
                list.finish_query();
                self.open_selected_pull_request();
            }
            KeyCode::Down => list.move_selection(1),
            KeyCode::Up => list.move_selection(-1),
            KeyCode::Backspace => list.pop_query(),
            KeyCode::Char(ch) if is_plain_text_key(key_event) => list.push_query(ch),
            _ => {}
        }
    }

    /// Mouse input on the pull request list: click a tab to switch, click a
    /// row to select it and again to open it, and wheel to move through
    /// rows. Returns whether anything visible changed.
    pub(in crate::app) fn handle_pull_request_list_mouse(
        &mut self,
        mouse_event: MouseEvent,
        terminal_width: u16,
        terminal_height: u16,
    ) -> bool {
        let position = Position::new(mouse_event.column, mouse_event.row);
        match mouse_event.kind {
            MouseEventKind::Moved => {
                return self.set_mouse_position(position, terminal_width, terminal_height);
            }
            // The wheel moves the selection; the list scrolls to follow it.
            MouseEventKind::ScrollDown => self.pull_requests.list_mut().move_selection(3),
            MouseEventKind::ScrollUp => self.pull_requests.list_mut().move_selection(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                self.mouse_position = Some(position);
                match ui::pull_request_list_target_at(
                    self,
                    mouse_event.column,
                    mouse_event.row,
                    terminal_width,
                    terminal_height,
                ) {
                    Some(PullRequestListTarget::Tab(filter)) => {
                        self.set_pull_request_list_filter(filter)
                    }
                    Some(PullRequestListTarget::Row(index)) => {
                        let list = self.pull_requests.list_mut();
                        if list.selected() == index {
                            self.open_selected_pull_request();
                        } else {
                            list.select(index);
                        }
                    }
                    None => return false,
                }
            }
            _ => return false,
        }
        true
    }
}
