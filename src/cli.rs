use crate::app::AppLaunchOptions;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod blame;

use self::blame::{BlameLocation, resolve_blame_launch_options};

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
            options.initial_blame_target = blame_options.initial_blame_target;
        }

        Ok(options)
    }
}
