use color_eyre::eyre::WrapErr;
use crossterm::{event::MouseEvent, terminal};
use ratatui::layout::Position;

use super::super::{ActivePane, App};
use crate::ui;

impl App {
    pub(super) fn handle_mouse_scroll(
        &mut self,
        mouse_event: MouseEvent,
        delta: i32,
    ) -> color_eyre::Result<()> {
        let (width, height) = terminal::size().wrap_err("failed to read terminal size")?;
        self.mouse_position = Some(Position::new(mouse_event.column, mouse_event.row));
        match ui::hovered_pane_at(self, mouse_event.column, mouse_event.row, width, height) {
            Some(ActivePane::Sidebar) => self.scroll_sidebar(delta),
            Some(ActivePane::Diff) => {
                self.clear_diff_text_selection();
                self.page_or_scroll_diff(delta);
            }
            None => {}
        }
        Ok(())
    }
}
