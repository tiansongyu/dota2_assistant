use dpda_recommender::{DraftContext, Weights};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub account_id: u32,
    pub steam64: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Draft {
    pub radiant: Vec<Option<u32>>,
    pub dire: Vec<Option<u32>>,
    pub own_team: String,
    pub banned: Vec<u32>,
}
impl Default for Draft {
    fn default() -> Self {
        Self {
            radiant: vec![None; 5],
            dire: vec![None; 5],
            own_team: "radiant".into(),
            banned: Vec::new(),
        }
    }
}
impl Draft {
    pub fn validate(&self, heroes: &BTreeSet<u32>) -> Result<(), String> {
        if self.radiant.len() != 5
            || self.dire.len() != 5
            || !["radiant", "dire"].contains(&self.own_team.as_str())
        {
            return Err("阵容必须有双方各 5 个槽位和有效己方阵营".into());
        }
        let mut seen = BTreeSet::new();
        for h in self
            .radiant
            .iter()
            .chain(&self.dire)
            .flatten()
            .chain(&self.banned)
        {
            if !heroes.contains(h) || !seen.insert(*h) {
                return Err("英雄 ID 不存在、重复选择或同时被禁用".into());
            }
        }
        Ok(())
    }
    pub fn context(&self, role: u8) -> DraftContext {
        let (allies, enemies) = if self.own_team == "radiant" {
            (&self.radiant, &self.dire)
        } else {
            (&self.dire, &self.radiant)
        };
        DraftContext {
            allies: allies.iter().flatten().copied().collect(),
            enemies: enemies.iter().flatten().copied().collect(),
            banned: self.banned.clone(),
            role,
            uncertain_slots: self
                .radiant
                .iter()
                .chain(&self.dire)
                .filter(|h| h.is_none())
                .count()
                .saturating_sub(1),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: String,
    pub selected_role: u8,
    pub half_life_days: f64,
    pub top_k: usize,
    pub weights: Weights,
    pub overlay_opacity: f64,
    pub overlay_compact: bool,
    pub overlay_click_through: bool,
    pub capture_hz: f64,
    pub calibration_json: String,
}
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            selected_role: 0,
            half_life_days: 180.0,
            top_k: 5,
            weights: Weights::default(),
            overlay_opacity: 0.94,
            overlay_compact: false,
            overlay_click_through: false,
            capture_hz: 1.0,
            calibration_json: String::new(),
        }
    }
}
impl AppSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !["dark", "light"].contains(&self.theme.as_str())
            || self.selected_role > 5
            || !(7.0..=1825.0).contains(&self.half_life_days)
            || !(3..=5).contains(&self.top_k)
            || !self.weights.validate()
            || !(0.3..=1.0).contains(&self.overlay_opacity)
            || !(0.1..=4.0).contains(&self.capture_hz)
        {
            return Err("设置超出有效范围：位置 0（自动）或 1—5、Top 3—5、半衰期 7—1825 天、采样 0.1—4 Hz、透明度 0.3—1".into());
        }
        if !self.calibration_json.trim().is_empty() {
            let p: CalibrationFiles = serde_json::from_str(&self.calibration_json)
                .map_err(|e| format!("校准文件设置 JSON 无效：{e}"))?;
            if p.profile_path.is_empty() || p.templates_path.is_empty() {
                return Err("需指定校准 profile_path 和模板 templates_path".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalibrationFiles {
    pub profile_path: String,
    pub templates_path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaptureStatus {
    pub supported: bool,
    pub status: String,
    pub message: String,
    pub window_title: Option<String>,
    pub capture_ms: Option<f64>,
    pub recognition_ms: Option<f64>,
    pub recommendation_ms: Option<f64>,
}
impl Default for CaptureStatus {
    fn default() -> Self {
        Self {
            supported: cfg!(windows),
            status: if cfg!(windows) { "idle" } else { "unsupported" }.into(),
            message: if cfg!(windows) {
                "等待 DOTA2 窗口和已校准模板；尚未通过实机验收"
            } else {
                "当前平台不能运行 Windows Graphics Capture；仍可同步战绩并手动输入阵容"
            }
            .into(),
            window_title: None,
            capture_ms: None,
            recognition_ms: None,
            recommendation_ms: None,
        }
    }
}
