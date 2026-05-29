use crossterm::terminal;

use super::super::App;
use crate::ui;

impl App {
    pub(super) fn redraw(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<()> {
        if let Ok((width, height)) = terminal::size()
            && let Some(viewport) = ui::prepare_diff_viewport_for_terminal(self, width, height)
        {
            self.update_diff_viewport(viewport.mode, viewport.width, viewport.start, viewport.end);
            self.maybe_queue_diff_highlight();
        }
        terminal.draw(|frame| ui::render(frame, self))?;
        self.maybe_queue_diff_highlight();
        Ok(())
    }
}
