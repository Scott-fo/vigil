//! Jumping between change blocks, across file boundaries.
//!
//! `]` and `[` move the diff cursor to the next or previous change block. Past
//! the last (or before the first) block of a file, the jump continues into the
//! neighbouring file in sidebar order. That file's diff usually loads
//! asynchronously, so the jump records a [`PendingChangeLanding`] which is
//! resolved the moment the new diff view is installed.

use super::super::{ActivePane, App};
use crate::git::ChangeDirection;

/// Display lines kept above a change when the jump scrolls the diff.
const CHANGE_CONTEXT_LINES: usize = 3;

/// A cross-file jump waiting for `file_path`'s diff to load. `Next` lands on
/// the file's first change block and `Previous` on its last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingChangeLanding {
    file_path: String,
    direction: ChangeDirection,
}

impl App {
    /// Moves the diff cursor to the adjacent change block, rolling over into
    /// the neighbouring file when this one has no more changes that way.
    pub(in crate::app) async fn jump_to_change(
        &mut self,
        direction: ChangeDirection,
    ) -> color_eyre::Result<()> {
        self.clear_diff_text_selection();
        let mode = self.diff_view_mode;
        let width = self.current_diff_display_width();
        let line_wrap = self.diff_line_wrap_mode;

        // From the sidebar, jumping means "review this file": land on its
        // first (or last) change rather than relative to a hidden cursor.
        let target = if self.active_pane == ActivePane::Sidebar {
            self.diff_view
                .edge_change_index(mode, width, line_wrap, direction)
        } else {
            self.diff_view.adjacent_change_index(
                mode,
                width,
                line_wrap,
                self.selected_diff_line_index,
                direction,
            )
        };
        self.active_pane = ActivePane::Diff;

        if let Some(index) = target {
            self.move_diff_cursor_to_change(index);
            return Ok(());
        }

        let Some(path) = self.neighbouring_file_path(direction) else {
            self.status_message = Some(
                match direction {
                    ChangeDirection::Next => "last change in review",
                    ChangeDirection::Previous => "first change in review",
                }
                .to_string(),
            );
            return Ok(());
        };

        self.pending_change_landing = Some(PendingChangeLanding {
            file_path: path.clone(),
            direction,
        });
        self.select_file_by_path(&path).await?;
        self.active_pane = ActivePane::Diff;
        Ok(())
    }

    /// Resolves every deferred cursor target against the diff view that was
    /// just installed. Call right after assigning `self.diff_view` for the
    /// selected file.
    pub(in crate::app) fn apply_pending_diff_targets(&mut self) {
        self.apply_pending_diff_search_target();
        self.apply_pending_change_landing();
    }

    fn apply_pending_change_landing(&mut self) {
        let Some(landing) = self.pending_change_landing.take() else {
            return;
        };
        if self.selected_file().map(|file| file.path.as_str()) != Some(landing.file_path.as_str()) {
            // The user moved elsewhere before the diff arrived.
            return;
        }

        match self.diff_view.edge_change_index(
            self.diff_view_mode,
            self.current_diff_display_width(),
            self.diff_line_wrap_mode,
            landing.direction,
        ) {
            Some(index) => self.move_diff_cursor_to_change(index),
            None => {
                self.selected_diff_line_index = self.diff_view.first_selectable_index(
                    self.diff_view_mode,
                    self.current_diff_display_width(),
                    self.diff_line_wrap_mode,
                );
                self.diff_scroll = 0;
            }
        }
    }

    fn move_diff_cursor_to_change(&mut self, index: usize) {
        self.selected_diff_line_index = index;
        let visible = self
            .diff_viewport
            .is_some_and(|viewport| index >= viewport.start && index < viewport.end);
        if !visible {
            self.diff_scroll = index
                .saturating_sub(CHANGE_CONTEXT_LINES)
                .min(u16::MAX as usize) as u16;
        }
    }

    /// The file after (or before) the selected one in sidebar order.
    fn neighbouring_file_path(&self, direction: ChangeDirection) -> Option<String> {
        let paths = self.visible_file_paths();
        let current = self.selected_file().map(|file| file.path.as_str())?;
        let position = paths.iter().position(|path| path == current)?;
        let neighbour = match direction {
            ChangeDirection::Next => position.checked_add(1)?,
            ChangeDirection::Previous => position.checked_sub(1)?,
        };
        paths.get(neighbour).cloned()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        app::DiffViewMode,
        git::{self, DiffView, FileEntry},
    };

    /// Unified display: -old +new a b +added c d [gap ↓] [gap ↑] x -removed y,
    /// so change blocks start at display indices 0, 4 and 10.
    const BLOCK_STARTS: [usize; 3] = [0, 4, 10];

