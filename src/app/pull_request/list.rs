use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use nucleo_matcher::{
    Config as MatcherConfig, Matcher,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::forge::{
    ForgeError, PullRequestList, PullRequestListFilter, PullRequestSummary, Snapshot,
};

use super::{
    super::navigation::{clamp_index, move_index},
    saved::Freshness,
    task::{OwnedTask, RequestSlot},
};

/// How often the list reloads while it is on screen; a live page older
/// than this may be out of date.
pub(super) const LIST_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Whether a list row names its pull request's current head and base
/// closely enough to open from.
///
/// Opening trusts a summary's commits when they are local, so an outdated
/// row would review the commits it names, not the pull request's current
/// ones, and diff them against a base the head may have been rebased off.
/// An outdated row is looked up first and opened from the live summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum RowCurrency {
    /// From a list load that started within the last list refresh, and not
    /// behind what this session's review of the pull request knows.
    Current,
    /// From a saved page, a load older than a list refresh or followed by a
    /// failed reload, or behind the head this session's review knows.
    Outdated,
}

/// The tabs of the pull request list, in display order.
pub const PULL_REQUEST_LIST_FILTERS: [PullRequestListFilter; 3] = [
    PullRequestListFilter::NeedsMyReview,
    PullRequestListFilter::Mine,
    PullRequestListFilter::AllOpen,
];

/// Whether typed characters go to the list's filter query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryInput {
    #[default]
    Off,
    /// `/` was pressed; characters edit the query until Enter or Esc.
    Editing,
}

/// A tab's rows and how current they are.
#[derive(Debug)]
struct LoadedPage {
    list: PullRequestList,
    freshness: Freshness,
}

/// State behind the pull request list screen. Loaded pages are kept per
/// filter so switching tabs shows the last page at once while it reloads;
/// a tab with nothing loaded yet can show a saved page until its first
/// load lands.
#[derive(Debug)]
pub(in crate::app) struct PullRequestListState {
    filter: PullRequestListFilter,
    loaded: HashMap<PullRequestListFilter, LoadedPage>,
    error: Option<(PullRequestListFilter, ForgeError)>,
    request: RequestSlot,
    requested_filter: Option<PullRequestListFilter>,
    /// When the running or last load started.
    requested_at: Option<Instant>,
    /// Indices into the current filter's page that match the query.
    visible: Vec<usize>,
    selected: usize,
    scroll: usize,
    query: String,
    query_input: QueryInput,
    matcher: Matcher,
    ticker: Option<OwnedTask>,
}

impl Default for PullRequestListState {
    fn default() -> Self {
        Self {
            filter: PullRequestListFilter::NeedsMyReview,
            loaded: HashMap::new(),
            error: None,
            request: RequestSlot::default(),
            requested_filter: None,
            requested_at: None,
            visible: Vec::new(),
            selected: 0,
            scroll: 0,
            query: String::new(),
            query_input: QueryInput::Off,
            matcher: Matcher::new(MatcherConfig::DEFAULT),
            ticker: None,
        }
    }
}

struct Candidate {
    index: usize,
    haystack: String,
}

impl AsRef<str> for Candidate {
    fn as_ref(&self) -> &str {
        &self.haystack
    }
}

impl PullRequestListState {
    pub(in crate::app) fn filter(&self) -> PullRequestListFilter {
        self.filter
    }

    /// Switches tabs. Returns whether the tab changed, which calls for a
    /// reload.
    pub(in crate::app) fn set_filter(&mut self, filter: PullRequestListFilter) -> bool {
        if filter == self.filter {
            return false;
        }
        self.filter = filter;
        self.selected = 0;
        self.scroll = 0;
        self.refilter();
        true
    }

    pub(in crate::app) fn cycle_filter(&mut self, delta: i32) -> bool {
        let position = PULL_REQUEST_LIST_FILTERS
            .iter()
            .position(|filter| *filter == self.filter)
            .unwrap_or(0) as i32;
        let count = PULL_REQUEST_LIST_FILTERS.len() as i32;
        let next = (position + delta).rem_euclid(count) as usize;
        self.set_filter(PULL_REQUEST_LIST_FILTERS[next])
    }

