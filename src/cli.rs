use crate::{
    app::AppLaunchOptions,
    git::{BlameTarget, git_output},
};

use clap::{Parser, Subcommand};
use color_eyre::eyre::{WrapErr, eyre};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(Debug, Parser)]
#[command(name = "vigil", disable_help_subcommand = true)]
pub struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    chooser_file: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Blame {
        #[arg(value_name = "FILE:LINE")]
        target: BlameLocation,
    },
}

#[derive(Debug, Clone)]
struct BlameLocation {
    file_path: PathBuf,
    line_number: usize,
}

#[derive(Debug, Clone)]
struct BlameOptions {
    repo_root: Option<PathBuf>,
    blame_target: Option<BlameTarget>,
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

impl Cli {
    pub async fn build() -> color_eyre::Result<AppLaunchOptions> {
        let this = Self::parse();

        let mut options = AppLaunchOptions {
            chooser_file: this.chooser_file,
            ..AppLaunchOptions::default()
        };

        if let Some(Command::Blame { target }) = this.command {
            let blame_options = resolve_blame_launch_options(target).await?;

            options.repo_root = blame_options.repo_root;
            options.initial_blame_target = blame_options.blame_target;
        }

        Ok(options)
    }
}

async fn resolve_blame_launch_options(target: BlameLocation) -> color_eyre::Result<BlameOptions> {
    let cwd = std::env::current_dir().wrap_err("failed to resolve current directory")?;

    let absolute_file = {
        let cwd: &Path = &cwd;
        let path: &Path = &target.file_path;

        if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        }
    };

    let probe_dir = &absolute_file.parent().unwrap_or(&absolute_file);
    let repo_root = PathBuf::from(
        git_output(probe_dir, &["rev-parse", "--show-toplevel"])
            .await?
            .trim(),
    );

    let relative_file = absolute_file
        .strip_prefix(&repo_root)
        .map_err(|_| {
            eyre!(
                "file is not inside the git repository: {}",
                absolute_file.display()
            )
        })?
        .to_string_lossy()
        .replace('\\', "/");

    Ok(BlameOptions {
        repo_root: Some(repo_root),
        blame_target: Some(BlameTarget {
            file_path: relative_file,
            line_number: target.line_number,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::BlameLocation;
    use std::{path::PathBuf, str::FromStr};

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
