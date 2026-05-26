use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const MAX_MODE_CHOICES: usize = 6;
pub const DEFAULT_TIME_MODES: [u64; 4] = [15, 30, 60, 120];
pub const DEFAULT_WORD_MODES: [usize; 4] = [10, 25, 50, 100];
pub const LANGUAGE_CATEGORIES: [&str; 2] = ["english", "russian"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_mode: String,
    pub default_time: u64,
    pub default_words: usize,
    pub time_modes: Vec<u64>,
    pub word_modes: Vec<usize>,
    pub language: String,
    pub interface_language: String,
    pub theme: String,
    pub visual_style: String,
    pub cursor_style: String,
    pub quick_restart: String,
    pub difficulty: String,
    pub repeat_quotes: String,
    pub blind_mode: bool,
    pub always_show_words_history: bool,
    pub speed_unit: String,
    pub min_speed: u64,
    pub min_accuracy: u64,
    pub min_word_burst: String,
    pub save_results: bool,
    pub key_sound_style: String,
    pub punctuation: bool,
    pub numbers: bool,
    pub mouse: bool,
    pub keybindings: KeyBindings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyBindings {
    pub quit: String,
    pub help: String,
    pub pause: String,
    pub close: String,
    pub activate: String,
    pub up: String,
    pub down: String,
    pub left: String,
    pub right: String,
    pub restart: String,
    pub retry_text: String,
    pub save_result: String,
    pub settings: String,
    pub toggle_punctuation: String,
    pub toggle_numbers: String,
    pub mode_time: String,
    pub mode_words: String,
    pub mode_quote: String,
    pub language: String,
    pub amount_left: String,
    pub amount_right: String,
    pub history: String,
    pub heatmap: String,
    pub delete_history: String,
    pub confirm: String,
    pub cancel: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_mode: "time".to_string(),
            default_time: 60,
            default_words: 50,
            time_modes: DEFAULT_TIME_MODES.to_vec(),
            word_modes: DEFAULT_WORD_MODES.to_vec(),
            language: "russian".to_string(),
            interface_language: "en".to_string(),
            theme: "dark".to_string(),
            visual_style: "space".to_string(),
            cursor_style: "block".to_string(),
            quick_restart: "tab".to_string(),
            difficulty: "normal".to_string(),
            repeat_quotes: "off".to_string(),
            blind_mode: false,
            always_show_words_history: false,
            speed_unit: "wpm".to_string(),
            min_speed: 0,
            min_accuracy: 0,
            min_word_burst: "off".to_string(),
            save_results: true,
            key_sound_style: "mechanical".to_string(),
            punctuation: false,
            numbers: false,
            mouse: true,
            keybindings: KeyBindings::default(),
        }
    }
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            quit: "ctrl+c".to_string(),
            help: "f1".to_string(),
            pause: "esc".to_string(),
            close: "esc,q".to_string(),
            activate: "enter,space".to_string(),
            up: "k,up".to_string(),
            down: "j,down".to_string(),
            left: "h,left".to_string(),
            right: "l,right".to_string(),
            restart: "tab".to_string(),
            retry_text: "ctrl+r".to_string(),
            save_result: "ctrl+s".to_string(),
            settings: "ctrl+enter,f2".to_string(),
            toggle_punctuation: "alt+p".to_string(),
            toggle_numbers: "alt+n".to_string(),
            mode_time: "alt+t".to_string(),
            mode_words: "alt+w".to_string(),
            mode_quote: "alt+q".to_string(),
            language: "alt+d".to_string(),
            amount_left: "alt+h".to_string(),
            amount_right: "alt+l".to_string(),
            history: "r".to_string(),
            heatmap: "e".to_string(),
            delete_history: "d,delete".to_string(),
            confirm: "y,enter".to_string(),
            cancel: "n,esc,q".to_string(),
        }
    }
}

impl Config {
    pub fn load_or_default() -> Result<Self> {
        Self::ensure_layout()?;
        let path = Self::path()?;
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        match toml::from_str::<Self>(&raw) {
            Ok(mut config) => {
                let missing_mode_choices =
                    !raw.contains("time_modes") || !raw.contains("word_modes");
                let mut changed = config.disable_legacy_fail_defaults();
                changed |= config.normalize_mode_choices();
                changed |= config.normalize_speed_unit();
                changed |= config.normalize_interface_language();
                changed |= config.migrate_legacy_key_sounds(&raw);
                changed |= config.normalize_key_sound_style();
                changed |= config.normalize_cursor_style();
                changed |= config.normalize_keybindings(&raw);
                changed |= missing_mode_choices;
                changed |= !raw.contains("interface_language");
                changed |= !raw.contains("speed_unit");
                changed |= !raw.contains("key_sound_style");
                changed |= !raw.contains("cursor_style");
                changed |= !raw.contains("[keybindings]");
                if changed {
                    config.save()?;
                }
                Ok(config)
            }
            Err(_) => Ok(Self::default()),
        }
    }

