use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

mod click;
mod drag;
mod footer;
mod hover;
mod scroll;

use super::App;

impl App {
    /// Applies a mouse event. Returns whether anything visible changed, so
    /// pointer motion that stays on the same target does not redraw.
    pub(super) async fn handle_mouse_event(
        &mut self,
        mouse_event: MouseEvent,
    ) -> color_eyre::Result<bool> {
        if self.mouse_input_blocked_by_modal() {
            return Ok(false);
        }

        match mouse_event.kind {
            MouseEventKind::Moved => return self.handle_mouse_moved(mouse_event),
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
            _ => return Ok(false),
        }
        Ok(true)
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
