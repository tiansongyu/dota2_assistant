import type { Hero, MatchRecord } from '../types';

export const roles = [
  { value: 1, label: '一号位', short: '核心' },
  { value: 2, label: '二号位', short: '中单' },
  { value: 3, label: '三号位', short: '劣单' },
  { value: 4, label: '四号位', short: '辅助' },
  { value: 5, label: '五号位', short: '辅助' },
];
export const positionNames = (positions: number[]) =>
  positions
    .map((position) => roles.find((role) => role.value === position)?.label ?? `位置 ${position}`)
    .join(' / ');
export const percent = (value: number | null | undefined) =>
  value == null ? '—' : `${(value * 100).toFixed(1)}%`;
export const decimal = (value: number | null | undefined, digits = 1) =>
  value == null ? '—' : value.toFixed(digits);
export const heroName = (heroes: Hero[], id: number | null) =>
  heroes.find((hero) => hero.id === id)?.localized_name ??
  (id == null ? '尚未选择' : `英雄 #${id}`);
export const date = (time: number | null) =>
  time == null ? '尚未同步' : new Date(time * 1000).toLocaleDateString('zh-CN');
export const dateTime = (time: number | null) =>
  time == null ? '尚未同步' : new Date(time * 1000).toLocaleString('zh-CN');
export function won(match: MatchRecord): boolean | null {
  const slot = match.player_slot;
  if (slot == null || match.radiant_win == null || !(slot <= 4 || (slot >= 128 && slot <= 132)))
    return null;
  return slot < 128 === match.radiant_win;
}
export function dateStart(input: string): number | null {
  if (!input) return null;
  const value = new Date(`${input}T00:00:00`).getTime() / 1000;
  return Number.isFinite(value) ? value : null;
}
export function dateEnd(input: string): number | null {
  if (!input) return null;
  const value = new Date(`${input}T23:59:59`).getTime() / 1000;
  return Number.isFinite(value) ? value : null;
}
