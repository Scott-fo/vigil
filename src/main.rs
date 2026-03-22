use crate::{
    app::{App, AppLaunchOptions},
    git::BlameTarget,
};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use color_eyre::eyre::{WrapErr, eyre};
use std::{
    io::stdout,
    path::{Path, PathBuf},
};

pub mod app;
pub mod event;
pub mod git;
pub mod sidebar;
pub mod splash;
pub mod theme;
pub mod ui;
pub mod watcher;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let launch_options = parse_launch_options()?;
    let terminal = ratatui::init();
    let _ = execute!(stdout(), EnableMouseCapture);
    let result = App::new_with_options(launch_options).await?.run(terminal).await;
    ratatui::restore();
    let _ = execute!(stdout(), DisableMouseCapture);
    result
}

fn parse_launch_options() -> color_eyre::Result<AppLaunchOptions> {
    let mut options = AppLaunchOptions::default();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--chooser-file" => {
                let path = args
                    .next()
                    .ok_or_else(|| eyre!("usage: vigil --chooser-file <path>"))?;
                options.chooser_file_path = Some(PathBuf::from(path));
            }
            "blame" => {
                let target = args
                    .next()
                    .ok_or_else(|| eyre!("usage: vigil blame <file>:<line>"))?;
                if args.next().is_some() {
                    return Err(eyre!("usage: vigil blame <file>:<line>"));
                }
                let blame_options = parse_blame_launch_options(&target)?;
                options.repo_root = blame_options.repo_root;
                options.initial_blame_target = blame_options.initial_blame_target;
                break;
            }
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(eyre!("unknown argument: {other}")),
        }
    }

    Ok(options)
}

fn parse_blame_launch_options(target: &str) -> color_eyre::Result<AppLaunchOptions> {
    let (file_part, line_part) = target
        .rsplit_once(':')
        .ok_or_else(|| eyre!("usage: vigil blame <file>:<line>"))?;
    let line_number = line_part
        .parse::<usize>()
        .map_err(|_| eyre!("invalid line number: {line_part}"))?;
    if line_number == 0 {
        return Err(eyre!("line number must be >= 1"));
    }

    let cwd = std::env::current_dir().wrap_err("failed to resolve current directory")?;
    let absolute_file = absolutize_path(&cwd, Path::new(file_part));
    let repo_root = resolve_repo_root_for_target(&absolute_file)?;
    let relative_file = absolute_file
        .strip_prefix(&repo_root)
        .map_err(|_| eyre!("file is not inside the git repository: {}", absolute_file.display()))?
        .to_string_lossy()
        .replace('\\', "/");

    Ok(AppLaunchOptions {
        repo_root: Some(repo_root),
        initial_blame_target: Some(BlameTarget {
            file_path: relative_file,
            line_number,
        }),
        chooser_file_path: None,
    })
}

fn absolutize_path(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn resolve_repo_root_for_target(path: &Path) -> color_eyre::Result<PathBuf> {
    let probe_dir = path.parent().unwrap_or(path);
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(probe_dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .wrap_err("failed to resolve git repository root")?;

    if !output.status.success() {
        return Err(eyre!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        ));
    }

    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

fn print_help() {
    println!("vigil");
    println!();
    println!("Usage:");
    println!("  vigil");
    println!("  vigil --chooser-file <path>");
    println!("  vigil blame <file>:<line>");
}
