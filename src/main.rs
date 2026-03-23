use vigil::{app::App, cli::Cli};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use std::io::stdout;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let launch_options = Cli::build().await?;

    let terminal = ratatui::init();
    let _ = execute!(stdout(), EnableMouseCapture);

    let result = App::new(launch_options).await?.run(terminal).await;

    ratatui::restore();
    let _ = execute!(stdout(), DisableMouseCapture);

    result
}
