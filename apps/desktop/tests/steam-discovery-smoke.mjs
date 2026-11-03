// Synthetic IPC is used only here to exercise UI states. This does not validate local Steam discovery.
// No real Steam files, personal history, passwords, or network APIs are read by this test.
import { chromium } from 'playwright';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '../../..');
const base = process.env.DPDA_STEAM_UI_URL ?? 'http://127.0.0.1:1423';
const output = resolve(root, 'artifacts/steam-discovery/ui-synthetic');
mkdirSync(output, { recursive: true });
let server;
async function ready() {
  try {
    return (await fetch(base, { signal: AbortSignal.timeout(800) })).ok;
  } catch {
    return false;
  }
}
if (!(await ready())) {
  assert.ok(!process.env.DPDA_STEAM_UI_URL, 'Configured Vite server is unavailable.');
  server = spawn(
    process.execPath,
    [
      resolve(import.meta.dirname, '../node_modules/vite/bin/vite.js'),
      '--host',
      '127.0.0.1',
      '--port',
      '1423',
      '--strictPort',
    ],
    { cwd: resolve(import.meta.dirname, '..'), stdio: 'ignore' },
  );
  for (let i = 0; i < 80 && !(await ready()); i++)
    await new Promise((done) => setTimeout(done, 100));
  if (!(await ready())) {
    server.kill();
    throw new Error('Steam account UI test server failed to start.');
  }
}
const browser = await chromium.launch({
  executablePath:
    process.env.DPDA_CHROME_PATH ??
    (existsSync('/usr/bin/google-chrome') ? '/usr/bin/google-chrome' : undefined),
  headless: true,
  args: ['--no-sandbox'],
});
const errors = [];
const external = [];
const checks = [];
const candidate = (account_id, most_recent = false) => ({
  account_id,
  steam64: String(76561197960265728n + BigInt(account_id)),
  display_name: `合成 UI 测试账号 ${account_id}`,
  most_recent,
  last_used_at: null,
});
const report = (candidates, suggested_account_id = null, status = 'found') => ({
  status,
  candidates,
  suggested_account_id,
  diagnostics: [],
  locations_checked: 2,
});
async function pageWith(state = null) {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
    reducedMotion: 'reduce',
  });
  await context.route('**/*', (route) => {
    if (new URL(route.request().url()).origin === new URL(base).origin) return route.continue();
    external.push(route.request().url());
    return route.abort();
  });
  if (state)
    await context.addInitScript((initial) => {
      window.isTauri = true;
      const synthetic = (window.__syntheticSteam = { ...initial, calls: [], pending: [] });
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        transformCallback() {
          return 1;
        },
        async invoke(command, args) {
          synthetic.calls.push({ command, args });
          if (command === 'get_snapshot') {
            const { emptySnapshot } = await import('/src/services/api.ts');
            const snapshot = structuredClone(emptySnapshot);
            snapshot.profile = synthetic.profile ?? null;
            snapshot.sync.status = synthetic.syncStatus ?? 'idle';
            return snapshot;
          }
          if (command === 'discover_steam_accounts') {
            if (synthetic.deferred) await new Promise((done) => synthetic.pending.push(done));
            if (synthetic.discoveryError) throw new Error('合成测试：读取失败');
            return structuredClone(synthetic.report);
          }
          if (command === 'save_account') {
            if (synthetic.saveError) throw new Error('合成测试：账号保存失败');
            const value = BigInt(args.input);
            const accountId = value > 4294967295n ? value - 76561197960265728n : value;
            synthetic.profile = {
              account_id: Number(accountId),
              steam64: String(76561197960265728n + accountId),
            };
            return;
          }
          if (command === 'start_sync') {
            synthetic.syncStatus = 'running';
            return;
          }
          if (command === 'pause_sync') {
            synthetic.syncStatus = 'paused';
            return;
          }
          if (command.startsWith('plugin:event|')) return 1;
          throw new Error(`Unexpected synthetic IPC: ${command}`);
        },
      };
    }, state);
  const page = await context.newPage();
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(base);
  await page.getByRole('heading', { name: '选好下一场。', exact: true }).waitFor();
  return page;
}
const calls = (page, command) =>
  page.evaluate(
    (name) => window.__syntheticSteam.calls.filter((call) => call.command === name),
    command,
  );
