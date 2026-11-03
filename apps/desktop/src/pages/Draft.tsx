import { useState } from 'react';
import { Crosshair, ExternalLink, RotateCcw, ShieldCheck, X } from 'lucide-react';
import { useApp } from '../hooks/useApp';
import { api } from '../services/api';
import { action } from '../stores/app-store';
import { heroName } from '../services/format';
import { HeroMark, HeroSelect, RoleSelect } from '../components/common';
import { CaptureDiagnosticsPanel } from '../components/CaptureDiagnostics';
import { Recommendations } from '../components/Recommendations';
import type { Draft as DraftState } from '../types';

export function DraftPage() {
  const { snapshot, busy } = useApp();
  const [ban, setBan] = useState<number | null>(null);
  const draft = snapshot.draft;
  const selected = [...draft.radiant, ...draft.dire].filter((id): id is number => id !== null);
  function update(next: DraftState) {
    void action('更新阵容', () => api.updateDraft(next));
  }
  return (
    <div className="page-content">
      <header className="page-heading">
        <div>
          <h1>BP 工作台</h1>
          <p>只推荐你愿意玩、适合当前位置的英雄。</p>
        </div>
        <button
          className="button"
          disabled={Boolean(busy)}
          onClick={() => {
            void action('打开悬浮窗', api.openOverlay);
          }}
        >
          <ExternalLink size={17} aria-hidden="true" />
          打开悬浮窗
        </button>
      </header>
      <section className="draft-toolbar">
        <RoleSelect
          value={snapshot.settings.selected_role}
          disabled={Boolean(busy)}
          onChange={(role) => {
            void action('更新位置', () =>
              api.saveSettings({ ...snapshot.settings, selected_role: role }),
            );
          }}
        />
        <label className="field inline-field">
          <span>我的阵营</span>
          <select
            value={draft.own_team}
            disabled={Boolean(busy)}
            onChange={(event) =>
              update({ ...draft, own_team: event.target.value as 'radiant' | 'dire' })
            }
          >
            <option value="radiant">天辉</option>
            <option value="dire">夜魇</option>
          </select>
        </label>
        <span className="status-pill">
          <Crosshair size={14} aria-hidden="true" />
          {snapshot.capture.window_title ? '已检测游戏窗口' : '手动确认模式'}
        </span>
        <button
          className="text-button"
          disabled={Boolean(busy)}
          onClick={() =>
            update({
              ...draft,
              radiant: Array(5).fill(null),
              dire: Array(5).fill(null),
              banned: [],
            })
          }
        >
          <RotateCcw size={15} aria-hidden="true" />
          重置阵容
        </button>
      </section>
      <div className="draft-layout">
        <div className="draft-board">
          <div className="notice">
            <ShieldCheck size={17} aria-hidden="true" />
            <span>
              手动选择表示确认英雄已锁定。悬停和预选英雄请保持空槽；不确定的识别结果不会参与推荐。
            </span>
          </div>
          {(['radiant', 'dire'] as const).map((team) => (
            <section className={`panel team-panel ${team}`} key={team}>
              <div className="panel-header">
                <div>
                  <h2>
                    <i className="team-indicator" />
                    {team === 'radiant' ? '天辉' : '夜魇'}
                    <span className="team-relation">
                      {team === draft.own_team ? '己方' : '敌方'}
                    </span>
                  </h2>
                  <p>{draft[team].filter(Boolean).length} / 5 位英雄已确认</p>
                </div>
              </div>
              <div className="hero-slots">
                {draft[team].map((id, slot) => (
                  <div className={`hero-slot ${id ? 'filled' : ''}`} key={slot}>
                    <HeroMark hero={snapshot.heroes.find((hero) => hero.id === id)} />
                    <span className="slot-label">{id ? '已确认' : `槽位 ${slot + 1}`}</span>
                    <HeroSelect
                      heroes={snapshot.heroes}
                      value={id}
                      label={`${team === 'radiant' ? '天辉' : '夜魇'}槽位${slot + 1}`}
                      excluded={[...selected, ...draft.banned]}
                      disabled={Boolean(busy)}
                      onChange={(heroId) =>
                        update({
                          ...draft,
                          [team]: draft[team].map((old, index) => (index === slot ? heroId : old)),
                        })
                      }
                    />
                  </div>
                ))}
              </div>
            </section>
          ))}
          <section className="panel banned-panel">
            <div className="panel-header">
              <div>
                <h2>已确认禁用</h2>
                <p>仅添加界面中可以确定的禁用英雄。</p>
              </div>
            </div>
            <div className="banned-editor">
              <HeroSelect
                heroes={snapshot.heroes}
                value={ban}
                label="选择已禁用英雄"
                onChange={setBan}
                excluded={[...selected, ...draft.banned]}
              />
              <button
                className="button"
                disabled={ban == null || Boolean(busy)}
                onClick={() => {
                  if (ban) {
                    update({ ...draft, banned: [...draft.banned, ban] });
                    setBan(null);
                  }
                }}
              >
                添加禁用
              </button>
            </div>
            <div className="chips">
              {draft.banned.length ? (
                draft.banned.map((id) => (
                  <span className="chip" key={id}>
                    {heroName(snapshot.heroes, id)}
                    <button
                      aria-label={`移除禁用 ${heroName(snapshot.heroes, id)}`}
                      disabled={Boolean(busy)}
                      onClick={() =>
                        update({ ...draft, banned: draft.banned.filter((old) => old !== id) })
                      }
                    >
                      <X size={13} />
                    </button>
                  </span>
                ))
              ) : (
                <span className="text-muted">未记录禁用英雄</span>
              )}
            </div>
          </section>
          <CaptureDiagnosticsPanel />
        </div>
        <aside className="panel recommendation-panel">
          <div className="panel-header">
            <div>
              <h2>你的最佳选择</h2>
              <p>当前英雄池内 Top {snapshot.settings.top_k}</p>
            </div>
            <span className="live-mark">本地计算</span>
          </div>
          <Recommendations />
          <p className="recommendation-note">
            推荐分用于比较候选英雄，不是本局预测胜率。个人样本、缺失资料和版本差异会影响依据强度。
          </p>
          {snapshot.capture.recommendation_ms != null && (
            <p className="performance-inline">
              最近计算 {snapshot.capture.recommendation_ms.toFixed(1)} ms
            </p>
          )}
        </aside>
      </div>
    </div>
  );
}
