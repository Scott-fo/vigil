use std::{io::stdout, process::Stdio};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use tokio::task;

use super::super::App;

impl App {
    pub(super) async fn run_editor_command(
        &mut self,
        command: String,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> Result<Result<std::process::ExitStatus, std::io::Error>, task::JoinError> {
        self.events.suspend();
        let _ = execute!(stdout(), DisableMouseCapture);
        ratatui::restore();

        let result = task::spawn_blocking(move || {
            std::process::Command::new("sh")
                .args(["-lc", &command])
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
        })
        .await;

        *terminal = ratatui::init();
        let _ = execute!(stdout(), EnableMouseCapture);
        self.events.resume();

        result
    }
}
