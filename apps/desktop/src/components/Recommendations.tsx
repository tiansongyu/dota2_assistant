import { ChevronDown } from 'lucide-react';
import { useApp } from '../hooks/useApp';
import { heroName, percent, positionNames } from '../services/format';
import { EmptyState, HeroMark } from './common';

export function Recommendations({ compact = false }: { compact?: boolean }) {
  const { snapshot } = useApp();
  return snapshot.recommendations.length ? (
    <div className={`recommendation-list ${compact ? 'is-compact' : ''}`}>
      {snapshot.recommendations.map((item, index) => (
        <article className="recommendation" key={item.hero_id}>
          <div className="recommendation-main">
            <span className="rank">{index + 1}</span>
            <HeroMark
              hero={snapshot.heroes.find((hero) => hero.id === item.hero_id)}
              small={compact}
            />
            <div className="recommendation-name">
              <h3>{heroName(snapshot.heroes, item.hero_id)}</h3>
              <span>{item.confidence}</span>
              <small>{positionNames(item.role_candidates)}</small>
            </div>
            <div className="score">
              <strong>{item.score.toFixed(1)}</strong>
              <small>推荐分</small>
            </div>
          </div>
          {!compact && (
            <>
              <div className="recommendation-evidence">
                <span>
                  个人历史 <strong>{percent(item.personal_win_rate)}</strong>
                </span>
                <span>
                  有效依据 <strong>{item.sample_size} 场</strong>
                </span>
              </div>
              <details>
                <summary>
                  查看推荐依据
                  <ChevronDown size={15} aria-hidden="true" />
                </summary>
                <ul>
                  {item.reasons.map((reason, reasonIndex) => (
                    <li key={reasonIndex}>{reason}</li>
                  ))}
                </ul>
              </details>
            </>
          )}
        </article>
      ))}
    </div>
  ) : (
    <EmptyState title="等待可用的推荐依据" icon="draft">
      同步个人战绩后自动分析有依据的位置，也可在英雄池中手动覆盖。确认当前已锁定阵容后查看推荐；样本不足时按实际数量显示。
    </EmptyState>
  );
}
