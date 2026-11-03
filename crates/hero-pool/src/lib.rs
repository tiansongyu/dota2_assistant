//! Personal history aggregation and explicit pool policy.
use dpda_domain::{HeroId, MatchRecord};
use dpda_statistics::{effective_samples, smoothed_rate, time_weight, wilson};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsWindow {
    pub matches: usize,
    pub outcome_samples: usize,
    pub wins: usize,
    pub win_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeroStats {
    pub hero_id: HeroId,
    pub matches: usize,
    pub outcome_samples: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: Option<f64>,
    pub smoothed_win_rate: f64,
    pub confidence_low: f64,
    pub confidence_high: f64,
    pub recent20: StatsWindow,
    pub recent50: StatsWindow,
    pub days90: StatsWindow,
    pub days180: StatsWindow,
    pub year: StatsWindow,
    pub current_patch: StatsWindow,
    pub weighted_win_rate: f64,
    pub effective_samples: f64,
    pub avg_kda: Option<f64>,
    pub avg_gpm: Option<f64>,
    pub avg_xpm: Option<f64>,
    pub lineup_coverage: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PoolMode {
    #[default]
    Auto,
    Manual,
    Hybrid,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolConfig {
    pub mode: PoolMode,
    pub included: Vec<HeroId>,
    pub excluded: Vec<HeroId>,
    pub roles: BTreeMap<String, Vec<u8>>,
    pub priorities: BTreeMap<String, f64>,
}

impl PoolConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .included
            .iter()
            .chain(&self.excluded)
            .any(|id| *id == 0)
            || self
                .roles
                .keys()
                .chain(self.priorities.keys())
                .any(|id| id.parse::<u32>().ok().filter(|value| *value > 0).is_none())
        {
            return Err("英雄 ID 必须是非零整数".into());
        }
        if self.roles.values().flatten().any(|r| !(1..=5).contains(r)) {
            return Err("位置必须为 1—5".into());
        }
        if self
            .priorities
            .values()
            .any(|p| !p.is_finite() || !(-1.0..=1.0).contains(p))
        {
            return Err("英雄优先级必须介于 -1 和 1".into());
        }
        Ok(())
    }

    pub fn candidates(&self, stats: &[HeroStats], role: u8) -> Vec<HeroId> {
        if !(1..=5).contains(&role) || self.validate().is_err() {
            return Vec::new();
        }
        let automatic = automatic_pool(stats);
        let mut ids: BTreeSet<_> = match self.mode {
            PoolMode::Auto | PoolMode::Hybrid => automatic.into_iter().collect(),
            PoolMode::Manual => BTreeSet::new(),
        };
        if self.mode != PoolMode::Auto {
            ids.extend(self.included.iter().copied());
        }
        ids.into_iter()
            .filter(|id| {
                *id > 0
                    && !self.excluded.contains(id)
                    && self
                        .roles
                        .get(&id.to_string())
                        .is_some_and(|roles| roles.contains(&role))
            })
            .collect()
    }
}

/// Conservative initial eligibility; sparse history is visible, not inflated into mastery.
pub fn automatic_pool(stats: &[HeroStats]) -> Vec<HeroId> {
    let mut eligible: Vec<_> = stats
        .iter()
        .filter(|s| s.matches >= 5 || s.days90.matches >= 3)
        .collect();
    eligible.sort_by(|a, b| {
        let key = |s: &HeroStats| {
            (s.matches as f64 + 1.0).ln()
                + 2.0 * (s.days90.matches as f64 + 1.0).ln()
                + s.smoothed_win_rate
        };
        key(b)
            .total_cmp(&key(a))
            .then_with(|| a.hero_id.cmp(&b.hero_id))
    });
    eligible.into_iter().take(20).map(|s| s.hero_id).collect()
}

fn window(records: &[&MatchRecord]) -> StatsWindow {
    let wins = records.iter().filter(|m| m.won() == Some(true)).count();
    let n = records.iter().filter(|m| m.won().is_some()).count();
    StatsWindow {
        matches: records.len(),
        outcome_samples: n,
        wins,
        win_rate: (n > 0).then(|| wins as f64 / n as f64),
    }
}
fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values.fold((0.0, 0usize), |(sum, n), x| (sum + x, n + 1));
    (count > 0).then(|| sum / count as f64)
}

