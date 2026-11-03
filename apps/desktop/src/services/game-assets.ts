/** Bundled game art only. No network fallback and no account or Tauri dependency. */
export const assetCategories = [
  'heroes',
  'hero-icons',
  'abilities',
  'items',
  'facets',
  'ui',
] as const;
export type AssetCategory = (typeof assetCategories)[number];
export type AssetStatus = 'downloaded' | 'not_found' | 'skipped' | 'failed';
export interface GameAsset {
  category: AssetCategory;
  key: string;
  id: number | null;
  ids?: number[];
  hero_id?: number;
  hero_ids?: number[];
  name: string;
  path: string | null;
  status: AssetStatus;
  source_url: string | null;
  sha256?: string;
  bytes?: number;
  width?: number;
  height?: number;
  shared?: boolean | string;
  reason?: string;
}
export interface GameAssetManifest {
  schema_version: 1;
  complete?: boolean;
  source: { name: string; commit: string | null; version: string | null; metadata_files?: unknown };
  generated_at: string;
  entries: GameAsset[];
  summary: Record<string, unknown>;
}
export const assetCategoryNames: Record<AssetCategory, string> = {
  heroes: '英雄肖像',
  'hero-icons': '英雄图标',
  abilities: '技能',
  items: '物品',
  facets: '命石',
  ui: '界面图标',
};
export const assetStatusNames: Record<AssetStatus, string> = {
  downloaded: '已下载',
  not_found: '来源未提供',
  skipped: '无需独立图标',
  failed: '未下载成功',
};
export const assetIdentity = (entry: GameAsset) => `${entry.category}:${entry.key}`;

export function gameAssetUrl(path: string | null): string | null {
  if (!path || path.startsWith('/') || /[\\:#?]/.test(path)) return null;
  const parts = path.split('/');
  if (parts.some((part) => !part || part === '.' || part === '..')) return null;
  return `/game-icons/${parts.map(encodeURIComponent).join('/')}`;
}
export function heroAssetUrl(heroId: number, small: boolean): string | null {
  return Number.isSafeInteger(heroId) && heroId > 0
    ? `/game-icons/${small ? 'hero-icons' : 'heroes'}/${heroId}.png`
    : null;
}

function parseManifest(value: unknown): GameAssetManifest {
  if (typeof value !== 'object' || value === null) throw new Error('资源目录格式无效。');
  const manifest = value as GameAssetManifest;
  if (
    manifest.schema_version !== 1 ||
    !Array.isArray(manifest.entries) ||
    typeof manifest.source?.name !== 'string' ||
    typeof manifest.generated_at !== 'string'
  ) {
    throw new Error('资源目录版本或内容无效，请重新准备游戏资源。');
  }
  const seen = new Set<string>();
  for (const entry of manifest.entries) {
    if (
      !entry ||
      !assetCategories.includes(entry.category) ||
      typeof entry.key !== 'string' ||
      !entry.key ||
      typeof entry.name !== 'string' ||
      !Object.hasOwn(assetStatusNames, entry.status) ||
      (entry.id !== null && !Number.isSafeInteger(entry.id)) ||
      (entry.ids !== undefined &&
        (!Array.isArray(entry.ids) || !entry.ids.every(Number.isSafeInteger))) ||
      (entry.path !== null && typeof entry.path !== 'string') ||
      (entry.hero_ids !== undefined &&
        (!Array.isArray(entry.hero_ids) || !entry.hero_ids.every(Number.isSafeInteger))) ||
      (entry.status === 'downloaded' && !gameAssetUrl(entry.path))
    ) {
      throw new Error('资源目录包含无效条目或本地路径，请重新准备游戏资源。');
    }
    if (seen.has(assetIdentity(entry)))
      throw new Error('资源目录包含重复条目，请重新准备游戏资源。');
    seen.add(assetIdentity(entry));
  }
  return manifest;
}

let pending: Promise<GameAssetManifest> | null = null;
export function loadGameAssets(refresh = false): Promise<GameAssetManifest> {
  if (refresh) pending = null;
  if (!pending) {
    pending = (async () => {
      const response = await fetch('/game-icons/manifest.json', {
        cache: 'no-cache',
        signal: AbortSignal.timeout(15000),
      });
      if (!response.ok)
        throw new Error(
          response.status === 404
            ? '尚未找到本地游戏资源。'
            : `无法读取本地资源目录（${response.status}）。`,
        );
      let value: unknown;
      try {
        value = await response.json();
      } catch {
        throw new Error('尚未找到有效的本地游戏资源目录。');
      }
      return parseManifest(value);
    })().catch((error: unknown) => {
      pending = null;
      throw error;
    });
  }
  return pending;
}
