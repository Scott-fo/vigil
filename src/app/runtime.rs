use super::App;

mod dispatch;
mod handlers;
mod render;
mod shutdown;

impl App {
    pub async fn run(mut self, mut terminal: ratatui::DefaultTerminal) -> color_eyre::Result<()> {
        self.redraw(&mut terminal)?;

        while self.running {
            let event = self.events.next().await?;
            self.handle_runtime_event(event, &mut terminal).await?;
        }

        Ok(())
    }
}