    /// Starts loading the shown tab at `now`, which dates its answer.
    pub(in crate::app) fn begin_load(&mut self, now: Instant) -> (u64, PullRequestListFilter) {
        self.requested_filter = Some(self.filter);
        self.requested_at = Some(now);
        (self.request.begin(), self.filter)
    }

    pub(in crate::app) fn attach_load(&mut self, id: u64, handle: tokio::task::JoinHandle<()>) {
        self.request.attach(id, handle);
    }

    pub(in crate::app) fn loading(&self) -> bool {
        self.request.in_flight()
    }

    /// Stores a loaded page. Returns false for stale responses: a newer load,
    /// or a load for a tab that is no longer shown.
    pub(in crate::app) fn finish_load(
        &mut self,
        id: u64,
        filter: PullRequestListFilter,
        result: Result<PullRequestList, ForgeError>,
    ) -> bool {
        if !self.request.complete(id) || self.requested_filter.take() != Some(filter) {
            return false;
        }
        let selected_number = self.selected_summary().map(|summary| summary.number);
        match result {
            Ok(list) => {
                let requested_at = self.requested_at.unwrap_or_else(Instant::now);
                self.loaded.insert(
                    filter,
                    LoadedPage {
                        list,
                        freshness: Freshness::Live { requested_at },
                    },
                );
                if self
                    .error
                    .as_ref()
                    .is_some_and(|(failed, _)| *failed == filter)
                {
                    self.error = None;
                }
            }
            Err(error) => self.error = Some((filter, error)),
        }
        self.refilter();
        if let Some(number) = selected_number
            && let Some(position) = self.position_of(number)
        {
            self.selected = position;
        }
        true
    }

    /// Shows a saved page for `filter` if the tab has nothing yet. Returns
    /// whether the page was taken; it never replaces a loaded page, so a
    /// saved page that arrives after the live one is dropped.
    pub(in crate::app) fn show_saved(
        &mut self,
        filter: PullRequestListFilter,
        snapshot: Snapshot<PullRequestList>,
    ) -> bool {
        if self.loaded.contains_key(&filter) {
            return false;
        }
        self.loaded.insert(
            filter,
            LoadedPage {
                list: snapshot.value,
                freshness: Freshness::Saved {
                    fetched_at: snapshot.fetched_at,
                },
            },
        );
        if filter == self.filter {
            self.refilter();
        }
        true
    }

    /// Whether `filter`'s tab has rows to show, live or saved.
    pub(in crate::app) fn has_page(&self, filter: PullRequestListFilter) -> bool {
        self.loaded.contains_key(&filter)
    }

    /// Stops a running load and periodic refresh, as when the screen closes.
    pub(in crate::app) fn stop(&mut self) {
        self.request.cancel();
        self.requested_filter = None;
        self.ticker = None;
    }

    pub(in crate::app) fn set_ticker(&mut self, ticker: OwnedTask) {
        self.ticker = Some(ticker);
    }

    pub(in crate::app) fn page(&self) -> Option<&PullRequestList> {
        self.loaded.get(&self.filter).map(|page| &page.list)
    }

    /// How current the shown tab's rows are; `None` when it has none.
    pub(in crate::app) fn freshness(&self) -> Option<&Freshness> {
        self.page_freshness(self.filter)
    }

    /// How current `filter`'s rows are; `None` when it has none.
    pub(in crate::app) fn page_freshness(
        &self,
        filter: PullRequestListFilter,
    ) -> Option<&Freshness> {
        self.loaded.get(&filter).map(|page| &page.freshness)
    }

    pub(in crate::app) fn error(&self) -> Option<&ForgeError> {
        self.error
            .as_ref()
            .filter(|(filter, _)| *filter == self.filter)
            .map(|(_, error)| error)
    }

