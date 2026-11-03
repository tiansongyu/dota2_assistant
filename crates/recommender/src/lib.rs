//! Explainable ranking, never an advertised probability of winning this game.
use dpda_domain::{HeroId, MatchRecord};
use dpda_hero_pool::{aggregate, HeroStats, PoolConfig};
use dpda_statistics::{effective_samples, smoothed_rate, time_weight};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Verified against dotaconstants 10.8.0. This is a standard-mode filter, not proof of ranked play.
pub fn standard_mode(game_mode: Option<u32>) -> bool {
    matches!(game_mode, Some(1 | 2 | 3 | 4 | 22))
}

/// Retrospective availability rule: the match must have ended before the cutoff.
/// Unknown durations are usable only when a stored fetch timestamp proves prior availability.
pub fn available_before(record: &MatchRecord, cutoff: i64) -> bool {
    if record.start_time < 0 || record.start_time >= cutoff {
        return false;
    }
    match record.duration {
        Some(duration) => record
            .start_time
            .checked_add(i64::from(duration))
            .is_some_and(|end| end < cutoff),
        None => record.fetched_at > 0 && record.fetched_at < cutoff,
    }
}

pub fn eligible_records(records: &[MatchRecord], cutoff: i64) -> Vec<MatchRecord> {
    let mut seen = BTreeSet::new();
    records
        .iter()
        .filter(|record| {
            record.hero_id > 0
                && standard_mode(record.game_mode)
                && available_before(record, cutoff)
                && seen.insert((record.account_id, record.match_id.as_str()))
        })
        .cloned()
        .collect()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Weights {
    pub personal: f64,
    pub counter: f64,
    pub synergy: f64,
    pub patch: f64,
}
impl Default for Weights {
    fn default() -> Self {
        Self {
            personal: 0.4,
            counter: 0.3,
            synergy: 0.2,
            patch: 0.1,
        }
    }
}
impl Weights {
    pub fn validate(&self) -> bool {
        let w = [self.personal, self.counter, self.synergy, self.patch];
        let total = w.iter().sum::<f64>();
        w.iter().all(|x| x.is_finite() && *x >= 0.0)
            && total.is_finite()
            && total > 0.0
            && self.personal + self.counter + self.synergy > 0.0
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DraftContext {
    pub allies: Vec<HeroId>,
    pub enemies: Vec<HeroId>,
    pub banned: Vec<HeroId>,
    pub role: u8,
    pub uncertain_slots: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub hero_id: HeroId,
    pub score: f64,
    pub sample_size: usize,
    pub confidence: String,
    pub reasons: Vec<String>,
    pub personal_win_rate: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub weights: Weights,
    pub half_life_days: f64,
    pub top_k: usize,
    pub current_patch: Option<u32>,
}
impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            weights: Weights::default(),
            half_life_days: 180.0,
            top_k: 5,
            current_patch: None,
        }
    }
}

/// Future external-provider schema only. No provider currently populates or scores it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopulationEvidence {
    pub hero_id: HeroId,
    pub other_hero_id: Option<HeroId>,
    pub kind: String,
    pub role: u8,
    pub patch: u32,
    pub rank_min: u32,
    pub rank_max: u32,
    pub wins: u32,
    pub matches: u32,
    pub source: String,
    pub fetched_at: i64,
}

#[derive(Debug, Default)]
struct LineupEvidence {
    score: Option<f64>,
    matches: usize,
    wins: usize,
    effective_samples: f64,
    recency_mass: f64,
}

