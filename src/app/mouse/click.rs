use color_eyre::eyre::WrapErr;
use crossterm::{event::MouseEvent, terminal};

use super::super::{ActivePane, App};
use crate::ui;

impl App {
    pub(super) async fn handle_mouse_left_down(
        &mut self,
        mouse_event: MouseEvent,
    ) -> color_eyre::Result<()> {
        let (width, height) = terminal::size().wrap_err("failed to read terminal size")?;
        if self.start_diff_text_selection(mouse_event, width, height) {
            return Ok(());
        }

        self.clear_diff_text_selection();
        if self.expand_clicked_diff_gap(mouse_event, width, height) {
            return Ok(());
        }

        if let Some(row_index) =
            ui::sidebar_item_index_at(self, mouse_event.column, mouse_event.row, width, height)
        {
            self.active_pane = ActivePane::Sidebar;
            self.select_sidebar_row(row_index).await?;
            let _ = self.toggle_focused_sidebar_directory();
        }
        Ok(())
    }

    fn start_diff_text_selection(
        &mut self,
        mouse_event: MouseEvent,
        width: u16,
        height: u16,
    ) -> bool {
        let Some(selection_point) =
            ui::diff_selection_point_at(self, mouse_event.column, mouse_event.row, width, height)
        else {
            return false;
        };

        self.active_pane = ActivePane::Diff;
        self.selected_diff_line_index = selection_point.display_index;
        self.diff_text_selection_anchor = Some(selection_point);
        self.diff_text_selection = None;
        true
    }

    fn expand_clicked_diff_gap(
        &mut self,
        mouse_event: MouseEvent,
        width: u16,
        height: u16,
    ) -> bool {
        let Some(display_index) =
            ui::diff_gap_click_at(self, mouse_event.column, mouse_event.row, width, height)
        else {
            return false;
        };

        self.active_pane = ActivePane::Diff;
        self.selected_diff_line_index = display_index;
        self.selected_diff_line_index = self.diff_view.expand_selected_gap(
            self.diff_view_mode,
            self.current_diff_display_width(),
            self.diff_line_wrap_mode,
            self.selected_diff_line_index,
            20,
        );
        true
    }
}
