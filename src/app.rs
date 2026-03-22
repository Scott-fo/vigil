use std::{collections::HashSet, path::PathBuf, process::Stdio};

use color_eyre::eyre::WrapErr;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
    execute,
};
use crossterm::terminal;
use ratatui::widgets::ListState;
use tokio::{process::Command, task};

use crate::{
    event::{Event, EventHandler},
    git::{self, DiffView, FileEntry, SharedHighlightRegistry},
    sidebar::{self, SidebarItem},
    ui,
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

enum AppCommand {
    OpenFileInEditor(String),
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

#[derive(Debug)]
pub struct App {
    pub running: bool,
    pub repo_root: PathBuf,
    pub events: EventHandler,
    pub active_pane: ActivePane,
    pub files: Vec<FileEntry>,
    pub sidebar_items: Vec<SidebarItem>,
    pub collapsed_directories: HashSet<String>,
    pub sidebar_state: ListState,
    pub selected_file_index: usize,
    pub diff_view: DiffView,
    pub diff_view_mode: DiffViewMode,
    pub diff_scroll: u16,
    pub highlight_registry: Option<SharedHighlightRegistry>,
    pub commit_modal_open: bool,
    pub commit_message: String,
    pub commit_error: Option<String>,
    pub discard_target: Option<FileEntry>,
    pub remote_sync: Option<RemoteSyncDirection>,
    pub snackbar_notice: Option<SnackbarNotice>,
    pub snackbar_generation: u64,
    pub status_message: Option<String>,
}

//

impl App {
    pub async fn new() -> color_eyre::Result<Self> {
        let repo_root = git::resolve_repo_root().await?;
        let mut app = Self {
            running: true,
            repo_root,
            events: EventHandler::new(),
            active_pane: ActivePane::Sidebar,
            files: Vec::new(),
            sidebar_items: Vec::new(),
            collapsed_directories: HashSet::new(),
            sidebar_state: ListState::default(),
            selected_file_index: 0,
            diff_view: DiffView::default(),
            diff_view_mode: DiffViewMode::Unified,
            diff_scroll: 0,
            highlight_registry: None,
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
                            self.status_message = Some(format!(
                                "{} changed file{}",
                                self.files.len(),
                                if self.files.len() == 1 { "" } else { "s" }
                            ));
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
            KeyCode::Char('r') => {
                self.refresh().await?;
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
            KeyCode::Char('v') => {
                self.diff_view_mode = match self.diff_view_mode {
                    DiffViewMode::Unified => DiffViewMode::Split,
                    DiffViewMode::Split => DiffViewMode::Unified,
                };
                self.diff_scroll = 0;
            }
            KeyCode::Char('d') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.scroll_diff(12);
            }
            KeyCode::Char('u') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.scroll_diff(-12);
            }
            KeyCode::Down | KeyCode::Char('j') => match self.active_pane {
                ActivePane::Sidebar => self.select_next_file().await?,
                ActivePane::Diff => self.scroll_diff(1),
            },
            KeyCode::Up | KeyCode::Char('k') => match self.active_pane {
                ActivePane::Sidebar => self.select_previous_file().await?,
                ActivePane::Diff => self.scroll_diff(-1),
            },
            KeyCode::Char(' ') => {
                if self.active_pane == ActivePane::Sidebar {
                    self.toggle_selected_file_stage().await?;
                }
            }
            KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('e') => {
                if let Some(file) = self.selected_file() {
                    return Ok(Some(AppCommand::OpenFileInEditor(file.path.clone())));
                }
            }
            KeyCode::Char('d') => {
                self.open_discard_modal();
            }
            KeyCode::PageDown => match self.active_pane {
                ActivePane::Sidebar => self.page_files_down().await?,
                ActivePane::Diff => self.scroll_diff(12),
            },
            KeyCode::PageUp => match self.active_pane {
                ActivePane::Sidebar => self.page_files_up().await?,
                ActivePane::Diff => self.scroll_diff(-12),
            },
            KeyCode::Home => match self.active_pane {
                ActivePane::Sidebar => self.select_file_at(0).await?,
                ActivePane::Diff => self.diff_scroll = 0,
            },
            KeyCode::End => match self.active_pane {
                ActivePane::Sidebar => {
                    if let Some(last_index) = self.files.len().checked_sub(1) {
                        self.select_file_at(last_index).await?;
                    }
                }
                ActivePane::Diff => self.diff_scroll = u16::MAX,
            },
            KeyCode::Char('g') => {
                let branch = self
                    .current_branch()
                    .await
                    .unwrap_or_else(|_| "HEAD".to_string());
                self.status_message = Some(format!(
                    "repo: {}  branch: {}",
                    self.repo_root.display(),
                    branch
                ));
            }
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

        match mouse_event.kind {
            MouseEventKind::ScrollDown => self.scroll_diff(3),
            MouseEventKind::ScrollUp => self.scroll_diff(-3),
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

    async fn current_branch(&self) -> color_eyre::Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo_root)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .await
            .wrap_err("failed to query current branch")?;

        if !output.status.success() {
            return Ok("HEAD".to_string());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    async fn refresh(&mut self) -> color_eyre::Result<()> {
        let previously_selected = self.selected_file().map(|file| file.path.clone());
        let files = git::load_files_with_status(&self.repo_root).await?;
        self.files = files;
        self.rebuild_sidebar_items();

        self.selected_file_index = previously_selected
            .as_deref()
            .and_then(|path| self.file_index_by_path(path))
            .unwrap_or(0);

        self.sync_sidebar_state();
        self.load_selected_diff().await?;
        self.status_message = Some(format!(
            "{} changed file{}",
            self.files.len(),
            if self.files.len() == 1 { "" } else { "s" }
        ));
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
            Some(file) => {
                git::load_diff_view(
                    &self.repo_root,
                    file,
                    self.highlight_registry.as_deref(),
                )
                .await?
            }
            None => DiffView::empty("No changed files found."),
        };
        Ok(())
    }

    async fn toggle_selected_file_stage(&mut self) -> color_eyre::Result<()> {
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
        if self.staged_file_count() == 0 {
            return;
        }

        self.commit_modal_open = true;
        self.commit_message.clear();
        self.commit_error = None;
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
        }
        Ok(())
    }

    async fn open_file_in_editor(
        &mut self,
        file_path: &str,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<()> {
        let editor_command = std::env::var("VISUAL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                std::env::var("EDITOR")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            });

        let Some(editor_command) = editor_command else {
            self.status_message = Some("Set VISUAL or EDITOR to open files from vigil.".to_string());
            return Ok(());
        };

        let full_path = self.repo_root.join(file_path);
        let quoted_path = quote_shell_arg(&full_path.to_string_lossy());

        let _ = execute!(stdout(), DisableMouseCapture);
        ratatui::restore();

        let result = task::spawn_blocking(move || {
            std::process::Command::new("sh")
                .args(["-lc", &format!("{editor_command} {quoted_path}")])
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
        })
        .await;

        *terminal = ratatui::init();
        let _ = execute!(stdout(), EnableMouseCapture);

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

    fn open_discard_modal(&mut self) {
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

    fn scroll_diff(&mut self, delta: i32) {
        self.diff_scroll = if delta.is_negative() {
            self.diff_scroll.saturating_sub(delta.unsigned_abs() as u16)
        } else {
            self.diff_scroll.saturating_add(delta as u16)
        };
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

    fn quit(&mut self) {
        self.running = false;
    }
}

fn quote_shell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
