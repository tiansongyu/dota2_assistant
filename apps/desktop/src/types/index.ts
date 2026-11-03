/** IPC v1. Stable game IDs; Steam64 and match IDs are always decimal strings. */
export interface Hero {
  id: number;
  name: string;
  localized_name: string;
  roles: string[];
}
export interface MatchRecord {
  match_id: string;
  account_id: number;
  hero_id: number;
  start_time: number;
  duration: number | null;
  game_mode: number | null;
  patch: number | null;
  player_slot: number | null;
  radiant_win: boolean | null;
  kills: number | null;
  deaths: number | null;
  assists: number | null;
  last_hits: number | null;
  denies: number | null;
  gold_per_min: number | null;
  xp_per_min: number | null;
  lane_role: number | null;
  team_heroes: number[];
  enemy_heroes: number[];
  lineup_complete: boolean;
  source: string;
  fetched_at: number;
}
export interface StatsWindow {
  matches: number;
  outcome_samples: number;
  wins: number;
  win_rate: number | null;
}
export interface HeroStats {
  hero_id: number;
  matches: number;
  outcome_samples: number;
  wins: number;
  losses: number;
  win_rate: number | null;
  smoothed_win_rate: number;
  confidence_low: number;
  confidence_high: number;
  recent20: StatsWindow;
  recent50: StatsWindow;
  days90: StatsWindow;
  days180: StatsWindow;
  year: StatsWindow;
  current_patch: StatsWindow;
  weighted_win_rate: number;
  effective_samples: number;
  avg_kda: number | null;
  avg_gpm: number | null;
  avg_xpm: number | null;
  lineup_coverage: number;
}
export interface PoolConfig {
  mode: 'auto' | 'manual' | 'hybrid';
  included: number[];
  excluded: number[];
  /** Overrides: absent key uses history inference; [] explicitly permits no positions. */
  roles: Record<string, number[]>;
  priorities: Record<string, number>;
}
export interface Draft {
  radiant: (number | null)[];
  dire: (number | null)[];
  own_team: 'radiant' | 'dire';
  banned: number[];
}
export interface Recommendation {
  hero_id: number;
  score: number;
  sample_size: number;
  confidence: string;
  reasons: string[];
  personal_win_rate: number | null;
  role_candidates: number[];
}
export interface RoleSupport {
  role: number;
  supporting_matches: number;
  exclusive_matches: number;
  early_matches: number;
  late_matches: number;
}
export interface HeroRoleAnalysis {
  hero_id: number;
  matches: number;
  analyzed_matches: number;
  unknown_matches: number;
  lane_only_matches: number;
  coverage: number;
  candidates: RoleSupport[];
  suggested_roles: number[];
}
export interface MatchRoleInference {
  match_id: string;
  hero_id: number;
  start_time: number;
  candidates: number[];
  strength: 'unknown' | 'limited' | 'moderate';
  method: string;
  qualifying_evidence: boolean;
  reasons: string[];
}
export interface RoleAnalysis {
  rule_version: string;
  account_id: number | null;
  total_matches: number;
  eligible_matches: number;
  detail_matches: number;
  analyzed_matches: number;
  unknown_matches: number;
  heroes: HeroRoleAnalysis[];
  matches: MatchRoleInference[];
  warnings: string[];
}
export interface RoleSyncProgress {
  account_id: number;
  status: string;
  eligible_matches: number;
  cached_matches: number;
  fetched: number;
  failed: number;
  request_budget: number;
  error: string | null;
}
export interface Profile {
  account_id: number;
  steam64: string;
}
export interface SyncProgress {
  account_id: number | null;
  status: string;
  offset: number;
  pages_fetched: number;
  imported: number;
  updated: number;
  failed: number;
  missing_lineups: number;
  from_time: number | null;
  to_time: number | null;
  last_synced_at: number | null;
  error: string | null;
}
export interface CaptureStatus {
  supported: boolean;
  status: string;
  message: string;
  window_title: string | null;
  capture_ms: number | null;
  recognition_ms: number | null;
  recommendation_ms: number | null;
}
export interface AppSettings {
  theme: 'dark' | 'light';
  selected_role: number;
  half_life_days: number;
  top_k: number;
  weights: { personal: number; counter: number; synergy: number; patch: number };
  overlay_opacity: number;
  overlay_compact: boolean;
  overlay_click_through: boolean;
  capture_hz: number;
  /** Versioned profile JSON, validated by the capture service before use. */
  calibration_json: string;
}
export interface AppSnapshot {
  schema_version: 1;
  profile: Profile | null;
  heroes: Hero[];
  hero_stats: HeroStats[];
  automatic_pool: number[];
  pool_candidates: number[];
  matches: MatchRecord[];
  pool: PoolConfig;
  effective_pool: PoolConfig;
  position_analysis: RoleAnalysis;
  role_sync: RoleSyncProgress | null;
  draft: Draft;
  recommendations: Recommendation[];
  sync: SyncProgress;
  capture: CaptureStatus;
  settings: AppSettings;
  data_updated_at: number | null;
}
export type Page = 'dashboard' | 'pool' | 'history' | 'draft' | 'assets' | 'settings';
export interface SlotDiagnosticState {
  slot_id: number;
  hero_id: number | null;
  confidence: number;
  recognition_state: 'EMPTY' | 'CANDIDATE' | 'CONFIRMED' | 'UNCERTAIN' | 'INVALID';
  timestamp: number;
  source: 'none' | 'visual_marker' | 'manual';
}
export interface CaptureDiagnostics {
  draft: {
    radiant: SlotDiagnosticState[];
    dire: SlotDiagnosticState[];
    draft_screen_detected: boolean;
    revision: number;
  };
  analysis: {
    slots: {
      slot_id: number;
      candidates: { hero_id: number; similarity: number }[];
      locked_marker_matched: boolean;
      empty_marker_matched: boolean;
    }[];
    elapsed_ms: number;
    reused_pixels: boolean;
    timestamp_ms: number;
    draft_similarity: number;
  } | null;
  status: CaptureStatus;
}
