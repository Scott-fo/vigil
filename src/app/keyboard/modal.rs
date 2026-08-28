use crossterm::event::KeyEvent;

use super::super::App;

impl App {
    pub(super) async fn handle_modal_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<bool> {
        if self.handle_blame_modal_key(key_event).await? {
            return Ok(true);
        }

        if self.handle_help_modal_key(key_event) {
            return Ok(true);
        }

        if self.handle_diff_stats_modal_key(key_event) {
            return Ok(true);
        }

        if self.handle_review_context_modal_key(key_event) {
            return Ok(true);
        }

        if self.handle_review_summary_modal_key(key_event) {
            return Ok(true);
        }

        if self.handle_theme_modal_key(key_event).await? {
            return Ok(true);
        }

        if self.handle_commit_search_key(key_event).await? {
            return Ok(true);
        }

        if self.handle_file_search_key(key_event).await? {
            return Ok(true);
        }

        if self.handle_file_filter_key(key_event)? {
            return Ok(true);
        }

        if self.handle_diff_search_key(key_event).await? {
            return Ok(true);
        }

        if self.handle_branch_compare_key(key_event).await? {
            return Ok(true);
        }

        if self.handle_branch_merge_key(key_event) {
            return Ok(true);
        }

        if self.handle_worktree_key(key_event).await? {
            return Ok(true);
        }

        if self.handle_commit_modal_key(key_event).await? {
            return Ok(true);
        }

        if self.handle_discard_modal_key(key_event).await? {
            return Ok(true);
        }

        Ok(false)
    }
}
