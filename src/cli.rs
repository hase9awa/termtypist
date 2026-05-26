use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::app::LaunchRequest;
use crate::config::Config;
use crate::storage::{ResultRow, Storage};
use crate::typing::{Mode, QuoteLength};

#[derive(Parser, Debug)]
#[command(
    name = "termtypist",
    version,
    about = "A terminal typing trainer inspired by Monkeytype"
)]
pub struct Cli {
    #[arg(long)]
    pub time: Option<u64>,
    #[arg(long)]
    pub words: Option<usize>,
    #[arg(long)]
    pub dictionary: Option<String>,
    #[arg(long)]
    pub theme: Option<String>,
    #[arg(long)]
    pub punctuation: bool,
    #[arg(long)]
    pub numbers: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Quote(QuoteArgs),
    Custom(CustomArgs),
    Stats {
        #[arg(long)]
        language: Option<String>,
        #[arg(long)]
        mode: Option<String>,
    },
    Replay {
        #[command(subcommand)]
        command: ReplayCommand,
    },
    Theme {
        #[command(subcommand)]
        command: ThemeCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Args, Debug)]
pub struct QuoteArgs {
    #[arg(long, value_enum, default_value_t = CliQuoteLength::Random)]
    pub length: CliQuoteLength,
}

#[derive(Args, Debug)]
pub struct CustomArgs {
    pub file: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum ReplayCommand {
    Last,
}

#[derive(Subcommand, Debug)]
pub enum ThemeCommand {
    List,
    Set { name: String },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    Export,
    Import { file: PathBuf },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum CliQuoteLength {
    Short,
    Medium,
    Long,
    Random,
}

impl From<CliQuoteLength> for QuoteLength {
    fn from(value: CliQuoteLength) -> Self {
        match value {
            CliQuoteLength::Short => Self::Short,
            CliQuoteLength::Medium => Self::Medium,
            CliQuoteLength::Long => Self::Long,
            CliQuoteLength::Random => Self::Random,
        }
    }
}

impl Cli {
    pub fn apply_overrides(&self, config: &mut Config) {
        if let Some(time) = self.time {
            config.default_mode = "time".to_string();
            config.default_time = time;
        }
        if let Some(words) = self.words {
            config.default_mode = "words".to_string();
            config.default_words = words;
        }
        if let Some(dictionary) = &self.dictionary {
            config.language = dictionary.clone();
        }
        if let Some(theme) = &self.theme {
            config.theme = theme.clone();
        }
        if self.punctuation {
            config.punctuation = true;
        }
        if self.numbers {
            config.numbers = true;
        }
    }

    pub fn to_launch_request(&self) -> Result<LaunchRequest> {
        let mode = if let Some(command) = &self.command {
            match command {
                Command::Quote(args) => Mode::Quote(args.length.into()),
                Command::Custom(args) => {
                    let text = read_custom_text(args)?;
                    Mode::Custom(text)
                }
                Command::Replay {
                    command: ReplayCommand::Last,
                } => {
                    return Ok(LaunchRequest::ReplayLast);
                }
                Command::Stats { .. } | Command::Theme { .. } | Command::Config { .. } => {
                    Mode::Time(60)
                }
            }
        } else if let Some(time) = self.time {
            Mode::Time(time)
        } else if let Some(words) = self.words {
            Mode::Words(words)
        } else {
            Mode::LastConfig
        };

        Ok(LaunchRequest::Mode(mode))
    }
}

fn read_custom_text(args: &CustomArgs) -> Result<String> {
    if let Some(file) = &args.file {
        return fs::read_to_string(file)
            .with_context(|| format!("failed to read custom text from {}", file.display()));
    }

    let mut text = String::new();
    io::stdin()
        .read_to_string(&mut text)
        .context("failed to read custom text from stdin")?;
    Ok(text)
}

pub fn print_stats(store: &Storage, results: &[ResultRow]) -> Result<()> {
    let best = store.personal_best(None, None)?;
    println!("termtypist stats");
    if let Some(best) = best {
        println!(
            "best: {:.0} wpm, {:.1}% acc, {} errors, {}",
            best.wpm, best.accuracy, best.errors, best.created_at
        );
    } else {
        println!("best: no results yet");
    }

    if results.is_empty() {
        println!("history: empty");
        return Ok(());
    }

    println!("recent:");
    for row in results {
        println!(
            "#{:03} {:<10} {:<10} {:>5.0}/{:<5.0} wpm {:>5.1}% acc {:>3} errors {:>4.0}s {:>4} chars {}",
            row.id,
            row.mode,
            row.language,
            row.wpm,
            row.raw_wpm,
            row.accuracy,
            row.errors,
            row.duration_sec,
            row.input_text.chars().count(),
            row.created_at
        );
    }

    Ok(())
}

pub fn handle_theme_command(command: &ThemeCommand, config: &mut Config) -> Result<()> {
    match command {
        ThemeCommand::List => {
            for theme in crate::themes::Theme::available() {
                let marker = if theme.name == config.theme { "*" } else { " " };
                println!("{marker} {}", theme.name);
            }
        }
        ThemeCommand::Set { name } => {
            crate::themes::Theme::named(name).with_context(|| format!("unknown theme: {name}"))?;
            config.theme = name.clone();
            config.save()?;
            println!("theme set to {name}");
        }
    }
    Ok(())
}

pub fn handle_config_command(command: &ConfigCommand, config: &mut Config) -> Result<()> {
    match command {
        ConfigCommand::Export => {
            println!("{}", toml::to_string_pretty(config)?);
        }
        ConfigCommand::Import { file } => {
            let raw = fs::read_to_string(file)
                .with_context(|| format!("failed to read config from {}", file.display()))?;
            let mut imported = toml::from_str::<Config>(&raw).context("invalid config file")?;
            imported.normalize_mode_choices();
            imported.normalize_speed_unit();
            imported.normalize_interface_language();
            imported.normalize_key_sound_style();
            imported.normalize_cursor_style();
            imported.normalize_keybindings(&raw);
            imported.save()?;
            println!("config imported from {}", file.display());
        }
    }
    Ok(())
}
