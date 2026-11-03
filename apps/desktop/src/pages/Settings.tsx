import { useState } from 'react';
import { Crosshair, Save, Square, Sun, Moon, ExternalLink } from 'lucide-react';
import { useApp } from '../hooks/useApp';
import { api } from '../services/api';
import { action } from '../stores/app-store';
import { AccountSetup } from '../components/AccountSetup';
import { Calibration } from '../components/Calibration';
import { RoleSelect } from '../components/common';
import { decimal } from '../services/format';
import type { AppSettings } from '../types';

export function Settings() {
  const { snapshot, busy } = useApp();
  const [settings, setSettings] = useState<AppSettings>(() => structuredClone(snapshot.settings));
  const [validation, setValidation] = useState('');
  const weightSum = Object.values(settings.weights).reduce((sum, weight) => sum + weight, 0);
  function patch(values: Partial<AppSettings>) {
    setSettings((previous) => ({ ...previous, ...values }));
    setValidation('');
  }
  async function save() {
    if (
      settings.half_life_days < 7 ||
      settings.half_life_days > 1825 ||
      !Number.isFinite(settings.half_life_days)
    ) {
      setValidation('半衰期必须在 7—1825 天之间。');
      return;
    }
    if (
      Math.abs(weightSum - 1) > 0.001 ||
      Object.values(settings.weights).some((weight) => !Number.isFinite(weight) || weight < 0)
    ) {
      setValidation('四项评分权重必须非负，总和为 1。');
      return;
    }
    if (settings.calibration_json.trim()) {
      try {
        JSON.parse(settings.calibration_json);
      } catch {
        setValidation('识别配置不是有效 JSON，请检查后再保存。');
        return;
      }
    }
    await action('保存设置', () => api.saveSettings(settings));
  }
  const dirty = JSON.stringify(settings) !== JSON.stringify(snapshot.settings);
  return (
    <div className="page-content">
      <header className="page-heading">
        <div>
          <h1>设置</h1>
          <p>控制数据、推荐和识别方式。</p>
        </div>
        <button
          className="button primary"
          disabled={Boolean(busy) || !dirty}
          onClick={() => {
            void save();
          }}
        >
          <Save size={17} aria-hidden="true" />
          {busy === '保存设置' ? '正在保存…' : '保存设置'}
        </button>
      </header>
      {validation && (
        <div className="inline-error" role="alert">
          {validation}
        </div>
      )}
      <AccountSetup compact />
      <div className="settings-grid">
        <section className="panel settings-panel">
          <h2>推荐参数</h2>
          <p>工程初始权重可调整；当前评分不代表获胜概率。</p>
          <RoleSelect
            value={settings.selected_role}
            onChange={(selected_role) => patch({ selected_role })}
          />
          <p className="hint">自动模式使用历史推断的位置；明确选择一个位置后优先使用你的选择。</p>
          <div className="form-grid">
            <label className="field">
              <span>时间衰减半衰期（天）</span>
              <input
                type="number"
                min="7"
                max="1825"
                value={settings.half_life_days}
                onChange={(event) => patch({ half_life_days: Number(event.target.value) })}
              />
            </label>
            <label className="field">
              <span>推荐数量</span>
              <select
                value={settings.top_k}
                onChange={(event) => patch({ top_k: Number(event.target.value) })}
              >
                {[3, 4, 5].map((value) => (
                  <option key={value} value={value}>
                    Top {value}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <div className="weights">
            {(
              [
                { key: 'personal', label: '个人熟练度' },
                { key: 'counter', label: '敌方对抗' },
                { key: 'synergy', label: '己方协同' },
                { key: 'patch', label: '版本强度' },
              ] as const
            ).map((item) => (
              <label className="weight-field" key={item.key}>
                <span>{item.label}</span>
                <input
                  type="number"
                  min="0"
                  max="1"
                  step="0.05"
                  value={settings.weights[item.key]}
                  onChange={(event) =>
                    patch({
                      weights: { ...settings.weights, [item.key]: Number(event.target.value) },
                    })
                  }
                />
              </label>
            ))}
          </div>
          <p className={`hint ${Math.abs(weightSum - 1) > 0.001 ? 'text-danger' : ''}`}>
            权重合计 {weightSum.toFixed(2)}，应为 1.00。缺少外部版本数据时不生成虚假统计。
          </p>
        </section>
        <section className="panel settings-panel">
          <h2>外观与悬浮窗</h2>
          <p>在游戏旁显示推荐，保持选人操作区域清晰。</p>
          <div className="theme-control">
            <button
              className={`button ${settings.theme === 'dark' ? 'selected' : ''}`}
              aria-pressed={settings.theme === 'dark'}
              onClick={() => patch({ theme: 'dark' })}
            >
              <Moon size={17} aria-hidden="true" />
              深色
            </button>
            <button
              className={`button ${settings.theme === 'light' ? 'selected' : ''}`}
              aria-pressed={settings.theme === 'light'}
              onClick={() => patch({ theme: 'light' })}
            >
              <Sun size={17} aria-hidden="true" />
              浅色
            </button>
          </div>
          <label className="field">
            <span>悬浮窗不透明度 {Math.round(settings.overlay_opacity * 100)}%</span>
            <input
              type="range"
              min="0.5"
              max="1"
              step="0.05"
              value={settings.overlay_opacity}
              onChange={(event) => patch({ overlay_opacity: Number(event.target.value) })}
            />
          </label>
          <label className="toggle-row">
            <span>紧凑模式</span>
            <input
              type="checkbox"
              checked={settings.overlay_compact}
              onChange={(event) => patch({ overlay_compact: event.target.checked })}
            />
          </label>
          <label className="toggle-row">
            <span>非交互点击穿透</span>
            <input
              type="checkbox"
              checked={settings.overlay_click_through}
              onChange={(event) => patch({ overlay_click_through: event.target.checked })}
            />
          </label>
          <p className="hint">
            点击穿透后，在此关闭可恢复操作。Ctrl+Shift+F8 显示/隐藏悬浮窗；Ctrl+Shift+F9
            恢复交互并显示悬浮窗。
          </p>
          <button
            className="button"
            disabled={Boolean(busy)}
            onClick={() => {
              void action('打开悬浮窗', api.openOverlay);
            }}
          >
            <ExternalLink size={16} aria-hidden="true" />
            打开悬浮窗
          </button>
        </section>
      </div>
      <section className="panel settings-panel">
        <div className="panel-header">
          <div>
            <h2>游戏窗口与识别</h2>
            <p>{snapshot.capture.message}</p>
          </div>
          <span className="status-pill">
            <i />
            {snapshot.capture.status}
          </span>
        </div>
        <div className="capture-controls">
          <label className="field">
            <span>采样频率</span>
            <select
              value={settings.capture_hz}
              onChange={(event) => patch({ capture_hz: Number(event.target.value) })}
            >
              <option value={1}>每秒 1 次</option>
              <option value={2}>每秒 2 次</option>
            </select>
          </label>
          <button
            className="button primary"
            disabled={Boolean(busy) || !snapshot.capture.supported}
            onClick={() => {
              void action('启动识别', api.startCapture);
            }}
          >
            <Crosshair size={16} aria-hidden="true" />
            启动识别
          </button>
          <button
            className="button"
            disabled={Boolean(busy) || !snapshot.capture.supported}
            onClick={() => {
              void action('停止识别', api.stopCapture);
            }}
          >
            <Square size={15} aria-hidden="true" />
            停止识别
          </button>
        </div>
        <div className="diagnostics">
          <span>
            游戏窗口 <strong>{snapshot.capture.window_title ?? '未发现'}</strong>
          </span>
          <span>
            捕获 <strong>{decimal(snapshot.capture.capture_ms)} ms</strong>
          </span>
          <span>
            识别 <strong>{decimal(snapshot.capture.recognition_ms)} ms</strong>
          </span>
          <span>
            推荐 <strong>{decimal(snapshot.capture.recommendation_ms)} ms</strong>
          </span>
        </div>
        <label className="field">
          <span>识别文件路径配置（JSON）</span>
          <textarea
            rows={4}
            spellCheck={false}
            placeholder={
              '{\n  "profile_path": "C:\\\\DPDA\\\\profile.json",\n  "templates_path": "C:\\\\DPDA\\\\manifest.json"\n}'
            }
            value={settings.calibration_json}
            onChange={(event) => patch({ calibration_json: event.target.value })}
          />
        </label>
        <p className="hint">
          先保存设置，再启动识别。配置绑定截图尺寸、DPI 与 UI
          版本；尺寸改变后须重新校准。下方工具可导出校准文件，英雄头像模板需另外准备真实来源。
        </p>
        <Calibration />
      </section>
      <section className="panel settings-panel privacy-panel">
        <h2>数据与隐私</h2>
        <p>
          战绩、英雄池和配置保存在本机
          SQLite。同步仅请求公开历史数据；当前对局不查询对手身份，不读取游戏内存，不注入进程，不监听游戏网络。截图校准在本机完成，不上传图片。
        </p>
        <p className="hint">
          数据覆盖由公开状态、数据平台历史留存和配额共同决定。输入账号 ID 仅是公开数据查询，并非
          Steam 登录。
        </p>
      </section>
    </div>
  );
}
