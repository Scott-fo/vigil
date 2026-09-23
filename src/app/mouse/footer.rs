use super::super::App;
use crate::ui::FooterAction;

impl App {
    /// Runs a clicked footer control through the same methods its key uses.
    pub(super) async fn run_footer_action(
        &mut self,
        action: FooterAction,
    ) -> color_eyre::Result<()> {
        match action {
            FooterAction::ToggleDiffViewMode => self.toggle_diff_view_mode(),
            FooterAction::ToggleLineWrap => self.toggle_diff_line_wrap_mode(),
            FooterAction::ToggleSidebar => self.toggle_sidebar_hidden(),
            FooterAction::OpenHelp => self.help_modal_open = true,
            FooterAction::SwitchPane => self.switch_active_pane(),
            FooterAction::ToggleStage => self.toggle_selected_file_stage().await?,
            FooterAction::FindFile => self.open_file_search_modal().await?,
            FooterAction::SearchDiff => self.open_diff_search_modal(),
        }
        Ok(())
    }
}
