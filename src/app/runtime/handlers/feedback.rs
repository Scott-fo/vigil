use super::super::super::{App, SnackbarVariant};

impl App {
    pub(in crate::app) fn handle_remote_sync_finished(&mut self, result: Result<String, String>) {
        self.remote_sync = None;
        match result {
            Ok(message) => {
                self.show_snackbar(message, SnackbarVariant::Info);
            }
            Err(message) => {
                self.show_snackbar(message, SnackbarVariant::Error);
            }
        }
    }

    pub(in crate::app) fn handle_clear_snackbar(&mut self, generation: u64) -> bool {
        if self.snackbar_generation != generation {
            return false;
        }

        self.snackbar_notice = None;
        true
    }
}
