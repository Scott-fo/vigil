use crossterm::event::{KeyCode, KeyEvent};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStatsState {
    Ready(git::ReviewDiffStats),
    Loading { file_count: usize },
    Unavailable { file_count: usize },
}

impl App {
    pub fn diff_stats_state(&self) -> DiffStatsState {
        if let Some(snapshot) = self.review_diff_snapshot.as_ref() {
            return DiffStatsState::Ready(snapshot.stats());
        }

        if self.review_diff_snapshot_task.is_some() {
            return DiffStatsState::Loading {
                file_count: self.files.len(),
            };
        }

        DiffStatsState::Unavailable {
            file_count: self.files.len(),
        }
    }

    pub(in crate::app) fn open_diff_stats_modal(&mut self) {
        self.diff_stats_modal_open = true;
    }

    pub(in crate::app) fn handle_diff_stats_modal_key(&mut self, key_event: KeyEvent) -> bool {
        if !self.diff_stats_modal_open {
            return false;
        }

        match key_event.code {
            KeyCode::Esc | KeyCode::F(2) | KeyCode::Enter | KeyCode::Char('q') => {
                self.diff_stats_modal_open = false;
            }
            _ => {}
        }

        true
    }
}
