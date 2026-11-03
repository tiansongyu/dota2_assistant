import { useMemo, useState } from 'react';
import { ChevronLeft, ChevronRight, Filter } from 'lucide-react';
import { useApp } from '../hooks/useApp';
import { EmptyState, MatchTable, Metric } from '../components/common';
import { dateStart, dateEnd, heroName, percent, roles, won } from '../services/format';

export function History() {
  const { snapshot } = useApp();
  const [hero, setHero] = useState(0);
  const [role, setRole] = useState(0);
  const [patch, setPatch] = useState(0);
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(50);
  const positions = useMemo(
    () => new Map(snapshot.position_analysis.matches.map((item) => [item.match_id, item])),
    [snapshot.position_analysis.matches],
  );
  const patches = [
    ...new Set(snapshot.matches.flatMap((match) => (match.patch == null ? [] : [match.patch]))),
  ].sort((a, b) => b - a);
  const filtered = useMemo(
    () =>
      snapshot.matches.filter(
        (match) =>
          (!hero || match.hero_id === hero) &&
          (!role ||
            (role === -1
              ? !positions.get(match.match_id)?.qualifying_evidence
              : positions.get(match.match_id)?.qualifying_evidence &&
                positions.get(match.match_id)?.candidates.includes(role))) &&
          (!patch || match.patch === patch) &&
          (!from || match.start_time >= (dateStart(from) ?? 0)) &&
          (!to || match.start_time <= (dateEnd(to) ?? Infinity)),
      ),
    [snapshot.matches, positions, hero, role, patch, from, to],
  );
  const known = filtered.filter((match) => won(match) !== null);
  const wins = known.filter((match) => won(match)).length;
  const months = new Map<string, { matches: number; wins: number }>();
  for (const match of known) {
    const d = new Date(match.start_time * 1000);
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
    const entry = months.get(key) ?? { matches: 0, wins: 0 };
    entry.matches++;
    if (won(match)) entry.wins++;
    months.set(key, entry);
  }
  const trend = [...months.entries()].sort(([a], [b]) => a.localeCompare(b)).slice(-12);
  const usages = new Map<number, number>();
  for (const match of filtered) usages.set(match.hero_id, (usages.get(match.hero_id) ?? 0) + 1);
  const pageCount = Math.max(1, Math.ceil(filtered.length / pageSize));
  const safePage = Math.min(page, pageCount - 1);
  return (
    <div className="page-content">
      <header className="page-heading">
        <div>
          <h1>历史战绩</h1>
          <p>查看已同步的真实比赛与数据覆盖情况。</p>
        </div>
        <span className="status-pill">
          <i />
          本地记录
        </span>
      </header>
      <section className="panel">
        <div className="filter-row history-filters">
          <Filter size={18} aria-hidden="true" />
          <label className="field">
            <span>英雄</span>
            <select
              value={hero}
              onChange={(event) => {
                setHero(Number(event.target.value));
                setPage(0);
              }}
            >
              <option value={0}>所有英雄</option>
              {snapshot.hero_stats.map((item) => (
                <option key={item.hero_id} value={item.hero_id}>
                  {heroName(snapshot.heroes, item.hero_id)}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>本场位置推断</span>
            <select
              value={role}
              onChange={(event) => {
                setRole(Number(event.target.value));
                setPage(0);
              }}
            >
              <option value={0}>全部位置</option>
              <option value={-1}>未知 / 依据不足</option>
              {roles.map((item) => (
                <option key={item.value} value={item.value}>
                  {item.label}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>数据版本 ID</span>
            <select
              value={patch}
              onChange={(event) => {
                setPatch(Number(event.target.value));
                setPage(0);
              }}
            >
              <option value={0}>全部版本</option>
              {patches.map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>起始日期</span>
            <input
              type="date"
              value={from}
              onChange={(event) => {
                setFrom(event.target.value);
                setPage(0);
              }}
            />
          </label>
          <label className="field">
            <span>结束日期</span>
            <input
              type="date"
              value={to}
              onChange={(event) => {
                setTo(event.target.value);
                setPage(0);
              }}
            />
          </label>
          <button
            className="text-button"
            onClick={() => {
              setHero(0);
              setRole(0);
              setPatch(0);
              setFrom('');
              setTo('');
              setPage(0);
            }}
          >
            重置
          </button>
        </div>
        <p className="hint padded-hint">
          位置筛选依据每场比赛的规则推断，候选位置可能重叠；未知保留为未知。手动英雄池位置不会改写历史比赛。缺失指标保留为空。
        </p>
      </section>
      <div className="metric-grid">
        <Metric
          label="筛选内比赛"
          value={filtered.length}
          detail={`${known.length} 场有已知胜负`}
        />
        <Metric
          label="历史胜率"
          value={percent(known.length ? wins / known.length : null)}
          detail={`${wins} 胜 / ${known.length - wins} 负`}
        />
        <Metric
          label="阵容完整度"
          value={percent(
            filtered.length
              ? filtered.filter((match) => match.lineup_complete).length / filtered.length
              : null,
          )}
          detail="完整的双方英雄阵容"
        />
        <Metric label="版本覆盖" value={patches.length} detail="按数据源的版本 ID 记录" />
      </div>
      {filtered.length > 0 && (
        <div className="dashboard-grid">
          <section className="panel">
            <div className="panel-header">
              <div>
                <h2>每月历史胜率</h2>
                <p>最近 12 个有记录月份；每月样本独立显示。</p>
              </div>
            </div>
            <div className="trend-chart" aria-label="各月胜率">
              {trend.map(([month, item]) => (
                <div className="trend-column" key={month}>
                  <span>{percent(item.wins / item.matches)}</span>
                  <div className="trend-bar-track">
                    <i style={{ height: `${(item.wins / item.matches) * 100}%` }} />
                  </div>
                  <small>
                    {month.slice(2)}
                    <br />
                    {item.matches} 场
                  </small>
                </div>
              ))}
            </div>
          </section>
          <section className="panel">
            <div className="panel-header">
              <div>
                <h2>英雄使用占比</h2>
                <p>当前筛选内使用最多的 5 位英雄。</p>
              </div>
            </div>
            <div className="usage-list">
              {[...usages.entries()]
                .sort((a, b) => b[1] - a[1])
                .slice(0, 5)
                .map(([id, count]) => (
                  <div key={id}>
                    <div>
                      <span>{heroName(snapshot.heroes, id)}</span>
                      <strong>
                        {count} 场 · {percent(count / filtered.length)}
                      </strong>
                    </div>
                    <div className="track">
                      <i style={{ width: `${(count / filtered.length) * 100}%` }} />
                    </div>
                  </div>
                ))}
            </div>
          </section>
        </div>
      )}
      <section className="panel">
        <div className="panel-header">
          <div>
            <h2>比赛记录</h2>
            <p>数据来源和缺失状态按原始记录保留。</p>
          </div>
          <span className="text-muted">共 {filtered.length} 场</span>
        </div>
        {filtered.length ? (
          <>
            <MatchTable
              heroes={snapshot.heroes}
              matches={filtered.slice(safePage * pageSize, (safePage + 1) * pageSize)}
              positions={positions}
            />
            <div className="pagination">
              <label className="field inline-field">
                <span>每页</span>
                <select
                  value={pageSize}
                  onChange={(event) => {
                    setPageSize(Number(event.target.value));
                    setPage(0);
                  }}
                >
                  {[25, 50, 100].map((size) => (
                    <option key={size} value={size}>
                      {size} 场
                    </option>
                  ))}
                </select>
              </label>
              <button
                className="button"
                disabled={safePage === 0}
                onClick={() => setPage((value) => Math.max(0, value - 1))}
              >
                <ChevronLeft size={16} aria-hidden="true" />
                上一页
              </button>
              <label className="field inline-field pagination-jump">
                <span>页码</span>
                <select
                  aria-label="跳转到页码"
                  value={safePage}
                  onChange={(event) => setPage(Number(event.target.value))}
                >
                  {Array.from({ length: pageCount }, (_, index) => (
                    <option key={index} value={index}>
                      {index + 1} / {pageCount}
                    </option>
                  ))}
                </select>
              </label>
              <button
                className="button"
                disabled={safePage + 1 >= pageCount}
                onClick={() => setPage((value) => value + 1)}
              >
                下一页
                <ChevronRight size={16} aria-hidden="true" />
              </button>
              <span className="pagination-range">
                第 {safePage * pageSize + 1}—{Math.min((safePage + 1) * pageSize, filtered.length)}{' '}
                场 / {filtered.length} 场
              </span>
            </div>
          </>
        ) : (
          <EmptyState title={snapshot.matches.length ? '没有符合筛选的比赛' : '还没有历史比赛'}>
            {snapshot.matches.length
              ? '调整日期、英雄或位置筛选。'
              : '在总览页关联公开账号并开始同步。'}
          </EmptyState>
        )}
      </section>
    </div>
  );
}
