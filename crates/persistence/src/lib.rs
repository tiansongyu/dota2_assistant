//! Local storage with transactional migrations. Missing data remains SQL NULL.
use chrono::Utc;
use dpda_domain::{steam64, MatchRecord, MatchRoleEvidence};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, path::Path, time::Duration};

const SCHEMA_VERSION: u32 = 2;
const MATCH_COLUMNS: &str = "match_id,account_id,hero_id,start_time,duration,game_mode,patch,player_slot,radiant_win,kills,deaths,assists,last_hits,denies,gold_per_min,xp_per_min,lane_role,lineup_complete,source,fetched_at";

#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("SQLite 操作失败：{0}")]
    Sql(#[from] rusqlite::Error),
    #[error("配置数据格式无效：{0}")]
    Json(#[from] serde_json::Error),
    #[error("数据库版本 {found} 高于当前支持版本 {supported}，请使用更新的软件")]
    FutureSchema { found: u32, supported: u32 },
    #[error("数据库完整性检查失败：{0}")]
    Integrity(String),
    #[error("无效的历史记录：{0}")]
    InvalidRecord(String),
}
pub type Result<T> = std::result::Result<T, PersistenceError>;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriteCounts {
    pub inserted: u64,
    pub updated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SyncProgress {
    pub account_id: u32,
    pub status: String,
    pub offset: u32,
    pub pages_fetched: u32,
    pub imported: u64,
    pub updated: u64,
    pub failed: u64,
    pub missing_lineups: u64,
    pub from_time: i64,
    pub to_time: i64,
    pub last_synced_at: Option<i64>,
    pub error: Option<String>,
}

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }
    fn from_connection(mut connection: Connection) -> Result<Self> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        let check: String = connection.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(PersistenceError::Integrity(check));
        }
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(PersistenceError::FutureSchema {
                found: version,
                supported: SCHEMA_VERSION,
            });
        }
        if version < 1 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(include_str!("../migrations/001_initial.sql"))?;
            transaction.pragma_update(None, "user_version", 1)?;
            transaction.commit()?;
        }
        if version < 2 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(include_str!("../migrations/002_role_evidence.sql"))?;
            transaction.pragma_update(None, "user_version", 2)?;
            transaction.commit()?;
        }
        Ok(Self { connection })
    }
    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }
    pub fn ensure_profile(&self, account_id: u32) -> Result<()> {
        ensure_profile(&self.connection, account_id)
    }
    pub fn profiles(&self) -> Result<Vec<u32>> {
        let mut query = self
            .connection
            .prepare("SELECT account_id FROM profiles ORDER BY created_at DESC")?;
        let profiles = query
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(profiles)
    }
    pub fn records(&self, account_id: u32) -> Result<Vec<MatchRecord>> {
        let mut query=self.connection.prepare(&format!("SELECT {MATCH_COLUMNS} FROM matches WHERE account_id=?1 ORDER BY start_time DESC,match_id DESC"))?;
        let mut records = query
            .query_map([account_id], row_match)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if records.is_empty() {
            return Ok(records);
        }
        // Fetch all lineups in one indexed query instead of one query per match.
        // The main result's chronological order remains untouched.
        let indices: HashMap<String, usize> = records
            .iter()
            .enumerate()
            .map(|(index, record)| (record.match_id.clone(), index))
            .collect();
        let mut lineups=self.connection.prepare("SELECT match_id,side,hero_id FROM match_heroes WHERE account_id=?1 ORDER BY match_id,hero_id")?;
        let rows = lineups.query_map([account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
            ))
        })?;
        for row in rows {
            let (match_id, side, hero_id) = row?;
            if let Some(index) = indices.get(&match_id) {
                if side == "ally" {
                    records[*index].team_heroes.push(hero_id);
                } else {
                    records[*index].enemy_heroes.push(hero_id);
                }
            }
        }
        Ok(records)
    }
    pub fn count(&self, account_id: u32) -> Result<u64> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM matches WHERE account_id=?1",
            [account_id],
            |row| read_count(row, 0),
        )?)
    }
    pub fn role_evidence(&self, account_id: u32) -> Result<Vec<MatchRoleEvidence>> {
        let mut query = self.connection.prepare(
            "SELECT evidence_json FROM role_evidence WHERE account_id=?1 ORDER BY match_id",
        )?;
        let rows = query
            .query_map([account_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| Ok(serde_json::from_str(&row)?))
            .collect()
    }
    pub fn save_role_evidence(&self, evidence: &MatchRoleEvidence) -> Result<()> {
        self.connection.execute(
            "INSERT INTO role_evidence(account_id,match_id,evidence_json,fetched_at) VALUES(?1,?2,?3,?4) ON CONFLICT(account_id,match_id) DO UPDATE SET evidence_json=excluded.evidence_json,fetched_at=excluded.fetched_at",
            params![evidence.account_id,evidence.match_id,serde_json::to_string(evidence)?,evidence.fetched_at],
        )?;
        Ok(())
    }
    pub fn missing_lineups(&self, account_id: u32) -> Result<u64> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM matches WHERE account_id=?1 AND lineup_complete=0",
            [account_id],
            |row| read_count(row, 0),
        )?)
    }
    pub fn upsert_records(&mut self, records: &[MatchRecord]) -> Result<WriteCounts> {
        let transaction = self.connection.transaction()?;
        let counts = upsert_records(&transaction, records)?;
        transaction.commit()?;
        Ok(counts)
    }
    /// Records, failed-row metadata and checkpoint either all commit or all roll back.
    pub fn commit_page(
        &mut self,
        records: &[MatchRecord],
        progress: &mut SyncProgress,
        failures: &[(String, String)],
    ) -> Result<WriteCounts> {
        let transaction = self.connection.transaction()?;
        ensure_profile(&transaction, progress.account_id)?;
        if records
            .iter()
            .any(|record| record.account_id != progress.account_id)
        {
            return Err(PersistenceError::InvalidRecord("分页包含其他账号".into()));
        }
        let counts = upsert_records(&transaction, records)?;
        for (key, reason) in failures {
            transaction.execute("INSERT INTO import_failures(account_id,record_key,reason,observed_at) VALUES(?1,?2,?3,?4) ON CONFLICT(account_id,record_key) DO UPDATE SET reason=excluded.reason,observed_at=excluded.observed_at",params![progress.account_id,key,reason,Utc::now().timestamp()])?;
        }
        let mut next = progress.clone();
        next.imported += counts.inserted;
        next.updated += counts.updated;
        next.failed = transaction.query_row(
            "SELECT COUNT(*) FROM import_failures WHERE account_id=?1",
            [next.account_id],
            |row| read_count(row, 0),
        )?;
        next.missing_lineups = transaction.query_row(
            "SELECT COUNT(*) FROM matches WHERE account_id=?1 AND lineup_complete=0",
            [next.account_id],
            |row| read_count(row, 0),
        )?;
        save_checkpoint(&transaction, &next)?;
        transaction.commit()?;
        *progress = next;
        Ok(counts)
    }
    pub fn checkpoint(&self, account_id: u32) -> Result<Option<SyncProgress>> {
        Ok(self.connection.query_row("SELECT account_id,status,offset,pages_fetched,imported,updated,failed,missing_lineups,from_time,to_time,last_synced_at,error FROM sync_jobs WHERE account_id=?1",[account_id],|row|Ok(SyncProgress{account_id:row.get(0)?,status:row.get(1)?,offset:row.get(2)?,pages_fetched:row.get(3)?,imported:read_count(row,4)?,updated:read_count(row,5)?,failed:read_count(row,6)?,missing_lineups:read_count(row,7)?,from_time:row.get(8)?,to_time:row.get(9)?,last_synced_at:row.get(10)?,error:row.get(11)?})).optional()?)
    }
    pub fn save_checkpoint(&self, progress: &SyncProgress) -> Result<()> {
        ensure_profile(&self.connection, progress.account_id)?;
        save_checkpoint(&self.connection, progress)
    }
    pub fn setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let value: Option<String> = self
            .connection
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        value
            .map(|text| serde_json::from_str(&text).map_err(Into::into))
            .transpose()
    }
    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let json = serde_json::to_string(value)?;
        self.connection.execute("INSERT INTO settings(key,value,updated_at) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",params![key,json,Utc::now().timestamp()])?;
        Ok(())
    }
    pub fn remove_setting(&self, key: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM settings WHERE key=?1", [key])?;
        Ok(())
    }
}

