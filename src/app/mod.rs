use std::{
    collections::HashSet,
    io::{Write, stdout},
    path::PathBuf,
    process::Stdio,
};

use color_eyre::eyre::WrapErr;
use crossterm::terminal;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers, MouseEvent,
        MouseEventKind,
    },
    execute,
};
use nucleo_matcher::{
    Config as MatcherConfig, Matcher,
    pattern::{CaseMatching, Normalization, Pattern},
};
use ratatui::widgets::ListState;
use tokio::fs;
use tokio::task;

mod branch_compare;
mod commit_search;
mod diff;

use crate::theme::config;
use crate::ui::splash;
use crate::{
    event::{DiffPrefetchedEvent, Event, EventHandler},
    git::{
        self, BlameCommitDetails, BlameTarget, BranchCompareSelection, CommitCompareSelection,
        CommitSearchEntry, DiffSelectionPoint, DiffView, FileEntry, SharedHighlightRegistry,
    },
    sidebar::{self, SidebarItem},
    theme::{self, ThemeMode},
    ui,
    watcher::RepoWatcher,
};
pub use self::diff::{DiffCacheKey, PreparedDiffViewport};
use self::diff::{DiffHighlightJob, DiffViewCache, DiffViewport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Sidebar,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffViewMode {
    Unified,
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchCompareField {
    Source,
    Destination,
}

#[derive(Debug, Clone)]
pub enum ReviewMode {
    WorkingTree,
    CommitCompare(CommitCompareSelection),
    BranchCompare(BranchCompareSelection),
}

#[derive(Debug, Default, Clone)]
pub struct AppLaunchOptions {
    pub repo_root: Option<PathBuf>,
    pub initial_blame_target: Option<BlameTarget>,
    pub chooser_file: Option<PathBuf>,
}

enum AppCommand {
    OpenFileInEditor(String),
    OpenFileInEditorAtLine(String, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSyncDirection {
    Pull,
    Push,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnackbarVariant {
    Info,
    Error,
}

#[derive(Debug, Clone)]
pub struct SnackbarNotice {
    pub message: String,
    pub variant: SnackbarVariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffTextSelection {
    pub anchor: DiffSelectionPoint,
    pub head: DiffSelectionPoint,
}

#[derive(Debug)]
pub struct App {
    pub running: bool,
    pub repo_root: PathBuf,
    pub chooser_file_path: Option<PathBuf>,
    pub repo_error: Option<String>,
    pub events: EventHandler,
    pub active_pane: ActivePane,
    pub review_mode: ReviewMode,
    pub files: Vec<FileEntry>,
    pub sidebar_items: Vec<SidebarItem>,
    pub collapsed_directories: HashSet<String>,
    pub sidebar_state: ListState,
    pub selected_file_index: usize,
    pub diff_view: DiffView,
    pub diff_view_mode: DiffViewMode,
    pub diff_scroll: u16,
    pub selected_diff_line_index: usize,
    pub diff_text_selection: Option<DiffTextSelection>,
    diff_text_selection_anchor: Option<DiffSelectionPoint>,
    pub diff_request_id: u64,
    diff_load_task: Option<task::JoinHandle<()>>,
    diff_highlight_task: Option<task::JoinHandle<()>>,
    diff_highlight_job: Option<DiffHighlightJob>,
    diff_highlight_complete: bool,
    diff_viewport: Option<DiffViewport>,
    background_tasks: Vec<task::JoinHandle<()>>,
    diff_view_cache: DiffViewCache,
    diff_cache_generation: u64,
    pending_diff_cache_key: Option<DiffCacheKey>,
    pub highlight_registry: Option<SharedHighlightRegistry>,
    pub repo_watcher: Option<RepoWatcher>,
    pub repo_watcher_loading: bool,
    pub blame_modal_open: bool,
    pub blame_target: Option<BlameTarget>,
    pub blame_loading: bool,
    pub blame_details: Option<BlameCommitDetails>,
    pub blame_error: Option<String>,
    pub blame_scroll: u16,
    pub blame_request_id: u64,
    blame_load_task: Option<task::JoinHandle<()>>,
    pub help_modal_open: bool,
    pub theme_modal_open: bool,
    pub theme_modal_query: String,
    pub theme_modal_selected_index: usize,
    pub theme_modal_initial_name: String,
    pub theme_modal_initial_mode: ThemeMode,
    pub theme_name: String,
    pub theme_mode: ThemeMode,
    pub theme_matcher: Matcher,
    pub commit_search_modal_open: bool,
    pub commit_search_query: String,
    pub commit_search_entries: Vec<CommitSearchEntry>,
    pub commit_search_loading: bool,
    pub commit_search_error: Option<String>,
    pub commit_search_selected_index: usize,
    pub commit_search_matcher: Matcher,
    pub branch_compare_modal_open: bool,
    pub branch_compare_loading: bool,
    pub branch_compare_error: Option<String>,
    pub branch_compare_active_field: BranchCompareField,
    pub branch_compare_available_refs: Vec<String>,
    pub branch_compare_source_query: String,
    pub branch_compare_destination_query: String,
    pub branch_compare_source_ref: Option<String>,
    pub branch_compare_destination_ref: Option<String>,
    pub branch_compare_selected_source_index: usize,
    pub branch_compare_selected_destination_index: usize,
    pub branch_compare_matcher: Matcher,
    pub commit_modal_open: bool,
    pub commit_message: String,
    pub commit_error: Option<String>,
    pub discard_target: Option<FileEntry>,
    pub remote_sync: Option<RemoteSyncDirection>,
    pub snackbar_notice: Option<SnackbarNotice>,
    pub snackbar_generation: u64,
    pub status_message: Option<String>,
}

impl App {
    fn build_base_app(
        repo_root: PathBuf,
        chooser_file_path: Option<PathBuf>,
        events: EventHandler,
        theme_name: String,
        theme_mode: ThemeMode,
    ) -> Self {
        Self {
            running: true,
            repo_root,
            chooser_file_path,
            repo_error: None,
            events,
            active_pane: ActivePane::Sidebar,
            review_mode: ReviewMode::WorkingTree,
            files: Vec::new(),
            sidebar_items: Vec::new(),
            collapsed_directories: HashSet::new(),
            sidebar_state: ListState::default(),
            selected_file_index: 0,
            diff_view: DiffView::default(),
            diff_view_mode: DiffViewMode::Split,
            diff_scroll: 0,
            selected_diff_line_index: 0,
            diff_text_selection: None,
            diff_text_selection_anchor: None,
            diff_request_id: 0,
            diff_load_task: None,
            diff_highlight_task: None,
            diff_highlight_job: None,
            diff_highlight_complete: false,
            diff_viewport: None,
            background_tasks: Vec::new(),
            diff_view_cache: DiffViewCache::default(),
            diff_cache_generation: 0,
            pending_diff_cache_key: None,
            highlight_registry: None,
            repo_watcher: None,
            repo_watcher_loading: false,
            blame_modal_open: false,
            blame_target: None,
            blame_loading: false,
            blame_details: None,
            blame_error: None,
            blame_scroll: 0,
            blame_request_id: 0,
            blame_load_task: None,
            help_modal_open: false,
            theme_modal_open: false,
            theme_modal_query: String::new(),
            theme_modal_selected_index: 0,
            theme_modal_initial_name: theme_name.clone(),
            theme_modal_initial_mode: theme_mode,
            theme_name,
            theme_mode,
            theme_matcher: Matcher::new(MatcherConfig::DEFAULT),
            commit_search_modal_open: false,
            commit_search_query: String::new(),
            commit_search_entries: Vec::new(),
            commit_search_loading: false,
            commit_search_error: None,
            commit_search_selected_index: 0,
            commit_search_matcher: Matcher::new(MatcherConfig::DEFAULT),
            branch_compare_modal_open: false,
            branch_compare_loading: false,
            branch_compare_error: None,
            branch_compare_active_field: BranchCompareField::Source,
            branch_compare_available_refs: Vec::new(),
            branch_compare_source_query: String::new(),
            branch_compare_destination_query: String::new(),
            branch_compare_source_ref: None,
            branch_compare_destination_ref: None,
            branch_compare_selected_source_index: 0,
            branch_compare_selected_destination_index: 0,
            branch_compare_matcher: Matcher::new(MatcherConfig::DEFAULT),
            commit_modal_open: false,
            commit_message: String::new(),
            commit_error: None,
            discard_target: None,
            remote_sync: None,
            snackbar_notice: None,
            snackbar_generation: 0,
            status_message: None,
        }
    }

    pub async fn new(options: AppLaunchOptions) -> color_eyre::Result<Self> {
        let repo_root = match options.repo_root {
            Some(path) => path,
            None => std::env::current_dir().wrap_err("failed to resolve current directory")?,
        };
        let preference = config::read_theme_preference();
        let theme_name = theme::resolve_theme_name(preference.theme.as_deref()).to_string();
        let theme_mode = preference.mode.unwrap_or(ThemeMode::Dark);
        theme::set_active_theme(&theme_name, theme_mode);
        let mut app = Self::build_base_app(
            repo_root,
            options.chooser_file,
            EventHandler::new(),
            theme_name.clone(),
            theme_mode,
        );
        app.refresh().await?;
        app.spawn_highlight_registry_init();
        if let Some(target) = options.initial_blame_target {
            app.open_blame_target(target);
        }
        Ok(app)
    }

    #[doc(hidden)]
    pub fn new_for_benchmarks(repo_root: PathBuf) -> Self {
        let theme_name = theme::resolve_theme_name(None).to_string();
        let theme_mode = ThemeMode::Dark;
        theme::set_active_theme(&theme_name, theme_mode);
        Self::build_base_app(
            repo_root,
            None,
            EventHandler::without_event_task(),
            theme_name,
            theme_mode,
        )
    }

    fn redraw(&mut self, terminal: &mut ratatui::DefaultTerminal) -> color_eyre::Result<()> {
        if let Ok((width, height)) = terminal::size()
            && let Some(viewport) = ui::prepare_diff_viewport_for_terminal(self, width, height)
        {
            self.update_diff_viewport(viewport.mode, viewport.width, viewport.start, viewport.end);
            self.maybe_queue_diff_highlight();
        }
        terminal.draw(|frame| ui::render(frame, self))?;
        self.maybe_queue_diff_highlight();
        Ok(())
    }

    pub async fn run(mut self, mut terminal: ratatui::DefaultTerminal) -> color_eyre::Result<()> {
        self.redraw(&mut terminal)?;

        while self.running {
            match self.events.next().await? {
                Event::Crossterm(event) => {
                    let should_redraw = match event {
                        crossterm::event::Event::Key(key_event)
                            if key_event.kind == crossterm::event::KeyEventKind::Press =>
                        {
                            if let Some(command) = self.handle_key_event(key_event).await? {
                                self.run_command(command, &mut terminal).await?;
                            }
                            true
                        }
                        crossterm::event::Event::Mouse(mouse_event) => {
                            self.handle_mouse_event(mouse_event).await?;
                            true
                        }
                        crossterm::event::Event::Resize(_, _) => true,
                        _ => false,
                    };

                    if self.running && should_redraw {
                        self.redraw(&mut terminal)?;
                    }
                }
                Event::HighlightRegistryReady(result) => {
                    match result {
                        Ok(registry) => {
                            self.highlight_registry = Some(registry);
                            self.spawn_highlight_prewarm();
                            self.queue_selected_diff_load(false, false);
                            self.status_message = Some(self.current_status_message());
                        }
                        Err(error) => {
                            self.status_message =
                                Some(format!("highlight registry init failed: {error}"));
                        }
                    }

                    if self.running {
                        self.redraw(&mut terminal)?;
                    }
                }
                Event::DiffLoaded { request_id, result } => {
                    if request_id == self.diff_request_id {
                        self.diff_load_task = None;
                        match result {
                            Ok(mut diff_view) => {
                                if let Some(cache_key) = self.pending_diff_cache_key.clone() {
                                    self.diff_view_cache
                                        .insert_plain(cache_key, diff_view.clone());
                                }
                                let max_index = diff_view.last_selectable_index(
                                    self.diff_view_mode,
                                    self.current_diff_display_width(),
                                );
                                self.selected_diff_line_index =
                                    self.selected_diff_line_index.min(max_index);
                                self.diff_view = diff_view;
                                self.diff_highlight_complete = self.highlight_registry.is_none();
                                self.status_message = Some(self.current_status_message());
                            }
                            Err(error) => {
                                self.diff_view = DiffView::empty(error);
                                self.diff_highlight_complete = true;
                            }
                        }

                        if self.running {
                            self.redraw(&mut terminal)?;
                        }
                    }
                }
                Event::DiffHighlightUpdated {
                    request_id,
                    complete,
                    result,
                } => {
                    if request_id == self.diff_request_id {
                        self.diff_highlight_task = None;
                        self.diff_highlight_job = None;

                        match result {
                            Ok(diff_view) => {
                                if let Some(cache_key) = self.pending_diff_cache_key.clone() {
                                    self.diff_view_cache.insert_highlighted(
                                        cache_key,
                                        diff_view.clone(),
                                        complete,
                                    );
                                }
                                if complete {
                                    self.diff_view = diff_view;
                                    self.diff_highlight_complete = true;
                                } else {
                                    self.diff_view.merge_highlighting_from(&diff_view);
                                }
                                self.status_message = Some(self.current_status_message());
                            }
                            Err(error) => {
                                self.status_message =
                                    Some(format!("syntax highlight failed: {error}"));
                            }
                        }

                        if self.running {
                            self.redraw(&mut terminal)?;
                        }
                    }
                }
                Event::DiffPrefetched(prefetched) => {
                    let DiffPrefetchedEvent {
                        generation,
                        key,
                        plain,
                        highlighted,
                    } = *prefetched;
                    if generation == self.diff_cache_generation {
                        self.diff_view_cache.insert_plain(key.clone(), plain);
                        if let Some(highlighted_view) = highlighted {
                            self.diff_view_cache
                                .insert_highlighted(key, highlighted_view, false);
                        }
                    }
                }
                Event::BlameLoaded { request_id, result } => {
                    if request_id == self.blame_request_id && self.blame_modal_open {
                        self.blame_loading = false;
                        self.blame_load_task = None;

                        match result {
                            Ok(details) => {
                                self.blame_target = Some(details.target.clone());
                                self.blame_details = Some(details);
                                self.blame_error = None;
                            }
                            Err(error) => {
                                self.blame_details = None;
                                self.blame_error = Some(error);
                            }
                        }

                        if self.running {
                            self.redraw(&mut terminal)?;
                        }
                    }
                }
                Event::CommitSearchLoaded(result) => {
                    self.handle_commit_search_loaded(result);

                    if self.running && self.commit_search_modal_open {
                        self.redraw(&mut terminal)?;
                    }
                }
                Event::BranchCompareLoaded(result) => {
                    self.handle_branch_compare_loaded(result);

                    if self.running && self.branch_compare_modal_open {
                        self.redraw(&mut terminal)?;
                    }
                }
                Event::RepoWatcherReady(repo_root, result) => {
                    if repo_root == self.repo_root {
                        self.repo_watcher_loading = false;
                        match result {
                            Ok(watcher) => {
                                self.repo_watcher = Some(watcher);
                            }
                            Err(error) => {
                                self.repo_watcher = None;
                                self.status_message = Some(format!("watcher unavailable: {error}"));
                            }
                        }

                        if self.running {
                            terminal.draw(|frame| ui::render(frame, &mut self))?;
                        }
                    }
                }
                Event::RepoChanged(paths) => {
                    if self.is_working_tree_mode()
                        && git::should_refresh_for_paths(&self.repo_root, &paths).await?
                    {
                        self.refresh().await?;
                        if self.should_restart_watcher_for_paths(&paths).await {
                            self.restart_repo_watcher();
                        }
                        if self.running {
                            self.redraw(&mut terminal)?;
                        }
                    }
                }
                Event::RemoteSyncFinished(result) => {
                    self.remote_sync = None;
                    match result {
                        Ok(message) => {
                            self.show_snackbar(message, SnackbarVariant::Info);
                        }
                        Err(message) => {
                            self.show_snackbar(message, SnackbarVariant::Error);
                        }
                    }

                    if self.running {
                        self.redraw(&mut terminal)?;
                    }
                }
                Event::ClearSnackbar(generation) => {
                    if self.snackbar_generation == generation {
                        self.snackbar_notice = None;
                        if self.running {
                            self.redraw(&mut terminal)?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<Option<AppCommand>> {
        if self.blame_modal_open {
            match key_event.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.close_blame_modal();
                }
                KeyCode::Enter | KeyCode::Char('o') => {
                    self.open_blame_commit_compare().await?;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.scroll_blame(3);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.scroll_blame(-3);
                }
                KeyCode::PageDown => {
                    self.scroll_blame(10);
                }
                KeyCode::PageUp => {
                    self.scroll_blame(-10);
                }
                _ => {}
            }
            return Ok(None);
        }

        if self.help_modal_open {
            match key_event.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter | KeyCode::Char('q') => {
                    self.help_modal_open = false;
                }
                _ => {}
            }
            return Ok(None);
        }

        if self.theme_modal_open {
            match key_event.code {
                KeyCode::Esc => {
                    self.cancel_theme_modal().await?;
                }
                KeyCode::Enter => {
                    self.confirm_theme_modal()?;
                }
                KeyCode::Char('m') => {
                    self.toggle_theme_mode_preview().await?;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.move_theme_selection(1).await?;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.move_theme_selection(-1).await?;
                }
                KeyCode::Backspace => {
                    self.theme_modal_query.pop();
                    self.sync_theme_selection_after_query_change().await?;
                }
                KeyCode::Char(ch)
                    if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && !key_event.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.theme_modal_query.push(ch);
                    self.sync_theme_selection_after_query_change().await?;
                }
                _ => {}
            }
            return Ok(None);
        }

        if self.commit_search_modal_open {
            match key_event.code {
                KeyCode::Esc => {
                    self.close_commit_search_modal();
                }
                KeyCode::Enter => {
                    if let Some(commit) = self.selected_commit_search_entry() {
                        self.enter_commit_compare(commit).await?;
                    }
                    self.close_commit_search_modal();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.move_commit_search_selection(1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.move_commit_search_selection(-1);
                }
                KeyCode::Backspace => {
                    self.commit_search_query.pop();
                    self.clamp_commit_search_selection();
                    self.commit_search_error = None;
                }
                KeyCode::Char(ch)
                    if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && !key_event.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.commit_search_query.push(ch);
                    self.clamp_commit_search_selection();
                    self.commit_search_error = None;
                }
                _ => {}
            }
            return Ok(None);
        }

        if self.branch_compare_modal_open {
            match key_event.code {
                KeyCode::Esc => self.close_branch_compare_modal(),
                KeyCode::Tab => self.toggle_branch_compare_field(),
                KeyCode::Enter => {
                    self.confirm_branch_compare().await?;
                }
                KeyCode::Down | KeyCode::Char('j') => self.move_branch_compare_selection(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_branch_compare_selection(-1),
                KeyCode::Backspace => {
                    self.active_branch_compare_query_mut().pop();
                    self.sync_branch_compare_selection_after_query_change();
                }
                KeyCode::Char(ch)
                    if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && !key_event.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.active_branch_compare_query_mut().push(ch);
                    self.sync_branch_compare_selection_after_query_change();
                }
                _ => {}
            }
            return Ok(None);
        }

        if self.commit_modal_open {
            match key_event.code {
                KeyCode::Esc => {
                    self.close_commit_modal();
                }
                KeyCode::Enter => {
                    self.confirm_commit().await?;
                }
                KeyCode::Backspace => {
                    self.commit_message.pop();
                    self.commit_error = None;
                }
                KeyCode::Char(ch)
                    if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && !key_event.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.commit_message.push(ch);
                    self.commit_error = None;
                }
                _ => {}
            }
            return Ok(None);
        }

        if self.discard_target.is_some() {
            match key_event.code {
                KeyCode::Esc => {
                    self.discard_target = None;
                }
                KeyCode::Enter => {
                    self.confirm_discard().await?;
                }
                _ => {}
            }
            return Ok(None);
        }

        match key_event.code {
            KeyCode::Esc if self.diff_text_selection.is_some() => {
                self.clear_diff_text_selection();
            }
            KeyCode::Esc | KeyCode::Char('q') => self.quit(),
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                if !self.copy_diff_selection_to_clipboard()? {
                    self.quit();
                }
            }
            KeyCode::Tab => {
                self.clear_diff_text_selection();
                self.active_pane = match self.active_pane {
                    ActivePane::Sidebar => ActivePane::Diff,
                    ActivePane::Diff => ActivePane::Sidebar,
                };
            }
            KeyCode::Char('?') => {
                self.help_modal_open = true;
            }
            KeyCode::Char('t') => {
                self.open_theme_modal();
            }
            KeyCode::Char('r') => {
                self.refresh().await?;
            }
            KeyCode::Char('i') => {
                self.initialize_repo_if_needed().await?;
            }
            KeyCode::Char('l') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.reset_to_working_tree().await?;
            }
            KeyCode::Char('p') => {
                self.start_pull();
            }
            KeyCode::Char('P') => {
                self.start_push();
            }
            KeyCode::Char('c') => {
                self.open_commit_modal();
            }
            KeyCode::Char('b') => {
                self.open_branch_compare_modal();
            }
            KeyCode::Char('g') => {
                self.open_commit_search_modal();
            }
            KeyCode::Char('v') => {
                self.clear_diff_text_selection();
                self.diff_view_mode = match self.diff_view_mode {
                    DiffViewMode::Unified => DiffViewMode::Split,
                    DiffViewMode::Split => DiffViewMode::Unified,
                };
                self.diff_scroll = 0;
                self.selected_diff_line_index = self
                    .diff_view
                    .first_selectable_index(self.diff_view_mode, self.current_diff_display_width());
            }
            KeyCode::Char('d') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.clear_diff_text_selection();
                self.page_or_scroll_diff(12);
            }
            KeyCode::Char('u') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.clear_diff_text_selection();
                self.page_or_scroll_diff(-12);
            }
            KeyCode::Down | KeyCode::Char('j') => match self.active_pane {
                ActivePane::Sidebar => self.select_next_file().await?,
                ActivePane::Diff => {
                    self.clear_diff_text_selection();
                    self.move_diff_selection(1);
                }
            },
            KeyCode::Up | KeyCode::Char('k') => match self.active_pane {
                ActivePane::Sidebar => self.select_previous_file().await?,
                ActivePane::Diff => {
                    self.clear_diff_text_selection();
                    self.move_diff_selection(-1);
                }
            },
            KeyCode::Char(' ') => {
                if self.active_pane == ActivePane::Sidebar {
                    self.toggle_selected_file_stage().await?;
                }
            }
            KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('e') => {
                if self.active_pane == ActivePane::Diff
                    && matches!(key_event.code, KeyCode::Enter)
                    && self
                        .diff_view
                        .selected_gap_action(
                            self.diff_view_mode,
                            self.current_diff_display_width(),
                            self.selected_diff_line_index,
                        )
                        .is_some()
                {
                    self.selected_diff_line_index = self.diff_view.expand_selected_gap(
                        self.diff_view_mode,
                        self.current_diff_display_width(),
                        self.selected_diff_line_index,
                        20,
                    );
                    return Ok(None);
                }

                if let Some(file_path) = self.selected_file().map(|file| file.path.clone()) {
                    if self.active_pane == ActivePane::Diff {
                        if let Some(line_number) = self.diff_view.selected_line_number(
                            self.diff_view_mode,
                            self.current_diff_display_width(),
                            self.selected_diff_line_index,
                        ) {
                            return Ok(Some(AppCommand::OpenFileInEditorAtLine(
                                file_path,
                                line_number,
                            )));
                        }

                        return Ok(Some(AppCommand::OpenFileInEditor(file_path)));
                    }

                    if self.active_pane == ActivePane::Sidebar {
                        return Ok(Some(AppCommand::OpenFileInEditor(file_path)));
                    }
                }
            }
            KeyCode::Char('d') => {
                self.open_discard_modal();
            }
            KeyCode::PageDown => match self.active_pane {
                ActivePane::Sidebar => self.page_files_down().await?,
                ActivePane::Diff => {
                    self.clear_diff_text_selection();
                    self.page_diff(12);
                }
            },
            KeyCode::PageUp => match self.active_pane {
                ActivePane::Sidebar => self.page_files_up().await?,
                ActivePane::Diff => {
                    self.clear_diff_text_selection();
                    self.page_diff(-12);
                }
            },
            KeyCode::Home => match self.active_pane {
                ActivePane::Sidebar => self.select_file_at(0).await?,
                ActivePane::Diff => {
                    self.clear_diff_text_selection();
                    self.selected_diff_line_index = self.diff_view.first_selectable_index(
                        self.diff_view_mode,
                        self.current_diff_display_width(),
                    );
                    self.diff_scroll = 0;
                }
            },
            KeyCode::End => match self.active_pane {
                ActivePane::Sidebar => {
                    if let Some(last_index) = self.files.len().checked_sub(1) {
                        self.select_file_at(last_index).await?;
                    }
                }
                ActivePane::Diff => {
                    self.clear_diff_text_selection();
                    self.selected_diff_line_index = self.diff_view.last_selectable_index(
                        self.diff_view_mode,
                        self.current_diff_display_width(),
                    );
                    self.diff_scroll = u16::MAX;
                }
            },
            _ => {}
        }

        Ok(None)
    }

    async fn handle_mouse_event(&mut self, mouse_event: MouseEvent) -> color_eyre::Result<()> {
        if self.blame_modal_open {
            return Ok(());
        }

        if self.commit_modal_open {
            return Ok(());
        }

        if self.discard_target.is_some() {
            return Ok(());
        }

        if self.help_modal_open {
            return Ok(());
        }

        if self.theme_modal_open {
            return Ok(());
        }

        if self.commit_search_modal_open {
            return Ok(());
        }

        if self.branch_compare_modal_open {
            return Ok(());
        }

        match mouse_event.kind {
            MouseEventKind::ScrollDown => {
                self.clear_diff_text_selection();
                self.page_or_scroll_diff(3);
            }
            MouseEventKind::ScrollUp => {
                self.clear_diff_text_selection();
                self.page_or_scroll_diff(-3);
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                let (width, height) = terminal::size().wrap_err("failed to read terminal size")?;
                if let Some(selection_point) = ui::diff_selection_point_at(
                    self,
                    mouse_event.column,
                    mouse_event.row,
                    width,
                    height,
                ) {
                    self.active_pane = ActivePane::Diff;
                    self.selected_diff_line_index = selection_point.display_index;
                    self.diff_text_selection_anchor = Some(selection_point);
                    self.diff_text_selection = None;
                    return Ok(());
                }

                self.clear_diff_text_selection();
                if let Some(display_index) =
                    ui::diff_gap_click_at(self, mouse_event.column, mouse_event.row, width, height)
                {
                    self.active_pane = ActivePane::Diff;
                    self.selected_diff_line_index = display_index;
                    self.selected_diff_line_index = self.diff_view.expand_selected_gap(
                        self.diff_view_mode,
                        self.current_diff_display_width(),
                        self.selected_diff_line_index,
                        20,
                    );
                    return Ok(());
                }

                if let Some(path) =
                    ui::sidebar_file_at(self, mouse_event.column, mouse_event.row, width, height)
                {
                    self.select_file_by_path(&path).await?;
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                let Some(anchor) = self.diff_text_selection_anchor else {
                    return Ok(());
                };
                let (width, height) = terminal::size().wrap_err("failed to read terminal size")?;
                if let Some(selection_point) = ui::diff_selection_drag_point_at(
                    self,
                    anchor.pane,
                    mouse_event.column,
                    mouse_event.row,
                    width,
                    height,
                ) {
                    self.active_pane = ActivePane::Diff;
                    self.selected_diff_line_index = selection_point.display_index;
                    self.diff_text_selection = Some(DiffTextSelection {
                        anchor,
                        head: selection_point,
                    });
                }
            }
            MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                let Some(anchor) = self.diff_text_selection_anchor.take() else {
                    return Ok(());
                };
                if self.diff_text_selection.is_none() {
                    self.selected_diff_line_index = anchor.display_index;
                    return Ok(());
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn refresh(&mut self) -> color_eyre::Result<()> {
        let previously_selected = self.selected_file().map(|file| file.path.clone());
        if self.is_working_tree_mode()
            && let Err(error) = self.sync_repo_state().await
        {
            self.enter_repo_error_state(error.to_string()).await?;
            return Ok(());
        }

        let files = match &self.review_mode {
            ReviewMode::WorkingTree => match git::load_files_with_status(&self.repo_root).await {
                Ok(files) => {
                    self.repo_error = None;
                    files
                }
                Err(error) => {
                    self.enter_repo_error_state(error.to_string()).await?;
                    return Ok(());
                }
            },
            ReviewMode::CommitCompare(selection) => {
                git::load_files_with_commit_diff(&self.repo_root, selection).await?
            }
            ReviewMode::BranchCompare(selection) => {
                git::load_files_with_branch_diff(&self.repo_root, selection).await?
            }
        };
        self.diff_cache_generation = self.diff_cache_generation.saturating_add(1);
        self.diff_view_cache.clear();
        self.pending_diff_cache_key = None;
        self.files = files;
        self.rebuild_sidebar_items();

        self.selected_file_index = previously_selected
            .as_deref()
            .and_then(|path| self.file_index_by_path(path))
            .or_else(|| {
                self.first_sidebar_file_path()
                    .and_then(|path| self.file_index_by_path(path))
            })
            .unwrap_or(0);

        self.sync_sidebar_state();
        self.queue_selected_diff_load(true, true);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }

    fn rebuild_sidebar_items(&mut self) {
        self.sidebar_items = sidebar::build_sidebar_items(&self.files, &self.collapsed_directories);
    }

    fn selected_file(&self) -> Option<&FileEntry> {
        self.files.get(self.selected_file_index)
    }

    fn clear_diff_text_selection(&mut self) {
        self.diff_text_selection = None;
        self.diff_text_selection_anchor = None;
    }

    fn copy_diff_selection_to_clipboard(&mut self) -> color_eyre::Result<bool> {
        if self.active_pane != ActivePane::Diff {
            return Ok(false);
        }

        let Some(selection) = self.diff_text_selection else {
            return Ok(false);
        };
        let text = self.diff_view.selected_text(
            self.diff_view_mode,
            self.current_diff_display_width(),
            selection.anchor,
            selection.head,
        );
        let Some(text) = text else {
            self.status_message = Some("selection is empty".to_string());
            return Ok(true);
        };

        write_osc52_clipboard(&text)?;
        self.status_message = Some("copied diff selection".to_string());
        Ok(true)
    }

    fn spawn_highlight_prewarm(&mut self) {
        let Some(registry) = self.highlight_registry.clone() else {
            return;
        };
        let selected_filetype = self.selected_file().and_then(|file| file.filetype);
        let mut warmed_filetypes = HashSet::new();
        let filetypes = self
            .files
            .iter()
            .filter_map(|file| file.filetype)
            .filter(|filetype| Some(*filetype) != selected_filetype)
            .filter(|filetype| warmed_filetypes.insert(*filetype))
            .collect::<Vec<_>>();
        if filetypes.is_empty() {
            return;
        }

        self.track_background_task(task::spawn(async move {
            let _ = task::spawn_blocking(move || {
                let _ = git::prewarm_highlight_registry(registry.as_ref(), filetypes);
            })
            .await;
        }));
    }

    fn file_index_by_path(&self, path: &str) -> Option<usize> {
        self.files.iter().position(|file| file.path == path)
    }

    fn visible_file_paths(&self) -> Vec<String> {
        self.sidebar_items
            .iter()
            .filter_map(|item| match item {
                SidebarItem::File { file, .. } => Some(file.path.clone()),
                SidebarItem::Header { .. } => None,
            })
            .collect()
    }

    fn first_sidebar_file_path(&self) -> Option<&str> {
        self.sidebar_items.iter().find_map(|item| match item {
            SidebarItem::File { file, .. } => Some(file.path.as_str()),
            SidebarItem::Header { .. } => None,
        })
    }

    fn selected_visible_file_index(&self) -> Option<usize> {
        let selected_path = self.selected_file()?.path.as_str();
        self.visible_file_paths()
            .iter()
            .position(|path| path == selected_path)
    }

    async fn select_file_by_path(&mut self, path: &str) -> color_eyre::Result<()> {
        if let Some(index) = self.file_index_by_path(path) {
            self.select_file_at(index).await?;
        }
        Ok(())
    }

    async fn toggle_selected_file_stage(&mut self) -> color_eyre::Result<()> {
        if !self.is_working_tree_mode() {
            self.status_message = Some("stage/unstage is unavailable in compare mode".to_string());
            return Ok(());
        }

        let Some(file) = self.selected_file().cloned() else {
            return Ok(());
        };

        git::toggle_file_stage(&self.repo_root, &file).await?;
        self.refresh_working_tree_file(&file.path).await?;
        self.status_message = Some(format!(
            "{} {}",
            if git::is_file_staged(&file.status) {
                "unstaged"
            } else {
                "staged"
            },
            file.path
        ));
        Ok(())
    }

    async fn refresh_working_tree_file(&mut self, path: &str) -> color_eyre::Result<()> {
        let previously_selected = self.selected_file().map(|file| file.path.clone());
        let updated_file = git::load_status_for_path(&self.repo_root, path).await?;

        if let Some(index) = self.file_index_by_path(path) {
            match updated_file {
                Some(file) => self.files[index] = file,
                None => {
                    self.files.remove(index);
                }
            }
        } else if let Some(file) = updated_file {
            self.files.push(file);
        }

        self.diff_cache_generation = self.diff_cache_generation.saturating_add(1);
        self.diff_view_cache.clear();
        self.pending_diff_cache_key = None;
        self.rebuild_sidebar_items();

        self.selected_file_index = previously_selected
            .as_deref()
            .and_then(|selected_path| self.file_index_by_path(selected_path))
            .or_else(|| {
                self.first_sidebar_file_path()
                    .and_then(|first_path| self.file_index_by_path(first_path))
            })
            .unwrap_or(0);

        self.sync_sidebar_state();
        self.queue_selected_diff_load(true, true);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }

    fn open_commit_modal(&mut self) {
        if !self.is_working_tree_mode() {
            self.status_message = Some("commit is unavailable in compare mode".to_string());
            return;
        }

        if self.staged_file_count() == 0 {
            return;
        }

        self.commit_modal_open = true;
        self.commit_message.clear();
        self.commit_error = None;
    }

    fn open_blame_target(&mut self, target: BlameTarget) {
        self.cancel_inflight_blame_load();
        self.blame_modal_open = true;
        self.blame_target = Some(target.clone());
        self.blame_loading = true;
        self.blame_details = None;
        self.blame_error = None;
        self.blame_scroll = 0;
        self.blame_request_id = self.blame_request_id.saturating_add(1);
        let request_id = self.blame_request_id;

        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.blame_load_task = Some(task::spawn(async move {
            let result = git::load_blame_commit_details(&repo_root, &target)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::BlameLoaded { request_id, result });
        }));
    }

    fn close_blame_modal(&mut self) {
        self.cancel_inflight_blame_load();
        self.blame_modal_open = false;
        self.blame_loading = false;
        self.blame_target = None;
        self.blame_details = None;
        self.blame_error = None;
        self.blame_scroll = 0;
    }

    fn cancel_inflight_blame_load(&mut self) {
        if let Some(task) = self.blame_load_task.take() {
            task.abort();
        }
    }

    fn scroll_blame(&mut self, delta: i32) {
        self.blame_scroll = if delta.is_negative() {
            self.blame_scroll
                .saturating_sub(delta.unsigned_abs() as u16)
        } else {
            self.blame_scroll.saturating_add(delta as u16)
        };
    }

    async fn open_blame_commit_compare(&mut self) -> color_eyre::Result<()> {
        let Some(details) = self.blame_details.clone() else {
            return Ok(());
        };

        let Some(selection) = details.compare_selection else {
            self.blame_error = Some("No committed change is available for this line.".to_string());
            return Ok(());
        };

        self.close_blame_modal();
        self.review_mode = ReviewMode::CommitCompare(selection);
        self.refresh().await
    }

    fn open_theme_modal(&mut self) {
        if self.theme_modal_open {
            return;
        }

        self.theme_modal_open = true;
        self.theme_modal_query.clear();
        self.theme_modal_initial_name = self.theme_name.clone();
        self.theme_modal_initial_mode = self.theme_mode;
        self.theme_modal_selected_index = theme::all()
            .iter()
            .position(|theme_entry| theme_entry.name == self.theme_name)
            .unwrap_or(0);
    }

    async fn cancel_theme_modal(&mut self) -> color_eyre::Result<()> {
        self.theme_name = self.theme_modal_initial_name.clone();
        self.theme_mode = self.theme_modal_initial_mode;
        theme::set_active_theme(&self.theme_name, self.theme_mode);
        self.theme_modal_open = false;
        self.theme_modal_query.clear();
        self.queue_selected_diff_load(false, false);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }

    fn confirm_theme_modal(&mut self) -> color_eyre::Result<()> {
        self.theme_modal_open = false;
        self.theme_modal_query.clear();
        match config::persist_theme_preference(&self.theme_name, self.theme_mode) {
            Ok(()) => {
                self.status_message = Some(format!(
                    "theme set to {} ({})",
                    self.theme_name,
                    self.theme_mode.as_str()
                ));
            }
            Err(error) => {
                self.status_message = Some(format!("failed to persist theme: {error}"));
            }
        }
        Ok(())
    }

    fn close_commit_modal(&mut self) {
        self.commit_modal_open = false;
        self.commit_message.clear();
        self.commit_error = None;
    }

    async fn confirm_commit(&mut self) -> color_eyre::Result<()> {
        match git::commit_staged_changes(&self.repo_root, &self.commit_message).await {
            Ok(()) => {
                let committed_message = self.commit_message.trim().to_string();
                self.close_commit_modal();
                self.refresh().await?;
                self.status_message = Some(format!("committed {}", committed_message));
            }
            Err(error) => {
                self.commit_error = Some(error.to_string());
            }
        }
        Ok(())
    }

    fn start_push(&mut self) {
        if self.remote_sync.is_some() {
            return;
        }

        self.remote_sync = Some(RemoteSyncDirection::Push);
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = git::push_to_remote(&repo_root)
                .await
                .map(|_| "Pushed to remote".to_string())
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::RemoteSyncFinished(result));
        }));
    }

    fn start_pull(&mut self) {
        if self.remote_sync.is_some() {
            return;
        }

        self.remote_sync = Some(RemoteSyncDirection::Pull);
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = git::pull_from_remote(&repo_root)
                .await
                .map(|_| "Pulled from remote".to_string())
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::RemoteSyncFinished(result));
        }));
    }

    async fn run_command(
        &mut self,
        command: AppCommand,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<()> {
        match command {
            AppCommand::OpenFileInEditor(path) => {
                if self.chooser_file_path.is_some() {
                    self.write_chooser_selection_and_exit(&path, None).await?;
                } else {
                    self.open_file_in_editor(&path, terminal).await?;
                }
            }
            AppCommand::OpenFileInEditorAtLine(path, line_number) => {
                if self.chooser_file_path.is_some() {
                    self.write_chooser_selection_and_exit(&path, Some(line_number))
                        .await?;
                } else {
                    self.open_file_in_editor_at_line(&path, line_number, terminal)
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn write_chooser_selection_and_exit(
        &mut self,
        file_path: &str,
        line_number: Option<usize>,
    ) -> color_eyre::Result<()> {
        let Some(chooser_file_path) = self.chooser_file_path.as_ref() else {
            return Ok(());
        };

        let absolute_path = self.repo_root.join(file_path);
        let payload = match line_number {
            Some(line_number) => format!("{}\n{}\n", absolute_path.display(), line_number),
            None => format!("{}\n\n", absolute_path.display()),
        };
        fs::write(chooser_file_path, payload)
            .await
            .wrap_err_with(|| {
                format!(
                    "failed to write chooser selection to {}",
                    chooser_file_path.display()
                )
            })?;
        self.running = false;
        Ok(())
    }

    async fn open_file_in_editor(
        &mut self,
        file_path: &str,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<()> {
        let Some(editor_command) = current_editor_command() else {
            self.status_message =
                Some("Set VISUAL or EDITOR to open files from vigil.".to_string());
            return Ok(());
        };

        let full_path = self.repo_root.join(file_path);
        let command = build_editor_shell_command(&editor_command, &full_path, None);
        let result = self.run_editor_command(command, terminal).await;

        match result {
            Ok(Ok(status)) if status.success() => {
                self.refresh().await?;
                self.status_message = Some(format!("opened {}", file_path));
            }
            Ok(Ok(status)) => {
                self.status_message = Some(format!(
                    "editor exited with code {}",
                    status.code().unwrap_or(1)
                ));
            }
            Ok(Err(error)) => {
                self.status_message = Some(format!("failed to launch editor: {error}"));
            }
            Err(error) => {
                self.status_message = Some(format!("editor task failed: {error}"));
            }
        }

        Ok(())
    }

    async fn open_file_in_editor_at_line(
        &mut self,
        file_path: &str,
        line_number: usize,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<()> {
        let Some(editor_command) = current_editor_command() else {
            self.status_message =
                Some("Set VISUAL or EDITOR to open files from vigil.".to_string());
            return Ok(());
        };

        let full_path = self.repo_root.join(file_path);
        let command = build_editor_shell_command(&editor_command, &full_path, Some(line_number));
        let result = self.run_editor_command(command, terminal).await;

        match result {
            Ok(Ok(status)) if status.success() => {
                self.refresh().await?;
                self.status_message = Some(format!("opened {}:{}", file_path, line_number));
            }
            Ok(Ok(status)) => {
                self.status_message = Some(format!(
                    "editor exited with code {}",
                    status.code().unwrap_or(1)
                ));
            }
            Ok(Err(error)) => {
                self.status_message = Some(format!("failed to launch editor: {error}"));
            }
            Err(error) => {
                self.status_message = Some(format!("editor task failed: {error}"));
            }
        }

        Ok(())
    }

    async fn run_editor_command(
        &mut self,
        command: String,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> Result<Result<std::process::ExitStatus, std::io::Error>, task::JoinError> {
        self.events.suspend();
        let _ = execute!(stdout(), DisableMouseCapture);
        ratatui::restore();

        let result = task::spawn_blocking(move || {
            std::process::Command::new("sh")
                .args(["-lc", &command])
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
        })
        .await;

        *terminal = ratatui::init();
        let _ = execute!(stdout(), EnableMouseCapture);
        self.events.resume();

        result
    }

    fn open_discard_modal(&mut self) {
        if !self.is_working_tree_mode() {
            self.status_message = Some("discard is unavailable in compare mode".to_string());
            return;
        }
        self.discard_target = self.selected_file().cloned();
    }

    async fn confirm_discard(&mut self) -> color_eyre::Result<()> {
        let Some(file) = self.discard_target.take() else {
            return Ok(());
        };

        git::discard_file_changes(&self.repo_root, &file).await?;
        self.refresh().await?;
        self.status_message = Some(format!("discarded {}", file.path));
        Ok(())
    }

    async fn select_file_at(&mut self, index: usize) -> color_eyre::Result<()> {
        if self.files.is_empty() {
            self.selected_file_index = 0;
            self.sync_sidebar_state();
            self.queue_selected_diff_load(true, true);
            return Ok(());
        }

        let bounded_index = index.min(self.files.len().saturating_sub(1));
        if bounded_index != self.selected_file_index {
            self.selected_file_index = bounded_index;
            self.sync_sidebar_state();
            self.queue_selected_diff_load(true, true);
        } else {
            self.sync_sidebar_state();
        }

        Ok(())
    }

    async fn select_next_file(&mut self) -> color_eyre::Result<()> {
        let visible_file_paths = self.visible_file_paths();
        if visible_file_paths.is_empty() {
            return Ok(());
        }
        let current_visible_index = self.selected_visible_file_index().unwrap_or(0);
        let next_visible_index = (current_visible_index + 1).min(visible_file_paths.len() - 1);
        self.select_file_by_path(&visible_file_paths[next_visible_index])
            .await
    }

    async fn select_previous_file(&mut self) -> color_eyre::Result<()> {
        let visible_file_paths = self.visible_file_paths();
        if visible_file_paths.is_empty() {
            return Ok(());
        }
        let current_visible_index = self.selected_visible_file_index().unwrap_or(0);
        let previous_visible_index = current_visible_index.saturating_sub(1);
        self.select_file_by_path(&visible_file_paths[previous_visible_index])
            .await
    }

    async fn page_files_down(&mut self) -> color_eyre::Result<()> {
        let visible_file_paths = self.visible_file_paths();
        if visible_file_paths.is_empty() {
            return Ok(());
        }
        let current_visible_index = self.selected_visible_file_index().unwrap_or(0);
        let next_visible_index = current_visible_index
            .saturating_add(10)
            .min(visible_file_paths.len() - 1);
        self.select_file_by_path(&visible_file_paths[next_visible_index])
            .await
    }

    async fn page_files_up(&mut self) -> color_eyre::Result<()> {
        let visible_file_paths = self.visible_file_paths();
        if visible_file_paths.is_empty() {
            return Ok(());
        }
        let current_visible_index = self.selected_visible_file_index().unwrap_or(0);
        let previous_visible_index = current_visible_index.saturating_sub(10);
        self.select_file_by_path(&visible_file_paths[previous_visible_index])
            .await
    }

    fn sync_sidebar_state(&mut self) {
        let selected_path = self.selected_file().map(|file| file.path.as_str());
        let selected_row = selected_path.and_then(|path| {
            self.sidebar_items.iter().position(
                |item| matches!(item, SidebarItem::File { file, .. } if file.path == path),
            )
        });
        self.sidebar_state.select(selected_row);
    }

    fn staged_file_count(&self) -> usize {
        self.files
            .iter()
            .filter(|file| git::is_file_staged(&file.status))
            .count()
    }

    fn show_snackbar(&mut self, message: String, variant: SnackbarVariant) {
        self.snackbar_generation = self.snackbar_generation.saturating_add(1);
        let generation = self.snackbar_generation;
        self.snackbar_notice = Some(SnackbarNotice { message, variant });

        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let _ = sender.send(Event::ClearSnackbar(generation));
        }));
    }

    pub fn filtered_theme_names(&mut self) -> Vec<&'static str> {
        let query = self.theme_modal_query.trim();
        if query.is_empty() {
            return theme::names().collect();
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let candidates = theme::names().collect::<Vec<_>>();
        pattern
            .match_list(candidates, &mut self.theme_matcher)
            .into_iter()
            .map(|(candidate, _score)| candidate)
            .collect()
    }

    async fn sync_theme_selection_after_query_change(&mut self) -> color_eyre::Result<()> {
        let filtered = self.filtered_theme_names();
        if filtered.is_empty() {
            self.theme_modal_selected_index = 0;
            return Ok(());
        }

        if let Some(index) = filtered
            .iter()
            .position(|name| *name == self.theme_name.as_str())
        {
            self.theme_modal_selected_index = index;
            return Ok(());
        }

        self.theme_modal_selected_index = 0;
        self.preview_theme(filtered[0], self.theme_mode).await
    }

    async fn move_theme_selection(&mut self, delta: i32) -> color_eyre::Result<()> {
        let filtered = self.filtered_theme_names();
        if filtered.is_empty() {
            self.theme_modal_selected_index = 0;
            return Ok(());
        }

        let current = self.theme_modal_selected_index.min(filtered.len() - 1);
        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            current.saturating_add(delta as usize)
        }
        .min(filtered.len() - 1);

        self.theme_modal_selected_index = next;
        self.preview_theme(filtered[next], self.theme_mode).await
    }

    async fn toggle_theme_mode_preview(&mut self) -> color_eyre::Result<()> {
        self.theme_mode = self.theme_mode.toggle();
        theme::set_active_theme(&self.theme_name, self.theme_mode);
        self.queue_selected_diff_load(false, false);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }

    async fn preview_theme(
        &mut self,
        theme_name: &str,
        theme_mode: ThemeMode,
    ) -> color_eyre::Result<()> {
        let resolved_name = theme::resolve_theme_name(Some(theme_name)).to_string();
        self.theme_name = resolved_name;
        self.theme_mode = theme_mode;
        theme::set_active_theme(&self.theme_name, self.theme_mode);
        self.queue_selected_diff_load(false, false);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }

    fn spawn_highlight_registry_init(&mut self) {
        let initial_filetypes = self
            .selected_file()
            .and_then(|file| file.filetype)
            .into_iter()
            .collect::<Vec<_>>();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = task::spawn_blocking(move || {
                git::HighlightRegistry::new_for_filetypes(initial_filetypes)
            })
            .await;
            let event = match result {
                Ok(Ok(registry)) => Event::HighlightRegistryReady(Ok(registry.into())),
                Ok(Err(error)) => Event::HighlightRegistryReady(Err(error.to_string())),
                Err(error) => Event::HighlightRegistryReady(Err(error.to_string())),
            };
            let _ = sender.send(event);
        }));
    }

    fn spawn_repo_watcher_init(&mut self) {
        if self.repo_watcher_loading {
            return;
        }

        self.repo_watcher_loading = true;
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = RepoWatcher::initialize(repo_root.clone(), sender.clone()).await;
            let _ = sender.send(Event::RepoWatcherReady(repo_root, result));
        }));
    }

    fn restart_repo_watcher(&mut self) {
        self.repo_watcher = None;
        self.repo_watcher_loading = false;
        self.spawn_repo_watcher_init();
    }

    fn track_background_task(&mut self, handle: task::JoinHandle<()>) {
        self.background_tasks.retain(|task| !task.is_finished());
        self.background_tasks.push(handle);
    }

    fn abort_background_tasks(&mut self) {
        for task in self.background_tasks.drain(..) {
            task.abort();
        }
    }

    fn quit(&mut self) {
        self.cancel_inflight_diff_load();
        self.cancel_inflight_blame_load();
        self.abort_background_tasks();
        self.repo_watcher = None;
        self.repo_watcher_loading = false;
        self.remote_sync = None;
        self.events.suspend();
        self.running = false;
    }

    pub fn show_splash(&self) -> bool {
        self.repo_error.is_some() || (self.is_working_tree_mode() && self.files.is_empty())
    }

    pub fn splash_error(&self) -> Option<&str> {
        self.repo_error.as_deref()
    }

    pub fn can_initialize_git_repo(&self) -> bool {
        self.repo_error
            .as_deref()
            .is_some_and(splash::is_not_git_repository_error)
    }

    pub fn review_mode_label(&self) -> String {
        match &self.review_mode {
            ReviewMode::WorkingTree => String::new(),
            ReviewMode::CommitCompare(selection) => {
                format!("Commit {}: {}", selection.short_hash, selection.subject)
            }
            ReviewMode::BranchCompare(selection) => {
                format!(
                    "Compare {} -> {}",
                    selection.source_ref, selection.destination_ref
                )
            }
        }
    }

    fn is_working_tree_mode(&self) -> bool {
        matches!(self.review_mode, ReviewMode::WorkingTree)
    }

    async fn reset_to_working_tree(&mut self) -> color_eyre::Result<()> {
        if self.is_working_tree_mode() {
            return Ok(());
        }

        self.review_mode = ReviewMode::WorkingTree;
        self.refresh().await
    }

    async fn initialize_repo_if_needed(&mut self) -> color_eyre::Result<()> {
        if !self.can_initialize_git_repo() {
            return Ok(());
        }

        git::init_repo(&self.repo_root).await?;
        self.review_mode = ReviewMode::WorkingTree;
        self.refresh().await?;
        self.status_message = Some(format!(
            "initialized git repo in {}",
            self.repo_root.display()
        ));
        Ok(())
    }

    async fn sync_repo_state(&mut self) -> color_eyre::Result<()> {
        let resolved_root = git::resolve_repo_root_from(&self.repo_root).await?;
        let watcher_needs_restart = self.repo_error.is_some()
            || (!self.repo_watcher_loading && self.repo_watcher.is_none())
            || self.repo_root != resolved_root;

        self.repo_root = resolved_root;
        self.repo_error = None;

        if watcher_needs_restart {
            self.restart_repo_watcher();
        }

        Ok(())
    }

    async fn enter_repo_error_state(&mut self, error: String) -> color_eyre::Result<()> {
        self.repo_error = Some(error);
        self.repo_watcher = None;
        self.repo_watcher_loading = false;
        self.files.clear();
        self.rebuild_sidebar_items();
        self.selected_file_index = 0;
        self.sync_sidebar_state();
        self.queue_selected_diff_load(true, true);
        self.status_message = Some(self.current_status_message());
        Ok(())
    }

    fn current_status_message(&self) -> String {
        self.repo_error
            .clone()
            .unwrap_or_else(|| self.default_status_message())
    }

    fn default_status_message(&self) -> String {
        match &self.review_mode {
            ReviewMode::WorkingTree => format!(
                "{} changed file{}",
                self.files.len(),
                if self.files.len() == 1 { "" } else { "s" }
            ),
            ReviewMode::CommitCompare(selection) => {
                format!(
                    "commit {}  {} file{}",
                    selection.short_hash,
                    self.files.len(),
                    if self.files.len() == 1 { "" } else { "s" }
                )
            }
            ReviewMode::BranchCompare(selection) => {
                format!(
                    "{} -> {}  {} file{}",
                    selection.source_ref,
                    selection.destination_ref,
                    self.files.len(),
                    if self.files.len() == 1 { "" } else { "s" }
                )
            }
        }
    }

    async fn should_restart_watcher_for_paths(&self, paths: &[PathBuf]) -> bool {
        for path in paths {
            if path
                .file_name()
                .is_some_and(|file_name| file_name == ".gitignore")
            {
                return true;
            }

            if let Ok(metadata) = fs::metadata(path).await
                && metadata.is_dir()
            {
                return true;
            }
        }

        false
    }
}

