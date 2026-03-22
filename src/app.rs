use std::{collections::HashSet, path::PathBuf, process::Stdio};

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

use crate::{
    event::{Event, EventHandler},
    git::{
        self, CommitCompareSelection, CommitSearchEntry, DiffView, FileEntry,
        SharedHighlightRegistry,
    },
    sidebar::{self, SidebarItem},
    splash, ui,
    watcher::RepoWatcher,
};
use std::io::stdout;

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

#[derive(Debug, Clone)]
pub enum ReviewMode {
    WorkingTree,
    CommitCompare(CommitCompareSelection),
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

struct CommitSearchCandidate {
    index: usize,
    haystack: String,
}

impl AsRef<str> for CommitSearchCandidate {
    fn as_ref(&self) -> &str {
        &self.haystack
    }
}

#[derive(Debug)]
pub struct App {
    pub running: bool,
    pub repo_root: PathBuf,
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
    pub highlight_registry: Option<SharedHighlightRegistry>,
    pub repo_watcher: Option<RepoWatcher>,
    pub repo_watcher_loading: bool,
    pub help_modal_open: bool,
    pub commit_search_modal_open: bool,
    pub commit_search_query: String,
    pub commit_search_entries: Vec<CommitSearchEntry>,
    pub commit_search_loading: bool,
    pub commit_search_error: Option<String>,
    pub commit_search_selected_index: usize,
    pub commit_search_matcher: Matcher,
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
    pub async fn new() -> color_eyre::Result<Self> {
        let repo_root = std::env::current_dir().wrap_err("failed to resolve current directory")?;
        let mut app = Self {
            running: true,
            repo_root,
            repo_error: None,
            events: EventHandler::new(),
            active_pane: ActivePane::Sidebar,
            review_mode: ReviewMode::WorkingTree,
            files: Vec::new(),
            sidebar_items: Vec::new(),
            collapsed_directories: HashSet::new(),
            sidebar_state: ListState::default(),
            selected_file_index: 0,
            diff_view: DiffView::default(),
            diff_view_mode: DiffViewMode::Unified,
            diff_scroll: 0,
            selected_diff_line_index: 0,
            highlight_registry: None,
            repo_watcher: None,
            repo_watcher_loading: false,
            help_modal_open: false,
            commit_search_modal_open: false,
            commit_search_query: String::new(),
            commit_search_entries: Vec::new(),
            commit_search_loading: false,
            commit_search_error: None,
            commit_search_selected_index: 0,
            commit_search_matcher: Matcher::new(MatcherConfig::DEFAULT),
            commit_modal_open: false,
            commit_message: String::new(),
            commit_error: None,
            discard_target: None,
            remote_sync: None,
            snackbar_notice: None,
            snackbar_generation: 0,
            status_message: None,
        };
        app.spawn_highlight_registry_init();
        app.refresh().await?;
        Ok(app)
    }

