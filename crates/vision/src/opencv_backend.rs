use super::*;
use dpda_draft_state::LockEvidence;
use opencv::{
    core::{self, Mat, Size},
    imgcodecs, imgproc,
    prelude::*,
};
use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    time::Instant,
};

fn image_error(error: impl std::fmt::Display) -> VisionError {
    VisionError::Image(error.to_string())
}

fn normalized(image: &impl core::ToInputArray) -> Result<Mat, VisionError> {
    let mut out = Mat::default();
    imgproc::resize(
        image,
        &mut out,
        Size::new(64, 36),
        0.0,
        0.0,
        imgproc::INTER_AREA,
    )
    .map_err(image_error)?;
    Ok(out)
}

fn load_gray(path: &Path) -> Result<Mat, VisionError> {
    let name = path
        .to_str()
        .ok_or_else(|| VisionError::Image("图像路径不是有效 UTF-8".into()))?;
    let mat = imgcodecs::imread(name, imgcodecs::IMREAD_GRAYSCALE).map_err(image_error)?;
    if mat.empty() {
        return Err(VisionError::Image(format!(
            "图像为空或无法读取：{}",
            path.display()
        )));
    }
    normalized(&mat)
}

fn standard_deviation(image: &Mat) -> Result<f64, VisionError> {
    let (mut mean, mut stddev) = (core::Scalar::default(), core::Scalar::default());
    core::mean_std_dev_def(image, &mut mean, &mut stddev).map_err(image_error)?;
    Ok(stddev[0])
}

fn similarity(image: &Mat, template: &Mat) -> Result<f64, VisionError> {
    // CCOEFF on a constant template is undefined; OpenCV can otherwise report 1.0.
    if standard_deviation(image)? < 2.0 || standard_deviation(template)? < 2.0 {
        return Ok(0.0);
    }
    correlation(image, template)
}

fn correlation(image: &Mat, template: &Mat) -> Result<f64, VisionError> {
    let mut result = Mat::default();
    imgproc::match_template_def(image, template, &mut result, imgproc::TM_CCOEFF_NORMED)
        .map_err(image_error)?;
    let score = f64::from(*result.at_2d::<f32>(0, 0).map_err(image_error)?);
    Ok(if score.is_finite() {
        score.clamp(-1.0, 1.0)
    } else {
        0.0
    })
}

struct HeroTemplate {
    hero_id: u32,
    pixels: Mat,
}

pub struct TemplateBank {
    pub manifest: TemplateManifest,
    templates: Vec<HeroTemplate>,
}

impl TemplateBank {
    pub fn load(manifest_path: impl AsRef<Path>) -> Result<Self, VisionError> {
        let manifest_path = manifest_path.as_ref();
        let manifest = TemplateManifest::load(manifest_path)?;
        let root = manifest_path.parent().unwrap_or(Path::new("."));
        let mut templates = vec![];
        for metadata in &manifest.templates {
            let path = safe_relative_path(root, &metadata.image_path)?;
            let pixels = load_gray(&path)?;
            if standard_deviation(&pixels)? < 2.0 {
                return Err(VisionError::Image(format!(
                    "英雄 {} 的模板近乎纯色",
                    metadata.hero_id
                )));
            }
            templates.push(HeroTemplate {
                hero_id: metadata.hero_id,
                pixels,
            });
        }
        Ok(Self {
            manifest,
            templates,
        })
    }

    fn rank(&self, crop: &Mat) -> Result<Vec<CandidateMatch>, VisionError> {
        if standard_deviation(crop)? < 2.0 {
            return Ok(vec![]);
        }
        let mut best_per_hero: BTreeMap<u32, f64> = BTreeMap::new();
        for template in &self.templates {
            // Template variance was validated at load time; query variance once per slot.
            let score = correlation(crop, &template.pixels)?;
            best_per_hero
                .entry(template.hero_id)
                .and_modify(|s| *s = s.max(score))
                .or_insert(score);
        }
        let mut ranked: Vec<_> = best_per_hero
            .into_iter()
            .map(|(hero_id, similarity)| CandidateMatch {
                hero_id,
                similarity,
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.similarity
                .total_cmp(&a.similarity)
                .then(a.hero_id.cmp(&b.hero_id))
        });
        ranked.truncate(3);
        Ok(ranked)
    }
}

