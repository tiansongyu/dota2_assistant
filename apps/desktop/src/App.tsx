import { Component, useEffect, useState } from 'react';
import type { ErrorInfo, ReactNode } from 'react';
import {
  Activity,
  ArrowLeft,
  ChartNoAxesCombined,
  ChevronRight,
  CircleHelp,
  Database,
  Expand,
  History as HistoryIcon,
  Images,
  LayoutDashboard,
  LoaderCircle,
  Minimize2,
  Monitor,
  Settings as SettingsIcon,
  Shield,
  Swords,
  Users,
  X,
} from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Dashboard } from './pages/Dashboard';
import { HeroPool } from './pages/HeroPool';
import { History } from './pages/History';
import { DraftPage } from './pages/Draft';
import { Settings } from './pages/Settings';
import { GameAssets } from './pages/GameAssets';
import { Recommendations } from './components/Recommendations';
import { useApp } from './hooks/useApp';
import { action, appStore, connect, refresh } from './stores/app-store';
import { api, desktopAvailable } from './services/api';
import { heroName, roles } from './services/format';
import type { Page } from './types';

const nav = [
  { id: 'dashboard', label: '总览', icon: LayoutDashboard },
  { id: 'pool', label: '我的英雄池', icon: Users },
  { id: 'history', label: '历史战绩', icon: HistoryIcon },
  { id: 'draft', label: 'BP 推荐', icon: Swords },
  { id: 'assets', label: '游戏资源', icon: Images },
  { id: 'settings', label: '设置', icon: SettingsIcon },
] as const;
class ErrorBoundary extends Component<{ children: ReactNode }, { error: string | null }> {
  state = { error: null as string | null };
  static getDerivedStateFromError(error: Error) {
    return { error: error.message };
  }
  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('DPDA UI error', error, info.componentStack);
  }
  render() {
    return this.state.error ? (
      <div className="fatal-error">
        <h1>界面暂时无法显示</h1>
        <p>{this.state.error}</p>
        <button className="button primary" onClick={() => window.location.reload()}>
          重新加载界面
        </button>
        <p>本地数据库不会因重新加载而删除。</p>
      </div>
    ) : (
      this.props.children
    );
  }
}
function Overlay() {
  const { snapshot, error, busy } = useApp();
  const compact = snapshot.settings.overlay_compact;
  const enemies = snapshot.draft[snapshot.draft.own_team === 'radiant' ? 'dire' : 'radiant'].filter(
    (id): id is number => id !== null,
  );
  return (
    <div
      className={`overlay-content ${compact ? 'compact' : ''}`}
      style={{
        backgroundColor: `color-mix(in srgb, var(--surface) ${snapshot.settings.overlay_opacity * 100}%, transparent)`,
      }}
    >
      <header className="overlay-header" data-tauri-drag-region>
        <Swords size={18} aria-hidden="true" />
        <strong data-tauri-drag-region>DPDA</strong>
        <span data-tauri-drag-region>
          {snapshot.settings.selected_role === 0
            ? '自动位置'
            : roles.find((role) => role.value === snapshot.settings.selected_role)?.label}
        </span>
        <button
          className="icon-button"
          aria-label={compact ? '展开悬浮窗' : '收起悬浮窗'}
          disabled={Boolean(busy)}
          onClick={() => {
            void action('切换悬浮窗模式', () =>
              api.saveSettings({ ...snapshot.settings, overlay_compact: !compact }),
            );
          }}
        >
          {compact ? <Expand size={15} /> : <Minimize2 size={15} />}
        </button>
        <button
          className="icon-button"
          aria-label="隐藏悬浮窗"
          onClick={() => {
            if (desktopAvailable) void action('隐藏悬浮窗', () => getCurrentWindow().hide());
          }}
        >
          <X size={16} />
        </button>
      </header>
      <div className="overlay-enemies">
        <span>敌方已确认</span>
        <p>
          {enemies.length
            ? enemies.map((id) => heroName(snapshot.heroes, id)).join(' / ')
            : '尚未确认英雄'}
        </p>
      </div>
      {error && <p className="inline-error">{error}</p>}
      <Recommendations compact={compact} />
      <footer>
        推荐分 ≠ 获胜概率<span>Ctrl+Shift+F8 隐藏</span>
      </footer>
    </div>
  );
}
function Shell() {
  const [page, setPage] = useState<Page>('dashboard');
  const { snapshot, loading, busy, error } = useApp();
  const isOverlay = new URLSearchParams(window.location.search).get('overlay') === '1';
  useEffect(connect, []);
  useEffect(() => {
    document.documentElement.dataset.theme = snapshot.settings.theme;
    document.body.classList.toggle('overlay-body', isOverlay);
  }, [snapshot.settings.theme, isOverlay]);
  if (isOverlay) return <Overlay />;
  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">
        跳至内容
      </a>
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-symbol">
            <Swords size={24} aria-hidden="true" />
          </div>
          <div>
            <strong>DPDA</strong>
            <span>Personal Draft Assistant</span>
          </div>
        </div>
        <nav aria-label="主要导航">
          {nav.map((item) => (
            <button
              key={item.id}
              className={`nav-item ${page === item.id ? 'active' : ''}`}
              aria-label={item.label}
              aria-current={page === item.id ? 'page' : undefined}
              onClick={() => setPage(item.id)}
            >
              <item.icon size={19} aria-hidden="true" />
              <span>{item.label}</span>
              {page === item.id && (
                <ChevronRight className="nav-chevron" size={15} aria-hidden="true" />
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="local-label">
            <Shield size={16} aria-hidden="true" />
            <span>本地分析 · 公开数据</span>
          </div>
          <button className="help-link" onClick={() => setPage('settings')}>
            <CircleHelp size={17} aria-hidden="true" />
            <span>数据与使用说明</span>
          </button>
          <span className="app-version">
            DPDA 0.1.0 <span>开发版本</span>
          </span>
        </div>
      </aside>
      <div className="workspace">
        <header className="topbar">
          <div className="breadcrumb">
            <span>个人工作台</span>
            <ChevronRight size={14} aria-hidden="true" />
            <strong>{nav.find((item) => item.id === page)?.label}</strong>
          </div>
          <div className="topbar-status">
            {busy ? (
              <span className="busy-label" role="status">
                <LoaderCircle size={14} className="spin" aria-hidden="true" />
                {busy}…
              </span>
            ) : (
              <span className="connection-status">
                <Database size={14} aria-hidden="true" />
                {desktopAvailable ? '本地数据' : '界面预览'}
              </span>
            )}
            <span className="account-badge">
              <i />
              {snapshot.profile ? `Account ${snapshot.profile.account_id}` : '未关联账号'}
            </span>
          </div>
        </header>
        <main id="main-content" tabIndex={-1}>
          {!desktopAvailable && (
            <div className="preview-notice">
              <Monitor size={17} aria-hidden="true" />
              <span>
                浏览器界面预览。请启动 Tauri 桌面应用以保存设置、导入真实战绩及捕获游戏窗口。
              </span>
            </div>
          )}
          {error && (
            <div className="error-banner" role="alert">
              <Activity size={17} aria-hidden="true" />
              <span>{error}</span>
              <button
                className="text-button"
                onClick={() => {
                  appStore.clearError();
                  void refresh();
                }}
              >
                重试读取
              </button>
              <button
                className="icon-button"
                aria-label="关闭错误提示"
                onClick={appStore.clearError}
              >
                <X size={17} />
              </button>
            </div>
          )}
          {loading && page !== 'assets' ? (
            <div className="loading-state" role="status">
              <LoaderCircle className="spin" size={28} />
              <h2>正在读取本地数据</h2>
              <p>初始化数据库和个人配置。</p>
            </div>
          ) : (
            <div key={`${page}-${snapshot.profile?.account_id ?? 'none'}`}>
              {page === 'dashboard' && <Dashboard navigate={setPage} />}
              {page === 'pool' && <HeroPool />}
              {page === 'history' && <History />}
              {page === 'draft' && <DraftPage />}
              {page === 'assets' && <GameAssets />}
              {page === 'settings' && <Settings />}
            </div>
          )}
        </main>
        <footer className="statusbar">
          <span>
            <ChartNoAxesCombined size={13} aria-hidden="true" />
            仅分析可访问的历史与界面可见信息
          </span>
          <button onClick={() => setPage('draft')}>
            <ArrowLeft size={12} aria-hidden="true" />
            BP 工作台
          </button>
        </footer>
      </div>
    </div>
  );
}
export function App() {
  return (
    <ErrorBoundary>
      <Shell />
    </ErrorBoundary>
  );
}
