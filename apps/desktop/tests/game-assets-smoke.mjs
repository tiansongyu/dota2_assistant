// Uses the actual downloaded manifest and images. HTTP faults are deliberate failure-state tests.
import { chromium } from 'playwright';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';

const root = resolve(import.meta.dirname, '../../..');
const manifestPath = resolve(root, 'assets/game-icons/manifest.json');
assert.ok(
  existsSync(manifestPath),
  'Prepare real icons with python3 scripts/fetch-game-icons.py first.',
);
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
assert.equal(
  manifest.complete,
  true,
  'Complete the real resource download before final UI verification.',
);
const output = resolve(
  root,
  'artifacts/game-icons/ui',
  process.env.DPDA_ASSET_UI_URL ? 'production' : 'development',
);
mkdirSync(output, { recursive: true });
const base = process.env.DPDA_ASSET_UI_URL ?? 'http://127.0.0.1:1422';
let server = null;
async function ready() {
  try {
    return (await fetch(base, { signal: AbortSignal.timeout(1000) })).ok;
  } catch {
    return false;
  }
}
if (!(await ready())) {
  assert.ok(!process.env.DPDA_ASSET_UI_URL, 'Configured UI server is unavailable.');
  server = spawn(
    process.execPath,
    [
      resolve(import.meta.dirname, '../node_modules/vite/bin/vite.js'),
      '--host',
      '127.0.0.1',
      '--port',
      '1422',
    ],
    { cwd: resolve(import.meta.dirname, '..'), stdio: 'ignore' },
  );
  for (let i = 0; i < 80 && !(await ready()); i++)
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
  if (!(await ready())) {
    server.kill();
    throw new Error('Resource UI server failed to start.');
  }
}
const browser = await chromium.launch({
  executablePath:
    process.env.DPDA_CHROME_PATH ??
    (existsSync('/usr/bin/google-chrome') ? '/usr/bin/google-chrome' : undefined),
  headless: true,
  args: ['--no-sandbox'],
});
const context = await browser.newContext({
  reducedMotion: 'reduce',
  viewport: { width: 1440, height: 1000 },
});
const page = await context.newPage();
const errors = [];
const external = [];
page.on('pageerror', (error) => errors.push(error.message));
context.on('request', (request) => {
  if (new URL(request.url()).origin !== new URL(base).origin) external.push(request.url());
});
await context.route('**/*', (route) =>
  new URL(route.request().url()).origin === new URL(base).origin ? route.continue() : route.abort(),
);
const names = {
  heroes: '英雄肖像',
  'hero-icons': '英雄图标',
  abilities: '技能',
  items: '物品',
  facets: '命石',
  ui: '界面图标',
};
const checks = [];
const nav = () => page.getByRole('navigation', { name: '主要导航' });
async function resourcePage() {
  await nav().getByRole('button', { name: '游戏资源', exact: true }).click();
  await page.locator('.asset-inventory').waitFor();
}
async function category(value) {
  await page
    .getByRole('group', { name: '资源类别' })
    .getByRole('button', { name: new RegExp(`^${value}`) })
    .click();
}
async function assertImages() {
  const images = page.locator('.asset-grid img');
  for (const img of await images.all()) {
    await img.scrollIntoViewIfNeeded();
    await img.evaluate((image) => image.decode());
    assert.ok(await img.evaluate((image) => image.complete && image.naturalWidth > 0));
    assert.ok(
      await img.evaluate(
        (image) =>
          image.clientWidth <= image.parentElement.clientWidth + 1 &&
          image.clientHeight <= image.parentElement.clientHeight + 1,
      ),
      'Images stay inside their reserved area without accidental clipping.',
    );
    assert.equal(new URL(await img.getAttribute('src'), base).origin, new URL(base).origin);
  }
  return images.count();
}
try {
  await page.goto(base);
  await page.getByRole('heading', { name: '选好下一场。', exact: true }).waitFor();
  await resourcePage();
  assert.ok(await page.getByText('未关联账号', { exact: true }).isVisible());
  assert.equal(
    await page.getByRole('combobox', { name: '资源状态', exact: true }).inputValue(),
    'downloaded',
  );
  const inventory = await page.locator('.asset-inventory').innerText();
  assert.ok(inventory.includes(manifest.entries.length.toLocaleString()));
  for (const [key, name] of Object.entries(names)) {
    await category(name);
    const expected = manifest.entries.filter(
      (entry) => entry.category === key && entry.status === 'downloaded',
    );
    assert.equal(await page.locator('.asset-card').count(), Math.min(48, expected.length));
    const images = await assertImages();
    checks.push({ category: key, first_page_images: images, available: expected.length });
    if (expected.length > 48) {
      const pages = Math.ceil(expected.length / 48);
      await page
        .getByRole('combobox', { name: '资源页码', exact: true })
        .selectOption(String(pages - 1));
      assert.equal(await page.locator('.asset-card').count(), expected.length - (pages - 1) * 48);
      await assertImages();
    }
  }
  await category('英雄肖像');
  const mappedHero = manifest.entries.find(
    (entry) =>
      entry.category === 'heroes' &&
      entry.status === 'downloaded' &&
      manifest.entries.some(
        (ability) =>
          ability.category === 'abilities' && ability.hero_ids?.includes(entry.hero_id ?? entry.id),
      ),
  );
  assert.ok(mappedHero, 'Real metadata contains a hero/ability relation.');
  await page.getByRole('textbox', { name: '搜索游戏资源', exact: true }).fill(mappedHero.key);
  await page.getByRole('button', { name: `查看 ${mappedHero.name} 英雄肖像`, exact: true }).click();
  await page.getByRole('button', { name: '筛选此英雄的技能', exact: true }).click();
  const related = manifest.entries.filter(
    (entry) =>
      entry.category === 'abilities' &&
      entry.hero_ids?.includes(mappedHero.hero_id ?? mappedHero.id),
  );
  assert.equal(
    await page.locator('.filter-count').textContent(),
    `${related.length.toLocaleString()} 条结果`,
  );
  const alias = manifest.entries.find((entry) => entry.ids?.length > 1);
  if (alias) {
    await page.getByRole('button', { name: '重置筛选', exact: true }).click();
    await category(names[alias.category]);
    await page.getByRole('combobox', { name: '资源状态', exact: true }).selectOption('all');
    await page
      .getByRole('textbox', { name: '搜索游戏资源', exact: true })
      .fill(String(alias.ids[1]));
    await page
      .getByRole('button', { name: `查看 ${alias.name} ${names[alias.category]}`, exact: true })
      .click();
    assert.ok((await page.locator('.asset-metadata').innerText()).includes(alias.ids.join(' / ')));
  }
  await page.getByRole('button', { name: '重置筛选', exact: true }).click();
  if ((await page.locator('.asset-card').count()) > 1) {
    await page.locator('.asset-card').nth(1).focus();
    await page.keyboard.press('Enter');
    assert.equal(await page.locator('.asset-card').nth(1).getAttribute('aria-pressed'), 'true');
  }
  await category('全部');
  await page.getByRole('combobox', { name: '资源状态', exact: true }).selectOption('missing');
  assert.equal(
    await page.locator('.filter-count').textContent(),
    `${manifest.entries.filter((entry) => entry.status !== 'downloaded').length.toLocaleString()} 条结果`,
  );
  if (await page.locator('.asset-card').count())
    assert.ok(await page.locator('.asset-status').first().isVisible());
  await page
    .getByRole('textbox', { name: '搜索游戏资源', exact: true })
    .fill('DPDA-NO-SUCH-ASSET-TEST');
  await page.getByRole('heading', { name: '没有符合筛选的资源', exact: true }).waitFor();
  await page.getByRole('button', { name: '重置筛选', exact: true }).click();
  for (const width of [1440, 1024, 375]) {
    await page.setViewportSize({ width, height: 1000 });
    const dimensions = await page.evaluate(() => ({
      width: innerWidth,
      scrollWidth: document.documentElement.scrollWidth,
    }));
    assert.ok(
      dimensions.scrollWidth <= dimensions.width + 1,
      `Resource layout overflow at ${width}`,
    );
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.screenshot({ path: resolve(output, 'catalog.png'), fullPage: true });
  await nav().getByRole('button', { name: '我的英雄池', exact: true }).click();
  await page.getByRole('textbox', { name: '搜索英雄', exact: true }).fill(String(mappedHero.id));
  await page.locator('.hero-select-button').click();
  for (const img of await page.locator('.hero-mark img').all()) {
    await img.scrollIntoViewIfNeeded();
    await img.evaluate((image) => image.decode());
  }
  assert.ok(await page.locator(`img[src="/game-icons/hero-icons/${mappedHero.id}.png"]`).count());
  assert.ok(await page.locator(`img[src="/game-icons/heroes/${mappedHero.id}.png"]`).count());
  const failed = await context.newPage();
  await failed.route('**/game-icons/manifest.json', (route) =>
    route.fulfill({ status: 404, body: '' }),
  );
  await failed.goto(base);
  await failed
    .getByRole('navigation', { name: '主要导航' })
    .getByRole('button', { name: '游戏资源', exact: true })
    .click();
  await failed.getByRole('heading', { name: '游戏资源暂时不可用', exact: true }).waitFor();
  await failed.getByText('python3 scripts/fetch-game-icons.py', { exact: true }).waitFor();
  await failed.close();
  const broken = await context.newPage();
  await broken.route(`**/game-icons/hero-icons/${mappedHero.id}.png`, (route) => route.abort());
  await broken.goto(base);
  await broken
    .getByRole('navigation', { name: '主要导航' })
    .getByRole('button', { name: '我的英雄池', exact: true })
    .click();
  await broken.getByRole('textbox', { name: '搜索英雄', exact: true }).fill(String(mappedHero.id));
  await broken.waitForFunction(
    () => document.querySelector('.hero-select-button .hero-mark')?.textContent?.trim().length > 0,
  );
  await broken.close();
  assert.deepEqual(external, [], 'No remote image or metadata fallback.');
  assert.deepEqual(errors, []);
  const report = {
    time: new Date().toISOString(),
    environment:
      'Chromium, actual locally downloaded game resources; no fabricated manifest or imagery',
    origin: base,
    manifest_commit: manifest.source.commit,
    entries: manifest.entries.length,
    downloaded: manifest.entries.filter((entry) => entry.status === 'downloaded').length,
    category_checks: checks,
    no_account_required: true,
    hero_ability_relations: true,
    alias_id_search: Boolean(alias),
    missing_status_filter: true,
    empty_results: true,
    viewport_widths: [1440, 1024, 375],
    hero_small_and_large_images: true,
    missing_manifest_http_fault: true,
    broken_hero_image_http_fault: true,
    remote_requests: external,
    runtime_errors: errors,
  };
  writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
  console.log(
    `PASS: ${manifest.entries.length} real catalog entries; six categories, search, pagination, relationships, local hero images and failure states.`,
  );
} catch (error) {
  await page.screenshot({ path: resolve(output, 'failure.png'), fullPage: true });
  throw error;
} finally {
  await browser.close();
  server?.kill();
}
