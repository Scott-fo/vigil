use crossterm::event::KeyEventKind;

use crate::{event::Event, ui};

use super::super::App;

impl App {
    pub(super) async fn handle_runtime_event(
        &mut self,
        event: Event,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<()> {
        match event {
            Event::Crossterm(event) => {
                if self.handle_crossterm_event(event, terminal).await? {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::HighlightRegistryReady(result) => {
                self.handle_highlight_registry_ready(result);
                self.redraw_if_running(terminal)?;
            }
            Event::DiffLoaded { request_id, result } => {
                if self.handle_diff_loaded(request_id, result) {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::DiffHighlightUpdated {
                request_id,
                complete,
                result,
            } => {
                if self.handle_diff_highlight_updated(request_id, complete, result) {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::DiffPrefetched(prefetched) => {
                self.handle_diff_prefetched(*prefetched);
            }
            Event::WorkingTreeStatusLoaded { request_id, result } => {
                if self.handle_working_tree_status_loaded(request_id, result) {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::BlameLoaded { request_id, result } => {
                if self.handle_blame_loaded(request_id, result) {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::CommitSearchLoaded(result) => {
                self.handle_commit_search_loaded(result);
                if self.commit_search_modal_open {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::DiffSearchIndexLoaded { request_id, result } => {
                if self.handle_diff_search_index_loaded(request_id, result) {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::DiffSearchResultsLoaded { request_id, result } => {
                if self.handle_diff_search_results_loaded(request_id, result) {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::BranchCompareLoaded(result) => {
                self.handle_branch_compare_loaded(result);
                if self.branch_compare_modal_open {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::BranchMergeFinished(result) => {
                self.handle_branch_merge_finished(result).await?;
                self.redraw_if_running(terminal)?;
            }
            Event::WorktreesLoaded(result) => {
                self.handle_worktrees_loaded(result);
                if self.worktree_modal_open {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::ReviewFinished { request_id, result } => {
                if self.handle_review_finished(request_id, result) {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::ReviewLoaded { request_id, result } => {
                if self.handle_review_loaded(request_id, result) {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::RepoWatcherReady(repo_root, result) => {
                if self.handle_repo_watcher_ready(repo_root, result) && self.running {
                    terminal.draw(|frame| ui::render(frame, self))?;
                }
            }
            Event::RepoChanged(paths) => {
                if self.handle_repo_changed(paths).await? {
                    self.redraw_if_running(terminal)?;
                }
            }
            Event::RemoteSyncFinished(result) => {
                self.handle_remote_sync_finished(result);
                self.redraw_if_running(terminal)?;
            }
            Event::ClearSnackbar(generation) => {
                if self.handle_clear_snackbar(generation) {
                    self.redraw_if_running(terminal)?;
                }
            }
        }
        Ok(())
    }

    async fn handle_crossterm_event(
        &mut self,
        event: crossterm::event::Event,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<bool> {
        match event {
            crossterm::event::Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                if let Some(command) = self.handle_key_event(key_event).await? {
                    self.run_command(command, terminal).await?;
                }
                Ok(true)
            }
            crossterm::event::Event::Mouse(mouse_event) => {
                self.handle_mouse_event(mouse_event).await?;
                Ok(true)
            }
            crossterm::event::Event::Resize(_, _) => Ok(true),
            _ => Ok(false),
        }
    }

    fn redraw_if_running(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<()> {
        if self.running {
            self.redraw(terminal)?;
        }
        Ok(())
    }
}
