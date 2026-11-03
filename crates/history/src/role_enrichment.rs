//! Bounded, cached detail collection for heuristic position analysis.
use crate::HistoryError;
use dpda_data_provider::{OpenDotaClient, ProviderError};
use dpda_domain::MatchRecord;
use dpda_persistence::Database;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RoleSyncProgress {
    pub account_id: u32,
    pub status: String,
    pub eligible_matches: usize,
    pub cached_matches: usize,
    pub fetched: usize,
    pub failed: usize,
    pub request_budget: usize,
    pub error: Option<String>,
}

/// Prefer recent play, then round-robin heroes so a frequently played hero cannot take the whole budget.
pub fn role_sample<'a>(
    records: &'a [MatchRecord],
    cached: &BTreeSet<String>,
    budget: usize,
) -> Vec<&'a MatchRecord> {
    if budget == 0
        || records
            .iter()
            .map(|r| r.account_id)
            .collect::<BTreeSet<_>>()
            .len()
            > 1
    {
        return Vec::new();
    }
    let now = chrono::Utc::now().timestamp();
    let mut seen = BTreeSet::new();
    let mut available: Vec<_> = records
        .iter()
        .filter(|r| {
            r.start_time >= 0
                && r.hero_id > 0
                && r.account_id > 0
                && !cached.contains(&r.match_id)
                && matches!(r.game_mode, Some(1 | 2 | 3 | 4 | 22))
                && r.duration
                    .is_some_and(|d| r.start_time.saturating_add(i64::from(d)) < now)
                && seen.insert(r.match_id.as_str())
        })
        .collect();
    available.sort_by_key(|r| (std::cmp::Reverse(r.start_time), &r.match_id));
    let mut chosen: Vec<_> = available
        .iter()
        .take((budget / 3).clamp(1, 80))
        .copied()
        .collect();
    let recent: BTreeSet<_> = chosen.iter().map(|r| r.match_id.as_str()).collect();
    let mut by_hero: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for r in available
        .iter()
        .filter(|r| !recent.contains(r.match_id.as_str()))
    {
        by_hero.entry(r.hero_id).or_default().push(*r);
    }
    for index in 0..available.len() {
        for records in by_hero.values() {
            if chosen.len() >= budget {
                return chosen;
            }
            if let Some(record) = records.get(index) {
                chosen.push(*record);
            }
        }
        if by_hero.values().all(|r| r.len() <= index + 1) {
            break;
        }
    }
    chosen.truncate(budget);
    chosen
}

