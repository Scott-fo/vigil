use crate::app::App;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use std::io::stdout;

pub mod app;
pub mod event;
pub mod git;
pub mod sidebar;
pub mod splash;
pub mod ui;
pub mod watcher;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let _ = execute!(stdout(), EnableMouseCapture);
    let result = App::new().await?.run(terminal).await;
    ratatui::restore();
    let _ = execute!(stdout(), DisableMouseCapture);
    result
}