    fn two_hunk_view() -> DiffView {
        git::build_diff_view_from_diff_text(
            concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -1,5 +1,6 @@\n",
                "-fn old() {}\n",
                "+fn new() {}\n",
                " fn a() {}\n",
                " fn b() {}\n",
                "+fn added() {}\n",
                " fn c() {}\n",
                " fn d() {}\n",
                "@@ -40,3 +41,2 @@\n",
                " fn x() {}\n",
                "-fn removed() {}\n",
                " fn y() {}\n",
            ),
            Some("rust"),
        )
    }

    fn file(path: &str) -> FileEntry {
        FileEntry {
            status: " M".to_string(),
            path: path.to_string(),
            label: path.rsplit('/').next().unwrap_or(path).to_string(),
            filetype: Some("rust"),
        }
    }

    /// Three files with every diff already cached, the first one selected
    /// and the diff pane focused.
    fn app_with_cached_diffs() -> App {
        let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-change-nav"));
        app.files = vec![file("a.rs"), file("b.rs"), file("c.rs")];
        app.diff_view_mode = DiffViewMode::Unified;
        app.rebuild_sidebar_items();
        app.sync_sidebar_state();
        for index in 0..app.files.len() {
            let key = app.diff_cache_key(&app.files[index]);
            app.diff_view_cache.insert_plain(key, two_hunk_view());
        }
        app.diff_view = two_hunk_view();
        app.active_pane = ActivePane::Diff;
        app
    }

    fn selected_path(app: &App) -> &str {
        app.selected_file().map(|file| file.path.as_str()).unwrap()
    }

    #[tokio::test]
    async fn next_change_walks_blocks_then_rolls_into_the_next_file() {
        let mut app = app_with_cached_diffs();

        app.jump_to_change(ChangeDirection::Next).await.unwrap();
        assert_eq!(app.selected_diff_line_index, BLOCK_STARTS[1]);
        app.jump_to_change(ChangeDirection::Next).await.unwrap();
        assert_eq!(app.selected_diff_line_index, BLOCK_STARTS[2]);

        app.jump_to_change(ChangeDirection::Next).await.unwrap();
        assert_eq!(selected_path(&app), "b.rs");
        assert_eq!(app.selected_diff_line_index, BLOCK_STARTS[0]);
        assert_eq!(app.pending_change_landing, None);
        assert_eq!(app.active_pane, ActivePane::Diff);
    }

    #[tokio::test]
    async fn previous_change_rolls_back_onto_the_last_block_of_the_previous_file() {
        let mut app = app_with_cached_diffs();
        app.select_file_by_path("b.rs").await.unwrap();
        app.active_pane = ActivePane::Diff;
        assert_eq!(app.selected_diff_line_index, 0);

        app.jump_to_change(ChangeDirection::Previous).await.unwrap();

        assert_eq!(selected_path(&app), "a.rs");
        assert_eq!(app.selected_diff_line_index, BLOCK_STARTS[2]);
    }

    #[tokio::test]
    async fn jumping_past_the_last_change_in_review_stays_put() {
        let mut app = app_with_cached_diffs();
        app.select_file_by_path("c.rs").await.unwrap();
        app.active_pane = ActivePane::Diff;
        app.selected_diff_line_index = BLOCK_STARTS[2];

        app.jump_to_change(ChangeDirection::Next).await.unwrap();

        assert_eq!(selected_path(&app), "c.rs");
        assert_eq!(app.selected_diff_line_index, BLOCK_STARTS[2]);
        assert_eq!(app.status_message.as_deref(), Some("last change in review"));
    }

    #[tokio::test]
    async fn from_the_sidebar_jumps_land_on_the_selected_files_edge_changes() {
        let mut app = app_with_cached_diffs();
        app.active_pane = ActivePane::Sidebar;

        app.jump_to_change(ChangeDirection::Previous).await.unwrap();

        assert_eq!(selected_path(&app), "a.rs");
        assert_eq!(app.selected_diff_line_index, BLOCK_STARTS[2]);
        assert_eq!(app.active_pane, ActivePane::Diff);
    }

    #[tokio::test]
    async fn landing_waits_for_an_asynchronously_loaded_diff() {
        let mut app = app_with_cached_diffs();
        let key = app.diff_cache_key(&app.files[1]);
        app.diff_view_cache = Default::default();
        app.selected_diff_line_index = BLOCK_STARTS[2];

        app.jump_to_change(ChangeDirection::Next).await.unwrap();
        assert_eq!(selected_path(&app), "b.rs");
        assert!(app.pending_change_landing.is_some());

        let request_id = app.diff_request_id;
        app.pending_diff_cache_key = Some(key);
        assert!(app.handle_diff_loaded(request_id, Ok(two_hunk_view())));

        assert_eq!(app.pending_change_landing, None);
        assert_eq!(app.selected_diff_line_index, BLOCK_STARTS[0]);
    }

    #[tokio::test]
    async fn stale_landing_is_dropped_when_another_file_is_selected() {
        let mut app = app_with_cached_diffs();
        app.pending_change_landing = Some(PendingChangeLanding {
            file_path: "c.rs".to_string(),
            direction: ChangeDirection::Previous,
        });
        app.selected_diff_line_index = 5;

        app.apply_pending_diff_targets();

        assert_eq!(app.pending_change_landing, None);
        assert_eq!(app.selected_diff_line_index, 5);
    }
}
