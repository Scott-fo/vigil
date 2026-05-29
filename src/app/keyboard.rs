use crossterm::event::KeyEvent;

mod global;
mod modal;
mod open;
mod pane;

use super::{App, editor::AppCommand};

pub(super) enum KeyOutcome {
    Handled,
    Command(AppCommand),
}

impl KeyOutcome {
    fn into_command(self) -> Option<AppCommand> {
        match self {
            KeyOutcome::Handled => None,
            KeyOutcome::Command(command) => Some(command),
        }
    }
}

fn handled() -> color_eyre::Result<Option<KeyOutcome>> {
    Ok(Some(KeyOutcome::Handled))
}

impl App {
    pub(super) async fn handle_key_event(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<Option<AppCommand>> {
        if self.handle_modal_key(key_event).await? {
            return Ok(None);
        }

        if let Some(outcome) = self.handle_global_key(key_event).await? {
            return Ok(outcome.into_command());
        }

        if let Some(outcome) = self.handle_pane_key(key_event).await? {
            return Ok(outcome.into_command());
        }

        Ok(None)
    }
}
