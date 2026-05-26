use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use rand::Rng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::keyboard::HEATMAP_EXTRA_SYMBOLS;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dictionary {
    pub name: String,
    pub language: String,
    pub words: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct GenerationOptions {
    pub punctuation: bool,
    pub numbers: bool,
}

#[derive(Debug, Error)]
pub enum DictionaryError {
    #[error("dictionary must have a name")]
    MissingName,
    #[error("dictionary must contain at least one word")]
    Empty,
}

impl Dictionary {
    pub fn validate(&self) -> std::result::Result<(), DictionaryError> {
        if self.name.trim().is_empty() {
            return Err(DictionaryError::MissingName);
        }
        if self.words.is_empty() {
            return Err(DictionaryError::Empty);
        }
        Ok(())
    }

    pub fn named(name: &str) -> Self {
        available()
            .into_iter()
            .find(|dictionary| dictionary.name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| available().remove(0))
    }
}

pub fn available() -> Vec<Dictionary> {
    merge_by_name(builtins(), load_user_dictionaries())
}

pub fn builtins() -> Vec<Dictionary> {
    vec![
        Dictionary {
            name: "english".to_string(),
            language: "en".to_string(),
            words: merged_words(
                "the be to of and a in that have i it for not on with he as you do at this but his by from they we say her she or an will my one all would there their",
                ENGLISH_EXTRA,
            ),
        },
        Dictionary {
            name: "english_1k".to_string(),
            language: "en".to_string(),
            words: merged_words(
                "time person year way day thing man world life hand part child eye woman place work week case point government company number group problem fact",
                ENGLISH_EXTRA,
            ),
        },
        Dictionary {
            name: "russian".to_string(),
            language: "ru".to_string(),
            words: merged_words(
                "один который для ничто конечный бы много сразу думать или вода женщина знать почему почти себя земля потом же пока кто человек еще год от до стать подумать друг хотеть главный страна со по для мочь жизнь первый более минута",
                RUSSIAN_EXTRA,
            ),
        },
        Dictionary {
            name: "russian_1k".to_string(),
            language: "ru".to_string(),
            words: merged_words(
                "и в не на я быть тот он с что а по это она этот к но они мы как из у который то за свой весь год от так о для ты же все",
                RUSSIAN_EXTRA,
            ),
        },
    ]
}

pub fn write_default_files() -> anyhow::Result<()> {
    migrate_flat_language_files()?;
    for dictionary in builtins() {
        let dir = crate::config::Config::language_category_dir(category_for_language(
            &dictionary.language,
        ))?;
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.txt", dictionary.name));
        if path.exists() {
            continue;
        }
        fs::write(&path, dictionary.words.join("\n"))
            .with_context(|| format!("failed to write dictionary to {}", path.display()))?;
    }
    Ok(())
}

pub fn generate_words(
    dictionary: &Dictionary,
    amount: usize,
    options: GenerationOptions,
) -> String {
    let mut rng = rand::thread_rng();
    let mut pool = unique_words(dictionary);
    pool.shuffle(&mut rng);

    decorate_word_iter(pool.into_iter().take(amount), options, &mut rng)
}

pub fn decorate_words(text: &str, options: GenerationOptions) -> String {
    let mut rng = rand::thread_rng();
    decorate_word_iter(
        text.split_whitespace().map(str::to_string),
        options,
        &mut rng,
    )
}

fn decorate_word_iter(
    words: impl Iterator<Item = String>,
    options: GenerationOptions,
    rng: &mut impl Rng,
) -> String {
    let mut output = Vec::new();
    let mut next_number_at = rng.gen_range(4..=8);
    let mut next_punctuation_at = rng.gen_range(3..=6);

    for (idx, base) in words.enumerate() {
        let mut word = base;

        if options.numbers && idx >= next_number_at {
            word = random_number_token(rng);
            next_number_at += rng.gen_range(5..=10);
        }

        if options.punctuation && idx >= next_punctuation_at {
            word = punctuate_word(word, rng);
            next_punctuation_at += rng.gen_range(3..=7);
        }

        output.push(word);
    }

    output.join(" ")
}

fn random_number_token(rng: &mut impl Rng) -> String {
    match rng.gen_range(0..5) {
        0 => rng.gen_range(0..10).to_string(),
        1 => rng.gen_range(10..100).to_string(),
        2 => rng.gen_range(100..1000).to_string(),
        3 => rng.gen_range(1000..10_000).to_string(),
        _ => format!("{}{}", rng.gen_range(1..10), rng.gen_range(0..10)),
    }
}

#[derive(Clone, Copy)]
enum PunctuationStyle {
    Prefix(&'static str),
    Suffix(&'static str),
    Wrap(&'static str, &'static str),
}

const PUNCTUATION_STYLES: &[PunctuationStyle] = &[
    PunctuationStyle::Wrap(HEATMAP_EXTRA_SYMBOLS[0], HEATMAP_EXTRA_SYMBOLS[0]),
    PunctuationStyle::Prefix(HEATMAP_EXTRA_SYMBOLS[1]),
    PunctuationStyle::Suffix(HEATMAP_EXTRA_SYMBOLS[2]),
    PunctuationStyle::Prefix(HEATMAP_EXTRA_SYMBOLS[3]),
    PunctuationStyle::Prefix(HEATMAP_EXTRA_SYMBOLS[4]),
    PunctuationStyle::Prefix(HEATMAP_EXTRA_SYMBOLS[5]),
    PunctuationStyle::Suffix(HEATMAP_EXTRA_SYMBOLS[6]),
    PunctuationStyle::Prefix(HEATMAP_EXTRA_SYMBOLS[7]),
    PunctuationStyle::Prefix(HEATMAP_EXTRA_SYMBOLS[8]),
    PunctuationStyle::Prefix(HEATMAP_EXTRA_SYMBOLS[9]),
    PunctuationStyle::Prefix(HEATMAP_EXTRA_SYMBOLS[10]),
    PunctuationStyle::Suffix(HEATMAP_EXTRA_SYMBOLS[11]),
    PunctuationStyle::Suffix(HEATMAP_EXTRA_SYMBOLS[12]),
    PunctuationStyle::Wrap(HEATMAP_EXTRA_SYMBOLS[13], HEATMAP_EXTRA_SYMBOLS[14]),
    PunctuationStyle::Wrap(HEATMAP_EXTRA_SYMBOLS[15], HEATMAP_EXTRA_SYMBOLS[16]),
    PunctuationStyle::Prefix(HEATMAP_EXTRA_SYMBOLS[17]),
    PunctuationStyle::Suffix(HEATMAP_EXTRA_SYMBOLS[18]),
    PunctuationStyle::Suffix(HEATMAP_EXTRA_SYMBOLS[19]),
    PunctuationStyle::Suffix(HEATMAP_EXTRA_SYMBOLS[20]),
    PunctuationStyle::Wrap(HEATMAP_EXTRA_SYMBOLS[21], HEATMAP_EXTRA_SYMBOLS[21]),
    PunctuationStyle::Wrap(HEATMAP_EXTRA_SYMBOLS[22], HEATMAP_EXTRA_SYMBOLS[22]),
    PunctuationStyle::Wrap(HEATMAP_EXTRA_SYMBOLS[23], HEATMAP_EXTRA_SYMBOLS[24]),
    PunctuationStyle::Suffix(HEATMAP_EXTRA_SYMBOLS[25]),
    PunctuationStyle::Suffix(HEATMAP_EXTRA_SYMBOLS[26]),
    PunctuationStyle::Suffix(HEATMAP_EXTRA_SYMBOLS[27]),
    PunctuationStyle::Suffix(HEATMAP_EXTRA_SYMBOLS[28]),
];

fn punctuate_word(word: String, rng: &mut impl Rng) -> String {
    let style = PUNCTUATION_STYLES
        .choose(rng)
        .copied()
        .unwrap_or(PunctuationStyle::Suffix("."));
    style.apply(word)
}

impl PunctuationStyle {
    fn apply(self, word: String) -> String {
        match self {
            Self::Prefix(mark) => format!("{mark}{word}"),
            Self::Suffix(mark) => format!("{word}{mark}"),
            Self::Wrap(open, close) => format!("{open}{word}{close}"),
        }
    }

    #[cfg(test)]
    fn symbols(self) -> Vec<&'static str> {
        match self {
            Self::Prefix(mark) | Self::Suffix(mark) => vec![mark],
            Self::Wrap(open, close) => vec![open, close],
        }
    }
}

pub fn available_unique_words(dictionary: &Dictionary) -> usize {
    unique_words(dictionary).len()
}

fn unique_words(dictionary: &Dictionary) -> Vec<String> {
    let mut words = dictionary.words.clone();
    words.sort();
    words.dedup();
    words
}

fn load_user_dictionaries() -> Vec<Dictionary> {
    let Ok(dir) = crate::config::Config::languages_dir() else {
        return Vec::new();
    };
    dictionary_file_paths(&dir)
        .into_iter()
        .filter_map(|path| load_dictionary_file(&path).ok())
        .filter(|dictionary| dictionary.validate().is_ok())
        .collect()
}

fn dictionary_file_paths(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut paths = Vec::new();
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            paths.extend(dictionary_file_paths(&path));
        } else if is_dictionary_file(&path) {
            paths.push(path);
        }
    }
    paths
}

