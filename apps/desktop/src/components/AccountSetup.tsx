import { useEffect, useId, useRef, useState } from 'react';
import { ArrowRight, LoaderCircle, RefreshCw, ShieldCheck, UserRound } from 'lucide-react';
import { api, desktopAvailable } from '../services/api';
import { dateTime } from '../services/format';
import { action } from '../stores/app-store';
import { useApp } from '../hooks/useApp';
import type { SteamDiscoveryReport } from '../types';

type DiscoveryState =
  | { phase: 'idle' | 'loading' }
  | { phase: 'ready'; report: SteamDiscoveryReport }
  | { phase: 'error'; message: string };

const discoveryMessages: Record<SteamDiscoveryReport['status'], string> = {
  found: '本机账号记录已读取。',
  partial: '部分记录未能读取。请核对并选择账号，或检查检测说明后重试。',
  not_installed: '未找到 Steam 安装记录。请安装并登录 Steam 后重新检测，或手动输入账号。',
  no_records: '未发现本机账号记录。请在 Steam 客户端登录一次后重新检测，或手动输入账号。',
  permission_denied: '没有权限读取 Steam 记录。请检查文件访问权限后重试，或手动输入账号。',
  invalid_format: 'Steam 记录格式无法解析。请打开 Steam 更新登录记录后重试，或手动输入账号。',
  io_error: 'Steam 记录暂时无法读取。请稍后重试，或手动输入账号。',
  unsupported_platform: '当前系统不支持自动发现 Steam 账号。你仍可手动输入账号。',
};

