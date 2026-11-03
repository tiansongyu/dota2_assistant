import { useMemo, useState } from 'react';
import { Ban, Check, Save, Search, Shield } from 'lucide-react';
import { useApp } from '../hooks/useApp';
import { api } from '../services/api';
import { action } from '../stores/app-store';
import { decimal, heroName, percent, positionNames, roles } from '../services/format';
import { EmptyState, HeroMark, WinRate } from '../components/common';
import type { PoolConfig } from '../types';

export function HeroPool() {
  const { snapshot, busy } = useApp();
  const [pool, setPool] = useState<PoolConfig>(() => structuredClone(snapshot.pool));
  const [search, setSearch] = useState('');
  const [roleFilter, setRoleFilter] = useState(0);
  const [selected, setSelected] = useState<number | null>(snapshot.hero_stats[0]?.hero_id ?? null);
  const [compare, setCompare] = useState<number | null>(null);
  const [onlyPlayed, setOnlyPlayed] = useState(false);
  const stats = useMemo(
    () => new Map(snapshot.hero_stats.map((hero) => [hero.hero_id, hero])),
    [snapshot.hero_stats],
  );
  const positionStats = new Map(
    snapshot.position_analysis.heroes.map((hero) => [hero.hero_id, hero]),
  );
  function effectiveRoles(id: number) {
    if (Object.hasOwn(pool.roles, id)) return pool.roles[id];
    // A locally removed override must immediately show the automatic result.
    if (Object.hasOwn(snapshot.pool.roles, id)) return positionStats.get(id)?.suggested_roles ?? [];
    return snapshot.effective_pool.roles[id] ?? [];
  }
  const heroes = snapshot.heroes
    .filter(
      (hero) =>
        (!onlyPlayed || stats.has(hero.id)) &&
        (!search ||
          hero.localized_name.toLowerCase().includes(search.toLowerCase()) ||
          hero.id.toString() === search) &&
        (!roleFilter || effectiveRoles(hero.id).includes(roleFilter)),
    )
    .sort(
      (a, b) =>
        (stats.get(b.id)?.matches ?? 0) - (stats.get(a.id)?.matches ?? 0) ||
        a.localized_name.localeCompare(b.localized_name),
    );
  const selectedHero = snapshot.heroes.find((hero) => hero.id === selected);
  const selectedStats = selected == null ? null : stats.get(selected);
  const selectedPosition = selected == null ? null : positionStats.get(selected);
  const compareStats = compare == null ? null : stats.get(compare);
  const dirty = JSON.stringify(pool) !== JSON.stringify(snapshot.pool);
  function include(id: number) {
    setPool((previous) => ({
      ...previous,
      included: previous.included.includes(id)
        ? previous.included.filter((value) => value !== id)
        : [...previous.included, id],
      excluded: previous.excluded.filter((value) => value !== id),
    }));
  }
  function exclude(id: number) {
    setPool((previous) => ({
      ...previous,
      excluded: previous.excluded.includes(id)
        ? previous.excluded.filter((value) => value !== id)
        : [...previous.excluded, id],
      included: previous.included.filter((value) => value !== id),
    }));
  }
  function role(id: number, position: number) {
    const assigned = effectiveRoles(id);
    setPool((previous) => ({
      ...previous,
      roles: {
        ...previous.roles,
        [id]: assigned.includes(position)
          ? assigned.filter((value) => value !== position)
          : [...assigned, position],
      },
    }));
  }
  function restoreAutomatic(id: number) {
    setPool((previous) => {
      const nextRoles = { ...previous.roles };
      delete nextRoles[id];
      return { ...previous, roles: nextRoles };
    });
  }
  return (
    <div className="page-content">
      <header className="page-heading">
        <div>
          <h1>我的英雄池</h1>
          <p>优先选择熟悉的英雄。明确排除的英雄不会进入推荐。</p>
        </div>
        <button
          className="button primary"
          disabled={!dirty || Boolean(busy)}
          onClick={() => {
            void action('保存英雄池', () => api.savePool(pool));
          }}
        >
          <Save size={17} aria-hidden="true" />
          {busy === '保存英雄池' ? '正在保存…' : '保存英雄池'}
        </button>
      </header>
      <section className="panel pool-toolbar">
        <div className="pool-summary">
          <span>
            自动建议 <strong>{snapshot.automatic_pool.length}</strong> 位英雄
          </span>
          <span>
            已保存配置 · 当前可推荐 <strong>{snapshot.pool_candidates.length}</strong> 位
          </span>
          {dirty && <span className="text-danger">有未保存修改</span>}
        </div>
        <div className="segmented" role="group" aria-label="英雄池模式">
          {(
            [
              { mode: 'auto', label: '自动英雄池' },
              { mode: 'manual', label: '手动英雄池' },
              { mode: 'hybrid', label: '混合英雄池' },
            ] as const
          ).map((item) => (
            <button
              key={item.mode}
              className={pool.mode === item.mode ? 'selected' : ''}
              aria-pressed={pool.mode === item.mode}
              onClick={() => setPool((previous) => ({ ...previous, mode: item.mode }))}
            >
              {item.label}
            </button>
          ))}
        </div>
        <p>
          {pool.mode === 'auto'
            ? '根据真实使用记录筛选，仍可排除不想玩的英雄。'
            : pool.mode === 'manual'
              ? '仅推荐你手动纳入且有可用位置依据的英雄。'
              : '在自动建议中增加或排除英雄。'}
          默认从个人历史推断位置；可手动覆盖。线路数据不直接等于五位置。
        </p>
        <div className="filter-row">
          <label className="search-field">
            <Search size={17} aria-hidden="true" />
            <input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder="搜索英雄名称或 ID"
              aria-label="搜索英雄"
            />
          </label>
          <label className="field inline-field">
            <span>位置</span>
            <select
              value={roleFilter}
              onChange={(event) => setRoleFilter(Number(event.target.value))}
            >
              <option value={0}>全部位置</option>
              {roles.map((item) => (
                <option key={item.value} value={item.value}>
                  {item.label}
                </option>
              ))}
            </select>
          </label>
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={onlyPlayed}
              onChange={(event) => setOnlyPlayed(event.target.checked)}
            />
            只看玩过的英雄
          </label>
          <span className="filter-count">{heroes.length} 位英雄</span>
        </div>
      </section>
      <div className="pool-layout">
        <section className="panel pool-list">
          {heroes.length ? (
            <div className="table-scroll">
              <table>
                <thead>
                  <tr>
                    <th>英雄</th>
                    <th>场次</th>
                    <th>历史胜率</th>
                    <th>最近 20 场</th>
                    <th>选择</th>
                  </tr>
                </thead>
                <tbody>
                  {heroes.map((hero) => {
                    const data = stats.get(hero.id);
                    const excluded = pool.excluded.includes(hero.id);
                    const included = pool.included.includes(hero.id);
                    return (
                      <tr key={hero.id} className={selected === hero.id ? 'selected-row' : ''}>
                        <td>
                          <button
                            className="hero-cell hero-select-button"
                            onClick={() => setSelected(hero.id)}
                          >
                            <HeroMark hero={hero} small />
                            <div>
                              <strong>{hero.localized_name}</strong>
                              <small>
                                {excluded
                                  ? '已明确排除'
                                  : Object.hasOwn(pool.roles, hero.id)
                                    ? `手动覆盖 · ${positionNames(effectiveRoles(hero.id)) || '不允许任何位置'}`
                                    : `历史推断 · ${positionNames(effectiveRoles(hero.id)) || '依据不足'}`}
                              </small>
                            </div>
                          </button>
                        </td>
                        <td>{data?.matches ?? 0}</td>
                        <td>{percent(data?.win_rate)}</td>
                        <td>{percent(data?.recent20.win_rate)}</td>
                        <td>
                          <div className="row-actions">
                            <button
                              className={`icon-button ${included ? 'is-on' : ''}`}
                              aria-label={`${included ? '移除手动纳入' : '手动纳入'} ${hero.localized_name}`}
                              title={
                                pool.mode === 'auto'
                                  ? '切换到手动或混合模式后纳入'
                                  : included
                                    ? '取消手动纳入'
                                    : '手动纳入'
                              }
                              aria-pressed={included}
                              disabled={pool.mode === 'auto'}
                              onClick={() => include(hero.id)}
                            >
                              <Check size={16} />
                            </button>
                            <button
                              className={`icon-button ${excluded ? 'is-danger' : ''}`}
                              aria-label={`${excluded ? '取消排除' : '排除'} ${hero.localized_name}`}
                              title={excluded ? '取消排除' : '不推荐此英雄'}
                              aria-pressed={excluded}
                              onClick={() => exclude(hero.id)}
                            >
                              <Ban size={16} />
                            </button>
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState title="没有符合筛选的英雄">调整搜索条件或关闭“只看玩过的英雄”。</EmptyState>
          )}
        </section>
        <aside className="panel hero-detail">
          {selectedHero ? (
            <>
              <div className="detail-heading">
                <HeroMark hero={selectedHero} />
                <div>
                  <h2>{selectedHero.localized_name}</h2>
                  <span className="text-muted">英雄 ID {selectedHero.id}</span>
                </div>
              </div>
              <div className="detail-stats">
                <WinRate
                  value={selectedStats?.win_rate ?? null}
                  samples={selectedStats?.outcome_samples ?? 0}
                />
                <p className="text-muted">
                  95% Wilson 区间
                  <br />
                  <strong>
                    {selectedStats
                      ? `${percent(selectedStats.confidence_low)} — ${percent(selectedStats.confidence_high)}`
                      : '—'}
                  </strong>
                </p>
              </div>
              <div className="detail-section">
                <h3>推荐使用的位置</h3>
                <p className="hint">
                  {Object.hasOwn(pool.roles, selectedHero.id)
                    ? '当前使用手动覆盖。'
                    : '当前使用个人历史推断。'}
                  点击位置可多选覆盖；清空全部表示此英雄不参与任何位置的推荐。
                </p>
                <div className="role-buttons">
                  {roles.map((item) => (
                    <button
                      key={item.value}
                      className={
                        effectiveRoles(selectedHero.id).includes(item.value) ? 'selected' : ''
                      }
                      aria-pressed={effectiveRoles(selectedHero.id).includes(item.value)}
                      onClick={() => role(selectedHero.id, item.value)}
                    >
                      {item.label}
                    </button>
                  ))}
                </div>
                <button
                  className="text-button"
                  disabled={!Object.hasOwn(pool.roles, selectedHero.id)}
                  onClick={() => restoreAutomatic(selectedHero.id)}
                >
                  恢复自动推断
                </button>
                <p className="hint">
                  自动建议：{positionNames(selectedPosition?.suggested_roles ?? []) || '依据不足'}。
                  {selectedPosition
                    ? `${selectedPosition.matches} 场适用比赛中，${selectedPosition.analyzed_matches} 场有可用位置依据，${selectedPosition.unknown_matches} 场未知；覆盖率 ${percent(selectedPosition.coverage)}。`
                    : '尚无历史位置记录。'}
                </p>
                {Boolean(selectedPosition?.candidates.length) && (
                  <dl className="stats-list position-evidence">
                    {selectedPosition?.candidates.map((candidate) => (
                      <div key={candidate.role}>
                        <dt>{positionNames([candidate.role])}</dt>
                        <dd>
                          {candidate.supporting_matches} 场支持 · {candidate.exclusive_matches}{' '}
                          场单一候选
                        </dd>
                      </div>
                    ))}
                  </dl>
                )}
                <p className="hint">
                  支持样本可同时包含多个位置，不相加作为总场次；推断不是官方位置标签。
                </p>
              </div>
              <label className="field">
                <span>个人优先级（-1—1）</span>
                <input
                  type="number"
                  min="-1"
                  max="1"
                  step="0.1"
                  value={pool.priorities[selectedHero.id] ?? 0}
                  onChange={(event) =>
                    setPool((previous) => ({
                      ...previous,
                      priorities: {
                        ...previous.priorities,
                        [selectedHero.id]: Math.min(1, Math.max(-1, Number(event.target.value))),
                      },
                    }))
                  }
                />
              </label>
              <dl className="stats-list">
                <div>
                  <dt>最近 50 场</dt>
                  <dd>{percent(selectedStats?.recent50.win_rate)}</dd>
                </div>
                <div>
                  <dt>近 90 天</dt>
                  <dd>{percent(selectedStats?.days90.win_rate)}</dd>
                </div>
                <div>
                  <dt>近 180 天</dt>
                  <dd>{percent(selectedStats?.days180.win_rate)}</dd>
                </div>
                <div>
                  <dt>近一年</dt>
                  <dd>{percent(selectedStats?.year.win_rate)}</dd>
                </div>
                <div>
                  <dt>当前数据版本</dt>
                  <dd>{percent(selectedStats?.current_patch.win_rate)}</dd>
                </div>
                <div>
                  <dt>KDA / GPM / XPM</dt>
                  <dd>
                    {decimal(selectedStats?.avg_kda)} / {decimal(selectedStats?.avg_gpm, 0)} /{' '}
                    {decimal(selectedStats?.avg_xpm, 0)}
                  </dd>
                </div>
                <div>
                  <dt>阵容数据完整度</dt>
                  <dd>{percent(selectedStats?.lineup_coverage)}</dd>
                </div>
                <div>
                  <dt>胜负未知</dt>
                  <dd>
                    {selectedStats ? selectedStats.matches - selectedStats.outcome_samples : 0} 场
                  </dd>
                </div>
              </dl>
              <div className="detail-section">
                <h3>对比另一位英雄</h3>
                <select
                  aria-label="对比英雄"
                  value={compare ?? ''}
                  onChange={(event) =>
                    setCompare(event.target.value ? Number(event.target.value) : null)
                  }
                >
                  <option value="">选择已使用的英雄</option>
                  {snapshot.hero_stats
                    .filter((item) => item.hero_id !== selected)
                    .map((item) => (
                      <option key={item.hero_id} value={item.hero_id}>
                        {heroName(snapshot.heroes, item.hero_id)}
                      </option>
                    ))}
                </select>
                {compareStats && (
                  <dl className="stats-list">
                    <div>
                      <dt>{heroName(snapshot.heroes, compare)} 场次</dt>
                      <dd>{compareStats.matches}</dd>
                    </div>
                    <div>
                      <dt>历史胜率</dt>
                      <dd>{percent(compareStats.win_rate)}</dd>
                    </div>
                    <div>
                      <dt>最近 20 场</dt>
                      <dd>{percent(compareStats.recent20.win_rate)}</dd>
                    </div>
                  </dl>
                )}
              </div>
              <p className="privacy-note">
                <Shield size={14} aria-hidden="true" />
                历史表现不代表本局获胜概率。
              </p>
            </>
          ) : (
            <EmptyState title="选择一位英雄">查看真实统计、位置推断与手动覆盖。</EmptyState>
          )}
        </aside>
      </div>
    </div>
  );
}
