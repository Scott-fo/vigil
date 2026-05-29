use crate::theme::{self, ThemeMode, config};

use super::super::DiffViewMode;

#[derive(Debug, Clone)]
pub(super) struct LaunchPreference {
    pub theme_name: String,
    pub theme_mode: ThemeMode,
    pub diff_view_mode: DiffViewMode,
}

impl LaunchPreference {
    pub(super) fn from_config() -> Self {
        let preference = config::read_tui_preference();
        Self {
            theme_name: theme::resolve_theme_name(preference.theme.as_deref()).to_string(),
            theme_mode: preference.mode.unwrap_or(ThemeMode::Dark),
            diff_view_mode: preference
                .diff_view_mode
                .as_deref()
                .and_then(|value| value.parse().ok())
                .unwrap_or(DiffViewMode::Split),
        }
    }

    pub(super) fn for_benchmarks() -> Self {
        Self {
            theme_name: theme::resolve_theme_name(None).to_string(),
            theme_mode: ThemeMode::Dark,
            diff_view_mode: DiffViewMode::Split,
        }
    }
}
