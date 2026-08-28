use crate::theme::{self, ThemeMode, config};

use super::super::{DiffLineWrapMode, DiffViewMode};

#[derive(Debug, Clone)]
pub(super) struct LaunchPreference {
    pub theme_name: String,
    pub theme_mode: ThemeMode,
    pub diff_view_mode: DiffViewMode,
    pub diff_line_wrap_mode: DiffLineWrapMode,
    pub exclude_file_suffixes: Vec<String>,
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
            diff_line_wrap_mode: preference
                .diff_line_wrap_mode
                .as_deref()
                .and_then(|value| value.parse().ok())
                .unwrap_or_default(),
            exclude_file_suffixes: preference.exclude_file_suffixes,
        }
    }

    pub(super) fn for_benchmarks() -> Self {
        Self {
            theme_name: theme::resolve_theme_name(None).to_string(),
            theme_mode: ThemeMode::Dark,
            diff_view_mode: DiffViewMode::Split,
            diff_line_wrap_mode: DiffLineWrapMode::default(),
            exclude_file_suffixes: Vec::new(),
        }
    }
}