const radio = (page, id) => page.locator(`input[name="steam-account"][value="${id}"]`);
async function settled(page) {
  await page.getByRole('button', { name: '重新检测', exact: true }).waitFor();
  await page.waitForFunction(
    () => document.querySelector('.steam-discovery')?.getAttribute('aria-busy') === 'false',
  );
}
async function updateReport(page, next) {
  await page.evaluate((value) => {
    window.__syntheticSteam.report = value;
  }, next);
  await page.getByRole('button', { name: '重新检测', exact: true }).click();
  await settled(page);
}
try {
  const preview = await pageWith();
  await preview
    .getByText('浏览器预览无法读取本机 Steam 记录。请打开桌面应用进行自动检测。', { exact: true })
    .waitFor();
  assert.equal(await preview.locator('input[name="steam-account"]').count(), 0);
  assert.equal(await preview.getByRole('button', { name: '重新检测', exact: true }).count(), 0);
  assert.equal(
    await preview.getByLabel('Steam32 Account ID 或 Steam64 ID', { exact: true }).isVisible(),
    false,
  );
  checks.push('browser preview reports unavailable local discovery and has no fake candidates');
  await preview.context().close();

  const page = await pageWith({
    report: report([candidate(42), candidate(43, true)], 43),
    deferred: true,
  });
  await page.getByText('正在读取本机 Steam 账号记录…', { exact: true }).waitFor();
  assert.equal((await calls(page, 'save_account')).length, 0);
  await page.evaluate(() => {
    window.__syntheticSteam.deferred = false;
    window.__syntheticSteam.pending.splice(0).forEach((done) => done());
  });
  await settled(page);
  assert.equal(await radio(page, 43).isChecked(), true);
  assert.equal(await radio(page, 42).isChecked(), false);
  await page.getByText('Steam64 · 76561197960265771', { exact: true }).waitFor();
  assert.equal((await calls(page, 'start_sync')).length, 0);
  await radio(page, 43).focus();
  await page.keyboard.press('ArrowUp');
  assert.equal(
    await radio(page, 42).isChecked(),
    true,
    'Native radio keyboard navigation selects the previous account.',
  );
  await page.getByRole('button', { name: '使用此账号', exact: true }).click();
  await page.getByText('Account 42', { exact: true }).waitFor();
  assert.deepEqual(
    (await calls(page, 'save_account')).map((call) => call.args.input),
    ['42'],
  );
  assert.equal(
    (await calls(page, 'start_sync')).length,
    0,
    'Saving an account never starts a sync.',
  );
  await page.getByRole('button', { name: '同步所选范围', exact: true }).click();
  await page.getByRole('button', { name: '暂停同步', exact: true }).waitFor();
  assert.equal((await calls(page, 'start_sync')).length, 1);
  await page.getByRole('button', { name: '暂停同步', exact: true }).click();
  await page.getByText('已暂停', { exact: true }).waitFor();
  checks.push(
    'loading, suggested preselection, exact Steam64 display, keyboard selection, explicit save, separate sync/pause',
  );
  await page.context().close();

  const ambiguous = await pageWith({ report: report([candidate(43, true)], null, 'partial') });
  await settled(ambiguous);
  assert.equal(
    await radio(ambiguous, 43).isChecked(),
    false,
    'Partial singleton with MostRecent must not be guessed by the UI.',
  );
  assert.equal(
    await ambiguous.getByRole('button', { name: '使用此账号', exact: true }).isDisabled(),
    true,
  );
  await updateReport(ambiguous, report([candidate(42), candidate(43)], null));
  assert.equal(await ambiguous.locator('input[name="steam-account"]:checked').count(), 0);
  await ambiguous.evaluate(() => {
    window.__syntheticSteam.saveError = true;
  });
  await radio(ambiguous, 42).check();
  await ambiguous.getByRole('button', { name: '使用此账号', exact: true }).click();
  await ambiguous.getByText('Error: 合成测试：账号保存失败', { exact: true }).waitFor();
  assert.equal(await radio(ambiguous, 42).isChecked(), true);
  await ambiguous.evaluate(() => {
    window.__syntheticSteam.saveError = false;
  });
  await ambiguous.getByRole('button', { name: '使用此账号', exact: true }).click();
  await ambiguous.getByText('Account 42', { exact: true }).waitFor();
  checks.push('partial/ambiguous accounts have no guessed preselection; failed save is retryable');
  await ambiguous.context().close();

  const states = await pageWith({ report: report([], null, 'no_records') });
  await settled(states);
  const expected = {
    not_installed: '未找到 Steam 安装记录。',
    no_records: '未发现本机账号记录。',
    permission_denied: '没有权限读取 Steam 记录。',
    invalid_format: 'Steam 记录格式无法解析。',
    io_error: 'Steam 记录暂时无法读取。',
    unsupported_platform: '当前系统不支持自动发现 Steam 账号。',
  };
  for (const [status, text] of Object.entries(expected)) {
    const next = report([], null, status);
    next.diagnostics = [
      { code: `synthetic_${status}`, source: '合成测试来源', message: `合成测试：${status}` },
    ];
    await updateReport(states, next);
    await states.getByText(text, { exact: false }).waitFor();
    assert.equal(await states.getByRole('button', { name: '使用此账号', exact: true }).count(), 0);
  }
  await states.locator('.discovery-diagnostics > summary').click();
  await states.getByText('合成测试：unsupported_platform', { exact: false }).waitFor();
  await states.evaluate(() => {
    window.__syntheticSteam.discoveryError = true;
  });
  await states.getByRole('button', { name: '重新检测', exact: true }).click();
  await states
    .getByText('本机账号暂时无法读取。可以重新检测，或在下方手动输入。', { exact: true })
    .waitFor();
  await states.evaluate(() => {
    window.__syntheticSteam.discoveryError = false;
  });
  await updateReport(states, report([candidate(44)], 44));
  assert.equal(await radio(states, 44).isChecked(), true);
  await states.locator('.manual-account > summary').click();
  const input = states.getByLabel('Steam32 Account ID 或 Steam64 ID', { exact: true });
  await input.fill('bad-id');
  await states.getByRole('button', { name: '关联账号', exact: true }).click();
  await states.getByText('请输入纯数字 Steam32 或 Steam64 ID。', { exact: true }).waitFor();
  assert.equal(await input.evaluate((element) => document.activeElement === element), true);
  const preciseId = '76561202255233022';
  await input.fill(preciseId);
  await states.getByRole('button', { name: '关联账号', exact: true }).click();
  await states.getByText('Account 4294967294', { exact: true }).waitFor();
  assert.deepEqual(
    (await calls(states, 'save_account')).map((call) => call.args.input),
    [preciseId],
  );
  assert.equal((await calls(states, 'start_sync')).length, 0);
  checks.push(
    'six actionable empty/error states, diagnostics, IPC failure retry, manual fallback, validation focus, Steam64 precision',
  );
  await states.context().close();

  const existing = await pageWith({
    profile: { account_id: 44, steam64: candidate(44).steam64 },
    report: report([candidate(43, true)], 43),
  });
  await existing.getByRole('navigation').getByRole('button', { name: '设置', exact: true }).click();
  await existing.getByText('当前分析账号 · 44', { exact: true }).waitFor();
  assert.equal((await calls(existing, 'discover_steam_accounts')).length, 0);
  await existing.getByRole('button', { name: '切换账号', exact: true }).click();
  await settled(existing);
  await existing.getByText('当前分析账号 · 44', { exact: true }).waitFor();
  assert.equal((await calls(existing, 'save_account')).length, 0);
  await existing.getByRole('button', { name: '取消切换', exact: true }).click();
  assert.equal(await existing.locator('input[name="steam-account"]').count(), 0);
  await existing.getByRole('button', { name: '切换账号', exact: true }).click();
  await settled(existing);
  await existing.getByRole('button', { name: '使用此账号', exact: true }).click();
  await existing.getByText('当前分析账号 · 43', { exact: true }).waitFor();
  assert.equal((await calls(existing, 'save_account')).length, 1);
  assert.equal((await calls(existing, 'start_sync')).length, 0);
  checks.push(
    'existing profile preserved until explicit switch; cancel closes candidates without mutation',
  );
  await existing.context().close();

  const long = candidate(42);
  long.display_name =
    '合成测试超长昵称_' + 'AccountName'.repeat(14) + '<img src=x onerror=alert(1)>';
  const layouts = await pageWith({ report: report([long, candidate(43, true)], 43) });
  await settled(layouts);
  for (const width of [1440, 1024, 375]) {
    await layouts.setViewportSize({ width, height: width === 375 ? 812 : 1000 });
    assert.ok(
      await layouts.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1),
      `No horizontal overflow at ${width}px.`,
    );
    assert.equal(await layouts.locator('.steam-candidates img').count(), 0);
    await layouts.screenshot({
      path: resolve(output, `synthetic-candidates-${width}.png`),
      fullPage: true,
    });
  }
  checks.push('1440/1024/375 layouts, long nickname wrapping, escaped markup and no remote images');
  await layouts.context().close();
  assert.deepEqual(errors, []);
  assert.deepEqual(external, []);
  writeFileSync(
    resolve(output, 'report.json'),
    JSON.stringify(
      {
        date: new Date().toISOString(),
        evidence:
          'Synthetic browser IPC states only; actual Steam discovery and platform integration require native tests.',
        checks,
        runtime_errors: errors,
        external_requests: external,
      },
      null,
      2,
    ),
  );
  console.log(
    `PASS: ${checks.length} Steam account UI test groups; synthetic IPC only, no external requests.`,
  );
} finally {
  await browser.close();
  server?.kill();
}
