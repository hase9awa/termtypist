mod app;
mod audio;
mod cli;
mod config;
mod dictionaries;
mod keyboard;
mod metrics;
mod quotes;
mod storage;
mod themes;
mod tui;
mod typing;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    let mut config = config::Config::load_or_default()?;
    dictionaries::write_default_files()?;
    themes::Theme::write_default_files()?;
    quotes::write_default_files()?;
    cli.apply_overrides(&mut config);

    match cli.command.as_ref() {
        Some(cli::Command::Stats { language, mode }) => {
            let store = storage::Storage::open_default()?;
            let results = store.recent_results(20, language.as_deref(), mode.as_deref())?;
            cli::print_stats(&store, &results)?;
        }
        Some(cli::Command::Theme { command }) => cli::handle_theme_command(command, &mut config)?,
        Some(cli::Command::Config { command }) => cli::handle_config_command(command, &mut config)?,
        _ => {
            let launch = cli.to_launch_request()?;
            tui::run(config, launch)?;
        }
    }

    Ok(())
}
