use color_eyre::eyre::WrapErr;
use crossterm::{event::MouseEvent, terminal};

use super::super::{ActivePane, App, DiffTextSelection};
use crate::ui;

impl App {
    pub(super) fn handle_mouse_left_drag(
        &mut self,
        mouse_event: MouseEvent,
    ) -> color_eyre::Result<()> {
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
        Ok(())
    }

    pub(super) fn handle_mouse_left_up(&mut self) {
        let Some(anchor) = self.diff_text_selection_anchor.take() else {
            return;
        };
        if self.diff_text_selection.is_none() {
            self.selected_diff_line_index = anchor.display_index;
        }
    }
}
