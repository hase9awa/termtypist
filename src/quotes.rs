use std::fs;
use std::path::Path;

use anyhow::Context;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::typing::QuoteLength;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteSet {
    pub name: String,
    pub language: String,
    pub short: Vec<String>,
    pub medium: Vec<String>,
    pub long: Vec<String>,
    pub random: Vec<String>,
}

impl QuoteSet {
    fn validate(&self) -> bool {
        !self.name.trim().is_empty()
            && !self.language.trim().is_empty()
            && (!self.short.is_empty()
                || !self.medium.is_empty()
                || !self.long.is_empty()
                || !self.random.is_empty())
    }
}

pub fn quote(length: QuoteLength, language: &str) -> String {
    let sets = available();
    let selected = sets
        .iter()
        .find(|set| set.language.eq_ignore_ascii_case(language))
        .or_else(|| {
            sets.iter()
                .find(|set| set.language.eq_ignore_ascii_case("en"))
        });

    let Some(set) = selected else {
        return fallback_quote(length, language);
    };

    choose_quote(set, length).unwrap_or_else(|| fallback_quote(length, language))
}

pub fn available() -> Vec<QuoteSet> {
    merge_by_name(builtins(), load_user_quotes())
}

pub fn builtins() -> Vec<QuoteSet> {
    vec![
        QuoteSet {
            name: "english".to_string(),
            language: "en".to_string(),
            short: quote_vec(ENGLISH_SHORT_QUOTES),
            medium: quote_vec(ENGLISH_MEDIUM_QUOTES),
            long: quote_vec(ENGLISH_LONG_QUOTES),
            random: Vec::new(),
        },
        QuoteSet {
            name: "russian".to_string(),
            language: "ru".to_string(),
            short: quote_vec(RUSSIAN_SHORT_QUOTES),
            medium: quote_vec(RUSSIAN_MEDIUM_QUOTES),
            long: quote_vec(RUSSIAN_LONG_QUOTES),
            random: Vec::new(),
        },
    ]
}

pub fn write_default_files() -> anyhow::Result<()> {
    let dir = crate::config::Config::quotes_dir()?;
    fs::create_dir_all(&dir)?;
    for quotes in builtins() {
        let path = dir.join(format!("{}.txt", quotes.name));
        if path.exists() {
            continue;
        }
        fs::write(&path, quote_lines(&quotes).join("\n"))
            .with_context(|| format!("failed to write quotes to {}", path.display()))?;
    }
    Ok(())
}

fn choose_quote(set: &QuoteSet, length: QuoteLength) -> Option<String> {
    let mut rng = rand::thread_rng();
    let preferred = match length {
        QuoteLength::Short => set.short.choose(&mut rng),
        QuoteLength::Medium => set.medium.choose(&mut rng),
        QuoteLength::Long => set.long.choose(&mut rng),
        QuoteLength::Random => {
            let pool = quote_lines(set);
            return pool.choose(&mut rng).cloned();
        }
    };
    preferred.cloned().or_else(|| {
        let pool = quote_lines(set);
        pool.choose(&mut rng).cloned()
    })
}

fn load_user_quotes() -> Vec<QuoteSet> {
    let Ok(dir) = crate::config::Config::quotes_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| load_quotes_file(&entry.path()).ok())
        .filter(QuoteSet::validate)
        .collect()
}

fn load_quotes_file(path: &Path) -> anyhow::Result<QuoteSet> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read quotes at {}", path.display()))?;
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => serde_json::from_str(&raw)
            .with_context(|| format!("invalid quotes json at {}", path.display())),
        Some("toml") => toml::from_str(&raw)
            .with_context(|| format!("invalid quotes toml at {}", path.display())),
        Some("txt") => Ok(quotes_from_txt(path, &raw)),
        _ => anyhow::bail!("unsupported quotes format at {}", path.display()),
    }
}

fn quotes_from_txt(path: &Path, raw: &str) -> QuoteSet {
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("custom")
        .to_string();
    let language = infer_language(&name, raw);
    let quotes = parse_quote_list(raw);
    let mut set = QuoteSet {
        name,
        language,
        short: Vec::new(),
        medium: Vec::new(),
        long: Vec::new(),
        random: quotes.clone(),
    };

    for quote in quotes {
        match quote.split_whitespace().count() {
            0..=12 => set.short.push(quote),
            13..=30 => set.medium.push(quote),
            _ => set.long.push(quote),
        }
    }

    set
}