fn is_dictionary_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("json" | "toml" | "txt")
    )
}

fn migrate_flat_language_files() -> anyhow::Result<()> {
    let dir = crate::config::Config::languages_dir()?;
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(());
    };

    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if !path.is_file() || !is_dictionary_file(&path) {
            continue;
        }

        let Ok(dictionary) = load_dictionary_file(&path) else {
            continue;
        };
        let category = category_for_language(&dictionary.language);
        let target_dir = crate::config::Config::language_category_dir(category)?;
        fs::create_dir_all(&target_dir)?;
        let Some(file_name) = path.file_name() else {
            continue;
        };
        let target = next_available_path(target_dir.join(file_name));
        fs::rename(&path, &target).with_context(|| {
            format!(
                "failed to move dictionary from {} to {}",
                path.display(),
                target.display()
            )
        })?;
    }

    Ok(())
}

fn next_available_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("dictionary");
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    for idx in 1.. {
        let file_name = if extension.is_empty() {
            format!("{stem}-{idx}")
        } else {
            format!("{stem}-{idx}.{extension}")
        };
        let candidate = parent.join(file_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    path
}

fn load_dictionary_file(path: &Path) -> anyhow::Result<Dictionary> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read dictionary at {}", path.display()))?;
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => serde_json::from_str(&raw)
            .with_context(|| format!("invalid dictionary json at {}", path.display())),
        Some("toml") => toml::from_str(&raw)
            .with_context(|| format!("invalid dictionary toml at {}", path.display())),
        Some("txt") => Ok(dictionary_from_txt(path, &raw)),
        _ => anyhow::bail!("unsupported dictionary format at {}", path.display()),
    }
}