fn lineup_evidence(
    records: &[&MatchRecord],
    targets: &[HeroId],
    enemy: bool,
    now: i64,
    half_life: f64,
    prior: f64,
) -> LineupEvidence {
    if targets.is_empty() {
        return LineupEvidence::default();
    }
    let mut seen = BTreeSet::new();
    let matches: Vec<_> = records
        .iter()
        .filter(|m| {
            let lineup = if enemy {
                &m.enemy_heroes
            } else {
                &m.team_heroes
            };
            m.lineup_complete
                && m.won().is_some()
                && targets.iter().any(|h| lineup.contains(h))
                && seen.insert((m.account_id, m.match_id.as_str()))
        })
        .collect();
    if matches.is_empty() {
        return LineupEvidence::default();
    }
    let (wins, total) = matches.iter().fold((0.0, 0.0), |(wins, total), m| {
        let w = time_weight((now - m.start_time) as f64 / 86400.0, half_life);
        (
            wins + if m.won() == Some(true) { w } else { 0.0 },
            total + w,
        )
    });
    // Delta to this hero's baseline; makes the common personal baseline appear only once.
    let rate = smoothed_rate(wins, total, prior, 20.0);
    LineupEvidence {
        score: Some((0.5 + rate - prior).clamp(0.0, 1.0)),
        matches: matches.len(),
        wins: matches.iter().filter(|m| m.won() == Some(true)).count(),
        effective_samples: effective_samples(
            &matches
                .iter()
                .map(|m| time_weight((now - m.start_time) as f64 / 86400.0, half_life))
                .collect::<Vec<_>>(),
        ),
        recency_mass: total,
    }
}

