use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

use crate::metrics::Metrics;
use crate::typing::{SpeedSample, TypingSession, mode_label};

#[derive(Debug, Clone)]
pub struct ResultRow {
    pub id: i64,
    pub created_at: String,
    pub mode: String,
    pub language: String,
    pub duration_sec: f64,
    pub wpm: f64,
    pub raw_wpm: f64,
    pub accuracy: f64,
    pub errors: usize,
    pub target_text: String,
    pub input_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyErrorStat {
    pub key: String,
    pub errors: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardHeatmap {
    pub language: String,
    pub tests: usize,
    pub total_errors: usize,
    pub keys: Vec<KeyErrorStat>,
}

pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn open_default() -> Result<Self> {
        let path = Self::default_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::open(path)
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&path)
            .with_context(|| format!("failed to open sqlite database at {}", path.display()))?;
        let storage = Self { conn };
        storage.migrate()?;
        Ok(storage)
    }

    pub fn default_path() -> Result<PathBuf> {
        Ok(dirs::data_dir()
            .context("could not locate data directory")?
            .join("termtypist")
            .join("results.sqlite"))
    }

    pub fn save_result(&self, session: &TypingSession, metrics: Metrics) -> Result<()> {
        self.conn.execute(
            "insert into results (
                created_at, mode, language, duration_sec, wpm, raw_wpm, accuracy,
                consistency, correct_chars, incorrect_chars, extra_chars, missed_chars,
                errors, target_text, input_text
            ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                Utc::now().to_rfc3339(),
                mode_label(&session.mode),
                session.language,
                session.elapsed().as_secs_f64(),
                metrics.wpm,
                metrics.raw_wpm,
                metrics.accuracy,
                metrics.consistency,
                metrics.correct_characters as i64,
                metrics.incorrect_characters as i64,
                metrics.extra_characters as i64,
                metrics.missed_characters as i64,
                metrics.errors as i64,
                session.target,
                session.input_string(),
            ],
        )?;
        let result_id = self.conn.last_insert_rowid();
        self.save_key_mistakes(result_id, &session.language, &session.key_mistakes())?;
        self.save_speed_samples(result_id, &session.samples())?;
        Ok(())
    }

    fn save_speed_samples(&self, result_id: i64, samples: &[SpeedSample]) -> Result<()> {
        for sample in samples {
            self.conn.execute(
                "insert into result_speed_samples (result_id, second, wpm, raw_wpm, errors)
                 values (?1, ?2, ?3, ?4, ?5)",
                params![
                    result_id,
                    sample.second,
                    sample.wpm,
                    sample.raw_wpm,
                    sample.errors,
                ],
            )?;
        }

        Ok(())
    }

    fn save_key_mistakes(
        &self,
        result_id: i64,
        language: &str,
        mistakes: &[(char, usize)],
    ) -> Result<()> {
        let mut normalized = std::collections::HashMap::<String, usize>::new();
        for (ch, errors) in mistakes {
            if let Some(key) = normalize_key(language, *ch) {
                *normalized.entry(key).or_insert(0) += *errors;
            }
        }

        for (key, errors) in normalized {
            self.conn.execute(
                "insert into result_key_errors (result_id, language, key, errors)
                 values (?1, ?2, ?3, ?4)",
                params![result_id, language, key, errors as i64],
            )?;
        }

        Ok(())
    }

    pub fn recent_results(
        &self,
        limit: usize,
        language: Option<&str>,
        mode: Option<&str>,
    ) -> Result<Vec<ResultRow>> {
        self.recent_results_page(limit, 0, language, mode)
    }

    pub fn recent_results_page(
        &self,
        limit: usize,
        offset: usize,
        language: Option<&str>,
        mode: Option<&str>,
    ) -> Result<Vec<ResultRow>> {
        let mut rows = self.conn.prepare(
            "select id, created_at, mode, language, duration_sec, wpm, raw_wpm, accuracy, errors, target_text, input_text
             from results
             where (?1 is null or language = ?1) and (?2 is null or mode like ?2 || '%')
             order by id desc
             limit ?3 offset ?4",
        )?;
        let mapped = rows.query_map(
            params![language, mode, limit as i64, offset as i64],
            map_row,
        )?;
        mapped
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn result_count(&self, language: Option<&str>, mode: Option<&str>) -> Result<usize> {
        let count = self.conn.query_row(
            "select count(*)
             from results
             where (?1 is null or language = ?1) and (?2 is null or mode like ?2 || '%')",
            params![language, mode],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count as usize)
    }

    pub fn result_languages(&self) -> Result<Vec<String>> {
        let mut rows = self.conn.prepare(
            "select language
             from results
             group by language
             order by max(id) desc",
        )?;
        let mapped = rows.query_map([], |row| row.get::<_, String>(0))?;
        mapped
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn result_samples(&self, result_id: i64) -> Result<Vec<SpeedSample>> {
        let mut rows = self.conn.prepare(
            "select second, wpm, raw_wpm, errors
             from result_speed_samples
             where result_id = ?1
             order by second asc, id asc",
        )?;
        let mapped = rows.query_map(params![result_id], |row| {
            Ok(SpeedSample {
                second: row.get(0)?,
                wpm: row.get(1)?,
                raw_wpm: row.get(2)?,
                errors: row.get(3)?,
            })
        })?;
        mapped
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn personal_best(
        &self,
        language: Option<&str>,
        mode: Option<&str>,
    ) -> Result<Option<ResultRow>> {
        self.conn
            .query_row(
                "select id, created_at, mode, language, duration_sec, wpm, raw_wpm, accuracy, errors, target_text, input_text
                 from results
                 where (?1 is null or language = ?1) and (?2 is null or mode like ?2 || '%')
                 order by wpm desc, accuracy desc
                 limit 1",
                params![language, mode],
                map_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn replay_last(&self) -> Result<Option<ResultRow>> {
        self.conn
            .query_row(
                "select id, created_at, mode, language, duration_sec, wpm, raw_wpm, accuracy, errors, target_text, input_text
                 from results order by id desc limit 1",
                [],
                map_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn keyboard_heatmap(&self, language: &str) -> Result<KeyboardHeatmap> {
        let tests = self.conn.query_row(
            "select count(*) from results where language = ?1",
            params![language],
            |row| row.get::<_, i64>(0),
        )? as usize;

        let mut counts = std::collections::HashMap::<String, usize>::new();
        let mut key_rows = self.conn.prepare(
            "select key, sum(errors)
             from result_key_errors
             where language = ?1
             group by key",
        )?;
        let mapped_key_rows = key_rows.query_map(params![language], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;
        for row in mapped_key_rows {
            let (key, errors) = row?;
            *counts.entry(key).or_insert(0) += errors;
        }

        let mut fallback_rows = self.conn.prepare(
            "select target_text, input_text
             from results
             where language = ?1
               and not exists (
                   select 1 from result_key_errors
                   where result_key_errors.result_id = results.id
               )
             order by id desc",
        )?;
        let mapped_fallback_rows = fallback_rows.query_map(params![language], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in mapped_fallback_rows {
            let (target, input) = row?;
            accumulate_key_errors(language, &target, &input, &mut counts);
        }

        let mut keys = counts
            .into_iter()
            .map(|(key, errors)| KeyErrorStat { key, errors })
            .collect::<Vec<_>>();
        keys.sort_by(|left, right| {
            right
                .errors
                .cmp(&left.errors)
                .then_with(|| left.key.cmp(&right.key))
        });
        let total_errors = keys.iter().map(|key| key.errors).sum();

        Ok(KeyboardHeatmap {
            language: language.to_string(),
            tests,
            total_errors,
            keys,
        })
    }

    pub fn clear_results(&self) -> Result<usize> {
        self.conn.execute("delete from result_speed_samples", [])?;
        self.conn.execute("delete from result_key_errors", [])?;
        let deleted = self.conn.execute("delete from results", [])?;
        self.conn
            .execute("delete from sqlite_sequence where name = 'results'", [])
            .ok();
        self.conn
            .execute(
                "delete from sqlite_sequence where name = 'result_key_errors'",
                [],
            )
            .ok();
        self.conn
            .execute(
                "delete from sqlite_sequence where name = 'result_speed_samples'",
                [],
            )
            .ok();
        Ok(deleted)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "create table if not exists results (
                id integer primary key autoincrement,
                created_at text not null,
                mode text not null,
                language text not null,
                duration_sec real not null,
                wpm real not null,
                raw_wpm real not null,
                accuracy real not null,
                consistency real not null,
                correct_chars integer not null,
                incorrect_chars integer not null,
                extra_chars integer not null,
                missed_chars integer not null,
                errors integer not null,
                target_text text not null,
                input_text text not null
            );
            create table if not exists result_key_errors (
                id integer primary key autoincrement,
                result_id integer not null,
                language text not null,
                key text not null,
                errors integer not null,
                foreign key(result_id) references results(id) on delete cascade
            );
            create index if not exists idx_result_key_errors_language
                on result_key_errors(language);
            create index if not exists idx_result_key_errors_result
                on result_key_errors(result_id);
            create table if not exists result_speed_samples (
                id integer primary key autoincrement,
                result_id integer not null,
                second real not null,
                wpm real not null,
                raw_wpm real not null,
                errors real not null,
                foreign key(result_id) references results(id) on delete cascade
            );
            create index if not exists idx_result_speed_samples_result
                on result_speed_samples(result_id);",
        )?;
        Ok(())
    }
}

fn accumulate_key_errors(
    language: &str,
    target: &str,
    input: &str,
    counts: &mut std::collections::HashMap<String, usize>,
) {
    let target_chars = target.chars().collect::<Vec<_>>();
    let input_chars = input.chars().collect::<Vec<_>>();

    for error in aligned_key_errors(&target_chars, &input_chars) {
        match error {
            KeyError::Substitution { expected } => add_key_error(language, expected, counts),
            KeyError::Deletion { expected } => add_key_error(language, expected, counts),
            KeyError::Insertion => {}
        }
    }
}

fn add_key_error(language: &str, ch: char, counts: &mut std::collections::HashMap<String, usize>) {
    if let Some(key) = normalize_key(language, ch) {
        *counts.entry(key).or_insert(0) += 1;
    }
}

fn normalize_key(language: &str, ch: char) -> Option<String> {
    let _ = language;
    if ch.is_whitespace() {
        return None;
    }
    Some(ch.to_lowercase().collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyError {
    Substitution { expected: char },
    Deletion { expected: char },
    Insertion,
}

fn aligned_key_errors(target: &[char], input: &[char]) -> Vec<KeyError> {
    let rows = target.len() + 1;
    let cols = input.len() + 1;
    let mut distance = vec![vec![0usize; cols]; rows];

    for (idx, row) in distance.iter_mut().enumerate() {
        row[0] = idx;
    }
    for (idx, cell) in distance[0].iter_mut().enumerate() {
        *cell = idx;
    }

    for target_idx in 1..rows {
        for input_idx in 1..cols {
            let substitution_cost = usize::from(target[target_idx - 1] != input[input_idx - 1]);
            distance[target_idx][input_idx] = (distance[target_idx - 1][input_idx] + 1)
                .min(distance[target_idx][input_idx - 1] + 1)
                .min(distance[target_idx - 1][input_idx - 1] + substitution_cost);
        }
    }

    let mut errors = Vec::new();
    let mut target_idx = best_aligned_target_prefix(&distance, input.len());
    let mut input_idx = input.len();
    while target_idx > 0 || input_idx > 0 {
        if target_idx > 0 && input_idx > 0 {
            let expected = target[target_idx - 1];
            let actual = input[input_idx - 1];
            let substitution_cost = usize::from(expected != actual);
            if distance[target_idx][input_idx]
                == distance[target_idx - 1][input_idx - 1] + substitution_cost
            {
                if substitution_cost == 1 {
                    errors.push(KeyError::Substitution { expected });
                }
                target_idx -= 1;
                input_idx -= 1;
                continue;
            }
        }

        if target_idx > 0
            && distance[target_idx][input_idx] == distance[target_idx - 1][input_idx] + 1
        {
            errors.push(KeyError::Deletion {
                expected: target[target_idx - 1],
            });
            target_idx -= 1;
            continue;
        }

        if input_idx > 0 {
            errors.push(KeyError::Insertion);
            input_idx -= 1;
        }
    }

    errors.reverse();
    errors
}

fn best_aligned_target_prefix(distance: &[Vec<usize>], input_len: usize) -> usize {
    let mut best_idx = 0;
    let mut best_distance = usize::MAX;
    for (idx, row) in distance.iter().enumerate() {
        let current = row[input_len];
        if current < best_distance || (current == best_distance && idx > best_idx) {
            best_idx = idx;
            best_distance = current;
        }
    }
    best_idx
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResultRow> {
    Ok(ResultRow {
        id: row.get(0)?,
        created_at: row.get(1)?,
        mode: row.get(2)?,
        language: row.get(3)?,
        duration_sec: row.get(4)?,
        wpm: row.get(5)?,
        raw_wpm: row.get(6)?,
        accuracy: row.get(7)?,
        errors: row.get::<_, i64>(8)? as usize,
        target_text: row.get(9)?,
        input_text: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{Storage, accumulate_key_errors};
    use crate::typing::{Mode, TypingSession};

    #[test]
    fn saves_and_reads_history() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::open(dir.path().join("results.sqlite")).unwrap();
        let mut session = TypingSession::new(Mode::Words(1), "en".to_string(), "hello".to_string());
        for ch in "hello".chars() {
            session.type_char(ch);
        }
        let metrics = session.metrics();
        store.save_result(&session, metrics).unwrap();
        let rows = store.recent_results(5, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(store.replay_last().unwrap().unwrap().target_text, "hello");
    }

    #[test]
    fn saves_samples_for_history_chart() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::open(dir.path().join("results.sqlite")).unwrap();
        let mut session = TypingSession::new(Mode::Words(1), "en".to_string(), "hello".to_string());
        for ch in "hello".chars() {
            session.type_char(ch);
        }
        let metrics = session.metrics();
        store.save_result(&session, metrics).unwrap();

        let row = store.recent_results(1, None, None).unwrap().remove(0);
        let samples = store.result_samples(row.id).unwrap();
        assert!(!samples.is_empty());
        assert!(samples[0].second >= 1.0);
    }

    #[test]
    fn pages_history_and_lists_languages() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::open(dir.path().join("results.sqlite")).unwrap();
        for (language, target) in [("en", "one"), ("ru", "два"), ("en", "three")] {
            let mut session =
                TypingSession::new(Mode::Words(1), language.to_string(), target.to_string());
            for ch in target.chars() {
                session.type_char(ch);
            }
            let metrics = session.metrics();
            store.save_result(&session, metrics).unwrap();
        }

        assert_eq!(store.result_count(None, None).unwrap(), 3);
        assert_eq!(
            store.recent_results_page(1, 1, None, None).unwrap().len(),
            1
        );
        assert_eq!(store.result_count(Some("en"), None).unwrap(), 2);
        let languages = store.result_languages().unwrap();
        assert_eq!(languages, vec!["en".to_string(), "ru".to_string()]);
    }

    #[test]
    fn builds_keyboard_heatmap_from_saved_mismatches() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::open(dir.path().join("results.sqlite")).unwrap();
        let mut session = TypingSession::new(Mode::Words(1), "en".to_string(), "abc".to_string());
        for ch in "axc".chars() {
            session.type_char(ch);
        }
        let metrics = session.metrics();
        store.save_result(&session, metrics).unwrap();

        let heatmap = store.keyboard_heatmap("en").unwrap();
        assert_eq!(heatmap.tests, 1);
        assert_eq!(heatmap.total_errors, 1);
        assert_eq!(heatmap.keys[0].key, "b");
        assert_eq!(heatmap.keys[0].errors, 1);
    }

    #[test]
    fn keyboard_heatmap_keeps_corrected_mistakes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::open(dir.path().join("results.sqlite")).unwrap();
        let mut session = TypingSession::new(Mode::Words(1), "en".to_string(), "abc".to_string());
        session.type_char('a');
        session.type_char('x');
        session.backspace();
        session.type_char('b');
        session.type_char('c');
        let metrics = session.metrics();
        store.save_result(&session, metrics).unwrap();

        let heatmap = store.keyboard_heatmap("en").unwrap();
        assert_eq!(heatmap.tests, 1);
        assert_eq!(heatmap.total_errors, 1);
        assert_eq!(heatmap.keys[0].key, "b");
    }

    #[test]
    fn heatmap_alignment_does_not_cascade_after_inserted_character() {
        let mut counts = HashMap::new();
        accumulate_key_errors("en", "abc def", "abxc def", &mut counts);

        assert!(counts.is_empty());
    }

    #[test]
    fn heatmap_ignores_spacing_errors() {
        let mut counts = HashMap::new();
        accumulate_key_errors("en", "abc def", "abc  def", &mut counts);

        assert!(counts.is_empty());
    }

    #[test]
    fn heatmap_counts_expected_key_for_substitution() {
        let mut counts = HashMap::new();
        accumulate_key_errors("ru", "тело", "тепо", &mut counts);

        assert_eq!(counts.values().sum::<usize>(), 1);
        assert_eq!(counts.get("л"), Some(&1));
        assert!(!counts.contains_key("п"));
    }

    #[test]
    fn heatmap_counts_expected_key_before_untyped_suffix() {
        let mut counts = HashMap::new();
        accumulate_key_errors("ru", "тело страна город", "тепо", &mut counts);

        assert_eq!(counts.values().sum::<usize>(), 1);
        assert_eq!(counts.get("л"), Some(&1));
    }

    #[test]
    fn heatmap_ignores_untyped_target_suffix() {
        let mut counts = HashMap::new();
        accumulate_key_errors("en", "abc def ghi jkl mno", "abc def", &mut counts);

        assert!(counts.is_empty());
    }

    #[test]
    fn clears_saved_history() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::open(dir.path().join("results.sqlite")).unwrap();
        let mut session = TypingSession::new(Mode::Words(1), "en".to_string(), "hello".to_string());
        for ch in "hello".chars() {
            session.type_char(ch);
        }
        let metrics = session.metrics();
        store.save_result(&session, metrics).unwrap();

        assert_eq!(store.clear_results().unwrap(), 1);
        assert!(store.recent_results(5, None, None).unwrap().is_empty());
        assert_eq!(store.keyboard_heatmap("en").unwrap().tests, 0);
    }
}