fn dictionary_from_txt(path: &Path, raw: &str) -> Dictionary {
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("custom")
        .to_string();
    Dictionary {
        language: language_from_category(path).unwrap_or_else(|| infer_language(&name, raw)),
        name,
        words: parse_word_list(raw),
    }
}

fn parse_word_list(raw: &str) -> Vec<String> {
    let mut words: Vec<String> = raw
        .split(is_word_separator)
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .map(ToString::to_string)
        .collect();
    words.sort();
    words.dedup();
    words
}

fn is_word_separator(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | ';'
                | '|'
                | '/'
                | '\\'
                | ':'
                | '='
                | '"'
                | '\''
                | '`'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
                | '!'
                | '?'
                | '.'
                | '…'
        )
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

fn language_from_category(path: &Path) -> Option<String> {
    match path.parent()?.file_name()?.to_str()? {
        "russian" => Some("ru".to_string()),
        "english" => Some("en".to_string()),
        _ => None,
    }
}

fn category_for_language(language: &str) -> &'static str {
    match language {
        "ru" | "russian" => "russian",
        _ => "english",
    }
}

fn merge_by_name(mut base: Vec<Dictionary>, custom: Vec<Dictionary>) -> Vec<Dictionary> {
    for dictionary in custom {
        if let Some(existing) = base
            .iter_mut()
            .find(|item| item.name.eq_ignore_ascii_case(&dictionary.name))
        {
            *existing = dictionary;
        } else {
            base.push(dictionary);
        }
    }
    base
}