    pub async fn run(mut self, mut terminal: ratatui::DefaultTerminal) -> color_eyre::Result<()> {
        terminal.draw(|frame| ui::render(frame, &mut self))?;

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
                        terminal.draw(|frame| ui::render(frame, &mut self))?;
                    }
                }
                Event::HighlightRegistryReady(result) => {
                    match result {
                        Ok(registry) => {
                            self.highlight_registry = Some(registry);
                            self.load_selected_diff().await?;
                            self.status_message = Some(self.current_status_message());
                        }
                        Err(error) => {
                            self.status_message =
                                Some(format!("highlight registry init failed: {error}"));
                        }
                    }

                    if self.running {
                        terminal.draw(|frame| ui::render(frame, &mut self))?;
                    }
                }
                Event::CommitSearchLoaded(result) => {
                    if self.commit_search_modal_open {
                        self.commit_search_loading = false;
                        match result {
                            Ok(entries) => {
                                self.commit_search_entries = entries;
                                self.commit_search_error = None;
                                self.clamp_commit_search_selection();
                            }
                            Err(error) => {
                                self.commit_search_entries.clear();
                                self.commit_search_error = Some(error);
                                self.commit_search_selected_index = 0;
                            }
                        }

                        if self.running {
                            terminal.draw(|frame| ui::render(frame, &mut self))?;
                        }
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
                            terminal.draw(|frame| ui::render(frame, &mut self))?;
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
                        terminal.draw(|frame| ui::render(frame, &mut self))?;
                    }
                }
                Event::ClearSnackbar(generation) => {
                    if self.snackbar_generation == generation {
                        self.snackbar_notice = None;
                        if self.running {
                            terminal.draw(|frame| ui::render(frame, &mut self))?;
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
        if self.help_modal_open {
            match key_event.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter | KeyCode::Char('q') => {
                    self.help_modal_open = false;
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
            KeyCode::Esc | KeyCode::Char('q') => self.quit(),
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.quit();
            }
            KeyCode::Tab => {
                self.active_pane = match self.active_pane {
                    ActivePane::Sidebar => ActivePane::Diff,
                    ActivePane::Diff => ActivePane::Sidebar,
                };
            }
            KeyCode::Char('?') => {
                self.help_modal_open = true;
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
            KeyCode::Char('g') => {
                self.open_commit_search_modal();
            }
            KeyCode::Char('v') => {
                self.diff_view_mode = match self.diff_view_mode {
                    DiffViewMode::Unified => DiffViewMode::Split,
                    DiffViewMode::Split => DiffViewMode::Unified,
                };
                self.diff_scroll = 0;
                self.selected_diff_line_index =
                    self.diff_view.first_selectable_index(self.diff_view_mode);
            }
            KeyCode::Char('d') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.page_or_scroll_diff(12);
            }
            KeyCode::Char('u') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.page_or_scroll_diff(-12);
            }
            KeyCode::Down | KeyCode::Char('j') => match self.active_pane {
                ActivePane::Sidebar => self.select_next_file().await?,
                ActivePane::Diff => self.move_diff_selection(1),
            },
            KeyCode::Up | KeyCode::Char('k') => match self.active_pane {
                ActivePane::Sidebar => self.select_previous_file().await?,
                ActivePane::Diff => self.move_diff_selection(-1),
            },
            KeyCode::Char(' ') => {
                if self.active_pane == ActivePane::Sidebar {
                    self.toggle_selected_file_stage().await?;
                }
            }
            KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('e') => {
                if let Some(file_path) = self.selected_file().map(|file| file.path.clone()) {
                    if self.active_pane == ActivePane::Diff {
                        if let Some(line_number) = self.diff_view.selected_line_number(
                            self.diff_view_mode,
                            self.selected_diff_line_index,
                        ) {
                            return Ok(Some(AppCommand::OpenFileInEditorAtLine(
                                file_path,
                                line_number,
                            )));
                        }
                    }

                    return Ok(Some(AppCommand::OpenFileInEditor(file_path)));
                }
            }
            KeyCode::Char('d') => {
                self.open_discard_modal();
            }
            KeyCode::PageDown => match self.active_pane {
                ActivePane::Sidebar => self.page_files_down().await?,
                ActivePane::Diff => self.page_diff(12),
            },
            KeyCode::PageUp => match self.active_pane {
                ActivePane::Sidebar => self.page_files_up().await?,
                ActivePane::Diff => self.page_diff(-12),
            },
            KeyCode::Home => match self.active_pane {
                ActivePane::Sidebar => self.select_file_at(0).await?,
                ActivePane::Diff => {
                    self.selected_diff_line_index =
                        self.diff_view.first_selectable_index(self.diff_view_mode);
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
                    self.selected_diff_line_index =
                        self.diff_view.last_selectable_index(self.diff_view_mode);
                    self.diff_scroll = u16::MAX;
                }
            },
            _ => {}
        }

        Ok(None)
    }

    async fn handle_mouse_event(&mut self, mouse_event: MouseEvent) -> color_eyre::Result<()> {
        if self.commit_modal_open {
            return Ok(());
        }

        if self.discard_target.is_some() {
            return Ok(());
        }

        if self.help_modal_open {
            return Ok(());
        }

        if self.commit_search_modal_open {
            return Ok(());
        }

        match mouse_event.kind {
            MouseEventKind::ScrollDown => self.page_or_scroll_diff(3),
            MouseEventKind::ScrollUp => self.page_or_scroll_diff(-3),
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                let (width, height) = terminal::size().wrap_err("failed to read terminal size")?;
                if let Some(path) =
                    ui::sidebar_file_at(self, mouse_event.column, mouse_event.row, width, height)
                {
                    self.select_file_by_path(&path).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn refresh(&mut self) -> color_eyre::Result<()> {
        let previously_selected = self.selected_file().map(|file| file.path.clone());
        if self.is_working_tree_mode() {
            if let Err(error) = self.sync_repo_state().await {
                self.enter_repo_error_state(error.to_string()).await?;
                return Ok(());
            }
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
        };
        self.files = files;
        self.rebuild_sidebar_items();

        self.selected_file_index = previously_selected
            .as_deref()
            .and_then(|path| self.file_index_by_path(path))
            .unwrap_or(0);

        self.sync_sidebar_state();
        self.load_selected_diff().await?;
        self.status_message = Some(self.current_status_message());
        Ok(())
    }

    fn rebuild_sidebar_items(&mut self) {
        self.sidebar_items = sidebar::build_sidebar_items(&self.files, &self.collapsed_directories);
    }

    fn selected_file(&self) -> Option<&FileEntry> {
        self.files.get(self.selected_file_index)
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

    async fn load_selected_diff(&mut self) -> color_eyre::Result<()> {
        self.diff_scroll = 0;
        self.diff_view = match self.selected_file() {
            Some(file) => match &self.review_mode {
                ReviewMode::WorkingTree => {
                    git::load_diff_view_for_working_tree(
                        &self.repo_root,
                        file,
                        self.highlight_registry.as_deref(),
                    )
                    .await?
                }
                ReviewMode::CommitCompare(selection) => {
                    git::load_diff_view_for_commit_compare(
                        &self.repo_root,
                        file,
                        selection,
                        self.highlight_registry.as_deref(),
                    )
                    .await?
                }
            },
            None => DiffView::empty("No changed files found."),
        };
        self.selected_diff_line_index = self.diff_view.first_selectable_index(self.diff_view_mode);
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
        self.refresh().await?;
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

    fn open_commit_search_modal(&mut self) {
        if self.commit_search_modal_open {
            return;
        }

        self.commit_search_modal_open = true;
        self.commit_search_query.clear();
        self.commit_search_entries.clear();
        self.commit_search_loading = true;
        self.commit_search_error = None;
        self.commit_search_selected_index = 0;

        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        task::spawn(async move {
            let result = git::list_searchable_commits(&repo_root, 12_000)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::CommitSearchLoaded(result));
        });
    }

    fn close_commit_search_modal(&mut self) {
        self.commit_search_modal_open = false;
        self.commit_search_loading = false;
        self.commit_search_error = None;
        self.commit_search_selected_index = 0;
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
        task::spawn(async move {
            let result = git::push_to_remote(&repo_root)
                .await
                .map(|_| "Pushed to remote".to_string())
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::RemoteSyncFinished(result));
        });
    }

    fn start_pull(&mut self) {
        if self.remote_sync.is_some() {
            return;
        }

        self.remote_sync = Some(RemoteSyncDirection::Pull);
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        task::spawn(async move {
            let result = git::pull_from_remote(&repo_root)
                .await
                .map(|_| "Pulled from remote".to_string())
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::RemoteSyncFinished(result));
        });
    }

    async fn run_command(
        &mut self,
        command: AppCommand,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<()> {
        match command {
            AppCommand::OpenFileInEditor(path) => {
                self.open_file_in_editor(&path, terminal).await?;
            }
            AppCommand::OpenFileInEditorAtLine(path, line_number) => {
                self.open_file_in_editor_at_line(&path, line_number, terminal)
                    .await?;
            }
        }
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
            self.load_selected_diff().await?;
            return Ok(());
        }

        let bounded_index = index.min(self.files.len().saturating_sub(1));
        if bounded_index != self.selected_file_index {
            self.selected_file_index = bounded_index;
            self.sync_sidebar_state();
            self.load_selected_diff().await?;
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
        task::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let _ = sender.send(Event::ClearSnackbar(generation));
        });
    }

    fn move_diff_selection(&mut self, delta: i32) {
        self.selected_diff_line_index = self.diff_view.move_selection(
            self.diff_view_mode,
            self.selected_diff_line_index,
            delta,
        );
    }

    pub fn filtered_commit_search_indices(&mut self) -> Vec<usize> {
        let query = self.commit_search_query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return (0..self.commit_search_entries.len()).collect();
        }

        let pattern = Pattern::parse(&query, CaseMatching::Ignore, Normalization::Smart);
        let candidates = self
            .commit_search_entries
            .iter()
            .enumerate()
            .map(|(index, entry)| CommitSearchCandidate {
                index,
                haystack: format!("{} {} {}", entry.short_hash, entry.hash, entry.subject),
            })
            .collect::<Vec<_>>();

        pattern
            .match_list(candidates, &mut self.commit_search_matcher)
            .into_iter()
            .map(|(candidate, _score)| candidate.index)
            .collect()
    }

    fn clamp_commit_search_selection(&mut self) {
        let filtered_len = self.filtered_commit_search_indices().len();
        self.commit_search_selected_index = self
            .commit_search_selected_index
            .min(filtered_len.saturating_sub(1));
    }

    fn move_commit_search_selection(&mut self, delta: i32) {
        let filtered_len = self.filtered_commit_search_indices().len();
        if filtered_len == 0 {
            self.commit_search_selected_index = 0;
            return;
        }

        let current = self.commit_search_selected_index.min(filtered_len - 1);
        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            current.saturating_add(delta as usize)
        }
        .min(filtered_len - 1);
        self.commit_search_selected_index = next;
    }

