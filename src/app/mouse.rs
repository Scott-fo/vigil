use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

mod click;
mod drag;
mod scroll;

use super::App;

impl App {
    pub(super) async fn handle_mouse_event(
        &mut self,
        mouse_event: MouseEvent,
    ) -> color_eyre::Result<()> {
        if self.mouse_input_blocked_by_modal() {
            return Ok(());
        }

        match mouse_event.kind {
            MouseEventKind::ScrollDown => {
                self.handle_mouse_scroll(mouse_event, 3)?;
            }
            MouseEventKind::ScrollUp => {
                self.handle_mouse_scroll(mouse_event, -3)?;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.handle_mouse_left_down(mouse_event).await?;
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.handle_mouse_left_drag(mouse_event)?;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.handle_mouse_left_up();
            }
            _ => {}
        }
        Ok(())
    }

    fn mouse_input_blocked_by_modal(&self) -> bool {
        self.blame_modal_open
            || self.commit_modal_open
            || self.discard_target.is_some()
            || self.diff_stats_modal_open
            || self.help_modal_open
            || self.review_summary_modal_open
            || self.theme_modal_open
            || self.commit_search_modal_open
            || self.branch_compare_modal_open
            || self.branch_merge_target.is_some()
            || self.diff_search_modal_open
            || self.file_filter_modal_open
            || self.worktree_modal_open
    }

    pub(super) fn clear_diff_text_selection(&mut self) {
        self.diff_text_selection = None;
        self.diff_text_selection_anchor = None;
    }
}
