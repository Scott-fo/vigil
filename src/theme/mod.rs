pub mod bank;
pub mod config;

use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use strum_macros::{EnumString, IntoStaticStr};

use self::bank::THEMES;

pub const DEFAULT_THEME_NAME: &str = "catppuccin-macchiato";

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr, Serialize, Deserialize)]
#[repr(u8)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Dark = 0,
    Light = 1,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    fn from_index(value: u8) -> Self {
        match value {
            1 => Self::Light,
            _ => Self::Dark,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiPreference {
    pub theme: Option<String>,
    #[serde(rename = "theme_mode")]
    pub mode: Option<ThemeMode>,
    #[serde(rename = "diff_view_mode")]
    pub diff_view_mode: Option<String>,
}

static ACTIVE_THEME_INDEX: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_THEME_MODE: AtomicU8 = AtomicU8::new(0);

#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,
    pub text: Color,
    pub text_muted: Color,
    pub selected_list_item_text: Color,
    pub background: Color,
    pub background_panel: Color,
    pub background_element: Color,
    pub background_menu: Color,
    pub border: Color,
    pub border_active: Color,
    pub border_subtle: Color,
    pub diff_added: Color,
    pub diff_removed: Color,
    pub diff_context: Color,
    pub diff_hunk_header: Color,
    pub diff_highlight_added: Color,
    pub diff_highlight_removed: Color,
    pub diff_added_bg: Color,
    pub diff_removed_bg: Color,
    pub diff_context_bg: Color,
    pub diff_line_number: Color,
    pub diff_added_line_number_bg: Color,
    pub diff_removed_line_number_bg: Color,
    pub syntax_comment: Color,
    pub syntax_keyword: Color,
    pub syntax_function: Color,
    pub syntax_variable: Color,
    pub syntax_string: Color,
    pub syntax_number: Color,
    pub syntax_type: Color,
    pub syntax_operator: Color,
    pub syntax_punctuation: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct ThemeDefinition {
    pub name: &'static str,
    pub dark: ThemePalette,
    pub light: ThemePalette,
}

impl ThemeDefinition {
    pub fn palette(self, mode: ThemeMode) -> ThemePalette {
        match mode {
            ThemeMode::Dark => self.dark,
            ThemeMode::Light => self.light,
        }
    }
}

pub fn all() -> &'static [ThemeDefinition] {
    THEMES
}

pub fn names() -> impl Iterator<Item = &'static str> {
    THEMES.iter().map(|theme| theme.name)
}

pub fn find(name: &str) -> Option<&'static ThemeDefinition> {
    THEMES.iter().find(|theme| theme.name == name)
}

pub fn resolve_theme_name(preferred: Option<&str>) -> &'static str {
    if let Some(name) = preferred
        && let Some(theme) = find(name)
    {
        return theme.name;
    }

    DEFAULT_THEME_NAME
}

pub fn set_active_theme(name: &str, mode: ThemeMode) -> &'static ThemeDefinition {
    let index = THEMES
        .iter()
        .position(|theme| theme.name == name)
        .unwrap_or(0);

    ACTIVE_THEME_INDEX.store(index, Ordering::Relaxed);
    ACTIVE_THEME_MODE.store(mode as u8, Ordering::Relaxed);

    &THEMES[index]
}

pub fn active_theme() -> &'static ThemeDefinition {
    &THEMES[ACTIVE_THEME_INDEX
        .load(Ordering::Relaxed)
        .min(THEMES.len().saturating_sub(1))]
}

pub fn active_mode() -> ThemeMode {
    ThemeMode::from_index(ACTIVE_THEME_MODE.load(Ordering::Relaxed))
}

pub fn active_palette() -> ThemePalette {
    active_theme().palette(active_mode())
}
