//! Resumable, idempotent historical imports; no network access during a draft.
mod role_enrichment;
use chrono::Utc;
use dpda_data_provider::{OpenDotaClient, PlayerPage, ProviderError};
pub use dpda_persistence::SyncProgress;
use dpda_persistence::{Database, PersistenceError};
pub use role_enrichment::{enrich_role_evidence, role_sample, RoleSyncProgress};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncOptions {
    pub account_id: u32,
    pub from_time: i64,
    pub to_time: i64,
    pub page_size: u32,
    /// An explicit per-run work budget; reaching it pauses without claiming completion.
    pub max_pages: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("同步参数无效：{0}")]
    InvalidOptions(String),
}

pub trait HistorySource {
    fn page(
        &self,
        account_id: u32,
        offset: u32,
        limit: u32,
    ) -> impl Future<Output = Result<PlayerPage, ProviderError>> + Send;
}
impl HistorySource for OpenDotaClient {
    async fn page(
        &self,
        account_id: u32,
        offset: u32,
        limit: u32,
    ) -> Result<PlayerPage, ProviderError> {
        self.player_matches(account_id, offset, limit).await
    }
}

pub async fn sync_history(
    client: &OpenDotaClient,
    db: &mut Database,
    options: SyncOptions,
    cancel: &AtomicBool,
    on_progress: impl FnMut(&SyncProgress),
) -> Result<SyncProgress, HistoryError> {
    sync_with_source(client, db, options, cancel, on_progress).await
}