pub fn recommend(
    records: &[MatchRecord],
    _display_stats: &[HeroStats],
    pool: &PoolConfig,
    draft: &DraftContext,
    config: &ModelConfig,
    now: i64,
) -> Vec<Recommendation> {
    if !config.weights.validate()
        || !(1..=5).contains(&draft.role)
        || !config.half_life_days.is_finite()
        || config.half_life_days <= 0.0
        || pool.validate().is_err()
    {
        return Vec::new();
    }
    // Rebuild from eligible facts: caller-supplied display aggregates can include Turbo or future games.
    let records = eligible_records(records, now);
    if records
        .iter()
        .map(|m| m.account_id)
        .collect::<BTreeSet<_>>()
        .len()
        > 1
    {
        return Vec::new();
    }
    let stats = aggregate(&records, now, config.half_life_days, config.current_patch);
    let unavailable: BTreeSet<_> = draft
        .allies
        .iter()
        .chain(&draft.enemies)
        .chain(&draft.banned)
        .copied()
        .collect();
    let mut result: Vec<_> = pool
        .candidates(&stats, draft.role)
        .into_iter()
        .filter(|h| !unavailable.contains(h))
        .map(|hero_id| {
            let stat = stats.iter().find(|s| s.hero_id == hero_id);
            let baseline = stat.map_or(0.5, |s| s.smoothed_win_rate);
            let records: Vec<_> = records
                .iter()
                .filter(|m| m.hero_id == hero_id && m.start_time < now)
                .collect();
            let lineup_records: Vec<_> = records.iter().copied().filter(|m|config.current_patch.is_none_or(|patch|m.patch==Some(patch))).collect();
            let counter = lineup_evidence(
                &lineup_records,
                &draft.enemies,
                true,
                now,
                config.half_life_days,
                baseline,
            );
            let synergy = lineup_evidence(
                &lineup_records,
                &draft.allies,
                false,
                now,
                config.half_life_days,
                baseline,
            );
            let personal = stat.filter(|s|s.outcome_samples>0).map(|s| {
                let recency_mass=records.iter().filter(|m|m.won().is_some()).map(|m|time_weight((now-m.start_time) as f64 / 86400.0,config.half_life_days)).sum::<f64>();
                let familiarity = (s.effective_samples.min(recency_mass) / 50.0).min(1.0);
                0.60 * s.weighted_win_rate + 0.30 * s.smoothed_win_rate + 0.10 * familiarity
            });
            let mut reasons = Vec::new();
            if let Some(s) = stat {
                reasons.push(format!(
                    "标准模式个人历史 {} 场，胜负已知 {} 场；平滑胜率 {:.1}%，原始胜率的 Wilson 95% 区间 {:.1}—{:.1}%",
                    s.matches,
                    s.outcome_samples,
                    s.smoothed_win_rate * 100.0,
                    s.confidence_low * 100.0,
                    s.confidence_high * 100.0
                ));
                reasons.push(format!(
                    "近期按 {} 天半衰期加权，有效样本 {:.1}；近 20 场胜负已知 {} 场",
                    config.half_life_days, s.effective_samples, s.recent20.outcome_samples
                ));
            } else {
                reasons.push("手动加入英雄；没有可用个人比赛，胜率使用中性先验，熟练度按零处理".into());
            }
            reasons.push(if counter.score.is_some() {
                format!(
                    "对阵当前至少一名敌方英雄：{} 场去重比赛、{} 胜，时间权重合计 {:.1}；历史关联，未证明因果克制",
                    counter.matches, counter.wins, counter.recency_mass
                )
            } else {
                "缺少对阵样本，克制分项回退到中性先验".into()
            });
            reasons.push(if synergy.score.is_some() {
                format!(
                    "与当前至少一名队友同队：{} 场去重比赛、{} 胜，时间权重合计 {:.1}；不能视为完整阵容胜率",
                    synergy.matches, synergy.wins, synergy.recency_mass
                )
            } else {
                "缺少协同样本，协同分项回退到中性先验".into()
            });
            reasons.push(
                "没有经核验的当前版本/分段总体数据，版本分项不计入；推荐分数不是本局胜率".into(),
            );
            if let Some(patch)=config.current_patch {
                reasons.push(format!("个人对阵与协同仅使用已知 patch {patch} 的比赛；缺省版本记录不参与该分项"));
            } else {
                reasons.push("个人对阵记录跨版本使用时间衰减；当前版本未确认，不能视为当前版本克制数据".into());
            }
            if draft.uncertain_slots > 0 {
                reasons.push(format!(
                    "阵容仍有 {} 个未确定槽位，结论会随选人改变",
                    draft.uncertain_slots
                ));
            }
            let parts = [
                (personal.unwrap_or(0.45), config.weights.personal),
                (counter.score.unwrap_or(0.5), config.weights.counter),
                (synergy.score.unwrap_or(0.5), config.weights.synergy),
            ];
            let (sum, weight) = parts
                .iter()
                .map(|(v, w)| (v * w, w))
                .fold((0.0, 0.0), |(s, w), (v, wi)| (s + v, w + wi));
            let priority = pool
                .priorities
                .get(&hero_id.to_string())
                .copied()
                .unwrap_or(0.0)
                .clamp(-1.0, 1.0);
            if priority != 0.0 {
                reasons.push(format!("手动优先级调整 {:+.1} 分", priority * 5.0));
            }
            let score = (if weight > 0.0 {
                (sum / weight) * 100.0
            } else {
                50.0
            } + priority * 5.0)
                .clamp(0.0, 100.0);
            let n = stat.map_or(0, |s| s.outcome_samples);
            let confidence =
                if n >= 100 && counter.effective_samples.min(counter.recency_mass) >= 20.0
                    && synergy.effective_samples.min(synergy.recency_mass) >= 20.0 && draft.uncertain_slots == 0 {
                    "中等"
                } else {
                    "有限"
                };
            Recommendation {
                hero_id,
                score: (score * 10.0).round() / 10.0,
                sample_size: n,
                confidence: confidence.into(),
                reasons,
                personal_win_rate: stat.and_then(|s| s.win_rate),
            }
        })
        .collect();
    result.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.hero_id.cmp(&b.hero_id))
    });
    result.truncate(config.top_k.clamp(3, 5));
    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestRow {
    pub match_id: String,
    pub start_time: i64,
    pub training_matches: usize,
    pub actual_hero: HeroId,
    pub recommended: Vec<HeroId>,
    pub covered: bool,
    pub observed_win: Option<bool>,
    pub baseline_covered: bool,
    pub patch: Option<u32>,
    pub recommendation_samples: Vec<usize>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SampleDistribution {
    pub min: Option<usize>,
    pub median: Option<f64>,
    pub max: Option<usize>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObservedOutcomes {
    pub samples: usize,
    pub wins: usize,
    pub win_rate: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchBacktest {
    pub patch: Option<u32>,
    pub evaluated: usize,
    pub top_k_coverage: f64,
    pub most_played_coverage: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestReport {
    pub protocol: String,
    pub input_matches: usize,
    pub standard_matches: usize,
    pub skipped_insufficient_history: usize,
    pub skipped_no_candidates: usize,
    pub evaluated: usize,
    pub top_k_coverage: Option<f64>,
    pub most_played_coverage: Option<f64>,
    pub rows: Vec<BacktestRow>,
    pub warnings: Vec<String>,
    pub training_samples: SampleDistribution,
    pub by_patch: Vec<PatchBacktest>,
    pub covered_outcomes: ObservedOutcomes,
}
/// Empty draft prevents leaking end-of-match lineups into an imagined draft chronology.
/// Measures historical-choice coverage, NOT a counterfactual benefit or policy win rate.
pub fn backtest(
    records: &[MatchRecord],
    pool: &PoolConfig,
    role: u8,
    config: &ModelConfig,
    min_training: usize,
) -> BacktestReport {
    let mut warnings = vec![
        "固定使用当前英雄池和位置配置；未重建用户过去的英雄池，因此这是回顾性选择覆盖评估".into(),
        "按比赛结束时间近似历史可用时间；数据源当时的解析/公开延迟无法从现有快照重建".into(),
        "无历史选人时间线，使用空阵容；不能评估实时视觉识别、克制分项或反事实胜率收益".into(),
    ];
    let mixed_accounts = records
        .iter()
        .map(|m| m.account_id)
        .collect::<BTreeSet<_>>()
        .len()
        > 1;
    let invalid_config = !config.weights.validate()
        || !config.half_life_days.is_finite()
        || config.half_life_days <= 0.0
        || !(1..=5).contains(&role)
        || pool.validate().is_err();
    let mut ordered: Vec<_> = records
        .iter()
        .filter(|m| m.hero_id > 0 && m.start_time >= 0 && standard_mode(m.game_mode))
        .collect();
    ordered.sort_by_key(|m| (m.start_time, m.match_id.as_str()));
    let mut seen = BTreeSet::new();
    ordered.retain(|m| seen.insert((m.account_id, m.match_id.as_str())));
    let mut rows = Vec::new();
    let mut skipped_insufficient_history = 0;
    let mut skipped_no_candidates = 0;
    if mixed_accounts {
        warnings.push("检测到多个账号，拒绝混合个人历史；本报告不包含评估行".into());
    }
    if invalid_config {
        warnings.push("模型或英雄池配置无效；本报告不包含评估行".into());
    }
    for test in &ordered {
        if mixed_accounts || invalid_config {
            break;
        }
        let training: Vec<_> = ordered
            .iter()
            .filter(|m| available_before(m, test.start_time))
            .map(|m| (*m).clone())
            .collect();
        if training.len() < min_training {
            skipped_insufficient_history += 1;
            continue;
        }
        let stats = aggregate(&training, test.start_time, config.half_life_days, None);
        let eligible = pool.candidates(&stats, role);
        if eligible.is_empty() {
            skipped_no_candidates += 1;
            continue;
        }
        let rec = recommend(
            &training,
            &stats,
            pool,
            &DraftContext {
                role,
                uncertain_slots: 9,
                ..Default::default()
            },
            config,
            test.start_time,
        );
        let recommended: Vec<_> = rec.iter().map(|r| r.hero_id).collect();
        let baseline: Vec<_> = stats
            .iter()
            .filter(|s| eligible.contains(&s.hero_id))
            .take(config.top_k.clamp(3, 5))
            .map(|s| s.hero_id)
            .collect();
        rows.push(BacktestRow {
            match_id: test.match_id.clone(),
            start_time: test.start_time,
            training_matches: training.len(),
            actual_hero: test.hero_id,
            covered: recommended.contains(&test.hero_id),
            recommended,
            observed_win: test.won(),
            baseline_covered: baseline.contains(&test.hero_id),
            patch: test.patch,
            recommendation_samples: rec.iter().map(|r| r.sample_size).collect(),
        });
    }
    let n = rows.len();
    if n < 100 {
        warnings.push(format!("仅 {n} 条可评估记录，不能据此声称推荐模型有效"));
    }
    if skipped_no_candidates > 0 {
        warnings.push(format!(
            "{skipped_no_candidates} 条记录因英雄池/位置没有合格候选而跳过"
        ));
    }
    let mut sizes = rows.iter().map(|r| r.training_matches).collect::<Vec<_>>();
    sizes.sort_unstable();
    let training_samples = SampleDistribution {
        min: sizes.first().copied(),
        max: sizes.last().copied(),
        median: (!sizes.is_empty()).then(|| {
            let n = sizes.len();
            if n % 2 == 0 {
                (sizes[n / 2 - 1] as f64 + sizes[n / 2] as f64) / 2.0
            } else {
                sizes[n / 2] as f64
            }
        }),
    };
    let mut grouped: BTreeMap<Option<u32>, Vec<&BacktestRow>> = BTreeMap::new();
    for row in &rows {
        grouped.entry(row.patch).or_default().push(row);
    }
    let by_patch = grouped
        .into_iter()
        .map(|(patch, rows)| PatchBacktest {
            patch,
            evaluated: rows.len(),
            top_k_coverage: rows.iter().filter(|r| r.covered).count() as f64 / rows.len() as f64,
            most_played_coverage: rows.iter().filter(|r| r.baseline_covered).count() as f64
                / rows.len() as f64,
        })
        .collect();
    let covered = rows
        .iter()
        .filter(|r| r.covered)
        .filter_map(|r| r.observed_win)
        .collect::<Vec<_>>();
    let wins = covered.iter().filter(|won| **won).count();
    let covered_outcomes = ObservedOutcomes {
        samples: covered.len(),
        wins,
        win_rate: (!covered.is_empty()).then(|| wins as f64 / covered.len() as f64),
    };
    BacktestReport {
        protocol:
            "训练比赛须在测试比赛开始前结束；标准模式；空阵容；固定手动位置配置；覆盖实际选用英雄，不是获胜概率或反事实收益"
                .into(),
        input_matches:records.len(),standard_matches:ordered.len(),skipped_insufficient_history,skipped_no_candidates,
        evaluated: n,
        top_k_coverage: (n > 0)
            .then(|| rows.iter().filter(|r| r.covered).count() as f64 / n as f64),
        most_played_coverage: (n > 0)
            .then(|| rows.iter().filter(|r| r.baseline_covered).count() as f64 / n as f64),
        rows,
        warnings,training_samples,by_patch,covered_outcomes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dpda_hero_pool::PoolMode;
    fn games() -> Vec<MatchRecord> {
        (1..=80)
            .map(|i| MatchRecord {
                match_id: i.to_string(),
                hero_id: (i % 4 + 1) as u32,
                start_time: i * 86400,
                duration: Some(3600),
                game_mode: Some(22),
                player_slot: Some(0),
                radiant_win: Some(i % 3 != 0),
                team_heroes: vec![10, 11, 12, 13],
                enemy_heroes: vec![20, 21, 22, 23, 24],
                lineup_complete: true,
                ..Default::default()
            })
            .collect()
    }
    fn pool() -> PoolConfig {
        PoolConfig {
            mode: PoolMode::Manual,
            included: vec![1, 2, 3, 4],
            roles: (1..=4).map(|i| (i.to_string(), vec![2])).collect(),
            ..Default::default()
        }
    }
    #[test]
    fn chosen_banned_excluded_and_role_filters_are_hard_rules() {
        let records = games();
        let stats = aggregate(&records, 100 * 86400, 180.0, None);
        let mut p = pool();
        p.excluded.push(4);
        let d = DraftContext {
            enemies: vec![1],
            banned: vec![2],
            role: 2,
            ..Default::default()
        };
        let r = recommend(
            &records,
            &stats,
            &p,
            &d,
            &ModelConfig::default(),
            100 * 86400,
        );
        assert_eq!(r.iter().map(|r| r.hero_id).collect::<Vec<_>>(), vec![3]);
        assert!(recommend(
            &records,
            &stats,
            &p,
            &DraftContext { role: 5, ..d },
            &ModelConfig::default(),
            100 * 86400
        )
        .is_empty());
    }
    #[test]
    fn opponent_evidence_deduplicates_matches_not_sum_of_pairs() {
        let records = games();
        let refs = records.iter().collect::<Vec<_>>();
        let evidence = lineup_evidence(&refs, &[20, 21, 22], true, 100 * 86400, 180.0, 0.5);
        assert_eq!(evidence.matches, 80);
    }
    #[test]
    fn temporal_test_uses_only_strict_past_and_no_final_lineup() {
        let r = backtest(&games(), &pool(), 2, &ModelConfig::default(), 10);
        assert_eq!(r.rows[0].training_matches, 10);
        assert_eq!(r.rows[0].start_time, 11 * 86400);
        let short = backtest(&games()[..30], &pool(), 2, &ModelConfig::default(), 10);
        assert_eq!(r.rows[0].recommended, short.rows[0].recommended);
    }
    #[test]
    fn future_and_turbo_display_stats_cannot_leak_into_recommendations() {
        let records = games();
        let now = 100 * 86400;
        let clean = recommend(
            &records,
            &[],
            &pool(),
            &DraftContext {
                role: 2,
                ..Default::default()
            },
            &ModelConfig::default(),
            now,
        );
        let mut polluted = records.clone();
        for mut m in games() {
            m.match_id = format!("turbo-{}", m.match_id);
            m.game_mode = Some(23);
            m.hero_id = 1;
            m.radiant_win = Some(true);
            polluted.push(m);
        }
        let mut future = games()[0].clone();
        future.match_id = "future".into();
        future.start_time = now + 1;
        future.hero_id = 1;
        polluted.push(future);
        let display = aggregate(&polluted, now + 10, 180.0, None);
        let actual = recommend(
            &polluted,
            &display,
            &pool(),
            &DraftContext {
                role: 2,
                ..Default::default()
            },
            &ModelConfig::default(),
            now,
        );
        assert_eq!(
            clean
                .iter()
                .map(|r| (r.hero_id, r.score, r.sample_size))
                .collect::<Vec<_>>(),
            actual
                .iter()
                .map(|r| (r.hero_id, r.score, r.sample_size))
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn overlapping_games_and_unknown_duration_do_not_enter_backtest_training() {
        let mut records = games()[..4].to_vec();
        records[0].duration = Some(3 * 86400);
        records[1].duration = None;
        records[1].fetched_at = 10 * 86400;
        let r = backtest(&records, &pool(), 2, &ModelConfig::default(), 1);
        assert_eq!(r.evaluated, 1);
        assert_eq!(r.rows[0].start_time, 4 * 86400);
        assert_eq!(r.rows[0].training_matches, 1);
    }
    #[test]
    fn unusable_pool_and_unknown_modes_do_not_report_zero_percent_as_a_model_result() {
        let report = backtest(
            &games(),
            &PoolConfig::default(),
            2,
            &ModelConfig::default(),
            10,
        );
        assert_eq!(report.evaluated, 0);
        assert_eq!(report.top_k_coverage, None);
        assert!(report.skipped_no_candidates > 0);
        let mut row = games()[0].clone();
        row.game_mode = None;
        assert!(eligible_records(&[row], 100 * 86400).is_empty());
    }
    #[test]
    fn nonfinite_or_unavailable_weights_are_invalid() {
        assert!(!Weights {
            personal: f64::MAX,
            counter: f64::MAX,
            synergy: 0.0,
            patch: 0.0
        }
        .validate());
        assert!(!Weights {
            personal: 0.0,
            counter: 0.0,
            synergy: 0.0,
            patch: 1.0
        }
        .validate());
    }
}
