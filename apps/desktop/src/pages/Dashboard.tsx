import { ArrowUpRight, Crosshair, SlidersHorizontal, Swords } from 'lucide-react';
import { useApp } from '../hooks/useApp';
import { AccountSetup } from '../components/AccountSetup';
import { EmptyState, HeroMark, MatchTable, Metric } from '../components/common';
import { SyncPanel } from '../components/SyncPanel';
import { percent, heroName, date } from '../services/format';
import type { Page } from '../types';

export function Dashboard({ navigate }: { navigate: (page: Page) => void }) {
  const { snapshot } = useApp();
  const stats = snapshot.hero_stats;
  const total = stats.reduce((sum, hero) => sum + hero.matches, 0);
  const outcomes = stats.reduce((sum, hero) => sum + hero.outcome_samples, 0);
  const wins = stats.reduce((sum, hero) => sum + hero.wins, 0);
  const common = [...stats].sort((a, b) => b.matches - a.matches).slice(0, 4);
  return (
    <div className="page-content">
      <header className="page-heading">
        <div>
          <h1>选好下一场。</h1>
          <p>从你的英雄池出发，读懂当前阵容。</p>
        </div>
        <button className="button primary" onClick={() => navigate('draft')}>
          <Swords size={17} aria-hidden="true" />
          进入 BP 工作台
        </button>
      </header>
      {!snapshot.profile && <AccountSetup />}
      <div className="metric-grid">
        <Metric
          label="已导入比赛"
          value={total.toLocaleString()}
          detail={total ? `${outcomes} 场胜负可用` : '关联账号后开始同步'}
        />
        <Metric
          label="个人历史胜率"
          value={percent(outcomes ? wins / outcomes : null)}
          detail="仅基于已知胜负比赛"
        />
        <Metric
          label="使用过的英雄"
          value={stats.length}
          detail={stats.length ? '英雄池可手动调整' : '等待历史数据'}
        />
        <Metric
          label="本地数据更新"
          value={date(snapshot.data_updated_at)}
          detail="离线仍可使用已同步数据"
        />
      </div>
      <SyncPanel />
      <div className="dashboard-grid">
        <section className="panel">
          <div className="panel-header">
            <div>
              <h2>常用英雄</h2>
              <p>你投入最多的英雄。</p>
            </div>
            <button className="text-button" onClick={() => navigate('pool')}>
              管理英雄池
              <ArrowUpRight size={15} aria-hidden="true" />
            </button>
          </div>
          {common.length ? (
            <div className="common-heroes">
              {common.map((hero) => (
                <button className="common-hero" key={hero.hero_id} onClick={() => navigate('pool')}>
                  <HeroMark hero={snapshot.heroes.find((item) => item.id === hero.hero_id)} />
                  <div>
                    <strong>{heroName(snapshot.heroes, hero.hero_id)}</strong>
                    <small>{hero.matches} 场比赛</small>
                  </div>
                  <strong className="numeric">{percent(hero.win_rate)}</strong>
                </button>
              ))}
            </div>
          ) : (
            <EmptyState title="还没有个人英雄记录">
              同步后会在这里显示真实场次与历史胜率。
            </EmptyState>
          )}
        </section>
        <section className="panel capture-summary">
          <div className="panel-header">
            <div>
              <h2>选人识别</h2>
              <p>仅分析游戏中可见的英雄。</p>
            </div>
            <Crosshair size={20} aria-hidden="true" />
          </div>
          <div className="capture-state">
            <span className="status-pill">
              <i />
              {snapshot.capture.window_title ?? '等待游戏窗口'}
            </span>
            <p>{snapshot.capture.message}</p>
          </div>
          <div className="capture-steps">
            <span>
              <i>1</i>配置并校准识别区域
            </span>
            <span>
              <i>2</i>确认已锁定的英雄
            </span>
            <span>
              <i>3</i>查看个人推荐
            </span>
          </div>
          <button className="button" onClick={() => navigate('settings')}>
            <SlidersHorizontal size={16} aria-hidden="true" />
            打开识别设置
          </button>
        </section>
      </div>
      <section className="panel">
        <div className="panel-header">
          <div>
            <h2>最近比赛</h2>
            <p>来自本地历史记录。</p>
          </div>
          <button className="text-button" onClick={() => navigate('history')}>
            全部战绩
            <ArrowUpRight size={15} aria-hidden="true" />
          </button>
        </div>
        {snapshot.matches.length ? (
          <MatchTable matches={snapshot.matches.slice(0, 5)} heroes={snapshot.heroes} compact />
        ) : (
          <EmptyState title="尚未导入比赛">
            关联公开账号并开始同步。未公开的比赛可能无法恢复。
          </EmptyState>
        )}
      </section>
    </div>
  );
}