    fn selected_commit_search_entry(&mut self) -> Option<CommitSearchEntry> {
        self.filtered_commit_search_indices()
            .get(self.commit_search_selected_index)
            .and_then(|index| self.commit_search_entries.get(*index))
            .cloned()
    }

    fn page_diff(&mut self, delta: i32) {
        self.move_diff_selection(delta);
    }

    fn scroll_diff(&mut self, delta: i32) {
        self.diff_scroll = if delta.is_negative() {
            self.diff_scroll.saturating_sub(delta.unsigned_abs() as u16)
        } else {
            self.diff_scroll.saturating_add(delta as u16)
        };
    }

    fn page_or_scroll_diff(&mut self, delta: i32) {
        match self.active_pane {
            ActivePane::Diff => self.page_diff(delta),
            ActivePane::Sidebar => self.scroll_diff(delta),
        }
    }

    fn spawn_highlight_registry_init(&self) {
        let sender = self.events.sender();
        task::spawn(async move {
            let result = task::spawn_blocking(git::HighlightRegistry::new).await;
            let event = match result {
                Ok(Ok(registry)) => Event::HighlightRegistryReady(Ok(registry.into())),
                Ok(Err(error)) => Event::HighlightRegistryReady(Err(error.to_string())),
                Err(error) => Event::HighlightRegistryReady(Err(error.to_string())),
            };
            let _ = sender.send(event);
        });
    }