fn words(raw: &str) -> Vec<String> {
    parse_word_list(raw)
}

fn merged_words(first: &str, second: &str) -> Vec<String> {
    let mut merged = words(first);
    merged.extend(words(second));
    merged.sort();
    merged.dedup();
    merged
}

const ENGLISH_EXTRA: &str = r#"
about above accept account across act action active activity actual add address adult affect after
again against age agency ago agree ahead air allow almost alone along already also although always
among amount analysis animal answer anyone anything appear apply approach area argue arm around arrive
art article artist ask attention audience authority available avoid away baby back bad bag ball bank
base beautiful because become bed before begin behavior behind believe benefit best better beyond big
bill bit black blood blue board body book born both box boy break bring brother budget build building
business buy call camera campaign can capital car card care career carry case catch cause cell center
central century certain chair chance change character charge check child choice choose church citizen
city civil claim class clear close coach cold collection college color come commercial common community
company compare computer concern condition conference consider consumer contain continue control cost
could country couple course court cover create culture cup current customer dark data daughter day deal
death debate decide decision deep defense degree describe design detail develop difference different
difficult dinner direction discover discuss disease doctor door draw dream drive drop drug during each
early east easy economy edge education effect effort eight either election else employee end energy enjoy
enough enter entire environment especially establish even evening event ever every everybody evidence
example experience explain eye face fact factor fail fall family far fast father fear federal feel field
fight figure fill final finally financial find fine fire firm first fish floor fly focus follow food foot
force foreign forget form forward four free friend front full fund future game garden gas general generation
get girl give glass goal good government great green ground group grow guess gun hair half hand happen
happy hard have head health hear heart heat heavy help here high history hold home hope hospital hot hotel
hour house however human hundred husband idea identify image imagine impact important improve include
increase indeed industry information inside instead institution interest interesting international interview
investment involve issue item itself job join just keep key kid kind kitchen know knowledge land language
large last late later laugh law lawyer lay lead leader learn least leave left leg legal less letter level
life light like likely line list listen little live local long look lose loss lot love low machine magazine
main maintain major make man manage management manager many market marriage material matter may maybe mean
measure media medical meet member memory mention message method middle might military mind minute miss
mission model modern moment money month morning mother mouth move movement movie much music must name
nation national natural nature near nearly necessary need network never new news newspaper next nice night
none north note nothing notice number occur off offer office officer official often oil old once open
operation opportunity option order organization other others outside over owner page pain painting paper
parent part participant particular partner party pass past patient pattern pay peace people perform perhaps
period person personal phone physical pick picture piece place plan plant play player point police policy
political poor popular position positive possible power practice prepare present president pressure pretty
prevent price private probably problem process produce product professional program project property protect
prove provide public pull purpose push quality question quickly quite radio raise range rate rather reach
read ready real reality realize reason receive recent recognize record red reduce reflect region relate
relationship religious remain remember remove report represent require research resource respond response
responsibility rest result return reveal rich right rise risk road rock role room rule run safe same save
scene school science scientist score sea season seat second section security see seek seem sell send senior
sense series serious service set seven several shake share she short shoulder show side sign significant
similar simple simply since sing single sister sit site situation six size skill skin small social society
soldier some somebody someone something sometimes son song soon sort sound source south southern space speak
special specific speech spend sport spring staff stage stand standard star start state statement station stay
step still stock stop store story strategy street strong structure student study stuff style subject success
successful such suddenly suffer suggest summer support sure surface system table take talk task tax teach
teacher team technology television tell tend term test than thank themselves theory thing think third those
though thought thousand threat three through throughout throw thus time today together tonight too top total
tough toward town trade traditional training travel treat treatment tree trial trip trouble true truth try
turn under understand unit until up upon us use usually value various very victim view violence visit voice
vote wait walk wall want war watch water way wear week weight well west western whatever when where whether
which while white whole whom whose why wide wife will win wind window wish within without woman wonder word
work worker world worry would write writer wrong yard yeah year yes yet young
"#;

