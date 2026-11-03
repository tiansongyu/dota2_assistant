//! Small, deterministic estimators. None of these are a calibrated game predictor.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Interval {
    pub low: f64,
    pub high: f64,
}

pub fn smoothed_rate(wins: f64, total: f64, prior_mean: f64, prior_strength: f64) -> f64 {
    let prior_mean = if prior_mean.is_finite() {
        prior_mean.clamp(0.0, 1.0)
    } else {
        0.5
    };
    if !wins.is_finite() || !total.is_finite() || !prior_strength.is_finite() {
        return prior_mean;
    }
    let total = total.max(0.0);
    let strength = prior_strength.max(0.0);
    if total + strength == 0.0 {
        return prior_mean.clamp(0.0, 1.0);
    }
    // Scale before addition to avoid overflow for very large finite weighted counts.
    let scale = total.max(strength);
    ((wins.clamp(0.0, total) / scale + prior_mean * (strength / scale))
        / (total / scale + strength / scale))
        .clamp(0.0, 1.0)
}

/// Wilson 95% interval for unweighted Bernoulli observations.
pub fn wilson(wins: usize, total: usize) -> Interval {
    if total == 0 {
        return Interval {
            low: 0.0,
            high: 1.0,
        };
    }
    let n = total as f64;
    let p = wins.min(total) as f64 / n;
    let z = 1.959963984540054;
    let denom = 1.0 + z * z / n;
    let center = (p + z * z / (2.0 * n)) / denom;
    let margin = z * ((p * (1.0 - p) + z * z / (4.0 * n)) / n).sqrt() / denom;
    Interval {
        low: (center - margin).max(0.0),
        high: (center + margin).min(1.0),
    }
}

pub fn time_weight(age_days: f64, half_life_days: f64) -> f64 {
    if !age_days.is_finite() || !half_life_days.is_finite() || half_life_days <= 0.0 {
        return 0.0;
    }
    2f64.powf(-age_days.max(0.0) / half_life_days)
}

/// Kish effective sample size; weighted win mass must not be interpreted as n.
pub fn effective_samples(weights: &[f64]) -> f64 {
    let scale = weights
        .iter()
        .copied()
        .filter(|w| w.is_finite() && *w > 0.0)
        .fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return 0.0;
    }
    let (sum, squares) =
        weights
            .iter()
            .filter(|w| w.is_finite() && **w > 0.0)
            .fold((0.0, 0.0), |(s, sq), w| {
                let normalized = w / scale;
                (s + normalized, sq + normalized * normalized)
            });
    if squares > 0.0 {
        sum * sum / squares
    } else {
        0.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationBin {
    pub lower: f64,
    pub upper: f64,
    pub count: usize,
    pub mean_prediction: Option<f64>,
    pub observed_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbabilityMetrics {
    pub samples: usize,
    pub brier_score: f64,
    pub log_loss: f64,
    pub reliability: Vec<CalibrationBin>,
}

/// Evaluation utility only; a caller must supply out-of-time predictions.
pub fn evaluate_probabilities(predictions: &[(f64, bool)]) -> Option<ProbabilityMetrics> {
    if predictions.is_empty()
        || predictions
            .iter()
            .any(|(p, _)| !p.is_finite() || !(0.0..=1.0).contains(p))
    {
        return None;
    }
    let count = predictions.len() as f64;
    let brier_score = predictions
        .iter()
        .map(|(p, y)| (p - u8::from(*y) as f64).powi(2))
        .sum::<f64>()
        / count;
    let log_loss = -predictions
        .iter()
        .map(|(p, y)| {
            let p = p.clamp(1e-12, 1.0 - 1e-12);
            if *y {
                p.ln()
            } else {
                (1.0 - p).ln()
            }
        })
        .sum::<f64>()
        / count;
    let reliability = (0..10)
        .map(|index| {
            let values: Vec<_> = predictions
                .iter()
                .filter(|(p, _)| ((*p * 10.0).floor() as usize).min(9) == index)
                .collect();
            CalibrationBin {
                lower: index as f64 / 10.0,
                upper: (index + 1) as f64 / 10.0,
                count: values.len(),
                mean_prediction: (!values.is_empty())
                    .then(|| values.iter().map(|(p, _)| p).sum::<f64>() / values.len() as f64),
                observed_rate: (!values.is_empty()).then(|| {
                    values.iter().filter(|(_, y)| *y).count() as f64 / values.len() as f64
                }),
            }
        })
        .collect();
    Some(ProbabilityMetrics {
        samples: predictions.len(),
        brier_score,
        log_loss,
        reliability,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_win_is_not_certainty() {
        assert!((smoothed_rate(1.0, 1.0, 0.5, 10.0) - 6.0 / 11.0).abs() < 1e-10);
        let i = wilson(1, 1);
        assert!((i.low - 0.20654931437723745).abs() < 1e-10);
        assert_eq!(
            wilson(0, 0),
            Interval {
                low: 0.0,
                high: 1.0
            }
        );
    }
    #[test]
    fn decays_and_effective_counts() {
        assert_eq!(time_weight(180.0, 180.0), 0.5);
        assert_eq!(time_weight(360.0, 180.0), 0.25);
        assert_eq!(effective_samples(&[1.0, 1.0, 1.0]), 3.0);
        assert!((effective_samples(&[1.0, 0.01]) - 1.019998).abs() < 1e-5);
    }
    #[test]
    fn metrics_known_predictions() {
        let result = evaluate_probabilities(&[(0.5, true), (0.5, false)]).unwrap();
        assert_eq!(result.brier_score, 0.25);
        assert!((result.log_loss - 2f64.ln()).abs() < 1e-10);
        assert_eq!(result.reliability[5].count, 2);
        assert!(evaluate_probabilities(&[(f64::NAN, true)]).is_none());
    }
    #[test]
    fn nonfinite_inputs_and_large_weights_do_not_escape_as_nan() {
        assert_eq!(smoothed_rate(f64::NAN, 10.0, 0.6, 10.0), 0.6);
        assert_eq!(smoothed_rate(0.0, 0.0, f64::NAN, 0.0), 0.5);
        assert_eq!(smoothed_rate(f64::MAX, f64::MAX, 0.5, f64::MAX), 0.75);
        assert_eq!(effective_samples(&[f64::MAX, f64::MAX]), 2.0);
    }
}
