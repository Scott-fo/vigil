use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use color_eyre::eyre::{WrapErr, eyre};

use crate::{
    app::AppLaunchOptions,
    git::{BlameTarget, git_output},
};

#[derive(Debug, Clone)]
pub(super) struct BlameLocation {
    file_path: PathBuf,
    line_number: usize,
}

impl FromStr for BlameLocation {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (file_part, line_part) = value
            .rsplit_once(':')
            .ok_or_else(|| "expected <file>:<line>".to_owned())?;

        let line_number = line_part
            .parse::<usize>()
            .map_err(|_| format!("invalid line number: {line_part}"))?;

        if line_number == 0 {
            return Err("line number must be >= 1".to_owned());
        }

        Ok(Self {
            file_path: PathBuf::from(file_part),
            line_number,
        })
    }
}

pub(super) async fn resolve_blame_launch_options(
    target: BlameLocation,
) -> color_eyre::Result<AppLaunchOptions> {
    let cwd = std::env::current_dir().wrap_err("failed to resolve current directory")?;
    let absolute_file = absolute_target_path(&cwd, &target.file_path);
    let repo_root = resolve_target_repo_root(&absolute_file).await?;
    let relative_file = repo_relative_target_path(&repo_root, &absolute_file)?;

    Ok(AppLaunchOptions {
        repo_root: Some(repo_root),
        initial_blame_target: Some(BlameTarget {
            file_path: relative_file,
            line_number: target.line_number,
        }),
        chooser_file: None,
    })
}

fn absolute_target_path(cwd: &Path, file_path: &Path) -> PathBuf {
    if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        cwd.join(file_path)
    }
}

async fn resolve_target_repo_root(absolute_file: &Path) -> color_eyre::Result<PathBuf> {
    let probe_dir = absolute_file.parent().unwrap_or(absolute_file);
    Ok(PathBuf::from(
        git_output(probe_dir, &["rev-parse", "--show-toplevel"])
            .await?
            .trim(),
    ))
}

fn repo_relative_target_path(repo_root: &Path, absolute_file: &Path) -> color_eyre::Result<String> {
    Ok(absolute_file
        .strip_prefix(repo_root)
        .map_err(|_| {
            eyre!(
                "file is not inside the git repository: {}",
                absolute_file.display()
            )
        })?
        .to_string_lossy()
        .replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use super::BlameLocation;

    #[test]
    fn parses_blame_location() {
        let target = BlameLocation::from_str("src/main.rs:42").unwrap();

        assert_eq!(target.file_path, PathBuf::from("src/main.rs"));
        assert_eq!(target.line_number, 42);
    }

    #[test]
    fn rejects_missing_line_separator() {
        let error = BlameLocation::from_str("src/main.rs").unwrap_err();

        assert_eq!(error, "expected <file>:<line>");
    }

    #[test]
    fn rejects_invalid_line_number() {
        let error = BlameLocation::from_str("src/main.rs:not-a-number").unwrap_err();

        assert_eq!(error, "invalid line number: not-a-number");
    }

    #[test]
    fn rejects_zero_line_number() {
        let error = BlameLocation::from_str("src/main.rs:0").unwrap_err();

        assert_eq!(error, "line number must be >= 1");
    }
}
