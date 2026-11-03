//! Profile-calibrated vision. No built-in coordinates or unverified lock heuristic.
use dpda_capture::{CaptureFrame, Rect};
use dpda_draft_state::Observation;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum VisionError {
    #[error("识别配置无效：{0}")]
    InvalidConfig(String),
    #[error("捕获尺寸或 DPI 与校准不一致，需要重新校准")]
    CalibrationMismatch,
    #[error("缺少 OpenCV backend，请启用 opencv-backend feature")]
    BackendUnavailable,
    #[error("图像读取或识别失败：{0}")]
    Image(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VisualAnchor {
    pub region: Rect,
    /// Relative to the calibration JSON directory; image captured by the user.
    pub image_path: String,
    pub min_similarity: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlotCalibration {
    pub slot_id: u8,
    pub portrait: Rect,
    /// Must identify a locked pick, not the portrait or a hover highlight.
    pub locked_marker: Option<VisualAnchor>,
    pub empty_marker: Option<VisualAnchor>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalibrationProfile {
    pub schema_version: u32,
    pub profile_name: String,
    pub ui_version: String,
    pub surface_width: u32,
    pub surface_height: u32,
    pub dpi: u32,
    pub draft_anchor: VisualAnchor,
    pub slots: Vec<SlotCalibration>,
}

impl CalibrationProfile {
    pub fn validate(&self) -> Result<(), VisionError> {
        if self.schema_version != 1
            || self.surface_width == 0
            || self.surface_height == 0
            || self.dpi == 0
            || self.ui_version.trim().is_empty()
            || self.slots.len() != 10
        {
            return Err(VisionError::InvalidConfig(
                "需要版本 1、尺寸/DPI、UI 版本与 10 个槽位".into(),
            ));
        }
        let mut ids = BTreeSet::new();
        self.validate_anchor(&self.draft_anchor)?;
        for slot in &self.slots {
            if slot.slot_id >= 10
                || !ids.insert(slot.slot_id)
                || !slot.portrait.fits(self.surface_width, self.surface_height)
            {
                return Err(VisionError::InvalidConfig(
                    "槽位编号重复、越界或区域无效".into(),
                ));
            }
            if let Some(lock) = &slot.locked_marker {
                self.validate_anchor(lock)?;
                if lock.region == slot.portrait {
                    return Err(VisionError::InvalidConfig(
                        "锁定标记必须独立于头像区域".into(),
                    ));
                }
            }
            if let Some(empty) = &slot.empty_marker {
                self.validate_anchor(empty)?;
            }
        }
        Ok(())
    }

    fn validate_anchor(&self, anchor: &VisualAnchor) -> Result<(), VisionError> {
        if !anchor.region.fits(self.surface_width, self.surface_height)
            || !anchor.min_similarity.is_finite()
            || !(0.8..=1.0).contains(&anchor.min_similarity)
        {
            return Err(VisionError::InvalidConfig(
                "标记区域越界或相似度阈值不在 0.8..1 范围".into(),
            ));
        }
        safe_relative_path(Path::new("."), &anchor.image_path)?;
        Ok(())
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, VisionError> {
        let profile: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), VisionError> {
        self.validate()?;
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    pub fn capture_region(&self) -> Rect {
        let mut regions = vec![self.draft_anchor.region];
        for slot in &self.slots {
            regions.push(slot.portrait);
            regions.extend(slot.locked_marker.iter().map(|m| m.region));
            regions.extend(slot.empty_marker.iter().map(|m| m.region));
        }
        let x = regions.iter().map(|r| r.x).min().unwrap_or(0);
        let y = regions.iter().map(|r| r.y).min().unwrap_or(0);
        let right = regions
            .iter()
            .map(|r| r.x.saturating_add(r.width))
            .max()
            .unwrap_or(x);
        let bottom = regions
            .iter()
            .map(|r| r.y.saturating_add(r.height))
            .max()
            .unwrap_or(y);
        Rect {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateMetadata {
    pub hero_id: u32,
    pub hero_name: String,
    pub localized_name: String,
    pub image_path: String,
    pub template_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateManifest {
    pub schema_version: u32,
    pub source: String,
    pub retrieved_at: String,
    pub templates: Vec<TemplateMetadata>,
}

impl TemplateManifest {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, VisionError> {
        let data: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        if data.schema_version != 1 || data.templates.is_empty() || data.source.is_empty() {
            return Err(VisionError::InvalidConfig(
                "模板清单需要版本、来源和至少一幅真实模板".into(),
            ));
        }
        for item in &data.templates {
            if item.hero_id == 0 || item.template_version.is_empty() {
                return Err(VisionError::InvalidConfig(
                    "模板缺少稳定英雄 ID 或版本".into(),
                ));
            }
            safe_relative_path(Path::new("."), &item.image_path)?;
        }
        Ok(data)
    }
}

pub(crate) fn safe_relative_path(root: &Path, name: &str) -> Result<PathBuf, VisionError> {
    let path = Path::new(name);
    if name.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(VisionError::InvalidConfig(
            "模板路径必须是目录内的相对路径".into(),
        ));
    }
    Ok(root.join(path))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandidateMatch {
    pub hero_id: u32,
    pub similarity: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlotDiagnostic {
    pub slot_id: u8,
    pub candidates: Vec<CandidateMatch>,
    pub locked_marker_matched: bool,
    pub empty_marker_matched: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameAnalysis {
    pub timestamp_ms: u64,
    pub draft_screen_detected: bool,
    pub draft_similarity: f64,
    pub observations: Vec<Observation>,
    pub slots: Vec<SlotDiagnostic>,
    pub elapsed_ms: f64,
    pub reused_pixels: bool,
}

pub fn check_frame_profile(
    frame: &CaptureFrame,
    profile: &CalibrationProfile,
) -> Result<(), VisionError> {
    frame
        .validate()
        .map_err(|e| VisionError::Image(e.to_string()))?;
    if frame.surface_width != profile.surface_width
        || frame.surface_height != profile.surface_height
        || frame.dpi != profile.dpi
    {
        return Err(VisionError::CalibrationMismatch);
    }
    Ok(())
}

#[cfg(feature = "opencv-backend")]
mod opencv_backend;
#[cfg(feature = "opencv-backend")]
pub use opencv_backend::{Recognizer, TemplateBank, read_image_frame};

#[cfg(feature = "opencv-backend")]
pub mod evaluation;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_cannot_escape_template_directory() {
        assert!(safe_relative_path(Path::new("bank"), "../secret").is_err());
        assert!(safe_relative_path(Path::new("bank"), "/secret").is_err());
        assert!(safe_relative_path(Path::new("bank"), "heroes/13.png").is_ok());
    }

    #[test]
    fn resolution_changes_never_silently_scale_coordinates() {
        let anchor = VisualAnchor {
            region: Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            image_path: "anchor.png".into(),
            min_similarity: 0.9,
        };
        let profile = CalibrationProfile {
            schema_version: 1,
            profile_name: "synthetic unit test".into(),
            ui_version: "test-only".into(),
            surface_width: 4,
            surface_height: 4,
            dpi: 96,
            draft_anchor: anchor,
            slots: vec![],
        };
        let frame = CaptureFrame {
            width: 8,
            height: 4,
            surface_width: 8,
            surface_height: 4,
            origin_x: 0,
            origin_y: 0,
            dpi: 96,
            timestamp_ms: 1,
            capture_ms: None,
            rgba: vec![0; 8 * 4 * 4],
        };
        assert!(matches!(
            check_frame_profile(&frame, &profile),
            Err(VisionError::CalibrationMismatch)
        ));
    }
}
