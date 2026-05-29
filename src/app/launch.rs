use std::path::PathBuf;

use color_eyre::eyre::WrapErr;

use crate::{event::EventHandler, git::BlameTarget, theme};

use super::App;

mod preferences;
mod state;

use self::preferences::LaunchPreference;

#[derive(Debug, Default, Clone)]
pub struct AppLaunchOptions {
    pub repo_root: Option<PathBuf>,
    pub initial_blame_target: Option<BlameTarget>,
    pub chooser_file: Option<PathBuf>,
}

impl App {
    pub async fn new(options: AppLaunchOptions) -> color_eyre::Result<Self> {
        let repo_root = match options.repo_root {
            Some(path) => path,
            None => std::env::current_dir().wrap_err("failed to resolve current directory")?,
        };
        let preference = LaunchPreference::from_config();
        theme::set_active_theme(&preference.theme_name, preference.theme_mode);
        let mut app = Self::build_base_app(
            repo_root,
            options.chooser_file,
            EventHandler::new(),
            preference,
        );
        app.queue_initial_working_tree_status_load();
        if let Some(target) = options.initial_blame_target {
            app.open_blame_target(target);
        }
        Ok(app)
    }

    #[doc(hidden)]
    pub fn new_for_benchmarks(repo_root: PathBuf) -> Self {
        let preference = LaunchPreference::for_benchmarks();
        theme::set_active_theme(&preference.theme_name, preference.theme_mode);
        Self::build_base_app(
            repo_root,
            None,
            EventHandler::without_event_task(),
            preference,
        )
    }
}