pub fn aggregate(
    records: &[MatchRecord],
    now: i64,
    half_life_days: f64,
    current_patch: Option<u32>,
) -> Vec<HeroStats> {
    let mut by_hero: BTreeMap<HeroId, Vec<&MatchRecord>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for m in records
        .iter()
        .filter(|m| m.hero_id > 0 && m.start_time >= 0 && m.start_time <= now)
    {
        if seen.insert((m.account_id, m.match_id.as_str())) {
            by_hero.entry(m.hero_id).or_default().push(m);
        }
    }
    let mut result: Vec<_> = by_hero
        .into_iter()
        .map(|(hero_id, mut ms)| {
            ms.sort_by_key(|m| std::cmp::Reverse(m.start_time));
            let all = window(&ms);
            let interval = wilson(all.wins, all.outcome_samples);
            let weighted: Vec<_> = ms
                .iter()
                .filter_map(|m| {
                    m.won().map(|y| {
                        (
                            time_weight((now - m.start_time) as f64 / 86400.0, half_life_days),
                            y,
                        )
                    })
                })
                .collect();
            let total: f64 = weighted.iter().map(|(w, _)| w).sum();
            let win_mass: f64 = weighted.iter().filter(|(_, y)| *y).map(|(w, _)| w).sum();
            let days = |days: i64| {
                window(
                    &ms.iter()
                        .copied()
                        .filter(|m| now - m.start_time <= days * 86400)
                        .collect::<Vec<_>>(),
                )
            };
            HeroStats {
                hero_id,
                matches: all.matches,
                outcome_samples: all.outcome_samples,
                wins: all.wins,
                losses: all.outcome_samples - all.wins,
                win_rate: all.win_rate,
                smoothed_win_rate: smoothed_rate(
                    all.wins as f64,
                    all.outcome_samples as f64,
                    0.5,
                    10.0,
                ),
                confidence_low: interval.low,
                confidence_high: interval.high,
                recent20: window(&ms.iter().copied().take(20).collect::<Vec<_>>()),
                recent50: window(&ms.iter().copied().take(50).collect::<Vec<_>>()),
                days90: days(90),
                days180: days(180),
                year: days(365),
                current_patch: window(
                    &ms.iter()
                        .copied()
                        .filter(|m| current_patch.is_some() && m.patch == current_patch)
                        .collect::<Vec<_>>(),
                ),
                weighted_win_rate: smoothed_rate(win_mass, total, 0.5, 10.0),
                effective_samples: effective_samples(
                    &weighted.iter().map(|(w, _)| *w).collect::<Vec<_>>(),
                ),
                avg_kda: mean(ms.iter().filter_map(|m| {
                    Some(
                        (f64::from(m.kills?) + f64::from(m.assists?)) / f64::from(m.deaths?.max(1)),
                    )
                })),
                avg_gpm: mean(ms.iter().filter_map(|m| m.gold_per_min.map(f64::from))),
                avg_xpm: mean(ms.iter().filter_map(|m| m.xp_per_min.map(f64::from))),
                lineup_coverage: ms.iter().filter(|m| m.lineup_complete).count() as f64
                    / ms.len() as f64,
            }
        })
        .collect();
    result.sort_by_key(|s| (std::cmp::Reverse(s.matches), s.hero_id));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn game(id: usize, won: bool) -> MatchRecord {
        MatchRecord {
            match_id: id.to_string(),
            hero_id: 1,
            start_time: 100,
            player_slot: Some(0),
            radiant_win: Some(won),
            ..Default::default()
        }
    }
    #[test]
    fn missing_results_are_not_losses_and_duplicates_do_not_count() {
        let mut records = vec![game(1, true), game(2, false), game(3, true), game(1, true)];
        records[2].radiant_win = None;
        let s = &aggregate(&records, 100, 180.0, None)[0];
        assert_eq!(
            (s.matches, s.outcome_samples, s.wins, s.losses),
            (3, 2, 1, 1)
        );
        assert_eq!(s.win_rate, Some(0.5));
        assert_eq!(s.current_patch.matches, 0);
    }
    #[test]
    fn exclusions_and_unknown_roles_never_leak() {
        let records: Vec<_> = (1..=8).map(|n| game(n, true)).collect();
        let stats = aggregate(&records, 100, 180.0, None);
        let mut config = PoolConfig::default();
        assert!(config.candidates(&stats, 2).is_empty());
        config.roles.insert("1".into(), vec![2]);
        assert_eq!(config.candidates(&stats, 2), vec![1]);
        config.excluded.push(1);
        config.included.push(1);
        config.mode = PoolMode::Hybrid;
        assert!(config.candidates(&stats, 2).is_empty());
    }
    #[test]
    fn invalid_ids_and_future_rows_are_rejected() {
        let mut future = game(1, true);
        future.start_time = 101;
        assert!(aggregate(&[future], 100, 180.0, None).is_empty());
        let config = PoolConfig {
            mode: PoolMode::Manual,
            included: vec![0],
            roles: BTreeMap::from([("0".into(), vec![2])]),
            ..Default::default()
        };
        assert!(config.validate().is_err());
        assert!(config.candidates(&[], 2).is_empty());
    }
    #[test]
    fn kda_sums_without_integer_overflow() {
        let mut record = game(1, true);
        record.kills = Some(u32::MAX);
        record.assists = Some(u32::MAX);
        record.deaths = Some(0);
        assert_eq!(
            aggregate(&[record], 100, 180.0, None)[0].avg_kda,
            Some(2.0 * f64::from(u32::MAX))
        );
    }
}