/// Public for deterministic transport tests and future authorized providers.
pub async fn sync_with_source(
    source: &impl HistorySource,
    db: &mut Database,
    options: SyncOptions,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(&SyncProgress),
) -> Result<SyncProgress, HistoryError> {
    if options.account_id == 0
        || options.account_id == u32::MAX
        || options.from_time < 0
        || options.to_time < options.from_time
        || !(1..=500).contains(&options.page_size)
        || options.max_pages == Some(0)
    {
        return Err(HistoryError::InvalidOptions(
            "检查账号、起止日期和分页大小（1–500）".into(),
        ));
    }
    db.ensure_profile(options.account_id)?;
    let previous = db.checkpoint(options.account_id)?;
    let mut progress = previous
        .filter(|p| {
            p.from_time == options.from_time
                && p.to_time == options.to_time
                && p.status != "completed"
        })
        .unwrap_or(SyncProgress {
            account_id: options.account_id,
            status: "running".into(),
            offset: 0,
            pages_fetched: 0,
            imported: 0,
            updated: 0,
            failed: 0,
            missing_lineups: 0,
            from_time: options.from_time,
            to_time: options.to_time,
            last_synced_at: None,
            error: None,
        });
    // OpenDota offset is not a stable snapshot cursor. Re-scan from zero after a pause;
    // the database reuses existing rows and protects enriched data from null refreshes.
    progress.status = "running".into();
    progress.offset = 0;
    progress.error = None;
    db.save_checkpoint(&progress)?;
    on_progress(&progress);
    let mut pages_this_run = 0;
    let mut previous_fingerprint = String::new();
    loop {
        if cancel.load(Ordering::Relaxed)
            || options.max_pages.is_some_and(|max| pages_this_run >= max)
        {
            progress.status = "paused".into();
            db.save_checkpoint(&progress)?;
            on_progress(&progress);
            return Ok(progress);
        }
        let request = source.page(options.account_id, progress.offset, options.page_size);
        tokio::pin!(request);
        let response = loop {
            tokio::select! {
                response=&mut request=>break response,
                _=tokio::time::sleep(Duration::from_millis(200))=>{
                    if cancel.load(Ordering::Relaxed) {
                        progress.status="paused".into();db.save_checkpoint(&progress)?;on_progress(&progress);return Ok(progress);
                    }
                }
            }
        };
        let page = match response {
            Ok(page) => page,
            Err(error) => {
                progress.status = if matches!(error, ProviderError::RateLimited { .. }) {
                    "rate_limited"
                } else {
                    "failed"
                }
                .into();
                progress.error = Some(error.to_string());
                db.save_checkpoint(&progress)?;
                on_progress(&progress);
                tracing::warn!(account_id=options.account_id,error=%error,"history sync interrupted");
                return Err(error.into());
            }
        };
        if page.received == 0 {
            progress.status = "completed".into();
            progress.last_synced_at = Some(Utc::now().timestamp());
            if db.count(options.account_id)? == 0 {
                progress.error = Some(
                    "数据源未返回范围内记录；可能是隐私设置、账号无记录或数据源覆盖不足".into(),
                );
            }
            db.save_checkpoint(&progress)?;
            on_progress(&progress);
            return Ok(progress);
        }
        let fingerprint = page
            .matches
            .iter()
            .map(|record| record.match_id.as_str())
            .chain(
                page.rejected
                    .iter()
                    .filter_map(|record| record.match_id.as_deref()),
            )
            .collect::<Vec<_>>()
            .join(",");
        if !fingerprint.is_empty() && fingerprint == previous_fingerprint {
            let error = ProviderError::InvalidData(
                "连续分页返回相同比赛，已停止以避免无限请求；请稍后重试".into(),
            );
            progress.status = "failed".into();
            progress.error = Some(error.to_string());
            db.save_checkpoint(&progress)?;
            on_progress(&progress);
            return Err(error.into());
        }
        previous_fingerprint = fingerprint;
        let records = page
            .matches
            .into_iter()
            .filter(|record| {
                record.start_time >= options.from_time && record.start_time <= options.to_time
            })
            .collect::<Vec<_>>();
        let failures = page
            .rejected
            .into_iter()
            .enumerate()
            .map(|(index, rejected)| {
                (
                    rejected
                        .match_id
                        .unwrap_or_else(|| format!("offset:{}:row:{index}", progress.offset)),
                    rejected.reason,
                )
            })
            .collect::<Vec<_>>();
        progress.offset = progress
            .offset
            .checked_add(
                u32::try_from(page.received)
                    .map_err(|_| HistoryError::InvalidOptions("分页响应过大".into()))?,
            )
            .ok_or_else(|| HistoryError::InvalidOptions("分页 offset 溢出".into()))?;
        progress.pages_fetched = progress.pages_fetched.saturating_add(1);
        progress.last_synced_at = Some(Utc::now().timestamp());
        if let Err(error) = db.commit_page(&records, &mut progress, &failures) {
            // Restore the last committed offset before reporting a failed write.
            if let Some(committed) = db.checkpoint(options.account_id)? {
                progress = committed;
            }
            progress.status = "failed".into();
            progress.error = Some(error.to_string());
            db.save_checkpoint(&progress)?;
            on_progress(&progress);
            return Err(error.into());
        }
        pages_this_run += 1;
        on_progress(&progress);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dpda_data_provider::{parse_player_page, RateLimitInfo};
    use dpda_domain::MatchRecord;
    use std::sync::Mutex;
    struct FixtureSource {
        rows: Vec<MatchRecord>,
        requests: Mutex<Vec<u32>>,
    }
    impl HistorySource for FixtureSource {
        async fn page(&self, _: u32, offset: u32, limit: u32) -> Result<PlayerPage, ProviderError> {
            self.requests.lock().unwrap().push(offset);
            let matches = self
                .rows
                .iter()
                .skip(offset as usize)
                .take(limit as usize)
                .cloned()
                .collect::<Vec<_>>();
            Ok(PlayerPage {
                received: matches.len(),
                matches,
                rejected: Vec::new(),
                rate_limit: RateLimitInfo::default(),
            })
        }
    }
    fn fixture() -> FixtureSource {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/opendota/player-projected.json"
        ))
        .unwrap();
        FixtureSource {
            rows: parse_player_page(&rows, 70388657, 100).matches,
            requests: Mutex::new(Vec::new()),
        }
    }
    fn options() -> SyncOptions {
        SyncOptions {
            account_id: 70388657,
            from_time: 0,
            to_time: 2_000_000_000,
            page_size: 1,
            max_pages: None,
        }
    }
    #[tokio::test]
    async fn real_fixture_import_pause_resume_and_dedup() {
        let source = fixture();
        let mut db = Database::in_memory().unwrap();
        let cancel = AtomicBool::new(false);
        let mut limited = options();
        limited.max_pages = Some(1);
        let paused = sync_with_source(&source, &mut db, limited, &cancel, |_| {})
            .await
            .unwrap();
        assert_eq!(paused.status, "paused");
        assert_eq!(db.count(70388657).unwrap(), 1);
        let finished = sync_with_source(&source, &mut db, options(), &cancel, |_| {})
            .await
            .unwrap();
        assert_eq!(finished.status, "completed");
        assert_eq!(finished.imported, 2);
        assert_eq!(db.count(70388657).unwrap(), 2);
        assert_eq!(finished.missing_lineups, 0);
        let repeated = sync_with_source(&source, &mut db, options(), &cancel, |_| {})
            .await
            .unwrap();
        assert_eq!(repeated.imported, 0);
        assert_eq!(repeated.updated, 0);
        assert_eq!(
            source.requests.lock().unwrap()[1],
            0,
            "resume rechecks head to avoid offset drift"
        );
    }
    #[tokio::test]
    async fn date_range_is_inclusive_and_not_changed_by_resume() {
        let source = fixture();
        let mut db = Database::in_memory().unwrap();
        let mut opts = options();
        opts.from_time = 1793662065;
        opts.to_time = 1793662065;
        sync_with_source(&source, &mut db, opts, &AtomicBool::new(false), |_| {})
            .await
            .unwrap();
        assert_eq!(db.records(70388657).unwrap()[0].match_id, "9073857007");
        assert_eq!(db.count(70388657).unwrap(), 1);
    }
    #[tokio::test]
    async fn cancellation_does_not_advance_uncommitted_pages() {
        let source = fixture();
        let mut db = Database::in_memory().unwrap();
        let p = sync_with_source(&source, &mut db, options(), &AtomicBool::new(true), |_| {})
            .await
            .unwrap();
        assert_eq!(p.offset, 0);
        assert_eq!(p.status, "paused");
        assert!(source.requests.lock().unwrap().is_empty());
    }
}
