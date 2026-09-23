use color_eyre::eyre::WrapErr;
use crossterm::{event::MouseEvent, terminal};
use ratatui::layout::Position;

use super::super::App;
use crate::ui;

impl App {
    pub(super) fn handle_mouse_moved(
        &mut self,
        mouse_event: MouseEvent,
    ) -> color_eyre::Result<bool> {
        let (width, height) = terminal::size().wrap_err("failed to read terminal size")?;
        Ok(self.set_mouse_position(
            Position::new(mouse_event.column, mouse_event.row),
            width,
            height,
        ))
    }

    /// Records the pointer cell. Returns true only when the element under the
    /// pointer changed, which is when hover highlights need a redraw.
    pub(in crate::app) fn set_mouse_position(
        &mut self,
        position: Position,
        terminal_width: u16,
        terminal_height: u16,
    ) -> bool {
        let hover_at = |app: &App, position: Option<Position>| {
            position.and_then(|position| {
                ui::hover_target_at(app, position.x, position.y, terminal_width, terminal_height)
            })
        };
        let before = hover_at(self, self.mouse_position);
        self.mouse_position = Some(position);
        before != hover_at(self, self.mouse_position)
    }
}