    pub fn save(&self) -> Result<()> {
        Self::ensure_layout()?;
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("failed to write config to {}", path.display()))?;
        Ok(())
    }

    pub fn path() -> Result<PathBuf> {
        Ok(Self::dir()?.join("config.toml"))
    }

    pub fn dir() -> Result<PathBuf> {
        if let Some(dir) = std::env::var_os("TERM_TYPIST_CONFIG_DIR") {
            return Ok(PathBuf::from(dir));
        }
        Ok(dirs::home_dir()
            .context("could not locate home directory")?
            .join(".config")
            .join("termtypist"))
    }

    pub fn languages_dir() -> Result<PathBuf> {
        Ok(Self::dir()?.join("languages"))
    }

    pub fn language_category_dir(category: &str) -> Result<PathBuf> {
        Ok(Self::languages_dir()?.join(category))
    }

    pub fn quotes_dir() -> Result<PathBuf> {
        Ok(Self::dir()?.join("quotes"))
    }

    pub fn themes_dir() -> Result<PathBuf> {
        Ok(Self::dir()?.join("themes"))
    }

    pub fn ensure_layout() -> Result<()> {
        fs::create_dir_all(Self::languages_dir()?)?;
        for category in LANGUAGE_CATEGORIES {
            fs::create_dir_all(Self::language_category_dir(category)?)?;
        }
        fs::create_dir_all(Self::quotes_dir()?)?;
        fs::create_dir_all(Self::themes_dir()?)?;
        Ok(())
    }

    pub fn time_mode_choices(&self) -> Vec<u64> {
        sanitize_u64_modes(&self.time_modes, &DEFAULT_TIME_MODES)
    }

    pub fn word_mode_choices(&self) -> Vec<usize> {
        sanitize_usize_modes(&self.word_modes, &DEFAULT_WORD_MODES)
    }

    pub fn normalize_mode_choices(&mut self) -> bool {
        let time_modes = self.time_mode_choices();
        let word_modes = self.word_mode_choices();
        let changed = self.time_modes != time_modes || self.word_modes != word_modes;
        self.time_modes = time_modes;
        self.word_modes = word_modes;
        changed
    }

    pub fn normalize_speed_unit(&mut self) -> bool {
        if matches!(self.speed_unit.as_str(), "wpm" | "cpm") {
            false
        } else {
            self.speed_unit = "wpm".to_string();
            true
        }
    }

    pub fn normalize_interface_language(&mut self) -> bool {
        let normalized = match self.interface_language.trim().to_ascii_lowercase().as_str() {
            "ru" | "russian" | "русский" => "ru",
            "en" | "english" => "en",
            _ => "en",
        };
        let changed = self.interface_language != normalized;
        self.interface_language = normalized.to_string();
        changed
    }

    pub fn normalize_key_sound_style(&mut self) -> bool {
        if matches!(
            self.key_sound_style.as_str(),
            "off" | "click" | "beep" | "thock" | "crisp" | "mechanical"
        ) {
            false
        } else {
            self.key_sound_style = "mechanical".to_string();
            true
        }
    }

    pub fn normalize_cursor_style(&mut self) -> bool {
        if matches!(self.cursor_style.as_str(), "block" | "underline" | "color") {
            false
        } else {
            self.cursor_style = "block".to_string();
            true
        }
    }

    pub fn normalize_keybindings(&mut self, raw: &str) -> bool {
        let had_keybindings = raw.contains("[keybindings]");
        let mut changed = false;
        if !had_keybindings {
            self.keybindings.restart = self.quick_restart.clone();
            changed = true;
        }
        changed |= fill_empty(&mut self.keybindings.quit, "ctrl+c");
        changed |= fill_empty(&mut self.keybindings.help, "f1");
        changed |= fill_empty(&mut self.keybindings.pause, "esc");
        changed |= fill_empty(&mut self.keybindings.close, "esc,q");
        changed |= fill_empty(&mut self.keybindings.activate, "enter,space");
        changed |= fill_empty(&mut self.keybindings.up, "k,up");
        changed |= fill_empty(&mut self.keybindings.down, "j,down");
        changed |= fill_empty(&mut self.keybindings.left, "h,left");
        changed |= fill_empty(&mut self.keybindings.right, "l,right");
        changed |= fill_empty(&mut self.keybindings.restart, &self.quick_restart);
        changed |= fill_empty(&mut self.keybindings.retry_text, "ctrl+r");
        changed |= fill_empty(&mut self.keybindings.save_result, "ctrl+s");
        changed |= fill_empty(&mut self.keybindings.settings, "ctrl+enter,f2");
        changed |= fill_empty(&mut self.keybindings.toggle_punctuation, "alt+p");
        changed |= fill_empty(&mut self.keybindings.toggle_numbers, "alt+n");
        changed |= fill_empty(&mut self.keybindings.mode_time, "alt+t");
        changed |= fill_empty(&mut self.keybindings.mode_words, "alt+w");
        changed |= fill_empty(&mut self.keybindings.mode_quote, "alt+q");
        changed |= fill_empty(&mut self.keybindings.language, "alt+d");
        changed |= fill_empty(&mut self.keybindings.amount_left, "alt+h");
        changed |= fill_empty(&mut self.keybindings.amount_right, "alt+l");
        changed |= fill_empty(&mut self.keybindings.history, "r");
        changed |= fill_empty(&mut self.keybindings.heatmap, "e");
        changed |= fill_empty(&mut self.keybindings.delete_history, "d,delete");
        changed |= fill_empty(&mut self.keybindings.confirm, "y,enter");
        changed |= fill_empty(&mut self.keybindings.cancel, "n,esc,q");
        changed |= replace_binding(&mut self.keybindings.help, "?", "f1");
        changed |= replace_binding(&mut self.keybindings.settings, "s", "ctrl+enter,f2");
        changed |= replace_binding(&mut self.keybindings.language, "alt+l", "alt+d");
        changed |= replace_binding(
            &mut self.keybindings.activate,
            "ctrl+enter,enter,space",
            "enter,space",
        );
        changed
    }

    fn migrate_legacy_key_sounds(&mut self, raw: &str) -> bool {
        if raw.contains("key_sounds = false") && self.key_sound_style != "off" {
            self.key_sound_style = "off".to_string();
            return true;
        }
        raw.contains("key_sounds")
    }

    fn disable_legacy_fail_defaults(&mut self) -> bool {
        if self.min_speed == 100 && self.min_accuracy == 90 && self.min_word_burst == "flex" {
            self.min_speed = 0;
            self.min_accuracy = 0;
            self.min_word_burst = "off".to_string();
            return true;
        }
        false
    }
}