pub async fn enrich_role_evidence(
    client: &OpenDotaClient,
    database: &mut Database,
    account_id: u32,
    request_budget: usize,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(&RoleSyncProgress),
) -> Result<RoleSyncProgress, HistoryError> {
    if !(1..=3000).contains(&request_budget) {
        return Err(HistoryError::InvalidOptions(
            "详情请求预算必须为 1–3000".into(),
        ));
    }
    let records = database.records(account_id)?;
    let eligible: BTreeSet<_> = role_sample(&records, &BTreeSet::new(), usize::MAX)
        .into_iter()
        .map(|r| r.match_id.clone())
        .collect();
    let cached: BTreeSet<_> = database
        .role_evidence(account_id)?
        .into_iter()
        .map(|e| e.match_id)
        .filter(|id| eligible.contains(id))
        .collect();
    let sample = role_sample(&records, &cached, request_budget);
    let mut progress = RoleSyncProgress {
        account_id,
        status: "running".into(),
        eligible_matches: eligible.len(),
        cached_matches: cached.len(),
        request_budget,
        ..Default::default()
    };
    let key = format!("role_sync:{account_id}");
    database.set_setting(&key, &progress)?;
    on_progress(&progress);
    for record in sample {
        if cancel.load(Ordering::Relaxed) {
            progress.status = "paused".into();
            break;
        }
        let request = client.role_evidence(record);
        tokio::pin!(request);
        let response = loop {
            tokio::select! {
                value=&mut request=>break Some(value),
                _=tokio::time::sleep(Duration::from_millis(200))=> {
                    if cancel.load(Ordering::Relaxed) { break None; }
                }
            }
        };
        match response {
            None => {
                progress.status = "paused".into();
                break;
            }
            Some(Ok(evidence)) => {
                database.save_role_evidence(&evidence)?;
                progress.fetched += 1;
                progress.cached_matches += 1;
            }
            Some(Err(error)) => {
                progress.failed += 1;
                progress.error = Some(error.to_string());
                if matches!(error, ProviderError::RateLimited { .. }) {
                    progress.status = "rate_limited".into();
                    break;
                }
                if progress.failed >= 3 {
                    progress.status = "failed".into();
                    break;
                }
            }
        }
        database.set_setting(&key, &progress)?;
        on_progress(&progress);
    }
    if progress.status == "running" {
        progress.status = if progress.cached_matches >= progress.eligible_matches {
            "completed"
        } else {
            "partial"
        }
        .into();
    }
    database.set_setting(&key, &progress)?;
    on_progress(&progress);
    Ok(progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(id: &str) -> MatchRecord {
        MatchRecord {
            match_id: id.into(),
            account_id: 1,
            hero_id: 1,
            start_time: 100,
            duration: Some(100),
            game_mode: Some(22),
            ..Default::default()
        }
    }

    #[test]
    fn sampling_zero_budget_never_issues_requests() {
        assert!(role_sample(&[row("1"), row("2")], &BTreeSet::new(), 0).is_empty());
    }

    #[test]
    fn sampling_deduplicates_and_excludes_invalid_or_unfinished_history() {
        let first = row("1");
        let mut invalid_time = row("2");
        invalid_time.start_time = -100;
        let mut invalid_hero = row("3");
        invalid_hero.hero_id = 0;
        let mut unfinished = row("4");
        unfinished.start_time = chrono::Utc::now().timestamp();
        let mut unknown_duration = row("5");
        unknown_duration.duration = None;
        let rows = vec![
            first.clone(),
            first,
            invalid_time,
            invalid_hero,
            unfinished,
            unknown_duration,
        ];
        let sample = role_sample(&rows, &BTreeSet::new(), 20);
        assert_eq!(
            sample
                .iter()
                .map(|r| r.match_id.as_str())
                .collect::<Vec<_>>(),
            vec!["1"]
        );
    }

    #[test]
    fn sampling_is_deterministic_independent_of_input_order() {
        let rows: Vec<_> = (1..=10)
            .map(|id| {
                let mut r = row(&id.to_string());
                r.hero_id = (id % 3) + 1;
                r.start_time = 100 + i64::from(id);
                r
            })
            .collect();
        let expected: Vec<_> = role_sample(&rows, &BTreeSet::new(), 5)
            .iter()
            .map(|r| r.match_id.clone())
            .collect();
        let mut reversed = rows.clone();
        reversed.reverse();
        let actual: Vec<_> = role_sample(&reversed, &BTreeSet::new(), 5)
            .iter()
            .map(|r| r.match_id.clone())
            .collect();
        assert_eq!(expected, actual);
        assert_eq!(expected[0], "10");
    }

    #[test]
    fn sampling_spreads_budget_and_skips_cached_future_and_special_modes() {
        let rows: Vec<_> = (0..30)
            .map(|i| MatchRecord {
                match_id: (i + 1).to_string(),
                account_id: 1,
                hero_id: if i < 20 { 1 } else { 2 },
                start_time: 100 - i,
                duration: Some(1),
                game_mode: Some(22),
                ..Default::default()
            })
            .collect();
        let cached = BTreeSet::from(["1".into()]);
        let sample = role_sample(&rows, &cached, 6);
        assert_eq!(sample.len(), 6);
        assert!(sample.iter().any(|r| r.hero_id == 2));
        assert!(sample.iter().all(|r| r.match_id != "1"));
        assert_eq!(
            sample
                .iter()
                .map(|r| &r.match_id)
                .collect::<BTreeSet<_>>()
                .len(),
            6
        );
        let mut excluded = rows[0].clone();
        excluded.game_mode = Some(23);
        assert!(role_sample(&[excluded], &BTreeSet::new(), 10).is_empty());
    }
}
