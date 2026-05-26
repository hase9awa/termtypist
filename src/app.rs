use std::time::Instant;

use anyhow::Result;

use crate::config::Config;
use crate::dictionaries::{
    Dictionary, GenerationOptions, available_unique_words, decorate_words, generate_words,
};
use crate::storage::Storage;
use crate::themes::{ResolvedTheme, Theme};
use crate::typing::{ErrorMode, Mode, TypingSession, mode_label, target_for_mode};

#[derive(Debug, Clone)]
pub enum LaunchRequest {
    Mode(Mode),
    ReplayLast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Pause,
    Help,
    Settings,
    LanguageMenu,
    History,
    Heatmap,
    ConfirmClearHistory,
    Results,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlHover {
    Punctuation,
    Numbers,
    Time,
    Words,
    Quote,
    Amount(usize),
    Language,
    Settings,
    ResultRestart,
    ResultRepeat,
    ResultQuit,
    ResultSpeed,
    ResultAccuracy,
    ResultTestType,
    ResultRaw,
    ResultCharacters,
    ResultConsistency,
    ResultTime,
    ResultOpen,
    SettingsHistory,
    SettingsHeatmap,
    SettingsClose,
    HistoryFilter(usize),
    HistoryClear,
    HistoryBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsButtonHover {
    History,
    Heatmap,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryActionHover {
    Clear,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPulseKind {
    Input,
    Click,
    Overlay,
    Setting,
    Restart,
}

#[derive(Debug, Clone, Copy)]
pub struct UiPulse {
    pub kind: UiPulseKind,
    pub started_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAnimationKind {
    HoverIn,
    HoverOut,
    ActivateIn,
    ActivateOut,
}

#[derive(Debug, Clone, Copy)]
pub struct ControlAnimation {
    pub target: ControlHover,
    pub kind: ControlAnimationKind,
    pub started_at: Instant,
}

pub struct App {
    pub config: Config,
    pub dictionary: Dictionary,
    pub word_pool: String,
    pub theme: ResolvedTheme,
    pub session: TypingSession,
    pub overlay: Overlay,
    pub settings_focus: Option<usize>,
    pub settings_button_hover: Option<SettingsButtonHover>,
    pub control_hover: Option<ControlHover>,
    pub control_animation: Option<ControlAnimation>,
    pub hover_exit_animation: Option<ControlAnimation>,
    pub language_menu_hover: Option<usize>,
    pub language_menu_offset: usize,
    pub history_language_filter: Option<String>,
    pub history_offset: usize,
    pub history_selected: usize,
    pub history_hover: Option<usize>,
    pub history_filter_hover: Option<usize>,
    pub history_action_hover: Option<HistoryActionHover>,
    pub heatmap_language: String,
    pub heatmap_hover_key: Option<String>,
    pub heatmap_hover_language: Option<String>,
    pub status: String,
    pub should_quit: bool,
    pub storage: Storage,
    pub ui_pulse: Option<UiPulse>,
}

impl App {
    pub fn new(config: Config, launch: LaunchRequest) -> Result<Self> {
        let storage = Storage::open_default()?;
        let theme = Theme::named(&config.theme)
            .unwrap_or_else(|| Theme::named("dark").expect("builtin theme"))
            .resolve();
        let dictionary = Dictionary::named(&config.language);
        dictionary.validate()?;
        let options = GenerationOptions {
            punctuation: config.punctuation,
            numbers: config.numbers,
        };

        let word_pool = generate_word_pool(&config, &dictionary);

        let (mode, target, status) = match launch {
            LaunchRequest::ReplayLast => {
                if let Some(row) = storage.replay_last()? {
                    (
                        Mode::Custom(row.target_text.clone()),
                        row.target_text,
                        tr_config(&config, "replaying last test", "повтор последнего теста")
                            .to_string(),
                    )
                } else {
                    let mode = mode_from_config(&config);
                    let target = target_from_word_pool(&mode, &word_pool, &dictionary, options);
                    (
                        mode,
                        target,
                        tr_config(
                            &config,
                            "no previous test to replay",
                            "нет предыдущего теста",
                        )
                        .to_string(),
                    )
                }
            }
            LaunchRequest::Mode(Mode::LastConfig) => {
                let mode = mode_from_config(&config);
                let target = target_from_word_pool(&mode, &word_pool, &dictionary, options);
                (
                    mode,
                    target,
                    tr_config(&config, "ready", "готово").to_string(),
                )
            }
            LaunchRequest::Mode(mode) => {
                let target = target_from_word_pool(&mode, &word_pool, &dictionary, options);
                (
                    mode,
                    target,
                    tr_config(&config, "ready", "готово").to_string(),
                )
            }
        };

        let language = dictionary.language.clone();
        let mut session = TypingSession::new(mode, language.clone(), target);
        apply_difficulty_to_session(&config, &mut session);
        Ok(Self {
            config,
            dictionary,
            word_pool,
            theme,
            session,
            overlay: Overlay::None,
            settings_focus: None,
            settings_button_hover: None,
            control_hover: None,
            control_animation: None,
            hover_exit_animation: None,
            language_menu_hover: None,
            language_menu_offset: 0,
            history_language_filter: None,
            history_offset: 0,
            history_selected: 0,
            history_hover: None,
            history_filter_hover: None,
            history_action_hover: None,
            heatmap_language: language,
            heatmap_hover_key: None,
            heatmap_hover_language: None,
            status,
            should_quit: false,
            storage,
            ui_pulse: None,
        })
    }

    pub fn pulse(&mut self, kind: UiPulseKind) {
        self.ui_pulse = Some(UiPulse {
            kind,
            started_at: Instant::now(),
        });
    }

    pub fn animate_control(&mut self, target: ControlHover, kind: ControlAnimationKind) {
        if self
            .hover_exit_animation
            .is_some_and(|animation| animation.target == target)
        {
            self.hover_exit_animation = None;
        }
        self.control_animation = Some(ControlAnimation {
            target,
            kind,
            started_at: Instant::now(),
        });
    }

    pub fn animate_hover_exit(&mut self, target: ControlHover) {
        if self.control_animation.is_some_and(|animation| {
            animation.target == target
                && matches!(
                    animation.kind,
                    ControlAnimationKind::HoverIn | ControlAnimationKind::HoverOut
                )
        }) {
            self.control_animation = None;
        }
        self.hover_exit_animation = Some(ControlAnimation {
            target,
            kind: ControlAnimationKind::HoverOut,
            started_at: Instant::now(),
        });
    }

    pub fn restart_new_text(&mut self) {
        let options = self.generation_options();
        if matches!(self.session.mode, Mode::Time(_) | Mode::Words(_)) {
            self.word_pool = generate_word_pool(&self.config, &self.dictionary);
        }
        let target = target_from_word_pool(
            &self.session.mode,
            &self.word_pool,
            &self.dictionary,
            options,
        );
        self.session.reset(target);
        apply_difficulty_to_session(&self.config, &mut self.session);
        self.overlay = Overlay::None;
        self.status = tr(self, "restarted", "перезапущено").to_string();
    }

    pub fn rebuild_current_text(&mut self) {
        let options = self.generation_options();
        let target = if matches!(self.session.mode, Mode::Time(_) | Mode::Words(_)) {
            target_from_word_pool(
                &self.session.mode,
                &self.word_pool,
                &self.dictionary,
                options,
            )
        } else {
            self.session.target.clone()
        };
        self.session.reset(target);
        apply_difficulty_to_session(&self.config, &mut self.session);
        self.overlay = Overlay::None;
    }

    pub fn retry_same_text(&mut self) {
        let target = self.session.target.clone();
        self.session.reset(target);
        apply_difficulty_to_session(&self.config, &mut self.session);
        self.overlay = Overlay::None;
        self.status = tr(self, "retrying same text", "повтор того же текста").to_string();
    }

    pub fn restart_requested(&mut self) {
        if matches!(self.session.mode, Mode::Quote(_))
            && self.config.repeat_quotes == "typing"
            && self.session.state == crate::typing::TestState::Running
        {
            self.retry_same_text();
        } else {
            self.restart_new_text();
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        if matches!(
            self.session.state,
            crate::typing::TestState::Running | crate::typing::TestState::Paused
        ) {
            self.status = tr(
                self,
                "restart before changing mode",
                "перезапустите перед сменой режима",
            )
            .to_string();
            return;
        }
        let should_restart = matches!(
            self.session.state,
            crate::typing::TestState::Finished | crate::typing::TestState::Failed
        );
        match &mode {
            Mode::Time(seconds) => {
                self.config.default_mode = "time".to_string();
                self.config.default_time = *seconds;
            }
            Mode::Words(words) => {
                self.config.default_mode = "words".to_string();
                self.config.default_words = *words;
            }
            Mode::Quote(_) => self.config.default_mode = "quote".to_string(),
            Mode::Custom(_) | Mode::LastConfig => {}
        }
        let options = self.generation_options();
        if should_restart && matches!(&mode, Mode::Time(_) | Mode::Words(_)) {
            self.word_pool = generate_word_pool(&self.config, &self.dictionary);
        }
        let target = target_from_word_pool(&mode, &self.word_pool, &self.dictionary, options);
        self.session.mode = mode;
        self.session.reset(target);
        apply_difficulty_to_session(&self.config, &mut self.session);
        if should_restart {
            self.overlay = Overlay::None;
        }
        self.status = mode_label_for_config(&self.config, &self.session.mode);
        let _ = self.config.save();
    }

    pub fn cycle_language_by(&mut self, delta: isize) {
        if self.session.state != crate::typing::TestState::Ready {
            self.status = tr(
                self,
                "finish or restart before changing language",
                "завершите или перезапустите перед сменой языка",
            )
            .to_string();
            return;
        }
        let names = dictionary_names();
        let current = names
            .iter()
            .position(|name| *name == self.dictionary.name)
            .unwrap_or(0);
        let next =
            names[(current as isize + delta).rem_euclid(names.len() as isize) as usize].clone();
        self.set_dictionary(&next);
    }

    pub fn set_dictionary(&mut self, name: &str) {
        if self.session.state != crate::typing::TestState::Ready {
            self.status = tr(
                self,
                "finish or restart before changing dictionary",
                "завершите или перезапустите перед сменой словаря",
            )
            .to_string();
            return;
        }
        self.dictionary = Dictionary::named(name);
        self.session.language = self.dictionary.language.clone();
        self.config.language = self.dictionary.name.clone();
        self.restart_new_text();
        let _ = self.config.save();
    }

    pub fn set_language(&mut self, language: &str) {
        if self.session.state != crate::typing::TestState::Ready {
            self.status = tr(
                self,
                "finish or restart before changing language",
                "завершите или перезапустите перед сменой языка",
            )
            .to_string();
            return;
        }
        let dictionaries = crate::dictionaries::available();
        let current_dictionary = dictionaries.iter().find(|dictionary| {
            dictionary.name == self.dictionary.name
                && dictionary.language.eq_ignore_ascii_case(language)
        });
        let fallback_dictionary = dictionaries
            .iter()
            .find(|dictionary| dictionary.language.eq_ignore_ascii_case(language));
        let Some(dictionary) = current_dictionary.or(fallback_dictionary) else {
            self.status = format!(
                "{} {language}",
                tr(self, "no dictionary for language", "нет словаря для языка")
            );
            return;
        };
        let name = dictionary.name.clone();
        self.set_dictionary(&name);
    }

    pub fn cycle_heatmap_language(&mut self, delta: isize) {
        let codes = heatmap_language_codes();
        let current = codes
            .iter()
            .position(|code| *code == self.heatmap_language.as_str())
            .unwrap_or(0);
        let len = codes.len() as isize;
        let next = (current as isize + delta).rem_euclid(len) as usize;
        self.set_heatmap_language(codes[next]);
    }

    pub fn set_heatmap_language(&mut self, language: &str) {
        if heatmap_language_codes().contains(&language) {
            self.heatmap_language = language.to_string();
            self.heatmap_hover_key = None;
            self.heatmap_hover_language = None;
            self.status = format!(
                "{}: {}",
                tr(self, "heatmap", "тепловая карта"),
                heatmap_language_label_for_config(&self.config, language)
            );
        }
    }

    pub fn toggle_punctuation(&mut self) {
        if self.session.state == crate::typing::TestState::Ready {
            self.config.punctuation = !self.config.punctuation;
            self.rebuild_current_text();
            self.status = format!(
                "{} {}",
                tr(self, "punctuation", "пунктуация"),
                on_off_for_config(&self.config, self.config.punctuation)
            );
            let _ = self.config.save();
        }
    }

    pub fn toggle_numbers(&mut self) {
        if self.session.state == crate::typing::TestState::Ready {
            self.config.numbers = !self.config.numbers;
            self.rebuild_current_text();
            self.status = format!(
                "{} {}",
                tr(self, "numbers", "числа"),
                on_off_for_config(&self.config, self.config.numbers)
            );
            let _ = self.config.save();
        }
    }

    fn generation_options(&self) -> GenerationOptions {
        GenerationOptions {
            punctuation: self.config.punctuation,
            numbers: self.config.numbers,
        }
    }

    pub fn toggle_save_results(&mut self) {
        self.config.save_results = !self.config.save_results;
        self.status = format!(
            "{} {}",
            tr(self, "save results", "сохранение результатов"),
            if self.config.save_results {
                on_off_for_config(&self.config, true)
            } else {
                on_off_for_config(&self.config, false)
            }
        );
        let _ = self.config.save();
    }

    pub fn cycle_key_sound_style_by(&mut self, delta: isize) {
        self.config.key_sound_style = cycle_value_by(
            &self.config.key_sound_style,
            &["off", "mechanical", "click", "thock", "crisp", "beep"],
            delta,
        );
        self.status = format!(
            "{}: {}",
            tr(self, "key sound", "звук клавиш"),
            self.config.key_sound_style
        );
        let _ = self.config.save();
    }

    pub fn cycle_interface_language_by(&mut self, delta: isize) {
        self.config.interface_language =
            cycle_value_by(&self.config.interface_language, &["en", "ru"], delta);
        self.status = format!(
            "{}: {}",
            tr(self, "interface language", "язык интерфейса"),
            interface_language_label_for_config(&self.config)
        );
        let _ = self.config.save();
    }

    pub fn toggle_blind_mode(&mut self) {
        self.config.blind_mode = !self.config.blind_mode;
        self.status = format!(
            "{} {}",
            tr(self, "blind mode", "слепой режим"),
            on_off_for_config(&self.config, self.config.blind_mode)
        );
        let _ = self.config.save();
    }

    pub fn toggle_words_history(&mut self) {
        self.config.always_show_words_history = !self.config.always_show_words_history;
        self.status = format!(
            "{} {}",
            tr(self, "words history", "история слов"),
            on_off_for_config(&self.config, self.config.always_show_words_history)
        );
        let _ = self.config.save();
    }

    pub fn cycle_speed_unit_by(&mut self, delta: isize) {
        self.config.speed_unit = cycle_value_by(&self.config.speed_unit, &["wpm", "cpm"], delta);
        self.status = format!(
            "{}: {}",
            tr(self, "speed unit", "единица скорости"),
            self.config.speed_unit
        );
        let _ = self.config.save();
    }

    pub fn cycle_difficulty_by(&mut self, delta: isize) {
        self.config.difficulty = cycle_value_by(
            &self.config.difficulty,
            &["normal", "expert", "master"],
            delta,
        );
        apply_difficulty_to_session(&self.config, &mut self.session);
        self.status = format!(
            "{}: {}",
            tr(self, "difficulty", "сложность"),
            self.config.difficulty
        );
        let _ = self.config.save();
    }

    pub fn cycle_quick_restart_by(&mut self, delta: isize) {
        self.config.quick_restart =
            cycle_value_by(&self.config.quick_restart, &["tab", "esc", "enter"], delta);
        self.config.keybindings.restart = self.config.quick_restart.clone();
        self.status = format!(
            "{}: {}",
            tr(self, "quick restart", "быстрый рестарт"),
            self.config.quick_restart
        );
        let _ = self.config.save();
    }

    pub fn cycle_repeat_quotes_by(&mut self, delta: isize) {
        self.config.repeat_quotes =
            cycle_value_by(&self.config.repeat_quotes, &["off", "typing"], delta);
        self.status = format!(
            "{}: {}",
            tr(self, "repeat quotes", "повтор цитат"),
            self.config.repeat_quotes
        );
        let _ = self.config.save();
    }

    pub fn cycle_min_speed_by(&mut self, delta: isize) {
        self.config.min_speed = cycle_number_by(
            self.config.min_speed,
            speed_threshold_values(&self.config),
            delta,
        );
        self.status = format!(
            "{}: {} {}",
            tr(self, "min speed", "мин. скорость"),
            numeric_setting(self.config.min_speed),
            speed_unit(&self.config)
        );
        let _ = self.config.save();
    }

    pub fn cycle_min_accuracy_by(&mut self, delta: isize) {
        self.config.min_accuracy =
            cycle_number_by(self.config.min_accuracy, &[0, 80, 90, 95, 100], delta);
        self.status = format!(
            "{}: {}",
            tr(self, "min accuracy", "мин. точность"),
            numeric_setting(self.config.min_accuracy)
        );
        let _ = self.config.save();
    }

    pub fn cycle_min_word_burst_by(&mut self, delta: isize) {
        self.config.min_word_burst = cycle_value_by(
            &self.config.min_word_burst,
            &["off", "flex", "50", "100"],
            delta,
        );
        self.status = format!(
            "{}: {}",
            tr(self, "min word burst", "мин. рывок слова"),
            self.config.min_word_burst
        );
        let _ = self.config.save();
    }

    pub fn cycle_theme_by(&mut self, delta: isize) {
        let names = theme_names();
        let current = names
            .iter()
            .position(|name| *name == self.config.theme)
            .unwrap_or(0);
        let next =
            names[(current as isize + delta).rem_euclid(names.len() as isize) as usize].clone();
        self.set_theme(&next);
    }

    pub fn set_theme(&mut self, name: &str) {
        match Theme::named(name) {
            Some(theme) => {
                self.config.theme = theme.name.clone();
                self.theme = theme.resolve();
                self.status = format!("{}: {}", tr(self, "theme", "тема"), self.config.theme);
                let _ = self.config.save();
            }
            None => {
                self.status = format!("{}: {name}", tr(self, "unknown theme", "неизвестная тема"))
            }
        }
    }

    pub fn cycle_visual_style_by(&mut self, delta: isize) {
        self.config.visual_style =
            cycle_value_by(&self.config.visual_style, &visual_style_names(), delta);
        self.status = format!(
            "{}: {}",
            tr(self, "visual style", "визуальный стиль"),
            self.config.visual_style
        );
        let _ = self.config.save();
    }

    pub fn cycle_cursor_style_by(&mut self, delta: isize) {
        self.config.cursor_style = cycle_value_by(
            &self.config.cursor_style,
            &["block", "underline", "color"],
            delta,
        );
        self.status = format!(
            "{}: {}",
            tr(self, "cursor", "курсор"),
            self.config.cursor_style
        );
        let _ = self.config.save();
    }

    pub fn save_finished_result(&mut self) {
        if !self.config.save_results {
            return;
        }
        if matches!(
            self.session.state,
            crate::typing::TestState::Finished | crate::typing::TestState::Failed
        ) {
            let metrics = self.session.metrics();
            match self.storage.save_result(&self.session, metrics) {
                Ok(()) => self.status = tr(self, "result saved", "результат сохранен").to_string(),
                Err(err) => {
                    self.status = format!("{}: {err}", tr(self, "save failed", "ошибка сохранения"))
                }
            }
        }
    }

    pub fn enforce_fail_conditions(&mut self) {
        if self.session.state != crate::typing::TestState::Running {
            return;
        }

        let metrics = self.session.metrics();
        if self.config.min_accuracy > 0
            && metrics.typed_characters > 0
            && metrics.accuracy < self.config.min_accuracy as f64
        {
            self.session.fail();
            self.status = format!(
                "{}: {} {}%",
                tr(self, "failed", "провал"),
                tr(self, "accuracy below", "точность ниже"),
                self.config.min_accuracy
            );
            return;
        }

        if self.config.min_speed > 0
            && self.session.elapsed().as_secs_f64() >= 2.0
            && speed_value(&self.config, &metrics) < self.config.min_speed as f64
        {
            self.session.fail();
            self.status = format!(
                "{}: {} {} {}",
                tr(self, "failed", "провал"),
                tr(self, "speed below", "скорость ниже"),
                self.config.min_speed,
                speed_unit(&self.config)
            );
            return;
        }

        let Some(word_threshold) = min_word_burst_threshold(&self.config.min_word_burst) else {
            return;
        };
        if let Some(word_raw) = self.session.last_word_raw_wpm()
            && word_raw < word_threshold
        {
            self.session.fail();
            self.status = format!(
                "{}: {} {word_threshold:.0} raw",
                tr(self, "failed", "провал"),
                tr(self, "word burst below", "рывок слова ниже")
            );
        }
    }
}

fn apply_difficulty_to_session(config: &Config, session: &mut TypingSession) {
    session.error_mode = match config.difficulty.as_str() {
        "expert" => ErrorMode::Expert,
        "master" => ErrorMode::Master,
        _ => ErrorMode::Normal,
    };
}

fn cycle_value_by(current: &str, values: &[&str], delta: isize) -> String {
    let idx = values
        .iter()
        .position(|value| *value == current)
        .unwrap_or(0);
    values[(idx as isize + delta).rem_euclid(values.len() as isize) as usize].to_string()
}

fn cycle_number_by(current: u64, values: &[u64], delta: isize) -> u64 {
    let idx = values
        .iter()
        .position(|value| *value == current)
        .unwrap_or(0);
    values[(idx as isize + delta).rem_euclid(values.len() as isize) as usize]
}

pub fn interface_language_label_for_config(config: &Config) -> &'static str {
    if is_russian_interface(config) {
        "русский"
    } else {
        "english"
    }
}

pub fn heatmap_language_label_for_config(config: &Config, language: &str) -> &'static str {
    match (is_russian_interface(config), language) {
        (true, "ru") => "русский",
        (true, _) => "английский",
        (false, "ru") => "russian",
        (false, _) => "english",
    }
}

pub fn mode_label_for_config(config: &Config, mode: &Mode) -> String {
    if !is_russian_interface(config) {
        return mode_label(mode);
    }
    match mode {
        Mode::LastConfig => "время".to_string(),
        Mode::Time(seconds) => format!("время {seconds}"),
        Mode::Words(words) => format!("слова {words}"),
        Mode::Quote(length) => {
            let label = match length {
                crate::typing::QuoteLength::Short => "короткая",
                crate::typing::QuoteLength::Medium => "средняя",
                crate::typing::QuoteLength::Long => "длинная",
                crate::typing::QuoteLength::Random => "случайная",
            };
            format!("цитата {label}")
        }
        Mode::Custom(_) => "свой текст".to_string(),
    }
}

pub fn on_off_for_config(config: &Config, value: bool) -> &'static str {
    match (is_russian_interface(config), value) {
        (true, true) => "вкл",
        (true, false) => "выкл",
        (false, true) => "on",
        (false, false) => "off",
    }
}

pub fn is_russian_interface(config: &Config) -> bool {
    config.interface_language == "ru"
}

fn tr<'a>(app: &App, en: &'a str, ru: &'a str) -> &'a str {
    tr_config(&app.config, en, ru)
}

fn tr_config<'a>(config: &Config, en: &'a str, ru: &'a str) -> &'a str {
    if is_russian_interface(config) { ru } else { en }
}

pub fn numeric_setting(value: u64) -> String {
    if value == 0 {
        "off".to_string()
    } else {
        value.to_string()
    }
}

pub fn speed_unit(config: &Config) -> &'static str {
    match config.speed_unit.as_str() {
        "cpm" => "cpm",
        _ => "wpm",
    }
}

pub fn speed_value(config: &Config, metrics: &crate::metrics::Metrics) -> f64 {
    match speed_unit(config) {
        "cpm" => metrics.cpm,
        _ => metrics.wpm,
    }
}

pub fn raw_speed_value(config: &Config, metrics: &crate::metrics::Metrics) -> f64 {
    match speed_unit(config) {
        "cpm" => metrics.raw_cpm,
        _ => metrics.raw_wpm,
    }
}

fn speed_threshold_values(config: &Config) -> &'static [u64] {
    match speed_unit(config) {
        "cpm" => &[0, 250, 500, 750, 1000],
        _ => &[0, 50, 100, 150, 200],
    }
}

fn min_word_burst_threshold(value: &str) -> Option<f64> {
    match value {
        "off" => None,
        "flex" => Some(50.0),
        raw => raw.parse::<f64>().ok().filter(|threshold| *threshold > 0.0),
    }
}

pub fn theme_names() -> Vec<String> {
    Theme::available()
        .into_iter()
        .map(|theme| theme.name)
        .collect()
}

pub fn visual_style_names() -> Vec<&'static str> {
    vec![
        "minimal",
        "space",
        "stardust",
        "fireflies",
        "snowfall",
        "embers",
    ]
}