fn fill_empty(value: &mut String, fallback: &str) -> bool {
    if value.trim().is_empty() {
        *value = fallback.to_string();
        true
    } else {
        false
    }
}

fn replace_binding(value: &mut String, old: &str, new: &str) -> bool {
    if value.trim() == old {
        *value = new.to_string();
        true
    } else {
        false
    }
}

fn sanitize_u64_modes(values: &[u64], defaults: &[u64]) -> Vec<u64> {
    let mut modes = Vec::new();
    for value in values.iter().copied().filter(|value| *value > 0) {
        if !modes.contains(&value) {
            modes.push(value);
        }
        if modes.len() == MAX_MODE_CHOICES {
            break;
        }
    }
    if modes.is_empty() {
        defaults.to_vec()
    } else {
        modes
    }
}

fn sanitize_usize_modes(values: &[usize], defaults: &[usize]) -> Vec<usize> {
    let mut modes = Vec::new();
    for value in values.iter().copied().filter(|value| *value > 0) {
        if !modes.contains(&value) {
            modes.push(value);
        }
        if modes.len() == MAX_MODE_CHOICES {
            break;
        }
    }
    if modes.is_empty() {
        defaults.to_vec()
    } else {
        modes
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn default_config_is_serializable() {
        let raw = toml::to_string(&Config::default()).unwrap();
        let parsed: Config = toml::from_str(&raw).unwrap();
        assert_eq!(parsed.default_time, 60);
        assert_eq!(parsed.time_modes, vec![15, 30, 60, 120]);
        assert_eq!(parsed.word_modes, vec![10, 25, 50, 100]);
        assert_eq!(parsed.interface_language, "en");
        assert_eq!(parsed.speed_unit, "wpm");
        assert!(parsed.save_results);
        assert_eq!(parsed.key_sound_style, "mechanical");
        assert_eq!(parsed.cursor_style, "block");
        assert_eq!(parsed.keybindings.left, "h,left");
        assert_eq!(parsed.keybindings.help, "f1");
        assert_eq!(parsed.keybindings.settings, "ctrl+enter,f2");
        assert_eq!(parsed.keybindings.activate, "enter,space");
        assert_eq!(parsed.keybindings.toggle_punctuation, "alt+p");
        assert_eq!(parsed.keybindings.mode_words, "alt+w");
        assert_eq!(parsed.keybindings.language, "alt+d");
        assert_eq!(parsed.keybindings.amount_left, "alt+h");
        assert_eq!(parsed.keybindings.amount_right, "alt+l");
    }

    #[test]
    fn default_fail_conditions_are_disabled() {
        let config = Config::default();
        assert_eq!(config.min_speed, 0);
        assert_eq!(config.min_accuracy, 0);
        assert_eq!(config.min_word_burst, "off");
    }

    #[test]
    fn migrates_old_fail_condition_defaults_to_off() {
        let mut config = Config {
            min_speed: 100,
            min_accuracy: 90,
            min_word_burst: "flex".to_string(),
            ..Config::default()
        };
        assert!(config.disable_legacy_fail_defaults());
        assert_eq!(config.min_speed, 0);
        assert_eq!(config.min_accuracy, 0);
        assert_eq!(config.min_word_burst, "off");
    }

    #[test]
    fn mode_choices_are_limited_and_cleaned() {
        let mut config = Config {
            time_modes: vec![0, 5, 5, 10, 15, 20, 25, 30, 35],
            word_modes: vec![0, 3, 3, 6, 9, 12, 15, 18, 21],
            ..Config::default()
        };
        assert!(config.normalize_mode_choices());
        assert_eq!(config.time_modes, vec![5, 10, 15, 20, 25, 30]);
        assert_eq!(config.word_modes, vec![3, 6, 9, 12, 15, 18]);
    }

    #[test]
    fn empty_mode_choices_fall_back_to_defaults() {
        let config = Config {
            time_modes: vec![0],
            word_modes: vec![],
            ..Config::default()
        };
        assert_eq!(config.time_mode_choices(), vec![15, 30, 60, 120]);
        assert_eq!(config.word_mode_choices(), vec![10, 25, 50, 100]);
    }

    #[test]
    fn invalid_speed_unit_falls_back_to_wpm() {
        let mut config = Config {
            speed_unit: "letters".to_string(),
            ..Config::default()
        };
        assert!(config.normalize_speed_unit());
        assert_eq!(config.speed_unit, "wpm");
    }

    #[test]
    fn invalid_interface_language_falls_back_to_english() {
        let mut config = Config {
            interface_language: "de".to_string(),
            ..Config::default()
        };
        assert!(config.normalize_interface_language());
        assert_eq!(config.interface_language, "en");
    }

    #[test]
    fn russian_interface_language_alias_is_valid() {
        let mut config = Config {
            interface_language: "russian".to_string(),
            ..Config::default()
        };
        assert!(config.normalize_interface_language());
        assert_eq!(config.interface_language, "ru");
    }

    #[test]
    fn invalid_key_sound_style_falls_back_to_mechanical() {
        let mut config = Config {
            key_sound_style: "random".to_string(),
            ..Config::default()
        };
        assert!(config.normalize_key_sound_style());
        assert_eq!(config.key_sound_style, "mechanical");
    }

    #[test]
    fn off_key_sound_style_is_valid() {
        let mut config = Config {
            key_sound_style: "off".to_string(),
            ..Config::default()
        };
        assert!(!config.normalize_key_sound_style());
        assert_eq!(config.key_sound_style, "off");
    }

    #[test]
    fn invalid_cursor_style_falls_back_to_block() {
        let mut config = Config {
            cursor_style: "beam".to_string(),
            ..Config::default()
        };
        assert!(config.normalize_cursor_style());
        assert_eq!(config.cursor_style, "block");
    }

    #[test]
    fn color_cursor_style_is_valid() {
        let mut config = Config {
            cursor_style: "color".to_string(),
            ..Config::default()
        };
        assert!(!config.normalize_cursor_style());
        assert_eq!(config.cursor_style, "color");
    }

    #[test]
    fn migrates_disabled_legacy_key_sounds_to_off() {
        let mut config = Config::default();
        assert!(config.migrate_legacy_key_sounds("key_sounds = false"));
        assert_eq!(config.key_sound_style, "off");
    }

    #[test]
    fn missing_keybindings_follow_legacy_restart_key() {
        let mut config = Config {
            quick_restart: "enter".to_string(),
            ..Config::default()
        };
        assert!(config.normalize_keybindings(""));
        assert_eq!(config.keybindings.restart, "enter");
        assert_eq!(config.keybindings.down, "j,down");
    }

    #[test]
    fn migrates_old_printable_main_key_defaults() {
        let mut config = Config::default();
        config.keybindings.help = "?".to_string();
        config.keybindings.settings = "s".to_string();
        config.keybindings.activate = "enter,space".to_string();
        config.keybindings.language = "alt+l".to_string();

        assert!(config.normalize_keybindings("[keybindings]"));
        assert_eq!(config.keybindings.help, "f1");
        assert_eq!(config.keybindings.settings, "ctrl+enter,f2");
        assert_eq!(config.keybindings.activate, "enter,space");
        assert_eq!(config.keybindings.language, "alt+d");
    }
}