    /// Total count GitHub reported for `filter`'s last load.
    pub(in crate::app) fn total_count(&self, filter: PullRequestListFilter) -> Option<u64> {
        self.loaded.get(&filter).map(|page| page.list.total_count)
    }

    pub(in crate::app) fn visible_rows(&self) -> Vec<&PullRequestSummary> {
        let Some(page) = self.page() else {
            return Vec::new();
        };
        self.visible
            .iter()
            .filter_map(|index| page.pull_requests.get(*index))
            .collect()
    }

    pub(in crate::app) fn selected(&self) -> usize {
        self.selected
    }

    pub(in crate::app) fn selected_summary(&self) -> Option<&PullRequestSummary> {
        let index = *self.visible.get(self.selected)?;
        self.page()?.pull_requests.get(index)
    }

    pub(in crate::app) fn move_selection(&mut self, delta: i32) {
        self.selected = move_index(self.selected, self.visible.len(), delta);
    }

    pub(in crate::app) fn select(&mut self, index: usize) {
        self.selected = clamp_index(index, self.visible.len());
    }

    pub(in crate::app) fn select_last(&mut self) {
        self.selected = self.visible.len().saturating_sub(1);
    }

    pub(in crate::app) fn scroll(&self) -> usize {
        self.scroll
    }

    pub(in crate::app) fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll;
    }

    pub(in crate::app) fn query(&self) -> &str {
        &self.query
    }

    pub(in crate::app) fn query_input(&self) -> QueryInput {
        self.query_input
    }

    pub(in crate::app) fn start_query(&mut self) {
        self.query_input = QueryInput::Editing;
    }

    /// Stops editing, keeping the query applied.
    pub(in crate::app) fn finish_query(&mut self) {
        self.query_input = QueryInput::Off;
    }

    /// Stops editing and drops the query.
    pub(in crate::app) fn clear_query(&mut self) {
        self.query_input = QueryInput::Off;
        if !self.query.is_empty() {
            self.query.clear();
            self.selected = 0;
            self.refilter();
        }
    }

    pub(in crate::app) fn push_query(&mut self, ch: char) {
        self.query.push(ch);
        self.selected = 0;
        self.scroll = 0;
        self.refilter();
    }

    pub(in crate::app) fn pop_query(&mut self) {
        if self.query.pop().is_some() {
            self.selected = 0;
            self.refilter();
        }
    }

    /// The pull request number the query names, as in `#17` or `17`.
    pub(in crate::app) fn query_number(&self) -> Option<u64> {
        let query = self.query.trim();
        query.strip_prefix('#').unwrap_or(query).parse().ok()
    }

    fn position_of(&self, number: u64) -> Option<usize> {
        let page = self.page()?;
        self.visible.iter().position(|index| {
            page.pull_requests
                .get(*index)
                .is_some_and(|summary| summary.number == number)
        })
    }

    fn refilter(&mut self) {
        let Some(page) = self.loaded.get(&self.filter).map(|page| &page.list) else {
            self.visible.clear();
            self.selected = 0;
            return;
        };
        let query = self.query.trim();
        self.visible = if query.is_empty() {
            (0..page.pull_requests.len()).collect()
        } else {
            let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
            let candidates = page
                .pull_requests
                .iter()
                .enumerate()
                .map(|(index, summary)| Candidate {
                    index,
                    haystack: format!(
                        "#{} {} {} {}",
                        summary.number, summary.title, summary.author, summary.head_ref_name
                    ),
                })
                .collect::<Vec<_>>();
            let mut matches = pattern.match_list(candidates, &mut self.matcher);
            // Keep GitHub's recency order among matches.
            matches.sort_by_key(|(candidate, _)| candidate.index);
            matches
                .into_iter()
                .map(|(candidate, _)| candidate.index)
                .collect()
        };
        self.selected = clamp_index(self.selected, self.visible.len());
    }
}
