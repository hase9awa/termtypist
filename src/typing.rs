use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::dictionaries::{Dictionary, GenerationOptions, available_unique_words, generate_words};
use crate::metrics::{self, Metrics};

const SPEED_SAMPLE_INTERVAL_SECS: f64 = 0.5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Mode {
    LastConfig,
    Time(u64),
    Words(usize),
    Quote(QuoteLength),
    Custom(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum QuoteLength {
    Short,
    Medium,
    Long,
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestState {
    Ready,
    Running,
    Paused,
    Finished,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharState {
    Pending,
    Correct,
    Incorrect,
    Extra,
    Missed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorMode {
    Normal,
    Expert,
    StopOnLetter,
    Master,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpeedSample {
    pub second: f64,
    pub wpm: f64,
    pub raw_wpm: f64,
    pub errors: f64,
}

#[derive(Debug, Clone)]
pub struct TypingSession {
    pub mode: Mode,
    pub language: String,
    pub target: String,
    pub input: Vec<char>,
    pub state: TestState,
    pub error_mode: ErrorMode,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    paused_at: Option<Instant>,
    paused_total: Duration,
    wpm_samples: Vec<f64>,
    samples: Vec<SpeedSample>,
    last_sample_at: f64,
    last_sample_correct_characters: usize,
    last_sample_typed_characters: usize,
    current_word_start: usize,
    current_word_started_at: Option<f64>,
    last_word_raw_wpm: Option<f64>,
    mistakes: usize,
    key_mistakes: HashMap<char, usize>,
}

impl TypingSession {
    pub fn new(mode: Mode, language: String, target: String) -> Self {
        Self {
            mode,
            language,
            target,
            input: Vec::new(),
            state: TestState::Ready,
            error_mode: ErrorMode::Normal,
            started_at: None,
            finished_at: None,
            paused_at: None,
            paused_total: Duration::ZERO,
            wpm_samples: Vec::new(),
            samples: Vec::new(),
            last_sample_at: 0.0,
            last_sample_correct_characters: 0,
            last_sample_typed_characters: 0,
            current_word_start: 0,
            current_word_started_at: None,
            last_word_raw_wpm: None,
            mistakes: 0,
            key_mistakes: HashMap::new(),
        }
    }

    pub fn reset(&mut self, target: String) {
        let mode = self.mode.clone();
        let language = self.language.clone();
        *self = Self::new(mode, language, target);
    }

    pub fn type_char(&mut self, ch: char) {
        if matches!(
            self.state,
            TestState::Paused | TestState::Finished | TestState::Failed
        ) {
            return;
        }
        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
            self.state = TestState::Running;
        }

        if !ch.is_whitespace()
            && self
                .input
                .last()
                .is_none_or(|previous| previous.is_whitespace())
        {
            self.current_word_start = self.input.len();
            self.current_word_started_at = Some(self.elapsed().as_secs_f64());
        }

        if self.error_mode == ErrorMode::StopOnLetter
            && let Some(expected) = self.target.chars().nth(self.input.len())
            && expected != ch
        {
            return;
        }

        let expected = self.effective_target_chars().get(self.input.len()).copied();
        if expected.is_none_or(|expected| expected != ch) {
            self.mistakes += 1;
            if let Some(expected) = expected {
                *self.key_mistakes.entry(expected).or_insert(0) += 1;
            }
        }

        self.input.push(ch);
        if ch.is_whitespace() {
            self.record_current_word_burst();
            if self.error_mode == ErrorMode::Expert && self.submitted_word_is_incorrect() {
                self.fail();
                return;
            }
        }

        if self.error_mode == ErrorMode::Master && !self.current_prefix_is_correct() {
            self.fail();
            return;
        }

        self.sample_speed();
        if self.should_finish() {
            self.finish();
        }
    }

    pub fn backspace(&mut self) {
        if matches!(self.state, TestState::Running | TestState::Ready) {
            self.input.pop();
        }
    }

    pub fn pause(&mut self) {
        if self.state == TestState::Running {
            self.state = TestState::Paused;
            self.paused_at = Some(Instant::now());
        }
    }

    pub fn resume(&mut self) {
        if self.state == TestState::Paused {
            if let Some(paused_at) = self.paused_at.take() {
                self.paused_total += paused_at.elapsed();
            }
            self.state = TestState::Running;
        }
    }

    pub fn finish(&mut self) {
        if !matches!(self.state, TestState::Finished | TestState::Failed) {
            self.record_current_word_burst();
            self.state = TestState::Finished;
            self.finished_at = Some(Instant::now());
        }
    }

    pub fn fail(&mut self) {
        if !matches!(self.state, TestState::Finished | TestState::Failed) {
            self.record_current_word_burst();
            self.state = TestState::Failed;
            self.finished_at = Some(Instant::now());
        }
    }

    pub fn elapsed(&self) -> Duration {
        let Some(started_at) = self.started_at else {
            return Duration::ZERO;
        };
        let end = self
            .finished_at
            .or(self.paused_at)
            .unwrap_or_else(Instant::now);
        end.saturating_duration_since(started_at)
            .saturating_sub(self.paused_total)
    }

    pub fn remaining(&self) -> Option<Duration> {
        if let Mode::Time(limit) = self.mode {
            return Some(Duration::from_secs(limit).saturating_sub(self.elapsed()));
        }
        None
    }

    pub fn progress(&self) -> f64 {
        match self.mode {
            Mode::Time(limit) => (self.elapsed().as_secs_f64() / limit as f64).clamp(0.0, 1.0),
            Mode::Words(amount) => {
                let typed_words = self.input_string().split_whitespace().count();
                (typed_words as f64 / amount.max(1) as f64).clamp(0.0, 1.0)
            }
            Mode::Quote(_) | Mode::Custom(_) => (self.input.len() as f64
                / self.target.chars().count().max(1) as f64)
                .clamp(0.0, 1.0),
            Mode::LastConfig => 0.0,
        }
    }

    pub fn metrics(&self) -> Metrics {
        let target = self.effective_target_chars();
        let include_missed = matches!(self.state, TestState::Finished | TestState::Failed)
            && matches!(self.mode, Mode::Words(_) | Mode::Quote(_) | Mode::Custom(_));
        let mut metrics = metrics::calculate(
            &target,
            &self.input,
            self.elapsed().as_secs_f64(),
            include_missed,
        );
        let visible_typed_errors = metrics.incorrect_characters + metrics.extra_characters;
        let corrected_errors = self.mistakes.saturating_sub(visible_typed_errors);
        if corrected_errors > 0 {
            let accuracy_total =
                metrics.typed_characters + metrics.missed_characters + corrected_errors;
            metrics.accuracy = if accuracy_total == 0 {
                100.0
            } else {
                (metrics.correct_characters as f64 / accuracy_total as f64 * 1000.0).round() / 10.0
            };
            metrics.errors += corrected_errors;
        }
        metrics.consistency = metrics::consistency(&self.wpm_samples);
        metrics
    }

    pub fn samples(&self) -> Vec<SpeedSample> {
        let mut samples = self.samples.clone();
        if samples.is_empty() {
            let metrics = self.metrics();
            samples.push(SpeedSample {
                second: 1.0,
                wpm: metrics.wpm,
                raw_wpm: metrics.raw_wpm,
                errors: metrics.errors as f64,
            });
        }
        samples
    }

    pub fn last_word_raw_wpm(&self) -> Option<f64> {
        self.last_word_raw_wpm
    }

    pub fn sample_speed(&mut self) {
        let elapsed = self.elapsed().as_secs_f64();
        if elapsed < 0.05 || elapsed - self.last_sample_at < SPEED_SAMPLE_INTERVAL_SECS {
            return;
        }

        let delta_seconds = (elapsed - self.last_sample_at).max(0.05);
        let metrics = self.metrics();
        let correct_delta = metrics
            .correct_characters
            .saturating_sub(self.last_sample_correct_characters);
        let typed_delta = metrics
            .typed_characters
            .saturating_sub(self.last_sample_typed_characters);
        let minutes = delta_seconds / 60.0;
        let instant_wpm = correct_delta as f64 / 5.0 / minutes;
        let instant_raw_wpm = typed_delta as f64 / 5.0 / minutes;

        self.last_sample_at = elapsed;
        self.last_sample_correct_characters = metrics.correct_characters;
        self.last_sample_typed_characters = metrics.typed_characters;
        self.wpm_samples.push(instant_wpm);
        self.samples.push(SpeedSample {
            second: elapsed,
            wpm: instant_wpm,
            raw_wpm: instant_raw_wpm,
            errors: metrics.errors as f64,
        });
    }

    pub fn input_string(&self) -> String {
        self.input.iter().collect()
    }

    pub fn key_mistakes(&self) -> Vec<(char, usize)> {
        let mut mistakes = self
            .key_mistakes
            .iter()
            .map(|(key, errors)| (*key, *errors))
            .collect::<Vec<_>>();
        mistakes.sort_by_key(|(key, _)| *key);
        mistakes
    }

    pub fn char_states(&self) -> Vec<(char, CharState)> {
        let target = self.effective_target_chars();
        let mut states = Vec::new();
        let include_missed = matches!(self.state, TestState::Finished | TestState::Failed)
            && matches!(self.mode, Mode::Words(_) | Mode::Quote(_) | Mode::Custom(_));
        for (idx, expected) in target.iter().enumerate() {
            match self.input.get(idx) {
                Some(actual) if actual == expected => states.push((*expected, CharState::Correct)),
                Some(_) => states.push((*expected, CharState::Incorrect)),
                None if include_missed => states.push((*expected, CharState::Missed)),
                None => states.push((*expected, CharState::Pending)),
            }
        }
        if self.input.len() > target.len() {
            for ch in self.input.iter().skip(target.len()) {
                states.push((*ch, CharState::Extra));
            }
        }
        states
    }

    fn should_finish(&self) -> bool {
        match self.mode {
            Mode::Time(limit) => self.elapsed().as_secs() >= limit,
            Mode::Words(_) => self.input.len() >= self.effective_target_chars().len(),
            Mode::Quote(_) | Mode::Custom(_) => self.input.len() >= self.target.chars().count(),
            Mode::LastConfig => false,
        }
    }

    fn effective_target_chars(&self) -> Vec<char> {
        let target: Vec<char> = self.target.chars().collect();
        match self.mode {
            Mode::Words(amount) => target_prefix_for_words(&target, amount),
            _ => target,
        }
    }

    fn current_prefix_is_correct(&self) -> bool {
        let target: Vec<char> = self.target.chars().collect();
        self.input
            .iter()
            .enumerate()
            .all(|(idx, ch)| target.get(idx).is_some_and(|expected| expected == ch))
    }

    fn submitted_word_is_incorrect(&self) -> bool {
        let word_end = self.input.len().saturating_sub(1);
        if self.current_word_start >= word_end {
            return false;
        }

        let target: Vec<char> = self.target.chars().collect();
        let target_end = target
            .iter()
            .enumerate()
            .skip(self.current_word_start)
            .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx))
            .unwrap_or(target.len());
        let input_word: String = self.input[self.current_word_start..word_end]
            .iter()
            .collect();
        let target_word: String = target
            .get(self.current_word_start..target_end)
            .unwrap_or_default()
            .iter()
            .collect();
        input_word != target_word
    }

    fn record_current_word_burst(&mut self) {
        let Some(started_at) = self.current_word_started_at.take() else {
            return;
        };
        let word_end = self
            .input
            .iter()
            .enumerate()
            .skip(self.current_word_start)
            .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx))
            .unwrap_or(self.input.len());
        if word_end <= self.current_word_start {
            return;
        }
        let chars = word_end - self.current_word_start;
        let minutes = ((self.elapsed().as_secs_f64() - started_at).max(0.05)) / 60.0;
        self.last_word_raw_wpm = Some(chars as f64 / 5.0 / minutes);
    }
}

fn target_prefix_for_words(target: &[char], amount: usize) -> Vec<char> {
    if amount == 0 {
        return Vec::new();
    }

    let mut words_seen = 0usize;
    let mut in_word = false;
    let mut end = target.len();

    for (idx, ch) in target.iter().enumerate() {
        if ch.is_whitespace() {
            if in_word {
                words_seen += 1;
                in_word = false;
                if words_seen >= amount {
                    end = idx;
                    break;
                }
            }
        } else if !in_word {
            in_word = true;
        }
    }

    target[..end].to_vec()
}

pub fn target_for_mode(mode: &Mode, dictionary: &Dictionary, options: GenerationOptions) -> String {
    match mode {
        Mode::LastConfig => generate_words(dictionary, 80, options),
        Mode::Time(seconds) => {
            let desired = match seconds {
                0..=15 => 60,
                16..=30 => 90,
                31..=60 => 120,
                _ => 180,
            };
            let amount = desired.min(available_unique_words(dictionary));
            generate_words(dictionary, amount, options)
        }
        Mode::Words(amount) => generate_words(
            dictionary,
            (*amount).min(available_unique_words(dictionary)),
            options,
        ),
        Mode::Quote(length) => crate::quotes::quote(*length, &dictionary.language),
        Mode::Custom(text) => text.clone(),
    }
}

pub fn mode_label(mode: &Mode) -> String {
    match mode {
        Mode::LastConfig => "time".to_string(),
        Mode::Time(seconds) => format!("time {seconds}"),
        Mode::Words(words) => format!("words {words}"),
        Mode::Quote(length) => format!("quote {}", quote_label(*length)),
        Mode::Custom(_) => "custom".to_string(),
    }
}

fn quote_label(length: QuoteLength) -> &'static str {
    match length {
        QuoteLength::Short => "short",
        QuoteLength::Medium => "medium",
        QuoteLength::Long => "long",
        QuoteLength::Random => "random",
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorMode, Mode, TypingSession, target_for_mode, target_prefix_for_words};
    use crate::dictionaries::{Dictionary, GenerationOptions};

    fn dictionary() -> Dictionary {
        Dictionary {
            name: "test".to_string(),
            language: "en".to_string(),
            words: vec!["one".to_string(), "two".to_string()],
        }
    }

    #[test]
    fn backspace_removes_previous_character() {
        let mut session = TypingSession::new(Mode::Words(1), "en".to_string(), "abc".to_string());
        session.type_char('a');
        session.type_char('x');
        session.backspace();
        assert_eq!(session.input_string(), "a");
    }

    #[test]
    fn accuracy_counts_corrected_mistakes() {
        let mut session = TypingSession::new(Mode::Words(1), "en".to_string(), "abc".to_string());
        session.type_char('a');
        session.type_char('x');
        session.backspace();
        session.type_char('b');
        session.type_char('c');

        let metrics = session.metrics();
        assert_eq!(metrics.errors, 1);
        assert_eq!(metrics.accuracy, 75.0);
        assert_eq!(session.key_mistakes(), vec![('b', 1)]);
    }

    #[test]
    fn stop_on_letter_rejects_wrong_character() {
        let mut session = TypingSession::new(Mode::Words(1), "en".to_string(), "abc".to_string());
        session.error_mode = ErrorMode::StopOnLetter;
        session.type_char('x');
        assert_eq!(session.input_string(), "");
    }

    #[test]
    fn expert_fails_when_submitting_incorrect_word() {
        let mut session =
            TypingSession::new(Mode::Words(2), "en".to_string(), "one two".to_string());
        session.error_mode = ErrorMode::Expert;
        for ch in "ono ".chars() {
            session.type_char(ch);
        }
        assert_eq!(session.state, super::TestState::Failed);
    }

    #[test]
    fn master_fails_on_first_wrong_key() {
        let mut session = TypingSession::new(Mode::Words(1), "en".to_string(), "abc".to_string());
        session.error_mode = ErrorMode::Master;
        session.type_char('x');
        assert_eq!(session.state, super::TestState::Failed);
    }

    #[test]
    fn target_generation_respects_words_mode() {
        let target = target_for_mode(
            &Mode::Words(10),
            &dictionary(),
            GenerationOptions {
                punctuation: false,
                numbers: false,
            },
        );
        assert_eq!(target.split_whitespace().count(), 2);
    }

    #[test]
    fn words_mode_uses_existing_target_prefix() {
        let target: Vec<char> = "one two three four".chars().collect();
        let prefix: String = target_prefix_for_words(&target, 2).into_iter().collect();
        assert_eq!(prefix, "one two");
    }

    #[test]
    fn time_mode_does_not_count_untyped_future_text_as_missed() {
        let mut session =
            TypingSession::new(Mode::Time(15), "en".to_string(), "abc def ghi".to_string());
        session.type_char('a');
        session.finish();

        let metrics = session.metrics();
        assert_eq!(metrics.missed_characters, 0);
        assert_eq!(metrics.errors, 0);
        assert!(
            session
                .char_states()
                .iter()
                .all(|(_, state)| *state != super::CharState::Missed)
        );
    }
}
