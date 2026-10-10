mod cli;
mod commands;
mod config;
mod display;
mod gemfile;
mod signals;
mod state;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { path }                     => commands::init(&path),
        Commands::State { json, raw, diff, r#type } => commands::state(json, raw, diff, r#type),
        Commands::Exec { command }                  => commands::exec(&command),
        Commands::Sync                              => commands::sync(),
        Commands::Add { gem, version }              => commands::add(&gem, version.as_deref()),
        Commands::Remove { gem }                    => commands::remove(&gem),
        Commands::Undo                              => commands::undo(),
        Commands::Bootstrap { version, devkit }     => commands::bootstrap(version.as_deref(), devkit),
        Commands::Run { command }                   => commands::run(&command),
        Commands::Env                               => commands::env(),
        Commands::Shell                             => commands::shell(),
    }
}