fn ensure_profile(connection: &Connection, account_id: u32) -> Result<()> {
    if account_id == 0 || account_id == u32::MAX {
        return Err(PersistenceError::InvalidRecord("Account ID 无效".into()));
    }
    connection.execute(
        "INSERT OR IGNORE INTO profiles(account_id,steam64,created_at) VALUES(?1,?2,?3)",
        params![account_id, steam64(account_id), Utc::now().timestamp()],
    )?;
    Ok(())
}
fn row_match(row: &rusqlite::Row<'_>) -> rusqlite::Result<MatchRecord> {
    Ok(MatchRecord {
        match_id: row.get(0)?,
        account_id: row.get(1)?,
        hero_id: row.get(2)?,
        start_time: row.get(3)?,
        duration: row.get(4)?,
        game_mode: row.get(5)?,
        patch: row.get(6)?,
        player_slot: row.get(7)?,
        radiant_win: row.get(8)?,
        kills: row.get(9)?,
        deaths: row.get(10)?,
        assists: row.get(11)?,
        last_hits: row.get(12)?,
        denies: row.get(13)?,
        gold_per_min: row.get(14)?,
        xp_per_min: row.get(15)?,
        lane_role: row.get(16)?,
        lineup_complete: row.get(17)?,
        source: row.get(18)?,
        fetched_at: row.get(19)?,
        team_heroes: Vec::new(),
        enemy_heroes: Vec::new(),
    })
}
fn load_lineup(connection: &Connection, record: &mut MatchRecord) -> Result<()> {
    let mut query=connection.prepare("SELECT side,hero_id FROM match_heroes WHERE account_id=?1 AND match_id=?2 ORDER BY hero_id")?;
    let rows = query.query_map(params![record.account_id, record.match_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
    })?;
    for row in rows {
        let (side, hero) = row?;
        if side == "ally" {
            record.team_heroes.push(hero);
        } else {
            record.enemy_heroes.push(hero);
        }
    }
    Ok(())
}
fn upsert_records(connection: &Connection, records: &[MatchRecord]) -> Result<WriteCounts> {
    let mut counts = WriteCounts::default();
    for incoming in records {
        if incoming.hero_id == 0
            || incoming.start_time < 0
            || incoming
                .match_id
                .parse::<u64>()
                .ok()
                .filter(|id| *id > 0)
                .is_none()
        {
            return Err(PersistenceError::InvalidRecord(format!(
                "比赛 {} 的必填字段无效",
                incoming.match_id
            )));
        }
        ensure_profile(connection, incoming.account_id)?;
        let mut existing = connection
            .query_row(
                &format!("SELECT {MATCH_COLUMNS} FROM matches WHERE account_id=?1 AND match_id=?2"),
                params![incoming.account_id, incoming.match_id],
                row_match,
            )
            .optional()?;
        if let Some(record) = existing.as_mut() {
            load_lineup(connection, record)?;
        }
        let mut record = incoming.clone();
        record.team_heroes.sort_unstable();
        record.team_heroes.dedup();
        record.enemy_heroes.sort_unstable();
        record.enemy_heroes.dedup();
        if record.lineup_complete
            && (record.team_heroes.len() != 4 || record.enemy_heroes.len() != 5)
        {
            return Err(PersistenceError::InvalidRecord(
                "完整阵容必须包含4位队友和5位敌方英雄".into(),
            ));
        }
        if let Some(old) = &existing {
            macro_rules! fill {($($field:ident),*)=>{$(record.$field=record.$field.or(old.$field);)*};}
            fill!(
                duration,
                game_mode,
                patch,
                player_slot,
                radiant_win,
                kills,
                deaths,
                assists,
                last_hits,
                denies,
                gold_per_min,
                xp_per_min,
                lane_role
            );
            if old.lineup_complete && !record.lineup_complete {
                record.team_heroes = old.team_heroes.clone();
                record.enemy_heroes = old.enemy_heroes.clone();
                record.lineup_complete = true;
            }
            // Re-fetching unchanged facts does not inflate the updated counter.
            let mut comparable = record.clone();
            comparable.fetched_at = old.fetched_at;
            if &comparable != old {
                counts.updated += 1;
            }
        } else {
            counts.inserted += 1;
        }
        connection.execute("INSERT INTO matches(account_id,match_id,hero_id,start_time,duration,game_mode,patch,player_slot,radiant_win,kills,deaths,assists,last_hits,denies,gold_per_min,xp_per_min,lane_role,lineup_complete,source,fetched_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20) ON CONFLICT(account_id,match_id) DO UPDATE SET hero_id=excluded.hero_id,start_time=excluded.start_time,duration=excluded.duration,game_mode=excluded.game_mode,patch=excluded.patch,player_slot=excluded.player_slot,radiant_win=excluded.radiant_win,kills=excluded.kills,deaths=excluded.deaths,assists=excluded.assists,last_hits=excluded.last_hits,denies=excluded.denies,gold_per_min=excluded.gold_per_min,xp_per_min=excluded.xp_per_min,lane_role=excluded.lane_role,lineup_complete=excluded.lineup_complete,source=excluded.source,fetched_at=excluded.fetched_at",params![record.account_id,record.match_id,record.hero_id,record.start_time,record.duration,record.game_mode,record.patch,record.player_slot,record.radiant_win,record.kills,record.deaths,record.assists,record.last_hits,record.denies,record.gold_per_min,record.xp_per_min,record.lane_role,record.lineup_complete,record.source,record.fetched_at])?;
        connection.execute(
            "DELETE FROM match_heroes WHERE account_id=?1 AND match_id=?2",
            params![record.account_id, record.match_id],
        )?;
        for (side, heroes) in [
            ("ally", &record.team_heroes),
            ("enemy", &record.enemy_heroes),
        ] {
            for hero in heroes {
                connection.execute("INSERT INTO match_heroes(account_id,match_id,side,hero_id) VALUES(?1,?2,?3,?4)",params![record.account_id,record.match_id,side,hero])?;
            }
        }
        connection.execute(
            "DELETE FROM import_failures WHERE account_id=?1 AND record_key=?2",
            params![record.account_id, record.match_id],
        )?;
    }
    Ok(counts)
}
fn read_count(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
fn write_count(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| PersistenceError::InvalidRecord("统计计数超出SQLite整数范围".into()))
}
fn save_checkpoint(connection: &Connection, p: &SyncProgress) -> Result<()> {
    connection.execute("INSERT INTO sync_jobs(account_id,status,offset,pages_fetched,imported,updated,failed,missing_lineups,from_time,to_time,last_synced_at,error) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(account_id) DO UPDATE SET status=excluded.status,offset=excluded.offset,pages_fetched=excluded.pages_fetched,imported=excluded.imported,updated=excluded.updated,failed=excluded.failed,missing_lineups=excluded.missing_lineups,from_time=excluded.from_time,to_time=excluded.to_time,last_synced_at=excluded.last_synced_at,error=excluded.error",params![p.account_id,p.status,p.offset,p.pages_fetched,write_count(p.imported)?,write_count(p.updated)?,write_count(p.failed)?,write_count(p.missing_lineups)?,p.from_time,p.to_time,p.last_synced_at,p.error])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> MatchRecord {
        MatchRecord {
            match_id: "9007199254740993".into(),
            account_id: 1,
            hero_id: 2,
            start_time: 100,
            player_slot: Some(128),
            radiant_win: Some(false),
            source: "test-synthetic".into(),
            ..Default::default()
        }
    }
    #[test]
    fn migrations_are_repeatable_and_ids_remain_exact() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("db.sqlite");
        let mut db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 2);
        assert_eq!(db.upsert_records(&[record()]).unwrap().inserted, 1);
        drop(db);
        let db = Database::open(&path).unwrap();
        let records = db.records(1).unwrap();
        assert_eq!(records[0].match_id, "9007199254740993");
        assert_eq!(records[0].kills, None);
    }
    #[test]
    fn schema_one_upgrades_without_losing_history_and_scopes_role_cache() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("v1.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../migrations/001_initial.sql"))
            .unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        ensure_profile(&connection, 1).unwrap();
        upsert_records(&connection, &[record()]).unwrap();
        drop(connection);
        let database = Database::open(&path).unwrap();
        assert_eq!(database.schema_version().unwrap(), 2);
        assert_eq!(database.records(1).unwrap(), vec![record()]);
        let evidence = MatchRoleEvidence {
            account_id: 1,
            match_id: record().match_id,
            source: "synthetic".into(),
            fetched_at: 200,
            players: vec![],
        };
        database.save_role_evidence(&evidence).unwrap();
        assert_eq!(database.role_evidence(1).unwrap(), vec![evidence.clone()]);
        assert!(database.role_evidence(2).unwrap().is_empty());
        let mut invalid = evidence;
        invalid.account_id = 2;
        assert!(database.save_role_evidence(&invalid).is_err());
    }
    #[test]
    fn duplicates_and_missing_refreshes_do_not_destroy_facts() {
        let mut db = Database::in_memory().unwrap();
        let mut m = record();
        m.kills = Some(5);
        db.upsert_records(&[m.clone()]).unwrap();
        assert_eq!(db.upsert_records(&[m]).unwrap(), WriteCounts::default());
        db.upsert_records(&[record()]).unwrap();
        assert_eq!(db.records(1).unwrap()[0].kills, Some(5));
        assert_eq!(db.count(1).unwrap(), 1);
    }
    #[test]
    fn invalid_page_rolls_back_all_records() {
        let mut db = Database::in_memory().unwrap();
        let mut bad = record();
        bad.hero_id = 0;
        assert!(db.upsert_records(&[record(), bad]).is_err());
        assert_eq!(db.count(1).unwrap(), 0);
    }
    #[test]
    fn settings_and_future_schema_are_checked() {
        let db = Database::in_memory().unwrap();
        db.set_setting("theme", &"dark").unwrap();
        assert_eq!(
            db.setting::<String>("theme").unwrap().as_deref(),
            Some("dark")
        );
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 3).unwrap();
        assert!(matches!(
            Database::from_connection(connection),
            Err(PersistenceError::FutureSchema { .. })
        ));
    }
    #[test]
    fn bulk_lineups_preserve_match_order_and_account_scope() {
        let mut db = Database::in_memory().unwrap();
        let mut early = record();
        early.match_id = "10".into();
        early.start_time = 100;
        early.team_heroes = vec![9, 3];
        early.enemy_heroes = vec![8];
        let mut late = record();
        late.match_id = "11".into();
        late.start_time = 200;
        late.team_heroes = vec![7];
        late.enemy_heroes = vec![11, 6];
        let mut other = early.clone();
        other.account_id = 2;
        other.team_heroes = vec![99];
        db.upsert_records(&[early, late, other]).unwrap();
        let rows = db.records(1).unwrap();
        assert_eq!(
            rows.iter().map(|m| m.match_id.as_str()).collect::<Vec<_>>(),
            vec!["11", "10"]
        );
        assert_eq!(rows[0].team_heroes, vec![7]);
        assert_eq!(rows[0].enemy_heroes, vec![6, 11]);
        assert_eq!(rows[1].team_heroes, vec![3, 9]);
        assert_eq!(rows[1].enemy_heroes, vec![8]);
        assert_eq!(db.records(2).unwrap()[0].team_heroes, vec![99]);
    }
}
