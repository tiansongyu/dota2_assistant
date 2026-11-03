import { useState } from 'react';
import { Pause, Play, RefreshCw } from 'lucide-react';
import { useApp } from '../hooks/useApp';
import { api } from '../services/api';
import { dateStart, dateEnd, dateTime } from '../services/format';
import { action } from '../stores/app-store';

const isoDate = (value: Date) =>
  `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, '0')}-${String(value.getDate()).padStart(2, '0')}`;
const statusText: Record<string, string> = {
  idle: '尚未开始',
  running: '正在同步',
  paused: '已暂停',
  completed: '本次同步完成',
  failed: '同步中断',
  error: '同步中断',
  rate_limited: '请求额度受限',
};
export function SyncPanel() {
  const { snapshot, busy } = useApp();
  const { sync } = snapshot;
  const [from, setFrom] = useState(() => {
    const d = new Date();
    d.setFullYear(d.getFullYear() - 5);
    return isoDate(d);
  });
  const [to, setTo] = useState(() => isoDate(new Date()));
  const running = sync.status === 'running';
  const canResume =
    ['paused', 'failed', 'error', 'rate_limited'].includes(sync.status) &&
    sync.from_time != null &&
    sync.to_time != null;
  const fromTime = dateStart(from);
  const toTime = dateEnd(to);
  const invalid = fromTime == null || toTime == null || fromTime > toTime;
  return (
    <section className="panel sync-panel">
      <div className="panel-header">
        <div>
          <h2>历史数据同步</h2>
          <p>导入可访问比赛，保留分页进度。</p>
        </div>
        <span className={`status-pill ${running ? 'active' : ''}`}>
          <i />
          {statusText[sync.status] ?? sync.status}
        </span>
      </div>
      <div className="sync-form">
        <label className="field">
          <span>起始日期</span>
          <input
            type="date"
            value={from}
            onChange={(event) => setFrom(event.target.value)}
            disabled={running}
          />
        </label>
        <label className="field">
          <span>结束日期</span>
          <input
            type="date"
            value={to}
            onChange={(event) => setTo(event.target.value)}
            disabled={running}
          />
        </label>
        <button
          className="button primary"
          disabled={Boolean(busy) || !snapshot.profile || (!running && invalid)}
          onClick={() => {
            void action(running ? '暂停同步' : '同步战绩', () =>
              running ? api.pauseSync() : api.startSync(fromTime!, toTime!),
            );
          }}
        >
          {running ? (
            <Pause size={16} aria-hidden="true" />
          ) : (
            <RefreshCw size={16} aria-hidden="true" />
          )}
          {running ? '暂停同步' : '同步所选范围'}
        </button>
        {canResume && (
          <button
            className="button"
            disabled={Boolean(busy)}
            onClick={() => {
              void action('恢复同步', () => api.startSync(sync.from_time!, sync.to_time!));
            }}
          >
            <Play size={16} aria-hidden="true" />
            继续断点
          </button>
        )}
      </div>
      {invalid && <p className="field-error">请选择有效日期范围，起始日期不能晚于结束日期。</p>}
      <div className="sync-counts" aria-live="polite">
        <span>
          已扫描 <strong>{sync.pages_fetched}</strong> 页
        </span>
        <span>
          新增 <strong>{sync.imported}</strong> 场
        </span>
        <span>
          更新 <strong>{sync.updated}</strong> 场
        </span>
        <span>
          失败 <strong>{sync.failed}</strong> 场
        </span>
        <span>
          阵容缺失 <strong>{sync.missing_lineups}</strong> 场
        </span>
      </div>
      {sync.error && (
        <p className="inline-error" role="alert">
          {sync.error}
        </p>
      )}
      <div className="panel-footer">
        <span>上次同步：{dateTime(sync.last_synced_at)}</span>
        <span>来源 OpenDota · 未公开记录可能不可访问</span>
      </div>
    </section>
  );
}