    fn spawn_repo_watcher_init(&mut self) {
        if self.repo_watcher_loading {
            return;
        }

        self.repo_watcher_loading = true;
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        task::spawn(async move {
            let result = RepoWatcher::initialize(repo_root.clone(), sender.clone()).await;
            let _ = sender.send(Event::RepoWatcherReady(repo_root, result));
        });
    }

    fn restart_repo_watcher(&mut self) {
        self.repo_watcher = None;
        self.repo_watcher_loading = false;
        self.spawn_repo_watcher_init();
    }

    fn quit(&mut self) {
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
        }
    }

    fn is_working_tree_mode(&self) -> bool {
        matches!(self.review_mode, ReviewMode::WorkingTree)
    }

    async fn enter_commit_compare(&mut self, commit: CommitSearchEntry) -> color_eyre::Result<()> {
        self.review_mode = ReviewMode::CommitCompare(CommitCompareSelection {
            base_ref: git::resolve_commit_base_ref(&commit),
            commit_hash: commit.hash.clone(),
            short_hash: commit.short_hash.clone(),
            subject: commit.subject.clone(),
        });
        self.refresh().await
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
        let resolved_root = git::resolve_repo_root().await?;
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
        self.load_selected_diff().await?;
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

            if let Ok(metadata) = fs::metadata(path).await {
                if metadata.is_dir() {
                    return true;
                }
            }
        }

        false
    }
}

fn quote_shell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
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