fn parse_quote_list(raw: &str) -> Vec<String> {
    let normalized = raw.replace("\r\n", "\n");
    if normalized.contains("\n---\n") {
        return normalized
            .split("\n---\n")
            .map(normalize_quote)
            .filter(|quote| !quote.is_empty())
            .collect();
    }

    normalized
        .lines()
        .map(normalize_quote)
        .filter(|quote| !quote.is_empty() && quote != "---")
        .collect()
}

fn normalize_quote(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn quote_lines(set: &QuoteSet) -> Vec<String> {
    let mut quotes = Vec::new();
    quotes.extend(set.short.iter().cloned());
    quotes.extend(set.medium.iter().cloned());
    quotes.extend(set.long.iter().cloned());
    quotes.extend(set.random.iter().cloned());
    quotes.sort();
    quotes.dedup();
    quotes
}

fn infer_language(name: &str, raw: &str) -> String {
    let normalized = name.to_lowercase();
    if normalized == "ru" || normalized.contains("russian") || normalized.contains("рус") {
        return "ru".to_string();
    }
    if normalized == "en" || normalized.contains("english") {
        return "en".to_string();
    }
    if raw
        .chars()
        .any(|ch| ('а'..='я').contains(&ch) || ('А'..='Я').contains(&ch))
    {
        return "ru".to_string();
    }
    "en".to_string()
}

fn merge_by_name(mut base: Vec<QuoteSet>, custom: Vec<QuoteSet>) -> Vec<QuoteSet> {
    for quotes in custom {
        if let Some(existing) = base
            .iter_mut()
            .find(|item| item.name.eq_ignore_ascii_case(&quotes.name))
        {
            *existing = quotes;
        } else {
            base.push(quotes);
        }
    }
    base
}

fn quote_vec(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_string()).collect()
}

fn fallback_quote(length: QuoteLength, language: &str) -> String {
    let ru = language == "ru";
    match (ru, length) {
        (true, QuoteLength::Short) => "Не ошибается тот, кто ничего не делает.".to_string(),
        (true, QuoteLength::Medium) => {
            "Каждая тренировка начинается с первого символа и заканчивается устойчивым ритмом."
                .to_string()
        }
        (true, QuoteLength::Long) => "Скорость печати приходит не от спешки, а от спокойного внимания к каждому слову, строке и исправленной ошибке.".to_string(),
        (false, QuoteLength::Short) => "Practice makes steady hands.".to_string(),
        (false, QuoteLength::Medium) => {
            "Typing speed grows when accuracy stays calm under pressure.".to_string()
        }
        (false, QuoteLength::Long) => "A focused session is built from small precise decisions, not from rushing through every word on the screen.".to_string(),
        (_, QuoteLength::Random) => {
            if ru {
                "Пока пальцы движутся ровно, мысли остаются свободными.".to_string()
            } else {
                "Smooth rhythm turns practice into quiet progress.".to_string()
            }
        }
    }
}

const ENGLISH_SHORT_QUOTES: &[&str] = &[
    "Practice makes steady hands.",
    "Accuracy gives speed a place to grow.",
    "Calm fingers find the next letter.",
    "Every clean line builds confidence.",
    "Rhythm starts with patient attention.",
    "Small corrections prevent larger mistakes.",
    "Focus turns effort into progress.",
    "A steady pace beats a frantic sprint.",
    "Good habits survive difficult passages.",
    "Clear eyes guide quick hands.",
    "Repeat the basics until they feel natural.",
    "Speed follows control.",
];

const ENGLISH_MEDIUM_QUOTES: &[&str] = &[
    "Typing speed grows when accuracy stays calm under pressure.",
    "A clean rhythm lets every word arrive without a fight.",
    "Strong practice rewards the typist who notices small mistakes early.",
    "The best sessions feel measured, focused, and slightly demanding.",
    "Every paragraph teaches the hands to trust the eyes.",
    "Useful speed is built from correct letters repeated many times.",
    "A relaxed posture keeps attention available for the next word.",
    "Errors lose power when they are seen, corrected, and understood.",
    "The keyboard becomes familiar when practice has variety and structure.",
    "Fast typing is quiet discipline made visible on the screen.",
    "Fresh text keeps the mind awake while the fingers learn patterns.",
    "Confidence grows when the next key is chosen without hesitation.",
];

