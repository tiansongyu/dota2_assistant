import type { ReactNode } from 'react';
import { Crosshair, Database, Swords } from 'lucide-react';
import type { Hero, MatchRecord } from '../types';
import { date, heroName, percent, roles, won } from '../services/format';

export function EmptyState({
  title,
  children,
  action,
  icon = 'data',
}: {
  title: string;
  children: ReactNode;
  action?: ReactNode;
  icon?: 'data' | 'draft' | 'capture';
}) {
  const Icon = icon === 'draft' ? Swords : icon === 'capture' ? Crosshair : Database;
  return (
    <div className="empty-state">
      <div className="empty-icon">
        <Icon size={27} aria-hidden="true" />
      </div>
      <h3>{title}</h3>
      <p>{children}</p>
      {action}
    </div>
  );
}
export function HeroMark({ hero, small = false }: { hero: Hero | undefined; small?: boolean }) {
  return (
    <div className={`hero-mark ${small ? 'small' : ''}`} aria-hidden="true">
      {hero?.localized_name
        .split(/[-\s]+/)
        .map((word) => word[0])
        .slice(0, 2)
        .join('') ?? '—'}
    </div>
  );
}
export function RoleSelect({
  value,
  onChange,
  disabled = false,
}: {
  value: number;
  onChange: (role: number) => void;
  disabled?: boolean;
}) {
  return (
    <label className="field inline-field">
      <span>计划位置</span>
      <select
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
        disabled={disabled}
      >
        {roles.map((role) => (
          <option key={role.value} value={role.value}>
            {role.label} · {role.short}
          </option>
        ))}
      </select>
    </label>
  );
}
export function HeroSelect({
  heroes,
  value,
  onChange,
  label,
  disabled,
  excluded = [],
}: {
  heroes: Hero[];
  value: number | null;
  onChange: (id: number | null) => void;
  label: string;
  disabled?: boolean;
  excluded?: number[];
}) {
  return (
    <select
      aria-label={label}
      value={value ?? ''}
      onChange={(event) => onChange(event.target.value ? Number(event.target.value) : null)}
      disabled={disabled}
    >
      <option value="">尚未选择</option>
      {heroes.map((hero) => (
        <option
          key={hero.id}
          value={hero.id}
          disabled={excluded.includes(hero.id) && value !== hero.id}
        >
          {hero.localized_name}
        </option>
      ))}
    </select>
  );
}
export function MatchTable({
  matches,
  heroes,
  compact = false,
}: {
  matches: MatchRecord[];
  heroes: Hero[];
  compact?: boolean;
}) {
  return (
    <div className="table-scroll">
      <table>
        <thead>
          <tr>
            <th>英雄 / 比赛</th>
            <th>结果</th>
            <th>日期</th>
            {!compact && (
              <>
                <th>K / D / A</th>
                <th>GPM / XPM</th>
                <th>版本</th>
                <th>阵容</th>
              </>
            )}
          </tr>
        </thead>
        <tbody>
          {matches.map((match) => {
            const result = won(match);
            return (
              <tr key={match.match_id}>
                <td>
                  <div className="hero-cell">
                    <HeroMark hero={heroes.find((hero) => hero.id === match.hero_id)} small />
                    <div>
                      <strong>{heroName(heroes, match.hero_id)}</strong>
                      <small>#{match.match_id}</small>
                    </div>
                  </div>
                </td>
                <td>
                  <span className={`result ${result === null ? '' : result ? 'win' : 'loss'}`}>
                    {result == null ? '未知' : result ? '胜利' : '失败'}
                  </span>
                </td>
                <td>{date(match.start_time)}</td>
                {!compact && (
                  <>
                    <td>
                      {match.kills ?? '—'} / {match.deaths ?? '—'} / {match.assists ?? '—'}
                    </td>
                    <td>
                      {match.gold_per_min ?? '—'} / {match.xp_per_min ?? '—'}
                    </td>
                    <td>{match.patch ?? '—'}</td>
                    <td>
                      <span className={match.lineup_complete ? 'text-success' : 'text-muted'}>
                        {match.lineup_complete ? '完整' : '有缺失'}
                      </span>
                    </td>
                  </>
                )}
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
export function Metric({
  label,
  value,
  detail,
}: {
  label: string;
  value: ReactNode;
  detail: string;
}) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{detail}</small>
    </div>
  );
}
export function WinRate({ value, samples }: { value: number | null; samples?: number }) {
  return (
    <div className="win-rate">
      <strong>{percent(value)}</strong>
      <div className="track">
        <i style={{ width: `${Math.max(0, Math.min(1, value ?? 0)) * 100}%` }} />
      </div>
      {samples != null && <small>{samples} 场已知胜负</small>}
    </div>
  );
}
