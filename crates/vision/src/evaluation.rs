//! Replay labelled screenshots in sequence. Missing real input produces no accuracy claim.
use crate::{Recognizer, VisionError, read_image_frame, safe_relative_path};
use dpda_draft_state::{DraftTracker, RecognitionState};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

#[derive(Debug, Deserialize)]
pub struct Dataset {
    pub schema_version: u32,
    /// Must explicitly be "real" or "synthetic". Report carries it through.
    pub provenance: String,
    pub source_description: String,
    pub samples: Vec<LabelledFrame>,
}

#[derive(Debug, Deserialize)]
pub struct LabelledFrame {
    pub session_id: String,
    pub image_path: String,
    pub timestamp_ms: u64,
    pub dpi: u32,
    pub draft_screen: bool,
    pub slots: Vec<SlotLabel>,
}

#[derive(Debug, Deserialize)]
pub struct SlotLabel {
    pub slot_id: u8,
    pub hero_id: Option<u32>,
    pub locked: bool,
}

#[derive(Debug, Serialize)]
pub struct EvaluationReport {
    pub provenance: String,
    pub source_description: String,
    pub frames: usize,
    pub labelled_slots: usize,
    pub confirmed_predictions: usize,
    pub expected_locked_slots: usize,
    pub accuracy: Option<f64>,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
    pub false_positive_rate: Option<f64>,
    pub mean_recognition_latency_ms: f64,
    pub p95_recognition_latency_ms: f64,
    pub draft_screen_accuracy: f64,
    pub note: String,
}

fn rate(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

pub fn evaluate(
    profile_path: impl AsRef<Path>,
    templates_path: impl AsRef<Path>,
    dataset_path: impl AsRef<Path>,
) -> Result<EvaluationReport, VisionError> {
    let dataset_path = dataset_path.as_ref();
    let dataset: Dataset = serde_json::from_slice(&std::fs::read(dataset_path)?)?;
    if dataset.schema_version != 1
        || dataset.samples.is_empty()
        || !["real", "synthetic"].contains(&dataset.provenance.as_str())
        || dataset.source_description.trim().is_empty()
    {
        return Err(VisionError::InvalidConfig(
            "测试集必须声明 real/synthetic、来源，并包含实际截图样本".into(),
        ));
    }
    let root = dataset_path.parent().unwrap_or(Path::new("."));
    let mut recognizer = Recognizer::from_files(profile_path, templates_path)?;
    let mut tracker = DraftTracker::default();
    let mut current_session = None;
    let mut seen_sessions = BTreeSet::new();
    let (
        mut total,
        mut correct,
        mut predicted,
        mut positive,
        mut true_positive,
        mut negatives,
        mut false_positive_negative,
    ) = (0, 0, 0, 0, 0, 0, 0);
    let mut draft_correct = 0;
    let mut latencies = vec![];
    for sample in &dataset.samples {
        if current_session != Some(&sample.session_id) {
            if !seen_sessions.insert(&sample.session_id) {
                return Err(VisionError::InvalidConfig(
                    "同一 session 的帧必须连续排列".into(),
                ));
            }
            tracker.reset();
            current_session = Some(&sample.session_id);
        }
        if sample.slots.len() != 10
            || sample
                .slots
                .iter()
                .map(|s| s.slot_id)
                .collect::<BTreeSet<_>>()
                .len()
                != 10
            || sample.slots.iter().any(|s| {
                s.slot_id >= 10 || s.hero_id == Some(0) || (s.locked && s.hero_id.is_none())
            })
        {
            return Err(VisionError::InvalidConfig(
                "每帧需为十个唯一槽位提供有效人工标签".into(),
            ));
        }
        let frame = read_image_frame(
            safe_relative_path(root, &sample.image_path)?,
            sample.dpi,
            sample.timestamp_ms,
        )?;
        let analysis = recognizer.analyze_frame(&frame)?;
        tracker
            .apply_frame(
                analysis.timestamp_ms,
                analysis.draft_screen_detected,
                &analysis.observations,
            )
            .map_err(|error| VisionError::InvalidConfig(error.to_string()))?;
        let snapshot = tracker.snapshot();
        latencies.push(analysis.elapsed_ms);
        draft_correct += usize::from(analysis.draft_screen_detected == sample.draft_screen);
        for label in &sample.slots {
            let state = snapshot
                .radiant
                .iter()
                .chain(&snapshot.dire)
                .find(|s| s.slot_id == label.slot_id)
                .expect("tracker always has ten slots");
            let actual = if state.recognition_state == RecognitionState::Confirmed {
                state.hero_id
            } else {
                None
            };
            let expected = if label.locked && sample.draft_screen {
                label.hero_id
            } else {
                None
            };
            total += 1;
            correct += usize::from(actual == expected);
            predicted += usize::from(actual.is_some());
            positive += usize::from(expected.is_some());
            true_positive += usize::from(actual.is_some() && actual == expected);
            negatives += usize::from(expected.is_none());
            false_positive_negative += usize::from(expected.is_none() && actual.is_some());
        }
    }
    latencies.sort_by(f64::total_cmp);
    let p95_index = ((latencies.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    Ok(EvaluationReport {
        provenance: dataset.provenance,
        source_description: dataset.source_description,
        frames: dataset.samples.len(),
        labelled_slots: total,
        confirmed_predictions: predicted,
        expected_locked_slots: positive,
        accuracy: rate(correct, total),
        precision: rate(true_positive, predicted),
        recall: rate(true_positive, positive),
        false_positive_rate: rate(false_positive_negative, negatives),
        mean_recognition_latency_ms: latencies.iter().sum::<f64>() / latencies.len() as f64,
        p95_recognition_latency_ms: latencies[p95_index],
        draft_screen_accuracy: draft_correct as f64 / dataset.samples.len() as f64,
        note: "按完整时间序列统计，包含连续确认的预热漏检。未知结果视为未确认；latency 不含图片解码或屏幕捕获。不代表游戏 FPS 开销。".into(),
    })
}
