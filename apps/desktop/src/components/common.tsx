import { Fragment, useState, type ReactNode } from 'react';
import { Crosshair, Database, Swords } from 'lucide-react';
import type { Hero, MatchRecord, MatchRoleInference } from '../types';
import { date, dateTime, heroName, percent, positionNames, roles, won } from '../services/format';
import { heroAssetUrl } from '../services/game-assets';

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
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  const url = hero ? heroAssetUrl(hero.id, small) : null;
  return (
    <div className={`hero-mark ${small ? 'small' : ''}`} aria-hidden="true">
      {url && failedUrl !== url ? (
        <img src={url} alt="" loading="lazy" decoding="async" onError={() => setFailedUrl(url)} />
      ) : (
        (hero?.localized_name
          .split(/[-\s]+/)
          .map((word) => word[0])
          .slice(0, 2)
          .join('') ?? '—')
      )}
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
        <option value={0}>自动 · 根据个人历史位置</option>
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
  positions,
}: {
  matches: MatchRecord[];
  heroes: Hero[];
  compact?: boolean;
  positions?: Map<string, MatchRoleInference>;
}) {
  const [expanded, setExpanded] = useState<string | null>(null);
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
                <th>位置推断</th>
                <th>GPM / XPM</th>
                <th>版本</th>
                <th>阵容</th>
                <th>详情</th>
              </>
            )}
          </tr>
        </thead>
        <tbody>
          {matches.map((match) => {
            const result = won(match);
            const inference = positions?.get(match.match_id);
            return (
              <Fragment key={match.match_id}>
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
                      <td className="match-position">
                        {inference?.qualifying_evidence
                          ? positionNames(inference.candidates)
                          : '未知'}
                        {inference?.qualifying_evidence && <small>规则推断</small>}
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
                      <td>
                        <button
                          className="text-button"
                          aria-expanded={expanded === match.match_id}
                          aria-controls={`match-${match.match_id}`}
                          onClick={() =>
                            setExpanded(expanded === match.match_id ? null : match.match_id)
                          }
                          aria-label={`查看比赛 ${match.match_id} 详情`}
                        >
                          {expanded === match.match_id ? '收起' : '查看'}
                        </button>
                      </td>
                    </>
                  )}
                </tr>
                {!compact && expanded === match.match_id && (
                  <tr className="match-detail-row">
                    <td colSpan={9} id={`match-${match.match_id}`}>
                      <MatchDetails match={match} heroes={heroes} inference={inference} />
                    </td>
                  </tr>
                )}
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function MatchDetails({
  match,
  heroes,
  inference,
}: {
  match: MatchRecord;
  heroes: Hero[];
  inference?: MatchRoleInference;
}) {
  const labels: Record<string, string> = {
    match_id: '比赛 ID',
    account_id: '账号 ID',
    hero_id: '英雄 ID',
    start_time: '开始时间',
    duration: '时长（秒）',
    game_mode: '游戏模式 ID',
    patch: '版本 ID',
    player_slot: '玩家槽位',
    radiant_win: '天辉是否获胜',
    kills: '击杀',
    deaths: '死亡',
    assists: '助攻',
    last_hits: '正补',
    denies: '反补',
    gold_per_min: 'GPM',
    xp_per_min: 'XPM',
    lane_role: '数据源线路标签',
    team_heroes: '己方英雄',
    enemy_heroes: '敌方英雄',
    lineup_complete: '阵容是否完整',
    source: '数据来源',
    fetched_at: '获取时间',
  };
  function display(key: string, value: unknown): string {
    if (value == null) return '缺失（null）';
    if (key === 'team_heroes' || key === 'enemy_heroes')
      return (
        (value as number[]).map((id) => `${heroName(heroes, id)} (#${id})`).join('、') || '空数组'
      );
    if (key === 'start_time' || key === 'fetched_at')
      return `${dateTime(value as number)} · ${String(value)}`;
    if (typeof value === 'object') return JSON.stringify(value);
    return String(value);
  }
  return (
    <div className="match-details">
      <h3>比赛 #{match.match_id} · 全部本地字段</h3>
      <p className="hint">
        字段保持数据源含义；线路标签不直接等于一至五号位。未获取的数据保留为空。
      </p>
      <dl className="match-field-grid">
        {Object.entries(match).map(([key, value]) => (
          <div key={key}>
            <dt>
              {labels[key] ?? key}
              <code>{key}</code>
            </dt>
            <dd>{display(key, value)}</dd>
          </div>
        ))}
      </dl>
      <section className="match-inference">
        <h4>位置推断及依据</h4>
        {inference ? (
          <>
            <p>
              {inference.qualifying_evidence
                ? positionNames(inference.candidates)
                : '未知（依据不足）'}{' '}
              · 证据级别：
              {inference.strength === 'moderate'
                ? '中等'
                : inference.strength === 'limited'
                  ? '有限'
                  : '未知'}
            </p>
            {!inference.qualifying_evidence && inference.candidates.length > 0 && (
              <p className="hint">
                有限线索：{positionNames(inference.candidates)}，未达到自动英雄池位置依据要求。
              </p>
            )}
            <p className="hint">
              规则方法：<code>{inference.method}</code>。证据级别不是识别准确率。
            </p>
            <ul>
              {inference.reasons.map((reason, index) => (
                <li key={index}>{reason}</li>
              ))}
            </ul>
          </>
        ) : (
          <p>暂无推断记录。</p>
        )}
      </section>
      <details>
        <summary>查看本地记录 JSON</summary>
        <pre>{JSON.stringify(match, null, 2)}</pre>
      </details>
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