export function AccountSetup({ compact = false }: { compact?: boolean }) {
  const { snapshot, busy } = useApp();
  const [input, setInput] = useState(snapshot.profile?.account_id.toString() ?? '');
  const [editing, setEditing] = useState(!snapshot.profile);
  const [discovery, setDiscovery] = useState<DiscoveryState>({ phase: 'idle' });
  const [attempt, setAttempt] = useState(0);
  const [selected, setSelected] = useState<number | null>(snapshot.profile?.account_id ?? null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [fieldError, setFieldError] = useState('');
  const hintId = useId();
  const errorId = useId();
  const candidatesId = useId();
  useEffect(() => {
    if (!editing || !desktopAvailable) return;
    let active = true;
    setDiscovery({ phase: 'loading' });
    api.discoverSteamAccounts().then(
      (report) => {
        if (!active) return;
        setDiscovery({ phase: 'ready', report });
        setSelected(
          report.candidates.some(
            (candidate) => candidate.account_id === report.suggested_account_id,
          )
            ? report.suggested_account_id
            : null,
        );
      },
      (error: unknown) => {
        if (active) setDiscovery({ phase: 'error', message: String(error) });
      },
    );
    return () => {
      active = false;
    };
  }, [editing, attempt]);

  async function save(value: string) {
    if (await action('保存账号', () => api.saveAccount(value))) setEditing(false);
  }
  async function submit(event: React.SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!/^\d+$/.test(input.trim())) {
      setFieldError('请输入纯数字 Steam32 或 Steam64 ID。');
      inputRef.current?.focus();
      return;
    }
    setFieldError('');
    // Keep Steam64 decimal text intact; only the backend normalizes its value.
    await save(input.trim());
  }
  const report = discovery.phase === 'ready' ? discovery.report : null;
  const selectedCandidate = report?.candidates.find(
    (candidate) => candidate.account_id === selected,
  );
  return (
    <section className={`account-setup ${compact ? 'compact' : ''}`}>
      {!compact && (
        <div className="setup-art" aria-hidden="true">
          <div className="draft-emblem">
            <span />
            <span />
            <span />
          </div>
        </div>
      )}
      <div className="setup-content">
        <span className="section-kicker">
          {snapshot.profile ? '公开账号' : '开始建立你的英雄池'}
        </span>
        <h2>{snapshot.profile ? '账号与数据' : '让熟悉的英雄，成为更好的选择。'}</h2>
        <p>从本机 Steam 记录中选择账号，或手动输入公开账号 ID。</p>
        {snapshot.profile && (
          <div className="current-account">
            <UserRound size={20} aria-hidden="true" />
            <div>
              <strong>当前分析账号 · {snapshot.profile.account_id}</strong>
              <span>Steam64 · {snapshot.profile.steam64}</span>
            </div>
            <button
              className="button"
              disabled={Boolean(busy)}
              onClick={() => setEditing((value) => !value)}
              aria-expanded={editing}
              aria-controls={candidatesId}
            >
              {editing ? '取消切换' : '切换账号'}
            </button>
          </div>
        )}
        {editing && (
          <div className="account-picker" id={candidatesId}>
            {desktopAvailable ? (
              <div className="steam-discovery" aria-busy={discovery.phase === 'loading'}>
                <div className="steam-discovery-heading">
                  <h3>本机 Steam 账号</h3>
                  <button
                    className="text-button"
                    disabled={Boolean(busy) || discovery.phase === 'loading'}
                    onClick={() => setAttempt((value) => value + 1)}
                  >
                    <RefreshCw size={14} aria-hidden="true" />
                    重新检测
                  </button>
                </div>
                {discovery.phase === 'loading' && (
                  <p className="discovery-message" role="status">
                    <LoaderCircle size={16} className="spin" aria-hidden="true" />
                    正在读取本机 Steam 账号记录…
                  </p>
                )}
                {discovery.phase === 'error' && (
                  <div className="discovery-message discovery-error" role="alert">
                    <p>本机账号暂时无法读取。可以重新检测，或在下方手动输入。</p>
                    <span>{discovery.message}</span>
                  </div>
                )}
                {report && (
                  <>
                    {report.candidates.length ? (
                      <>
                        {report.status !== 'found' && (
                          <p className="discovery-message" role="status">
                            {discoveryMessages[report.status]}
                          </p>
                        )}
                        <fieldset
                          className="steam-candidates"
                          disabled={Boolean(busy)}
                          aria-describedby={hintId}
                        >
                          <legend className="sr-only">选择本机 Steam 账号</legend>
                          {report.candidates.map((candidate) => (
                            <label
                              key={candidate.account_id}
                              className={`steam-candidate ${selected === candidate.account_id ? 'selected' : ''}`}
                            >
                              <input
                                type="radio"
                                name="steam-account"
                                value={candidate.account_id}
                                checked={selected === candidate.account_id}
                                onChange={() => setSelected(candidate.account_id)}
                              />
                              <div className="steam-candidate-info">
                                <div className="steam-candidate-title">
                                  <strong>
                                    {candidate.display_name || `账号 ${candidate.account_id}`}
                                  </strong>
                                  {candidate.most_recent && <span>本机最近使用</span>}
                                </div>
                                <span>Steam64 · {candidate.steam64}</span>
                                <small>
                                  Account ID · {candidate.account_id}
                                  {candidate.last_used_at != null &&
                                    ` · 记录时间 ${dateTime(candidate.last_used_at)}`}
                                </small>
                              </div>
                            </label>
                          ))}
                        </fieldset>
                        <div className="account-select-actions">
                          <button
                            className="button primary"
                            disabled={Boolean(busy) || !selectedCandidate}
                            onClick={() => {
                              if (selectedCandidate)
                                void save(String(selectedCandidate.account_id));
                            }}
                          >
                            {busy === '保存账号' ? '正在保存…' : '使用此账号'}
                            <ArrowRight size={17} aria-hidden="true" />
                          </button>
                          <span>保存后，在“历史数据同步”中开始导入战绩。</span>
                        </div>
                      </>
                    ) : (
                      <p className="discovery-message" role="status">
                        {discoveryMessages[report.status]}
                      </p>
                    )}
                    {report.diagnostics.length > 0 && (
                      <details className="discovery-diagnostics">
                        <summary>查看检测说明（{report.diagnostics.length}）</summary>
                        <ul>
                          {report.diagnostics.map((diagnostic, index) => (
                            <li key={`${diagnostic.code}-${index}`}>
                              {diagnostic.message}
                              <code>
                                {diagnostic.source} · {diagnostic.code}
                              </code>
                            </li>
                          ))}
                        </ul>
                      </details>
                    )}
                  </>
                )}
              </div>
            ) : (
              <p className="discovery-message">
                浏览器预览无法读取本机 Steam 记录。请打开桌面应用进行自动检测。
              </p>
            )}
            <details className="manual-account">
              <summary>手动输入账号 ID</summary>
              <form onSubmit={submit} className="account-form">
                <label className="field">
                  <span>Steam32 Account ID 或 Steam64 ID</span>
                  <input
                    ref={inputRef}
                    name="account_id"
                    spellCheck={false}
                    inputMode="numeric"
                    autoComplete="off"
                    value={input}
                    onChange={(event) => {
                      setInput(event.target.value);
                      setFieldError('');
                    }}
                    placeholder="输入你的公开账号 ID"
                    aria-invalid={Boolean(fieldError)}
                    aria-describedby={`${hintId}${fieldError ? ` ${errorId}` : ''}`}
                  />
                </label>
                <button className="button primary" type="submit" disabled={Boolean(busy)}>
                  {busy === '保存账号'
                    ? '正在保存…'
                    : snapshot.profile
                      ? '保存手动账号'
                      : '关联账号'}
                  <ArrowRight size={17} aria-hidden="true" />
                </button>
              </form>
              {fieldError && (
                <p className="field-error" role="alert" id={errorId}>
                  {fieldError}
                </p>
              )}
            </details>
          </div>
        )}
        <p className="privacy-note" id={hintId}>
          <ShieldCheck size={15} aria-hidden="true" />
          只读本机记录，不检测在线状态，也不代表 Steam 身份认证。保存账号不会自动联网同步。
        </p>
      </div>
    </section>
  );
}
