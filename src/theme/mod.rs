pub mod bank;

use crate::theme::bank::THEMES;
use ratatui::style::Color;
use strum_macros::{EnumString, IntoStaticStr};

use std::{
    env, fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU8, AtomicUsize, Ordering},
};

pub const DEFAULT_THEME_NAME: &str = "catppuccin-macchiato";

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[repr(u8)]
#[strum(serialize_all = "snake_case")]
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

    fn as_index(self) -> u8 {
        self as u8
    }

    fn from_index(value: u8) -> Self {
        match value {
            1 => Self::Light,
            _ => Self::Dark,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThemePreference {
    pub theme: Option<String>,
    pub mode: Option<ThemeMode>,
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
    ACTIVE_THEME_MODE.store(mode.as_index(), Ordering::Relaxed);
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

pub fn read_theme_preference() -> ThemePreference {
    let file_preference = read_theme_preference_from_config();
    let env_theme = env::var("VIGIL_THEME")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let env_mode = env::var("VIGIL_THEME_MODE")
        .ok()
        .and_then(|value| value.trim().parse().ok());

    ThemePreference {
        theme: env_theme.or(file_preference.theme),
        mode: env_mode.or(file_preference.mode),
    }
}

pub fn persist_theme_preference(theme: &str, mode: ThemeMode) -> io::Result<()> {
    let path = resolve_tui_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        format!(
            "{{\n  \"theme\": \"{}\",\n  \"theme_mode\": \"{}\"\n}}\n",
            theme,
            mode.as_str()
        ),
    )
}

fn read_theme_preference_from_config() -> ThemePreference {
    let primary_path = resolve_tui_config_path();
    let contents = fs::read_to_string(&primary_path);
    let Ok(contents) = contents else {
        return ThemePreference {
            theme: None,
            mode: None,
        };
    };

    ThemePreference {
        theme: extract_json_string_value(&contents, "theme"),
        mode: extract_json_string_value(&contents, "theme_mode")
            .and_then(|value| value.trim().parse().ok()),
    }
}

fn resolve_tui_config_path() -> PathBuf {
    if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
        let trimmed = xdg_data_home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("vigil").join("tui.json");
        }
    }

    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("share")
        .join("vigil")
        .join("tui.json")
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn extract_json_string_value(contents: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let start = contents.find(&needle)?;
    let rest = &contents[start + needle.len()..];
    let colon = rest.find(':')?;
    let mut chars = rest[colon + 1..].chars();

    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        if ch != '"' {
            return None;
        }

        let mut value = String::new();
        let mut escaped = false;
        for ch in chars {
            if escaped {
                value.push(ch);
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => return Some(value),
                _ => value.push(ch),
            }
        }
        return None;
    }

    None
}