const RUSSIAN_EXTRA: &str = r#"
август автор адрес аллея армия артист банк берег беседа билет близкий богатый болезнь больше брат
будто буква бумага быстро важный вдруг век верить вес ветер вечер вещь взгляд видеть видимо внимание
вода воздух возможность вопрос ворота восток время встреча выбор говорить год голос город готовый
граница гражданин группа далеко дальше движение дверь девушка действие дело деньги деревня день дерево
десять деталь директор длинный добро документ долг дом дорога друг думать душа желание жена жизнь
журнал забыть завод завтра задача закон зал заметить занятие запад запах защита земля зеркало зима
значение знать игра идея идти изменить имя иногда интерес история источник карта картина квартира
кино класс книга комната компания конец контроль корабль короткий космос край красный крепкий культура
легкий лето линия лист лицо лучше магазин мальчик машина место метр минута мир мнение много молодой
молча момент море Москва мост музыка мысль назад найти народ начало неделя нельзя несколько новый
номер ночь нужно образ общий окно около отец ответ память пара парк партия первый письмо писать план
плечо площадь победа повод погода поддержка поезд поздний показать поле полезный полный помощь понять
порядок последний поставить поэтому появиться правда праздник предмет прежде президент прийти пример
природа причина проблема программа продукт производство просто путь работа рабочий радость раз разный
район рано рассказ ребенок решение республика результат река речь родина родитель роль рука русский
рядом самый свет свободный связь сегодня север секунда семья сердце сильный система сказать случай
следующий слово служба смысл снова собака событие совсем совет современный солнце состав способ
спросить среда средство старый стать стена сторона столица стол страна строить студент судьба сын
счастье театр тело теперь территория техника тихий товар точка труд улица уровень урок успех утро
участие факт февраль фигура фильм форма хороший хотя цель центр часть человек чувство школа язык
ясный абсолютный авария агент активный акт анализ аптека апрель архив атом база батарея белый
безопасность бизнес библиотека благодарность больница будущий вариант верх весна вечерний виноват
власть влияние внутренний вместе волна война воля вопросительный воскресенье воспоминание время
высокий газета глубокий главный глаза герой готовить громкий данные двор движение десяток диалог
добрый договор дождь должность достаток доход другой единый железо женщина живой задача заметка
запись звезда звонок здоровый зеленый зеркало известный инженер инструмент искусство июль июнь
кабинет камень качество квартал километр клиент ключ команда комитет компьютер коридор костюм кофе
кровать круг крупный курс лес личный ложка май материал медленный мелкий мера месяц метод миллион
мнение модель море мороз мужчина наблюдать название налог направление наука необходимый небесный
недавно нижний нормальный область обещание общество обычный огонь одежда операция опыт остров отдел
открытый очередь память пассажир период песня письмо питание плечо поворот подготовка позиция
покупка политика польза портрет порядок правило предмет предложение представитель причина проверка
проект просьба прямой пятница размер развитие разговор различный рынок ровный сад самолет сбор
свежий свободный сезон секрет серьезный сигнал ситуация скорость слабый служить случайный смелый
снег собственный сосед спокойный срочно старший стекло стиль страница сумма телефон температура
теория теплый товар товарищ транспорт требование третий университет участок февраль характер хлеб
хозяин холодный цвет церковь цифра чай широкий энергия этаж январь
"#;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::keyboard::HEATMAP_EXTRA_SYMBOLS;

    use super::{
        Dictionary, GenerationOptions, PUNCTUATION_STYLES, builtins, decorate_words,
        dictionary_from_txt, generate_words,
    };

    #[test]
    fn validates_empty_dictionary() {
        let dictionary = Dictionary {
            name: "bad".to_string(),
            language: "en".to_string(),
            words: vec![],
        };
        assert!(dictionary.validate().is_err());
    }

    #[test]
    fn generates_requested_amount() {
        let dictionary = builtins().remove(0);
        let text = generate_words(
            &dictionary,
            12,
            GenerationOptions {
                punctuation: false,
                numbers: false,
            },
        );
        assert_eq!(text.split_whitespace().count(), 12);
    }

    #[test]
    fn can_add_numbers_and_punctuation() {
        let dictionary = builtins().remove(0);
        let text = generate_words(
            &dictionary,
            25,
            GenerationOptions {
                punctuation: true,
                numbers: true,
            },
        );
        assert!(text.chars().any(|ch| ch.is_ascii_digit()));
        assert!(
            HEATMAP_EXTRA_SYMBOLS
                .iter()
                .any(|symbol| text.contains(*symbol))
        );
    }

    #[test]
    fn punctuation_styles_cover_heatmap_extra_symbols() {
        let expected = HEATMAP_EXTRA_SYMBOLS
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let actual = PUNCTUATION_STYLES
            .iter()
            .flat_map(|style| (*style).symbols())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn decorating_without_options_keeps_word_pool() {
        let text = "one two three four five";
        let decorated = decorate_words(
            text,
            GenerationOptions {
                punctuation: false,
                numbers: false,
            },
        );
        assert_eq!(decorated, text);
    }

    #[test]
    fn generated_words_do_not_repeat() {
        let dictionary = builtins().remove(0);
        let text = generate_words(
            &dictionary,
            50,
            GenerationOptions {
                punctuation: false,
                numbers: false,
            },
        );
        let words: Vec<_> = text.split_whitespace().collect();
        let unique: std::collections::HashSet<_> = words.iter().copied().collect();
        assert_eq!(words.len(), unique.len());
        assert!(
            words
                .iter()
                .all(|word| !word.chars().any(|ch| ch.is_ascii_digit()))
        );
    }

    #[test]
    fn parses_txt_dictionary_with_common_separators() {
        let dictionary = dictionary_from_txt(
            Path::new("mixed.txt"),
            "alpha beta,gamma;delta|epsilon\nzeta:eta. theta",
        );
        assert_eq!(dictionary.name, "mixed");
        assert_eq!(dictionary.language, "en");
        assert_eq!(
            dictionary.words,
            vec![
                "alpha".to_string(),
                "beta".to_string(),
                "delta".to_string(),
                "epsilon".to_string(),
                "eta".to_string(),
                "gamma".to_string(),
                "theta".to_string(),
                "zeta".to_string(),
            ]
        );
    }

    #[test]
    fn builtin_dictionaries_have_release_sized_word_pools() {
        for dictionary in builtins() {
            let unique = dictionary
                .words
                .iter()
                .collect::<std::collections::HashSet<_>>();
            assert!(
                unique.len() >= 250,
                "{} has only {} unique words",
                dictionary.name,
                unique.len()
            );
        }
    }
}
