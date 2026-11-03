//! Sanitized, account-scoped observations used by the position heuristic.
//! No other player's account ID is needed or retained.
use crate::HeroId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlayerRoleEvidence {
    pub player_slot: u8,
    pub hero_id: HeroId,
    pub lane_role: Option<u8>,
    pub gold_per_min: Option<u32>,
    pub last_hits: Option<u32>,
    /// Mean of valid gold_t samples at minutes 10, 11 and 12, not final gold.
    pub early_gold: Option<f64>,
    /// Mean of valid lh_t samples at minutes 10, 11 and 12.
    pub early_last_hits: Option<f64>,
    /// Upstream estimated position; never treated as an observed ground truth.
    pub position_est: Option<u8>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MatchRoleEvidence {
    pub account_id: u32,
    pub match_id: String,
    pub source: String,
    pub fetched_at: i64,
    pub players: Vec<PlayerRoleEvidence>,
}