struct LoadedAnchor {
    metadata: VisualAnchor,
    pixels: Mat,
}

struct LoadedSlot {
    calibration: SlotCalibration,
    lock: Option<LoadedAnchor>,
    empty: Option<LoadedAnchor>,
}

pub struct Recognizer {
    profile: CalibrationProfile,
    bank: TemplateBank,
    draft_anchor: LoadedAnchor,
    slots: Vec<LoadedSlot>,
    cached: Option<(u64, FrameAnalysis)>,
}

fn load_anchor(anchor: &VisualAnchor, root: &Path) -> Result<LoadedAnchor, VisionError> {
    let pixels = load_gray(&safe_relative_path(root, &anchor.image_path)?)?;
    if standard_deviation(&pixels)? < 2.0 {
        return Err(VisionError::Image(
            "视觉标记必须包含可辨认纹理，不能使用纯色图".into(),
        ));
    }
    Ok(LoadedAnchor {
        metadata: anchor.clone(),
        pixels,
    })
}

impl Recognizer {
    pub fn from_files(
        profile_path: impl AsRef<Path>,
        manifest_path: impl AsRef<Path>,
    ) -> Result<Self, VisionError> {
        let profile_path = profile_path.as_ref();
        let profile = CalibrationProfile::load(profile_path)?;
        let root = profile_path.parent().unwrap_or(Path::new("."));
        let bank = TemplateBank::load(manifest_path)?;
        let draft_anchor = load_anchor(&profile.draft_anchor, root)?;
        let mut slots = vec![];
        for calibration in &profile.slots {
            slots.push(LoadedSlot {
                calibration: calibration.clone(),
                lock: calibration
                    .locked_marker
                    .as_ref()
                    .map(|a| load_anchor(a, root))
                    .transpose()?,
                empty: calibration
                    .empty_marker
                    .as_ref()
                    .map(|a| load_anchor(a, root))
                    .transpose()?,
            });
        }
        Ok(Self {
            profile,
            bank,
            draft_anchor,
            slots,
            cached: None,
        })
    }

    pub fn profile(&self) -> &CalibrationProfile {
        &self.profile
    }

    pub fn analyze_frame(&mut self, frame: &CaptureFrame) -> Result<FrameAnalysis, VisionError> {
        let started = Instant::now();
        check_frame_profile(frame, &self.profile)?;
        let mut hasher = DefaultHasher::new();
        frame.rgba.hash(&mut hasher);
        (frame.origin_x, frame.origin_y, frame.width, frame.height).hash(&mut hasher);
        let fingerprint = hasher.finish();
        if let Some((previous, result)) = &self.cached
            && *previous == fingerprint
        {
            let mut result = result.clone();
            result.timestamp_ms = frame.timestamp_ms;
            for obs in &mut result.observations {
                obs.timestamp = frame.timestamp_ms;
            }
            result.elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
            result.reused_pixels = true;
            return Ok(result);
        }
        let rgba = Mat::from_slice(&frame.rgba).map_err(image_error)?;
        let rgba = rgba.reshape(4, frame.height as i32).map_err(image_error)?;
        let mut gray = Mat::default();
        imgproc::cvt_color_def(&rgba, &mut gray, imgproc::COLOR_RGBA2GRAY).map_err(image_error)?;
        let draft_similarity = self.anchor_similarity(&gray, frame, &self.draft_anchor)?;
        let draft_screen_detected = draft_similarity >= self.draft_anchor.metadata.min_similarity;
        let mut observations = vec![];
        let mut diagnostics = vec![];
        if draft_screen_detected {
            for slot in &self.slots {
                let crop = self.crop(&gray, frame, slot.calibration.portrait)?;
                let candidates = self.bank.rank(&crop)?;
                let locked = slot
                    .lock
                    .as_ref()
                    .map(|a| {
                        self.anchor_similarity(&gray, frame, a)
                            .map(|s| s >= a.metadata.min_similarity)
                    })
                    .transpose()?
                    .unwrap_or(false);
                let empty = slot
                    .empty
                    .as_ref()
                    .map(|a| {
                        self.anchor_similarity(&gray, frame, a)
                            .map(|s| s >= a.metadata.min_similarity)
                    })
                    .transpose()?
                    .unwrap_or(false);
                let first = candidates.first();
                let confidence = first.map_or(0.0, |c| c.similarity.max(0.0));
                // A single hero bank cannot establish a margin against a different hero.
                let margin = match (first, candidates.get(1)) {
                    (Some(a), Some(b)) => a.similarity - b.similarity,
                    _ => 0.0,
                };
                let empty = empty && confidence < 0.80;
                observations.push(Observation {
                    slot_id: slot.calibration.slot_id,
                    hero_id: if empty || confidence < 0.5 {
                        None
                    } else {
                        first.map(|c| c.hero_id)
                    },
                    confidence,
                    margin,
                    empty,
                    valid: true,
                    lock_evidence: if locked {
                        LockEvidence::VisualMarker
                    } else {
                        LockEvidence::None
                    },
                    timestamp: frame.timestamp_ms,
                });
                diagnostics.push(SlotDiagnostic {
                    slot_id: slot.calibration.slot_id,
                    candidates,
                    locked_marker_matched: locked,
                    empty_marker_matched: empty,
                });
            }
        }
        let analysis = FrameAnalysis {
            timestamp_ms: frame.timestamp_ms,
            draft_screen_detected,
            draft_similarity,
            observations,
            slots: diagnostics,
            elapsed_ms: started.elapsed().as_secs_f64() * 1_000.0,
            reused_pixels: false,
        };
        self.cached = Some((fingerprint, analysis.clone()));
        Ok(analysis)
    }

