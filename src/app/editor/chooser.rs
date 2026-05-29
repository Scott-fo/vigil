use color_eyre::eyre::WrapErr;
use tokio::fs;

use super::{super::App, target::EditorOpenTarget};

impl App {
    pub(super) async fn write_chooser_selection_and_exit(
        &mut self,
        target: &EditorOpenTarget,
    ) -> color_eyre::Result<()> {
        let Some(chooser_file_path) = self.chooser_file_path.as_ref() else {
            return Ok(());
        };

        let payload = match target.line_number {
            Some(line_number) => format!("{}\n{}\n", target.full_path.display(), line_number),
            None => format!("{}\n\n", target.full_path.display()),
        };
        fs::write(chooser_file_path, payload)
            .await
            .wrap_err_with(|| {
                format!(
                    "failed to write chooser selection to {}",
                    chooser_file_path.display()
                )
            })?;
        self.running = false;
        Ok(())
    }
}
