import { useEffect, useState } from 'react';
import { RefreshCw } from 'lucide-react';
import { api, desktopAvailable } from '../services/api';
import { useApp } from '../hooks/useApp';
import { heroName } from '../services/format';
import type { CaptureDiagnostics } from '../types';

const labels = {
  EMPTY: '空槽',
  CANDIDATE: '候选',
  CONFIRMED: '已确认',
  UNCERTAIN: '不确定',
  INVALID: '无效',
};
export function CaptureDiagnosticsPanel() {
  const { snapshot } = useApp();
  const [open, setOpen] = useState(false);
  const [data, setData] = useState<CaptureDiagnostics | null>(null);
  const [error, setError] = useState('');
  const [refreshId, setRefreshId] = useState(0);
  useEffect(() => {
    if (!open || !desktopAvailable) return;
    let active = true;
    let pending = false;
    async function read() {
      if (pending) return;
      pending = true;
      try {
        const result = await api.captureDiagnostics();
        if (active) {
          setData(result);
          setError('');
        }
      } catch (cause) {
        if (active) setError(String(cause));
      } finally {
        pending = false;
      }
    }
    void read();
    const timer =
      snapshot.capture.status === 'running'
        ? window.setInterval(() => {
            void read();
          }, 1000)
        : null;
    return () => {
      active = false;
      if (timer != null) window.clearInterval(timer);
    };
  }, [open, snapshot.capture.status, refreshId]);
  const slots = data ? [...data.draft.radiant, ...data.draft.dire] : [];
  return (
    <details
      className="panel capture-diagnostic-panel"
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary>
        识别诊断<span>候选、置信度和锁定依据</span>
      </summary>
      <div className="diagnostic-content">
        <p className="hint">
          候选与不确定结果不进入已选阵容。相似度只是图像匹配指标，不是识别正确率或获胜概率。
        </p>
        <div className="diagnostic-heading">
          <span className="status-pill">
            {data?.draft.draft_screen_detected ? '检测到选人界面' : '未确认选人界面'}
          </span>
          <button
            className="text-button"
            disabled={!desktopAvailable}
            onClick={() => setRefreshId((value) => value + 1)}
          >
            <RefreshCw size={13} aria-hidden="true" />
            刷新诊断
          </button>
        </div>
        {!desktopAvailable && <p className="hint">请在桌面应用中读取捕获诊断。</p>}
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        {slots.length > 0 && (
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>槽位</th>
                  <th>识别状态</th>
                  <th>英雄</th>
                  <th>相似度</th>
                  <th>锁定证据</th>
                  <th>次选候选</th>
                </tr>
              </thead>
              <tbody>
                {slots.map((slot) => {
                  const frame = data?.analysis?.slots.find((item) => item.slot_id === slot.slot_id);
                  const runnerUp = frame?.candidates[1];
                  return (
                    <tr key={slot.slot_id}>
                      <td>
                        {slot.slot_id < 5 ? '天辉' : '夜魇'} {(slot.slot_id % 5) + 1}
                      </td>
                      <td
                        className={
                          slot.recognition_state === 'CONFIRMED'
                            ? 'text-success'
                            : slot.recognition_state === 'INVALID'
                              ? 'text-danger'
                              : 'text-muted'
                        }
                      >
                        {labels[slot.recognition_state]}
                      </td>
                      <td>{heroName(snapshot.heroes, slot.hero_id)}</td>
                      <td>{slot.confidence.toFixed(3)}</td>
                      <td>
                        {slot.source === 'manual'
                          ? '手动确认'
                          : slot.source === 'visual_marker'
                            ? '独立视觉标记'
                            : '无'}
                      </td>
                      <td>
                        {runnerUp
                          ? `${heroName(snapshot.heroes, runnerUp.hero_id)} ${runnerUp.similarity.toFixed(3)}`
                          : '—'}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        {data?.analysis && (
          <p className="hint">
            最近帧 {new Date(data.analysis.timestamp_ms).toLocaleTimeString('zh-CN')}；识别耗时{' '}
            {data.analysis.elapsed_ms.toFixed(1)} ms；
            {data.analysis.reused_pixels ? '画面未变化，复用匹配结果' : '本帧重新匹配'}。
          </p>
        )}
      </div>
    </details>
  );
}