    fn crop(&self, gray: &Mat, frame: &CaptureFrame, region: Rect) -> Result<Mat, VisionError> {
        let x = region
            .x
            .checked_sub(frame.origin_x)
            .ok_or(VisionError::CalibrationMismatch)?;
        let y = region
            .y
            .checked_sub(frame.origin_y)
            .ok_or(VisionError::CalibrationMismatch)?;
        if !(Rect {
            x,
            y,
            width: region.width,
            height: region.height,
        })
        .fits(frame.width, frame.height)
        {
            return Err(VisionError::CalibrationMismatch);
        }
        let roi = Mat::roi(
            gray,
            core::Rect::new(
                x as i32,
                y as i32,
                region.width as i32,
                region.height as i32,
            ),
        )
        .map_err(image_error)?;
        normalized(&roi)
    }

    fn anchor_similarity(
        &self,
        gray: &Mat,
        frame: &CaptureFrame,
        anchor: &LoadedAnchor,
    ) -> Result<f64, VisionError> {
        similarity(
            &self.crop(gray, frame, anchor.metadata.region)?,
            &anchor.pixels,
        )
    }
}

pub fn read_image_frame(
    path: impl AsRef<Path>,
    dpi: u32,
    timestamp_ms: u64,
) -> Result<CaptureFrame, VisionError> {
    let path = path.as_ref();
    let image = imgcodecs::imread(
        path.to_str()
            .ok_or_else(|| VisionError::Image("图像路径不是有效 UTF-8".into()))?,
        imgcodecs::IMREAD_COLOR,
    )
    .map_err(image_error)?;
    if image.empty() {
        return Err(VisionError::Image("截图为空或格式不支持".into()));
    }
    let mut rgba = Mat::default();
    imgproc::cvt_color_def(&image, &mut rgba, imgproc::COLOR_BGR2RGBA).map_err(image_error)?;
    Ok(CaptureFrame {
        width: rgba.cols() as u32,
        height: rgba.rows() as u32,
        surface_width: rgba.cols() as u32,
        surface_height: rgba.rows() as u32,
        origin_x: 0,
        origin_y: 0,
        dpi,
        timestamp_ms,
        capture_ms: None,
        rgba: rgba.data_bytes().map_err(image_error)?.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_profile_replay_keeps_unlocked_heroes_as_candidates() {
        // Explicitly synthetic: exercises calibration/IO/cache, never game accuracy.
        let folder = tempfile::tempdir().unwrap();
        let pixels: Vec<u8> = (0..64 * 36).map(|i| ((i * 31) % 251) as u8).collect();
        let other: Vec<u8> = pixels.iter().map(|p| 255 - p).collect();
        for (name, values) in [
            ("hero.png", &pixels),
            ("other.png", &other),
            ("gate.png", &pixels),
        ] {
            let mat = Mat::from_slice(values).unwrap();
            let mat = mat.reshape(1, 36).unwrap();
            imgcodecs::imwrite(
                folder.path().join(name).to_str().unwrap(),
                &mat,
                &core::Vector::new(),
            )
            .unwrap();
        }
        let profile = CalibrationProfile {
            schema_version: 1,
            profile_name: "synthetic unit test".into(),
            ui_version: "synthetic".into(),
            surface_width: 640,
            surface_height: 144,
            dpi: 96,
            draft_anchor: VisualAnchor {
                region: Rect {
                    x: 0,
                    y: 72,
                    width: 64,
                    height: 36,
                },
                image_path: "gate.png".into(),
                min_similarity: 0.9,
            },
            slots: (0..10)
                .map(|i| SlotCalibration {
                    slot_id: i,
                    portrait: Rect {
                        x: u32::from(i) * 64,
                        y: 0,
                        width: 64,
                        height: 36,
                    },
                    locked_marker: None,
                    empty_marker: None,
                })
                .collect(),
        };
        profile.save(folder.path().join("profile.json")).unwrap();
        let manifest = TemplateManifest {
            schema_version: 1,
            source: "synthetic unit test, not game imagery".into(),
            retrieved_at: "not applicable".into(),
            templates: [(13, "hero.png"), (25, "other.png")]
                .into_iter()
                .map(|(hero_id, path)| TemplateMetadata {
                    hero_id,
                    hero_name: "synthetic".into(),
                    localized_name: "合成测试".into(),
                    image_path: path.into(),
                    template_version: "synthetic".into(),
                })
                .collect(),
        };
        std::fs::write(
            folder.path().join("templates.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let mut frame = CaptureFrame {
            width: 640,
            height: 144,
            surface_width: 640,
            surface_height: 144,
            origin_x: 0,
            origin_y: 0,
            dpi: 96,
            timestamp_ms: 1,
            capture_ms: None,
            rgba: vec![0; 640 * 144 * 4],
        };
        for y in 0..36 {
            for x in 0..640 {
                let v = pixels[y * 64 + x % 64];
                frame.rgba[(y * 640 + x) * 4..(y * 640 + x) * 4 + 4]
                    .copy_from_slice(&[v, v, v, 255]);
            }
            for x in 0..64 {
                let v = pixels[y * 64 + x];
                frame.rgba[((y + 72) * 640 + x) * 4..((y + 72) * 640 + x) * 4 + 4]
                    .copy_from_slice(&[v, v, v, 255]);
            }
        }
        let mut recognizer = Recognizer::from_files(
            folder.path().join("profile.json"),
            folder.path().join("templates.json"),
        )
        .unwrap();
        let first = recognizer.analyze_frame(&frame).unwrap();
        assert!(first.draft_screen_detected);
        assert_eq!(first.observations.len(), 10);
        assert_eq!(first.observations[0].hero_id, Some(13));
        assert_eq!(first.observations[0].lock_evidence, LockEvidence::None);
        assert!(!first.reused_pixels);
        frame.timestamp_ms = 2;
        let second = recognizer.analyze_frame(&frame).unwrap();
        assert!(second.reused_pixels);
        assert_eq!(second.observations[0].timestamp, 2);
        frame.dpi = 144;
        assert!(matches!(
            recognizer.analyze_frame(&frame),
            Err(VisionError::CalibrationMismatch)
        ));
    }

    #[test]
    fn synthetic_correlations_are_finite_and_constant_images_are_rejected() {
        // Synthetic algorithm unit test; no claim about DOTA screenshot accuracy.
        let pixels: Vec<u8> = (0..64 * 36).map(|i| ((i * 31) % 251) as u8).collect();
        let mat = Mat::from_slice(&pixels).unwrap();
        let mat = mat.reshape(1, 36).unwrap().try_clone().unwrap();
        assert!(similarity(&mat, &mat).unwrap() > 0.999);
        let constant =
            Mat::new_rows_cols_with_default(36, 64, core::CV_8UC1, core::Scalar::all(42.0))
                .unwrap();
        assert_eq!(similarity(&constant, &constant).unwrap(), 0.0);
    }
}