const ENGLISH_LONG_QUOTES: &[&str] = &[
    "A focused session is built from small precise decisions, not from rushing through every word on the screen.",
    "When the hands move too quickly for the eyes, mistakes multiply, but a steady rhythm gives attention enough room to guide each letter.",
    "The point of practice is not to win one test, but to make accurate movement feel ordinary during every difficult sentence.",
    "A good typist reads ahead just enough to stay prepared, then lets the current word receive full attention before moving on.",
    "Progress often appears quietly, after many sessions where the only visible work was correcting small habits and keeping the pace honest.",
    "The strongest rhythm is flexible: it slows down for unfamiliar words, recovers after errors, and returns to speed without panic.",
    "A useful typing test should challenge memory, timing, accuracy, and endurance without hiding mistakes behind decoration or noise.",
    "Better results come from noticing patterns: which letters cause hesitation, which words break rhythm, and which shortcuts create errors.",
    "A long passage teaches patience because every rushed correction costs more time than the careful keystroke that would have avoided it.",
    "The keyboard rewards consistency, so the most valuable practice is the kind that can be repeated tomorrow with the same attention.",
];

const RUSSIAN_SHORT_QUOTES: &[&str] = &[
    "Не ошибается тот, кто ничего не делает.",
    "Точный ритм сильнее спешки.",
    "Внимание начинается с первой буквы.",
    "Ровные руки берегут скорость.",
    "Каждая строка тренирует терпение.",
    "Ошибку легче исправить сразу.",
    "Сначала точность, потом скорость.",
    "Спокойный темп держит мысль.",
    "Привычка растет из повторения.",
    "Чистый набор дает уверенность.",
    "Сложное слово требует паузы.",
    "Практика любит порядок.",
];

const RUSSIAN_MEDIUM_QUOTES: &[&str] = &[
    "Каждая тренировка начинается с первого символа и заканчивается устойчивым ритмом.",
    "Скорость становится надежной, когда пальцы не спорят с вниманием.",
    "Хороший набор виден не по спешке, а по чистым строкам.",
    "Короткая пауза перед трудным словом часто экономит целую секунду.",
    "Чем спокойнее взгляд, тем увереннее руки находят следующую клавишу.",
    "Ошибки полезны, если после них меняется привычка движения.",
    "Разные тексты развивают память, точность, темп и выносливость.",
    "Сильный результат складывается из маленьких правильных решений.",
    "Тренировка становится честной, когда каждая ошибка остается заметной.",
    "Ровный ритм помогает читать вперед и не терять текущую строку.",
    "Клавиатура быстро запоминается, если практика остается регулярной.",
    "Уверенность появляется там, где точность повторяется каждый день.",
];

const RUSSIAN_LONG_QUOTES: &[&str] = &[
    "Скорость печати приходит не от спешки, а от спокойного внимания к каждому слову, строке и исправленной ошибке.",
    "Когда пальцы торопятся быстрее взгляда, ошибки начинают множиться, но ровный ритм возвращает набору точность и спокойствие.",
    "Главная цель тренировки не в одном удачном результате, а в том, чтобы правильные движения становились привычными.",
    "Хороший наборщик читает немного вперед, но не бросает текущее слово без внимания и не теряет порядок букв.",
    "Заметный прогресс часто приходит после тихих занятий, где вся работа состояла в исправлении маленьких неточных привычек.",
    "Сильный ритм умеет замедляться на сложных местах, переживать ошибку и возвращаться к скорости без лишнего напряжения.",
    "Полезный тест должен проверять память, внимание, точность и выносливость, не пряча ошибки за красивым оформлением.",
    "Лучшие результаты появляются, когда становятся понятны трудные буквы, слабые сочетания и слова, которые ломают темп.",
    "Длинный текст учит терпению, потому что каждое лишнее исправление занимает больше времени, чем аккуратное нажатие.",
    "Клавиатура награждает постоянство, поэтому самая ценная тренировка та, которую завтра можно повторить с тем же вниманием.",
];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{builtins, parse_quote_list, quotes_from_txt};

    #[test]
    fn parses_txt_quotes_from_lines() {
        let quotes = parse_quote_list("First quote.\nSecond quote.\n\nThird quote.");
        assert_eq!(
            quotes,
            vec![
                "First quote.".to_string(),
                "Second quote.".to_string(),
                "Third quote.".to_string()
            ]
        );
    }

    #[test]
    fn builds_quote_set_from_txt_filename_and_lengths() {
        let set = quotes_from_txt(
            Path::new("russian.txt"),
            "Короткая цитата.\nЭто уже более длинная цитата для проверки средней категории.",
        );
        assert_eq!(set.name, "russian");
        assert_eq!(set.language, "ru");
        assert_eq!(set.random.len(), 2);
        assert_eq!(set.short.len(), 2);
    }

    #[test]
    fn builtin_quote_sets_have_real_variety() {
        for set in builtins() {
            assert!(set.short.len() >= 10);
            assert!(set.medium.len() >= 10);
            assert!(set.long.len() >= 10);
        }
    }
}
