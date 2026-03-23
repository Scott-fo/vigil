use super::{ThemeMode, ThemePreference};

use std::{env, fs, io, path::PathBuf};

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

    let preference = ThemePreference {
        theme: Some(theme.to_owned()),
        mode: Some(mode),
    };

    let contents = serde_json::to_string_pretty(&preference).map_err(io::Error::other)?;
    fs::write(path, format!("{contents}\n"))
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

    serde_json::from_str(&contents).unwrap_or(ThemePreference {
        theme: None,
        mode: None,
    })
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
