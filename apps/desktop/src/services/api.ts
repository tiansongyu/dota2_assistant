import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import heroes from '../../../../assets/heroes.json';
import type { AppSettings, AppSnapshot, CaptureDiagnostics, Draft, PoolConfig } from '../types';

export const desktopAvailable = isTauri();
export const emptySnapshot: AppSnapshot = {
  schema_version: 1,
  profile: null,
  heroes,
  hero_stats: [],
  automatic_pool: [],
  pool_candidates: [],
  matches: [],
  pool: { mode: 'auto', included: [], excluded: [], roles: {}, priorities: {} },
  effective_pool: { mode: 'auto', included: [], excluded: [], roles: {}, priorities: {} },
  position_analysis: {
    rule_version: '',
    account_id: null,
    total_matches: 0,
    eligible_matches: 0,
    detail_matches: 0,
    analyzed_matches: 0,
    unknown_matches: 0,
    heroes: [],
    matches: [],
    warnings: [],
  },
  role_sync: null,
  draft: {
    radiant: Array(5).fill(null),
    dire: Array(5).fill(null),
    own_team: 'radiant',
    banned: [],
  },
  recommendations: [],
  sync: {
    account_id: null,
    status: 'idle',
    offset: 0,
    pages_fetched: 0,
    imported: 0,
    updated: 0,
    failed: 0,
    missing_lineups: 0,
    from_time: null,
    to_time: null,
    last_synced_at: null,
    error: null,
  },
  capture: {
    supported: false,
    status: 'unavailable',
    message: '启动 Windows 桌面应用后检测游戏窗口。',
    window_title: null,
    capture_ms: null,
    recognition_ms: null,
    recommendation_ms: null,
  },
  settings: {
    theme: 'dark',
    selected_role: 0,
    half_life_days: 180,
    top_k: 5,
    weights: { personal: 0.4, counter: 0.3, synergy: 0.2, patch: 0.1 },
    overlay_opacity: 0.95,
    overlay_compact: true,
    overlay_click_through: false,
    capture_hz: 1,
    calibration_json: '',
  },
  data_updated_at: null,
};

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!desktopAvailable)
    throw new Error('当前为浏览器界面预览。请通过 Tauri 桌面应用保存配置和同步真实战绩。');
  return invoke<T>(command, args);
}
export const api = {
  snapshot: () => call<AppSnapshot>('get_snapshot'),
  saveAccount: (input: string) => call<void>('save_account', { input }),
  startSync: (fromTime: number, toTime: number) => call<void>('start_sync', { fromTime, toTime }),
  pauseSync: () => call<void>('pause_sync'),
  refreshRoles: () => call<void>('start_role_analysis'),
  savePool: (pool: PoolConfig) => call<void>('save_pool', { pool }),
  updateDraft: (draft: Draft) => call<void>('update_draft', { draft }),
  saveSettings: (settings: AppSettings) => call<void>('save_settings', { settings }),
  openOverlay: () => call<void>('open_overlay'),
  startCapture: () => call<void>('start_capture'),
  stopCapture: () => call<void>('stop_capture'),
  captureDiagnostics: () => call<CaptureDiagnostics>('get_capture_diagnostics'),
};
export async function listenForUpdates(update: () => void): Promise<() => void> {
  if (!desktopAvailable) return () => {};
  return listen('dpda://state-updated', update);
}
