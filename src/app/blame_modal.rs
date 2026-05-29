use crossterm::event::{KeyCode, KeyEvent};
use tokio::task;

use super::{App, ReviewMode, navigation::scroll_u16};
use crate::{
    event::Event,
    git::{self, BlameTarget},
};

impl App {
    pub(super) async fn handle_blame_modal_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if !self.blame_modal_open {
            return Ok(false);
        }

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

        Ok(true)
    }

    pub(super) fn open_blame_target(&mut self, target: BlameTarget) {
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

    pub(super) fn close_blame_modal(&mut self) {
        self.cancel_inflight_blame_load();
        self.blame_modal_open = false;
        self.blame_loading = false;
        self.blame_target = None;
        self.blame_details = None;
        self.blame_error = None;
        self.blame_scroll = 0;
    }

    pub(super) fn cancel_inflight_blame_load(&mut self) {
        if let Some(task) = self.blame_load_task.take() {
            task.abort();
        }
    }

    pub(super) fn scroll_blame(&mut self, delta: i32) {
        self.blame_scroll = scroll_u16(self.blame_scroll, delta);
    }

    pub(super) async fn open_blame_commit_compare(&mut self) -> color_eyre::Result<()> {
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
}
