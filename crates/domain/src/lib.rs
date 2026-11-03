//! Shared domain values. IDs remain stable across data providers and IPC.
use serde::{Deserialize, Serialize};

mod roles;
pub use roles::{MatchRoleEvidence, PlayerRoleEvidence};

pub type HeroId = u32;
const STEAM64_BASE: u64 = 76_561_197_960_265_728;

#[derive(Debug, thiserror::Error, PartialEq)]
#[error("请输入有效的 Steam32 Account ID 或个人 Steam64 ID（纯数字）")]
pub struct InvalidAccountId;

/// Public account lookup is not Steam authentication.
pub fn normalize_account_id(input: &str) -> Result<u32, InvalidAccountId> {
    let input = input.trim();
    if input.is_empty() || !input.bytes().all(|c| c.is_ascii_digit()) {
        return Err(InvalidAccountId);
    }
    let value: u64 = input.parse().map_err(|_| InvalidAccountId)?;
    let account = if value > u32::MAX as u64 {
        value.checked_sub(STEAM64_BASE).ok_or(InvalidAccountId)?
    } else {
        value
    };
    let account = u32::try_from(account).map_err(|_| InvalidAccountId)?;
    if account == 0 || account == u32::MAX {
        return Err(InvalidAccountId);
    }
    Ok(account)
}

pub fn steam64(account: u32) -> String {
    (STEAM64_BASE + u64::from(account)).to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hero {
    pub id: HeroId,
    pub name: String,
    pub localized_name: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MatchRecord {
    pub match_id: String,
    pub account_id: u32,
    pub hero_id: HeroId,
    pub start_time: i64,
    pub duration: Option<u32>,
    pub game_mode: Option<u32>,
    pub patch: Option<u32>,
    pub player_slot: Option<u8>,
    pub radiant_win: Option<bool>,
    pub kills: Option<u32>,
    pub deaths: Option<u32>,
    pub assists: Option<u32>,
    pub last_hits: Option<u32>,
    pub denies: Option<u32>,
    pub gold_per_min: Option<u32>,
    pub xp_per_min: Option<u32>,
    /// Provider lane label; deliberately NOT the five-position role.
    pub lane_role: Option<u8>,
    #[serde(default)]
    pub team_heroes: Vec<HeroId>,
    #[serde(default)]
    pub enemy_heroes: Vec<HeroId>,
    pub lineup_complete: bool,
    pub source: String,
    pub fetched_at: i64,
}

impl MatchRecord {
    pub fn won(&self) -> Option<bool> {
        let slot = self.player_slot?;
        if !(slot <= 4 || (128..=132).contains(&slot)) {
            return None;
        }
        Some(self.radiant_win? == (slot < 128))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn steam_identifiers_are_exact() {
        assert_eq!(normalize_account_id("70388657"), Ok(70388657));
        assert_eq!(normalize_account_id(&steam64(70388657)), Ok(70388657));
        for input in [
            "0",
            "4294967295",
            "-1",
            "1e9",
            "76561197960265728",
            "99999999999999999999",
            "https://steamcommunity.com/id/name",
        ] {
            assert!(normalize_account_id(input).is_err(), "{input}");
        }
        let encoded = serde_json::to_string(&steam64(70388657)).unwrap();
        assert!(encoded.starts_with('"'));
    }
    #[test]
    fn result_requires_valid_slot_and_outcome() {
        let mut m = MatchRecord {
            player_slot: Some(128),
            radiant_win: Some(false),
            ..Default::default()
        };
        assert_eq!(m.won(), Some(true));
        m.player_slot = Some(5);
        assert_eq!(m.won(), None);
        m.player_slot = Some(0);
        m.radiant_win = None;
        assert_eq!(m.won(), None);
    }
}
