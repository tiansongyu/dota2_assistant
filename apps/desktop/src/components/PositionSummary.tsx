import type { RoleAnalysis } from '../types';
import { percent, roles } from '../services/format';
import { EmptyState } from './common';
import { useApp } from '../hooks/useApp';
import { api } from '../services/api';
import { action } from '../stores/app-store';

export function PositionSummary({ analysis }: { analysis: RoleAnalysis }) {
  const { snapshot, busy } = useApp();
  const progress = snapshot.role_sync;
  const running = progress?.status === 'running';
  const statusLabels: Record<string, string> = {
    running: '正在补充位置依据…',
    paused: '位置数据同步已暂停',
    completed: '位置数据同步完成',
    partial: '本批次已完成，可继续补充缺少的详情',
    rate_limited: '数据源限流，稍后可继续补充',
    failed: '位置数据同步失败，可重试',
  };
  const evidence = analysis.matches.filter((match) => match.qualifying_evidence);
  return (
    <section className="panel position-summary">
      <div className="panel-header">
        <div>
          <h2>历史位置推断</h2>
          <p>根据可访问比赛的线路与资源记录分析，不是官方位置标签。</p>
        </div>
        <button
          className="button"
          disabled={
            Boolean(busy) || !snapshot.profile || (!running && snapshot.sync.status === 'running')
          }
          onClick={() => {
            void action(
              running ? '暂停位置同步' : '补充位置数据',
              running ? api.pauseSync : api.refreshRoles,
            );
          }}
        >
          {running ? '暂停位置同步' : '补充位置数据'}
        </button>
      </div>
      {progress && (
        <div className="position-sync" aria-live="polite">
          <span>{statusLabels[progress.status] ?? `位置数据同步：${progress.status}`}</span>
          <span>
            适用 {progress.eligible_matches} 场 · 已缓存 {progress.cached_matches} 场 · 本次获取{' '}
            {progress.fetched} 场 · 失败 {progress.failed} 场
          </span>
          <small>
            本次请求上限 {progress.request_budget} 条；暂停后可再次补充，仅请求缺少的详情。
          </small>
          {progress.error && (
            <p className="text-danger" role="alert">
              {progress.error}
            </p>
          )}
        </div>
      )}
      {analysis.total_matches ? (
        <>
          <div className="position-coverage">
            <span>
              已导入 <strong>{analysis.total_matches}</strong> 场
            </span>
            <span>
              适用分析 <strong>{analysis.eligible_matches}</strong> 场
            </span>
            <span>
              详情可用 <strong>{analysis.detail_matches}</strong> 场
            </span>
            <span>
              有位置依据 <strong>{analysis.analyzed_matches}</strong> 场
            </span>
            <span>
              位置未知 <strong>{analysis.unknown_matches}</strong> 场
            </span>
          </div>
          <div className="position-distribution">
            {roles.map((role) => {
              const count = evidence.filter((match) =>
                match.candidates.includes(role.value),
              ).length;
              return (
                <div key={role.value}>
                  <div>
                    <span>
                      {role.label} <small>{role.short}</small>
                    </span>
                    <strong>{count} 场</strong>
                  </div>
                  <div className="track">
                    <i
                      style={{ width: `${evidence.length ? (count / evidence.length) * 100 : 0}%` }}
                    />
                  </div>
                </div>
              );
            })}
          </div>
          <p className="hint padded-hint">
            图中为有可用依据的 {evidence.length}{' '}
            场比赛对各位置的支持次数；同一场可支持多个位置，各项不能相加作为总场次。
            适用比赛的位置依据覆盖率{' '}
            {percent(
              analysis.eligible_matches
                ? analysis.analyzed_matches / analysis.eligible_matches
                : null,
            )}
            ；未知场次仅在适用比赛中统计。 规则版本：{analysis.rule_version || '—'}。
          </p>
        </>
      ) : (
        <EmptyState title="等待个人位置数据">
          同步后自动分析有依据的位置；样本不足会保留未知，也可以在英雄池中手动指定。
        </EmptyState>
      )}
      {analysis.warnings.length > 0 && (
        <details className="position-warnings">
          <summary>分析限制（{analysis.warnings.length}）</summary>
          <ul>
            {analysis.warnings.map((warning, index) => (
              <li key={index}>{warning}</li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}