pub fn dictionary_names() -> Vec<String> {
    crate::dictionaries::available()
        .into_iter()
        .map(|dictionary| dictionary.name)
        .collect()
}

pub fn dictionary_languages() -> Vec<String> {
    let mut languages = Vec::new();
    for dictionary in crate::dictionaries::available() {
        if dictionary.language.trim().is_empty()
            || languages
                .iter()
                .any(|language: &String| language.eq_ignore_ascii_case(&dictionary.language))
        {
            continue;
        }
        languages.push(dictionary.language);
    }
    languages
}

pub fn heatmap_language_codes() -> [&'static str; 2] {
    ["en", "ru"]
}

pub fn mode_from_config(config: &Config) -> Mode {
    match config.default_mode.as_str() {
        "words" => Mode::Words(config.default_words),
        "quote" => Mode::Quote(crate::typing::QuoteLength::Random),
        _ => Mode::Time(config.default_time),
    }
}

fn generate_word_pool(config: &Config, dictionary: &Dictionary) -> String {
    let desired = config
        .time_mode_choices()
        .into_iter()
        .map(desired_words_for_time)
        .chain(config.word_mode_choices())
        .chain([
            config.default_words,
            desired_words_for_time(config.default_time),
        ])
        .max()
        .unwrap_or(80);
    generate_words(
        dictionary,
        desired.min(available_unique_words(dictionary)),
        GenerationOptions {
            punctuation: false,
            numbers: false,
        },
    )
}

fn target_from_word_pool(
    mode: &Mode,
    word_pool: &str,
    dictionary: &Dictionary,
    options: GenerationOptions,
) -> String {
    match mode {
        Mode::Time(seconds) => decorate_words(
            &word_prefix(word_pool, desired_words_for_time(*seconds)),
            options,
        ),
        Mode::Words(amount) => decorate_words(&word_prefix(word_pool, *amount), options),
        Mode::LastConfig => decorate_words(&word_prefix(word_pool, 80), options),
        Mode::Quote(_) | Mode::Custom(_) => target_for_mode(mode, dictionary, options),
    }
}

fn desired_words_for_time(seconds: u64) -> usize {
    match seconds {
        0..=15 => 60,
        16..=30 => 90,
        31..=60 => 120,
        _ => 180,
    }
}

fn word_prefix(text: &str, amount: usize) -> String {
    text.split_whitespace()
        .take(amount)
        .collect::<Vec<_>>()
        .join(" ")
}