fn quote_shell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn write_osc52_clipboard(text: &str) -> color_eyre::Result<()> {
    let encoded = encode_base64(text.as_bytes());
    let mut output = stdout();
    write!(output, "\x1b]52;c;{encoded}\x07")?;
    output.flush()?;
    Ok(())
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);

        encoded.push(TABLE[(first >> 2) as usize] as char);
        encoded.push(TABLE[((first & 0b0000_0011) << 4 | (second >> 4)) as usize] as char);

        if chunk.len() > 1 {
            encoded.push(TABLE[((second & 0b0000_1111) << 2 | (third >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }

        if chunk.len() > 2 {
            encoded.push(TABLE[(third & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    encoded
}

fn current_editor_command() -> Option<String> {
    std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn build_editor_shell_command(
    editor_command: &str,
    full_path: &std::path::Path,
    line_number: Option<usize>,
) -> String {
    let quoted_path = quote_shell_arg(&full_path.to_string_lossy());
    match line_number {
        Some(line_number) if editor_supports_plus_line(editor_command) => {
            format!("{editor_command} +{line_number} {quoted_path}")
        }
        _ => format!("{editor_command} {quoted_path}"),
    }
}

fn editor_supports_plus_line(editor_command: &str) -> bool {
    let editor = editor_command.split_whitespace().next().unwrap_or_default();
    let binary = editor.rsplit('/').next().unwrap_or(editor);
    matches!(binary, "nvim" | "vim" | "vi" | "vimdiff" | "nvim-qt")
}
