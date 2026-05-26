use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub wpm: f64,
    pub raw_wpm: f64,
    pub cpm: f64,
    pub raw_cpm: f64,
    pub accuracy: f64,
    pub consistency: f64,
    pub correct_characters: usize,
    pub incorrect_characters: usize,
    pub extra_characters: usize,
    pub missed_characters: usize,
    pub typed_characters: usize,
    pub errors: usize,
}

pub fn calculate(
    target: &[char],
    input: &[char],
    elapsed_secs: f64,
    include_missed: bool,
) -> Metrics {
    let mut correct = 0;
    let mut incorrect = 0;
    let mut extra = 0;

    for (idx, typed) in input.iter().enumerate() {
        match target.get(idx) {
            Some(expected) if expected == typed => correct += 1,
            Some(_) => incorrect += 1,
            None => extra += 1,
        }
    }

    let missed = if include_missed && target.len() > input.len() {
        target.len() - input.len()
    } else {
        0
    };

    let typed = input.len();
    let minutes = (elapsed_secs / 60.0).max(1.0 / 60.0);
    let cpm = correct as f64 / minutes;
    let raw_cpm = typed as f64 / minutes;
    let wpm = correct as f64 / 5.0 / minutes;
    let raw_wpm = typed as f64 / 5.0 / minutes;
    let accuracy_total = typed + missed;
    let accuracy = if accuracy_total == 0 {
        100.0
    } else {
        round_one_decimal(correct as f64 / accuracy_total as f64 * 100.0)
    };

    Metrics {
        wpm,
        raw_wpm,
        cpm,
        raw_cpm,
        accuracy,
        consistency: 100.0,
        correct_characters: correct,
        incorrect_characters: incorrect,
        extra_characters: extra,
        missed_characters: missed,
        typed_characters: typed,
        errors: incorrect + extra + missed,
    }
}

fn round_one_decimal(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

pub fn consistency(samples: &[f64]) -> f64 {
    if samples.len() < 2 {
        return 100.0;
    }

    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    if mean <= f64::EPSILON {
        return 100.0;
    }

    let variance = samples
        .iter()
        .map(|sample| (sample - mean).powi(2))
        .sum::<f64>()
        / samples.len() as f64;
    let coefficient = variance.sqrt() / mean;
    (100.0 - coefficient * 100.0).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::{calculate, consistency};

    #[test]
    fn calculates_speed_and_accuracy() {
        let target: Vec<char> = "hello world".chars().collect();
        let input: Vec<char> = "hello wurld".chars().collect();
        let metrics = calculate(&target, &input, 60.0, false);
        assert_eq!(metrics.correct_characters, 10);
        assert_eq!(metrics.incorrect_characters, 1);
        assert_eq!(metrics.typed_characters, 11);
        assert_eq!(metrics.wpm, 2.0);
        assert_eq!(metrics.raw_wpm, 2.2);
        assert_eq!(metrics.cpm, 10.0);
        assert_eq!(metrics.raw_cpm, 11.0);
        assert_eq!(metrics.accuracy, 90.9);
    }

    #[test]
    fn counts_missed_characters_when_requested() {
        let target: Vec<char> = "hello".chars().collect();
        let input: Vec<char> = "he".chars().collect();
        let metrics = calculate(&target, &input, 30.0, true);
        assert_eq!(metrics.missed_characters, 3);
        assert_eq!(metrics.errors, 3);
        assert_eq!(metrics.accuracy, 40.0);
    }

    #[test]
    fn consistency_penalizes_speed_spikes() {
        assert!(consistency(&[50.0, 52.0, 51.0]) > consistency(&[10.0, 90.0, 10.0]));
    }
}
