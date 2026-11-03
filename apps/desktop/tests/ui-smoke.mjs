// Real Chromium UI checks with an empty local state. No invented match data or IPC mocks.
import { chromium } from 'playwright';
import { existsSync, mkdirSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { unzipSync, strFromU8 } from 'fflate';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';

const root = resolve(import.meta.dirname, '../../..');
const base = process.env.DPDA_UI_URL ?? 'http://127.0.0.1:1421';
let server = null;
async function serverReady() {
  try {
    return (await fetch(base, { signal: AbortSignal.timeout(800) })).ok;
  } catch {
    return false;
  }
}
if (!(await serverReady())) {
  if (process.env.DPDA_UI_URL) throw new Error(`UI server unavailable: ${base}`);
  server = spawn(
    process.execPath,
    [
      resolve(import.meta.dirname, '../node_modules/vite/bin/vite.js'),
      '--host',
      '127.0.0.1',
      '--port',
      '1421',
    ],
    { cwd: resolve(import.meta.dirname, '..'), stdio: 'ignore' },
  );
  for (let attempt = 0; attempt < 60 && !(await serverReady()); attempt++) {
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
  }
  if (!(await serverReady())) {
    server.kill();
    throw new Error('Vite UI server failed to start');
  }
}
const executablePath =
  process.env.DPDA_CHROME_PATH ??
  (existsSync('/usr/bin/google-chrome') ? '/usr/bin/google-chrome' : undefined);
const browser = await chromium.launch({ executablePath, headless: true, args: ['--no-sandbox'] });
const context = await browser.newContext({ reducedMotion: 'reduce' });
const page = await context.newPage();
const errors = [];
page.on('pageerror', (error) => errors.push(error.message));
const report = [];
try {
  for (const viewport of [
    { width: 1440, height: 1000 },
    { width: 1024, height: 768 },
    { width: 375, height: 812 },
  ]) {
    await page.setViewportSize(viewport);
    await page.goto(base);
    await page.getByRole('heading', { name: '选好下一场。' }).waitFor();
    assert.ok(await page.getByText('浏览器界面预览。', { exact: false }).isVisible());
    const pages = [
      ['总览', '选好下一场。'],
      ['我的英雄池', '我的英雄池'],
      ['历史战绩', '历史战绩'],
      ['BP 推荐', 'BP 工作台'],
      ['设置', '设置'],
    ];
    for (const [nav, heading] of pages) {
      await page
        .getByRole('navigation', { name: '主要导航' })
        .getByRole('button', { name: nav, exact: true })
        .click();
      await page.getByRole('heading', { name: heading, exact: true }).waitFor();
      const width = await page.evaluate(() => ({
        viewport: innerWidth,
        document: document.documentElement.scrollWidth,
      }));
      assert.ok(
        width.document <= width.viewport + 1,
        `${nav} overflows at ${viewport.width}: ${JSON.stringify(width)}`,
      );
      report.push({ page: nav, width: viewport.width, overflow: false });
    }
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole('navigation').getByRole('button', { name: '总览', exact: true }).click();
  await page.getByLabel('Steam32 Account ID 或 Steam64 ID', { exact: true }).fill('invalid');
  await page.getByRole('button', { name: '关联账号', exact: true }).click();
  await page.getByText('请输入纯数字 Steam32 或 Steam64 ID。').waitFor();
  await page
    .getByLabel('Steam32 Account ID 或 Steam64 ID', { exact: true })
    .fill('76561198030654385');
  await page.getByRole('button', { name: '关联账号', exact: true }).click();
  await page
    .getByText('当前为浏览器界面预览。请通过 Tauri 桌面应用保存配置和同步真实战绩。')
    .waitFor();
  await page.getByRole('button', { name: '关闭错误提示' }).click();
  await page
    .getByRole('navigation')
    .getByRole('button', { name: '我的英雄池', exact: true })
    .click();
  await page.getByRole('button', { name: '混合英雄池', exact: true }).click();
  await page.getByRole('textbox', { name: '搜索英雄' }).fill('Anti-Mage');
  await page.getByRole('button', { name: 'Anti-Mage 历史推断 · 依据不足', exact: true }).click();
  await page.getByRole('button', { name: '一号位', exact: true }).click();
  assert.equal(
    await page.getByRole('button', { name: '一号位', exact: true }).getAttribute('aria-pressed'),
    'true',
  );
  await page.getByRole('button', { name: '恢复自动推断', exact: true }).click();
  assert.equal(
    await page.getByRole('button', { name: '一号位', exact: true }).getAttribute('aria-pressed'),
    'false',
  );
  await page.getByRole('button', { name: '手动纳入 Anti-Mage', exact: true }).click();
  assert.equal(
    await page
      .getByRole('button', { name: '移除手动纳入 Anti-Mage', exact: true })
      .getAttribute('aria-pressed'),
    'true',
  );
  await page.getByRole('button', { name: '排除 Anti-Mage', exact: true }).click();
  assert.equal(
    await page
      .getByRole('button', { name: '取消排除 Anti-Mage', exact: true })
      .getAttribute('aria-pressed'),
    'true',
  );
  assert.equal(
    await page
      .getByRole('button', { name: '手动纳入 Anti-Mage', exact: true })
      .getAttribute('aria-pressed'),
    'false',
  );
  await page.getByRole('navigation').getByRole('button', { name: '设置', exact: true }).click();
  assert.equal(
    await page.getByRole('combobox', { name: '计划位置', exact: true }).inputValue(),
    '0',
  );
  await page.getByLabel('时间衰减半衰期（天）').fill('5');
  await page.getByRole('button', { name: '保存设置', exact: true }).click();
  await page.getByText('半衰期必须在 7—1825 天之间。').waitFor();
  // Exercise annotation/export plumbing with an actual rendered UI screenshot.
  // This is explicitly NOT a DOTA2 screenshot or recognition accuracy evidence.
  const annotationImage = resolve(tmpdir(), 'dpda-ui-only-calibration-input.png');
  await page.screenshot({ path: annotationImage });
  await page.locator('.calibration > summary').click();
  await page.getByLabel('真实 DOTA2 截图', { exact: true }).setInputFiles(annotationImage);
  await page.locator('.calibration-surface img').waitFor();
  await page.getByLabel('UI / 游戏版本', { exact: true }).fill('UI-TEST-ONLY-NOT-DOTA');
  await page.getByRole('button', { name: '下载校准包', exact: true }).click();
  await page.getByText('请先标注选人界面标记和双方全部 10 个头像槽位。').waitFor();
  const targets = [
    'draft_anchor',
    ...Array.from({ length: 10 }, (_, index) => `portrait_${index}`),
  ];
  for (const [index, target] of targets.entries()) {
    await page.getByLabel('当前标注区域', { exact: true }).selectOption(target);
    await page.getByLabel('x', { exact: true }).fill(String(index * 30));
    await page.getByLabel('y', { exact: true }).fill('40');
    await page.getByLabel('width', { exact: true }).fill('25');
    await page.getByLabel('height', { exact: true }).fill('25');
  }
  const downloadEvent = page.waitForEvent('download');
  await page.getByRole('button', { name: '下载校准包', exact: true }).click();
  const download = await downloadEvent;
  const files = unzipSync(new Uint8Array(readFileSync(await download.path())));
  const calibration = JSON.parse(strFromU8(files['profile.json']));
  assert.equal(calibration.not_validated, true);
  assert.equal(calibration.slots.length, 10);
  assert.equal(calibration.surface_width, 1440);
  assert.equal(calibration.ui_version, 'UI-TEST-ONLY-NOT-DOTA');
  assert.equal(calibration.slots[0].locked_marker, null);
  assert.equal(Object.keys(files).filter((name) => name.endsWith('.png')).length, 11);
  await page.getByRole('navigation').getByRole('button', { name: 'BP 推荐', exact: true }).click();
  await page.locator('.capture-diagnostic-panel > summary').click();
  await page.getByText('请在桌面应用中读取捕获诊断。').waitFor();
  await page.goto(`${base}/?overlay=1`);
  await page.setViewportSize({ width: 360, height: 440 });
  await page.getByText('敌方已确认', { exact: true }).waitFor();
  await page.getByText('自动位置', { exact: true }).waitFor();
  assert.ok((await page.evaluate(() => document.documentElement.scrollWidth)) <= 360);
  await page.goto(base);
  await page.setViewportSize({ width: 1440, height: 1050 });
  await page.getByRole('heading', { name: '选好下一场。' }).waitFor();
  mkdirSync(resolve(root, 'docs/images'), { recursive: true });
  await page.screenshot({ path: resolve(root, 'docs/images/ui-empty.png'), fullPage: true });
  assert.deepEqual(errors, [], 'No JavaScript runtime errors');
  writeFileSync(
    resolve(root, 'tests/ui/browser-smoke-result.json'),
    JSON.stringify(
      {
        date: new Date().toISOString(),
        environment: 'Linux Google Chrome headless; real rendered empty UI, no IPC mocks',
        checks: report,
        account_validation: true,
        browser_ipc_failure_visible: true,
        pool_exclusion: true,
        automatic_role_default_and_override_reset: true,
        settings_validation: true,
        overlay_empty_layout: true,
        calibration_export_plumbing: true,
        calibration_input_is_not_dota: true,
        runtime_errors: errors,
      },
      null,
      2,
    ),
  );
  console.log(
    `PASS: ${report.length} page/viewport layouts; account validation, IPC error, pool exclusion, settings validation, diagnostics, overlay and calibration ZIP export; no runtime errors.`,
  );
} catch (error) {
  await page.screenshot({ path: resolve(root, 'tests/ui/ui-failure.png'), fullPage: true });
  console.log(await page.locator('main').ariaSnapshot());
  throw error;
} finally {
  await browser.close();
  server?.kill();
}
