import { useRef, useState } from 'react';
import { ArrowRight, ShieldCheck } from 'lucide-react';
import { api } from '../services/api';
import { action } from '../stores/app-store';
import { useApp } from '../hooks/useApp';

export function AccountSetup({ compact = false }: { compact?: boolean }) {
  const { snapshot, busy } = useApp();
  const [input, setInput] = useState(snapshot.profile?.account_id.toString() ?? '');
  const inputRef = useRef<HTMLInputElement>(null);
  const [fieldError, setFieldError] = useState('');
  async function submit(event: React.SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!/^\d+$/.test(input.trim())) {
      setFieldError('请输入纯数字 Steam32 或 Steam64 ID。');
      inputRef.current?.focus();
      return;
    }
    setFieldError('');
    await action('保存账号', () => api.saveAccount(input.trim()));
  }
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
        <p>关联公开 DOTA2 账号，从真实历史比赛中了解你的英雄池与阵容表现。</p>
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
              aria-describedby="account-hint"
            />
          </label>
          <button className="button primary" type="submit" disabled={Boolean(busy)}>
            {busy === '保存账号' ? '正在保存…' : snapshot.profile ? '切换账号' : '关联账号'}
            <ArrowRight size={17} aria-hidden="true" />
          </button>
        </form>
        {fieldError && (
          <p className="field-error" role="alert">
            {fieldError}
          </p>
        )}
        <p className="privacy-note" id="account-hint">
          <ShieldCheck size={15} aria-hidden="true" />
          仅用于查询公开战绩，不代表 Steam 身份认证。个人分析保存在本机。
        </p>
      </div>
    </section>
  );
}
