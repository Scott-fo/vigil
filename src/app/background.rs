use tokio::task;

use super::{App, SnackbarNotice, SnackbarVariant};
use crate::{event::Event, git};

impl App {
    /// Shows a transient notice. Errors stay up longer since they are often
    /// several lines of git output worth reading.
    pub(super) fn show_snackbar(&mut self, message: String, variant: SnackbarVariant) {
        let visible_for = match variant {
            SnackbarVariant::Info => std::time::Duration::from_secs(2),
            SnackbarVariant::Error => std::time::Duration::from_secs(6),
        };
        self.snackbar_generation = self.snackbar_generation.saturating_add(1);
        let generation = self.snackbar_generation;
        self.snackbar_notice = Some(SnackbarNotice { message, variant });

        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            tokio::time::sleep(visible_for).await;
            let _ = sender.send(Event::ClearSnackbar(generation));
        }));
    }

    pub(super) fn spawn_highlight_registry_init(&mut self) {
        if self.highlight_registry.is_some() || self.highlight_registry_loading {
            return;
        }

        let initial_filetypes = self
            .selected_file()
            .and_then(|file| file.filetype)
            .into_iter()
            .collect::<Vec<_>>();
        self.highlight_registry_loading = true;
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = task::spawn_blocking(move || {
                git::HighlightRegistry::new_for_filetypes(initial_filetypes)
            })
            .await;
            let event = match result {
                Ok(Ok(registry)) => Event::HighlightRegistryReady(Ok(registry.into())),
                Ok(Err(error)) => Event::HighlightRegistryReady(Err(error.to_string())),
                Err(error) => Event::HighlightRegistryReady(Err(error.to_string())),
            };
            let _ = sender.send(event);
        }));
    }

    pub(super) fn track_background_task(&mut self, handle: task::JoinHandle<()>) {
        self.background_tasks.retain(|task| !task.is_finished());
        self.background_tasks.push(handle);
    }

    pub(super) fn abort_background_tasks(&mut self) {
        for task in self.background_tasks.drain(..) {
            task.abort();
        }
    }
}
