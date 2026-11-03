//! Explainable position candidates, never calibrated role probabilities.
//!
//! Early team farm ranks are preferred over final economic outcomes. Lane labels
//! alone cannot distinguish a farming player from a support in the same lane.
use dpda_domain::{HeroId, MatchRecord};
pub use dpda_domain::{MatchRoleEvidence, PlayerRoleEvidence};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const ROLE_RULE_VERSION: &str = "team-farm-lane-v1";
const MIN_SUPPORTING_MATCHES: usize = 3;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RoleStrength {
    Unknown,
    Limited,
    Moderate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchRoleInference {
    pub match_id: String,
    pub hero_id: HeroId,
    pub start_time: i64,
    pub candidates: Vec<u8>,
    pub strength: RoleStrength,
    pub method: String,
    /// Only complete-team, concordant two-metric farm evidence qualifies.
    pub qualifying_evidence: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleSupport {
    pub role: u8,
    pub supporting_matches: usize,
    pub exclusive_matches: usize,
    pub early_matches: usize,
    pub late_matches: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeroRoleAnalysis {
    pub hero_id: HeroId,
    pub matches: usize,
    pub analyzed_matches: usize,
    pub unknown_matches: usize,
    pub lane_only_matches: usize,
    pub coverage: f64,
    /// Role counts overlap when one match has several possible positions.
    pub candidates: Vec<RoleSupport>,
    pub suggested_roles: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAnalysis {
    pub rule_version: String,
    pub account_id: Option<u32>,
    pub total_matches: usize,
    pub eligible_matches: usize,
    pub detail_matches: usize,
    pub analyzed_matches: usize,
    pub unknown_matches: usize,
    pub heroes: Vec<HeroRoleAnalysis>,
    pub matches: Vec<MatchRoleInference>,
    pub warnings: Vec<String>,
}

fn valid_slot(slot: u8) -> bool {
    slot <= 4 || (128..=132).contains(&slot)
}

fn eligible(record: &MatchRecord, now: i64) -> bool {
    record.hero_id > 0
        && matches!(record.game_mode, Some(1 | 2 | 3 | 4 | 22))
        && record.start_time >= 0
        && record.start_time < now
        && match record.duration {
            Some(duration) => record
                .start_time
                .checked_add(i64::from(duration))
                .is_some_and(|end| end < now),
            None => record.fetched_at > 0 && record.fetched_at < now,
        }
}

fn lane_candidates(lane: Option<u8>) -> Vec<u8> {
    match lane {
        Some(1) => vec![1, 4, 5],
        Some(2) => vec![2, 4, 5],
        Some(3) => vec![3, 4, 5],
        _ => Vec::new(),
    }
}

fn rank_range(values: &[f64], index: usize) -> (usize, usize) {
    let target = values[index];
    (
        1 + values.iter().filter(|&&value| value > target).count(),
        values.iter().filter(|&&value| value >= target).count(),
    )
}

fn valid_early(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite() && *v >= 0.0)
}

fn infer(record: &MatchRecord, detail: Option<&MatchRoleEvidence>) -> MatchRoleInference {
    let mut out = MatchRoleInference {
        match_id: record.match_id.clone(),
        hero_id: record.hero_id,
        start_time: record.start_time,
        candidates: lane_candidates(record.lane_role),
        strength: RoleStrength::Unknown,
        method: if lane_candidates(record.lane_role).is_empty() {
            "insufficient_data"
        } else {
            "lane_only"
        }
        .into(),
        qualifying_evidence: false,
        reasons: vec!["分路标签不能独立区分核心与辅助；未知样本不计入自动位置建议".into()],
    };
    let Some(detail) = detail else {
        out.reasons.push("缺少同场队友经济与补刀证据".into());
        return out;
    };
    let Some(slot) = record.player_slot.filter(|slot| valid_slot(*slot)) else {
        out.reasons
            .push("个人槽位缺失或无效，无法核对所属队伍".into());
        return out;
    };
    let team: Vec<_> = detail
        .players
        .iter()
        .filter(|p| valid_slot(p.player_slot) && (p.player_slot < 128) == (slot < 128))
        .collect();
    if team.len() != 5
        || team
            .iter()
            .map(|p| p.player_slot)
            .collect::<BTreeSet<_>>()
            .len()
            != 5
        || team
            .iter()
            .map(|p| p.hero_id)
            .collect::<BTreeSet<_>>()
            .len()
            != 5
        || team.iter().any(|p| p.hero_id == 0)
    {
        out.reasons
            .push("同队五个唯一槽位与英雄不完整，不能计算队内排名".into());
        return out;
    }
    let Some(index) = team
        .iter()
        .position(|p| p.player_slot == slot && p.hero_id == record.hero_id)
    else {
        out.reasons.push("详情与个人比赛的英雄或槽位不匹配".into());
        return out;
    };
    let player = team[index];
    let detail_lane = player.lane_role.filter(|lane| (1..=3).contains(lane));
    let record_lane = record.lane_role.filter(|lane| (1..=3).contains(lane));
    let lane = match (detail_lane, record_lane) {
        (Some(a), Some(b)) if a != b => {
            out.reasons
                .push("详情与历史分路标签冲突，保留核心多位置候选".into());
            None
        }
        (Some(a), _) => Some(a),
        (_, b) => b,
    };
    out.candidates = lane_candidates(lane);
    out.method = if out.candidates.is_empty() {
        "insufficient_data"
    } else {
        "lane_only"
    }
    .into();
    let early_gold: Option<Vec<_>> = team.iter().map(|p| valid_early(p.early_gold)).collect();
    let early_lh: Option<Vec<_>> = team
        .iter()
        .map(|p| valid_early(p.early_last_hits))
        .collect();
    let (gold, last_hits, early) = match (early_gold, early_lh) {
        (Some(gold), Some(last_hits)) if record.duration.is_some_and(|d| d >= 12 * 60) => {
            (gold, last_hits, true)
        }
        _ => {
            let gold: Option<Vec<_>> = team.iter().map(|p| p.gold_per_min.map(f64::from)).collect();
            let last_hits: Option<Vec<_>> =
                team.iter().map(|p| p.last_hits.map(f64::from)).collect();
            let (Some(gold), Some(last_hits)) = (gold, last_hits) else {
                out.reasons
                    .push("同队双指标不完整；没有用缺失值补零排名".into());
                return out;
            };
            (gold, last_hits, false)
        }
    };
    let gold_rank = rank_range(&gold, index);
    let lh_rank = rank_range(&last_hits, index);
    out.method = if early {
        "early_team_farm"
    } else {
        "final_team_farm"
    }
    .into();
    out.reasons.push(format!(
        "{}：经济队内排名{}—{}，补刀排名{}—{}；并列名次不强行拆分",
        if early {
            "10—12分钟均值"
        } else {
            "赛后数据"
        },
        gold_rank.0,
        gold_rank.1,
        lh_rank.0,
        lh_rank.1
    ));
    if gold_rank.1 <= 3 && lh_rank.1 <= 3 {
        out.candidates = lane.map_or_else(|| vec![1, 2, 3], |lane| vec![lane]);
        out.qualifying_evidence = true;
        out.reasons.push(
            "双指标均处于队内前三，结合分路形成核心位置候选；未按财富第一直接指定一号位".into(),
        );
    } else if gold_rank.0 >= 4 && lh_rank.0 >= 4 {
        out.candidates = vec![4, 5];
        out.qualifying_evidence = true;
        out.reasons
            .push("双指标均处于队内后两位，保留四/五号位并列；无法可靠区分两种辅助职责".into());
    } else {
        out.reasons
            .push("经济与补刀结论冲突，或并列跨越核心/辅助边界；本场不累计位置支持样本".into());
    }
    if out.qualifying_evidence {
        out.strength = if early && out.candidates.len() == 1 {
            RoleStrength::Moderate
        } else {
            RoleStrength::Limited
        };
    }
    if !early {
        out.reasons
            .push("终局经济受表现、比赛时长与职责变化影响，仅作有限规则证据".into());
    }
    if player.position_est.is_some() {
        out.reasons
            .push("上游position_est也是推断值，不独立用作标签或提高可信度".into());
    }
    out
}

/// Analyze one account's historical evidence. Each match counts once even if it
/// supports several candidate roles; per-role counts are explicitly overlapping.
pub fn analyze_roles(
    records: &[MatchRecord],
    details: &[MatchRoleEvidence],
    now: i64,
) -> RoleAnalysis {
    let accounts: BTreeSet<_> = records.iter().map(|r| r.account_id).collect();
    let mut out = RoleAnalysis {
        rule_version: ROLE_RULE_VERSION.into(),
        account_id: (accounts.len() == 1).then(|| *accounts.first().unwrap()),
        total_matches: records
            .iter()
            .map(|r| (r.account_id, r.match_id.as_str()))
            .collect::<BTreeSet<_>>()
            .len(),
        eligible_matches: 0,
        detail_matches: 0,
        analyzed_matches: 0,
        unknown_matches: 0,
        heroes: Vec::new(),
        matches: Vec::new(),
        warnings: vec![
            "位置由个人比赛中的队内资源与分路规则推断，未经人工标签验证，不是概率或确认事实".into(),
            "每个位置至少需要3场独立农钱证据；并列角色的支持场次重叠，不能相加为比赛总数".into(),
            "仅分析已结束的标准模式；未使用静态英雄印象推断个人位置".into(),
        ],
    };
    let Some(account) = out.account_id.filter(|id| *id > 0) else {
        if !records.is_empty() {
            out.warnings
                .push("输入包含多个账号或无效账号，拒绝混合个人位置统计".into());
        }
        return out;
    };
    let mut detail_map: BTreeMap<&str, &MatchRoleEvidence> = BTreeMap::new();
    for detail in details.iter().filter(|d| {
        d.account_id == account && d.fetched_at > 0 && d.fetched_at <= now && !d.source.is_empty()
    }) {
        let entry = detail_map.entry(detail.match_id.as_str()).or_insert(detail);
        if detail.fetched_at > entry.fetched_at {
            *entry = detail;
        }
    }
    let mut seen = BTreeSet::new();
    for record in records {
        if !eligible(record, now) || !seen.insert(record.match_id.as_str()) {
            continue;
        }
        let detail = detail_map.get(record.match_id.as_str()).copied();
        out.detail_matches += usize::from(detail.is_some());
        out.matches.push(infer(record, detail));
    }
    out.matches.sort_by(|a, b| {
        b.start_time
            .cmp(&a.start_time)
            .then(a.match_id.cmp(&b.match_id))
    });
    out.eligible_matches = out.matches.len();
    out.analyzed_matches = out.matches.iter().filter(|m| m.qualifying_evidence).count();
    out.unknown_matches = out.eligible_matches - out.analyzed_matches;
    let mut by_hero: BTreeMap<HeroId, Vec<&MatchRoleInference>> = BTreeMap::new();
    for inference in &out.matches {
        by_hero
            .entry(inference.hero_id)
            .or_default()
            .push(inference);
    }
    for (hero_id, matches) in by_hero {
        let analyzed = matches.iter().filter(|m| m.qualifying_evidence).count();
        let candidates: Vec<_> = (1..=5)
            .filter_map(|role| {
                let supporting: Vec<_> = matches
                    .iter()
                    .filter(|m| m.qualifying_evidence && m.candidates.contains(&role))
                    .collect();
                (!supporting.is_empty()).then(|| RoleSupport {
                    role,
                    supporting_matches: supporting.len(),
                    exclusive_matches: supporting
                        .iter()
                        .filter(|m| m.candidates.len() == 1)
                        .count(),
                    early_matches: supporting
                        .iter()
                        .filter(|m| m.method == "early_team_farm")
                        .count(),
                    late_matches: supporting
                        .iter()
                        .filter(|m| m.method == "final_team_farm")
                        .count(),
                })
            })
            .collect();
        let suggested_roles = candidates
            .iter()
            .filter(|c| c.supporting_matches >= MIN_SUPPORTING_MATCHES)
            .map(|c| c.role)
            .collect();
        out.heroes.push(HeroRoleAnalysis {
            hero_id,
            matches: matches.len(),
            analyzed_matches: analyzed,
            unknown_matches: matches.len() - analyzed,
            lane_only_matches: matches.iter().filter(|m| m.method == "lane_only").count(),
            coverage: analyzed as f64 / matches.len() as f64,
            candidates,
            suggested_roles,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str, lane: Option<u8>, index: usize) -> (MatchRecord, MatchRoleEvidence) {
        let record = MatchRecord {
            account_id: 42,
            match_id: id.into(),
            hero_id: index as u32 + 1,
            start_time: 100,
            duration: Some(1800),
            game_mode: Some(22),
            player_slot: Some(index as u8),
            lane_role: lane,
            fetched_at: 2000,
            ..Default::default()
        };
        let detail = MatchRoleEvidence {
            account_id: 42,
            match_id: id.into(),
            source: "synthetic-unit-test".into(),
            fetched_at: 2000,
            players: (0..5)
                .map(|i| PlayerRoleEvidence {
                    player_slot: i,
                    hero_id: u32::from(i) + 1,
                    lane_role: (i as usize == index).then_some(lane).flatten(),
                    gold_per_min: Some(600 - u32::from(i) * 100),
                    last_hits: Some(300 - u32::from(i) * 60),
                    early_gold: Some(6000.0 - f64::from(i) * 1000.0),
                    early_last_hits: Some(80.0 - f64::from(i) * 15.0),
                    position_est: None,
                })
                .collect(),
        };
        (record, detail)
    }

    #[test]
    fn lane_alone_never_becomes_a_confirmed_position() {
        let (record, _) = sample("1", Some(1), 0);
        let analysis = analyze_roles(&[record], &[], 3000);
        assert_eq!(analysis.matches[0].candidates, vec![1, 4, 5]);
        assert_eq!(analysis.analyzed_matches, 0);
        assert!(analysis.heroes[0].suggested_roles.is_empty());
        assert_eq!(analysis.heroes[0].lane_only_matches, 1);
        let (record, _) = sample("2", None, 0);
        let analysis = analyze_roles(&[record], &[], 3000);
        assert_eq!(analysis.heroes[0].lane_only_matches, 0);
        assert_eq!(analysis.matches[0].method, "insufficient_data");
    }

    #[test]
    fn core_lane_and_three_unique_matches_allow_automatic_roles() {
        let samples: Vec<_> = (1..=3)
            .map(|id| sample(&id.to_string(), Some(2), 0))
            .collect();
        let mut records: Vec<_> = samples.iter().map(|(r, _)| r.clone()).collect();
        records.push(records[0].clone());
        let details: Vec<_> = samples.into_iter().map(|(_, d)| d).collect();
        let analysis = analyze_roles(&records, &details, 3000);
        assert_eq!(analysis.analyzed_matches, 3);
        assert_eq!(analysis.heroes[0].suggested_roles, vec![2]);
        assert_eq!(analysis.matches[0].strength, RoleStrength::Moderate);
        assert_eq!(analysis.heroes[0].candidates[0].supporting_matches, 3);
    }

    #[test]
    fn supports_remain_four_and_five_even_with_provider_position() {
        let samples: Vec<_> = (1..=3)
            .map(|id| sample(&id.to_string(), Some(1), 4))
            .collect();
        let records: Vec<_> = samples.iter().map(|(r, _)| r.clone()).collect();
        let mut details: Vec<_> = samples.into_iter().map(|(_, d)| d).collect();
        for d in &mut details {
            d.players[4].position_est = Some(5);
        }
        let analysis = analyze_roles(&records, &details, 3000);
        assert_eq!(analysis.heroes[0].suggested_roles, vec![4, 5]);
        assert_eq!(analysis.analyzed_matches, 3);
        assert_eq!(
            analysis.heroes[0]
                .candidates
                .iter()
                .map(|s| s.supporting_matches)
                .sum::<usize>(),
            6
        );
        assert_eq!(analysis.matches[0].strength, RoleStrength::Limited);
    }

    #[test]
    fn ties_and_conflicting_metrics_do_not_force_farm_groups() {
        let (record, mut detail) = sample("1", Some(3), 2);
        detail.players[3].early_gold = detail.players[2].early_gold;
        let analysis = analyze_roles(std::slice::from_ref(&record), &[detail.clone()], 3000);
        assert!(!analysis.matches[0].qualifying_evidence);
        detail.players[2].early_gold = Some(100.0);
        let analysis = analyze_roles(&[record], &[detail], 3000);
        assert!(!analysis.matches[0].qualifying_evidence);
    }

    #[test]
    fn missing_or_nonfinite_early_data_falls_back_with_limited_strength() {
        let (record, mut detail) = sample("1", Some(3), 2);
        detail.players[0].early_gold = Some(f64::NAN);
        let analysis = analyze_roles(std::slice::from_ref(&record), &[detail.clone()], 3000);
        assert_eq!(analysis.matches[0].method, "final_team_farm");
        assert_eq!(analysis.matches[0].strength, RoleStrength::Limited);
        detail.players[0].last_hits = None;
        detail.players[2].position_est = Some(3);
        let analysis = analyze_roles(&[record], &[detail], 3000);
        assert_eq!(analysis.analyzed_matches, 0);
    }

    #[test]
    fn wrong_accounts_invalid_teams_and_future_details_are_ignored() {
        let (record, mut detail) = sample("1", Some(2), 1);
        detail.account_id = 99;
        assert_eq!(
            analyze_roles(std::slice::from_ref(&record), &[detail.clone()], 3000).detail_matches,
            0
        );
        detail.account_id = 42;
        detail.fetched_at = 4000;
        assert_eq!(
            analyze_roles(std::slice::from_ref(&record), &[detail.clone()], 3000).detail_matches,
            0
        );
        detail.fetched_at = 2000;
        detail.players[0].player_slot = 1;
        assert_eq!(
            analyze_roles(std::slice::from_ref(&record), &[detail], 3000).analyzed_matches,
            0
        );
        let mut other = record.clone();
        other.account_id = 99;
        assert!(analyze_roles(&[record, other], &[], 3000).heroes.is_empty());
    }

    #[test]
    fn nonstandard_and_unfinished_matches_do_not_enter_analysis() {
        let (mut record, detail) = sample("1", Some(2), 1);
        record.game_mode = Some(23);
        assert_eq!(
            analyze_roles(
                std::slice::from_ref(&record),
                std::slice::from_ref(&detail),
                3000
            )
            .eligible_matches,
            0
        );
        record.game_mode = Some(22);
        assert_eq!(
            analyze_roles(&[record], &[detail], 1500).eligible_matches,
            0
        );
    }

    #[test]
    fn core_without_lane_preserves_multiple_positions_and_sample_threshold() {
        let samples: Vec<_> = (1..=3).map(|id| sample(&id.to_string(), None, 1)).collect();
        let records: Vec<_> = samples.iter().map(|(r, _)| r.clone()).collect();
        let details: Vec<_> = samples.into_iter().map(|(_, d)| d).collect();
        let small = analyze_roles(&records[..2], &details, 3000);
        assert!(small.heroes[0].suggested_roles.is_empty());
        let analysis = analyze_roles(&records, &details, 3000);
        assert_eq!(analysis.heroes[0].suggested_roles, vec![1, 2, 3]);
        assert_eq!(analysis.matches[0].strength, RoleStrength::Limited);
        assert!(analysis.heroes[0]
            .candidates
            .iter()
            .all(|c| c.exclusive_matches == 0));
    }

    #[test]
    fn dire_roster_and_lane_conflict_are_handled_without_hero_stereotypes() {
        let (mut record, mut detail) = sample("1", Some(2), 1);
        record.player_slot = Some(129);
        for player in &mut detail.players {
            player.player_slot += 128;
        }
        let analysis = analyze_roles(std::slice::from_ref(&record), &[detail.clone()], 3000);
        assert_eq!(analysis.matches[0].candidates, vec![2]);
        detail.players[1].lane_role = Some(3);
        let analysis = analyze_roles(&[record], &[detail], 3000);
        assert_eq!(analysis.matches[0].candidates, vec![1, 2, 3]);
        assert_eq!(analysis.matches[0].strength, RoleStrength::Limited);
    }
}
