import { useEffect, useMemo, useState } from 'react';
import {
  ChevronLeft,
  ChevronRight,
  ImageOff,
  Images,
  LoaderCircle,
  RefreshCw,
  Search,
} from 'lucide-react';
import { EmptyState } from '../components/common';
import {
  assetCategories,
  assetCategoryNames,
  assetIdentity,
  assetStatusNames,
  gameAssetUrl,
  loadGameAssets,
  type AssetCategory,
  type GameAsset,
  type GameAssetManifest,
} from '../services/game-assets';

const PAGE_SIZE = 48;
type Availability = 'downloaded' | 'all' | 'missing';

function AssetImage({ entry, detail = false }: { entry: GameAsset; detail?: boolean }) {
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  const url = entry.status === 'downloaded' ? gameAssetUrl(entry.path) : null;
  return (
    <div
      className={`asset-image ${entry.category === 'heroes' ? 'portrait' : ''} ${detail ? 'detail-image' : ''}`}
    >
      {url && failedUrl !== url ? (
        <img
          src={url}
          alt={detail ? entry.name : ''}
          loading="lazy"
          decoding="async"
          width={entry.width}
          height={entry.height}
          onError={() => setFailedUrl(url)}
        />
      ) : (
        <div className="asset-image-fallback">
          <ImageOff size={detail ? 32 : 23} aria-hidden="true" />
          <span>{url ? '本地图片不可读' : assetStatusNames[entry.status]}</span>
        </div>
      )}
    </div>
  );
}

export function GameAssets() {
  const [manifest, setManifest] = useState<GameAssetManifest | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const [category, setCategory] = useState<AssetCategory | 'all'>('heroes');
  const [availability, setAvailability] = useState<Availability>('downloaded');
  const [query, setQuery] = useState('');
  const [heroFilter, setHeroFilter] = useState<number | null>(null);
  const [page, setPage] = useState(0);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    void loadGameAssets(revision > 0)
      .then((value) => {
        if (active) {
          setManifest(value);
          setError('');
          setLoading(false);
        }
      })
      .catch((reason: unknown) => {
        if (active) {
          setError(reason instanceof Error ? reason.message : '读取本地资源时发生错误。');
          setLoading(false);
        }
      });
    return () => {
      active = false;
    };
  }, [revision]);
  const entries = manifest?.entries;
  const filtered = useMemo(() => {
    const search = query.trim().toLowerCase();
    return (entries ?? []).filter(
      (entry) =>
        (category === 'all' || entry.category === category) &&
        (availability === 'all' ||
          (availability === 'downloaded'
            ? entry.status === 'downloaded'
            : entry.status !== 'downloaded')) &&
        (heroFilter === null ||
          entry.hero_id === heroFilter ||
          entry.hero_ids?.includes(heroFilter)) &&
        (!search ||
          `${entry.name} ${entry.key} ${entry.id ?? ''} ${(entry.ids ?? []).join(' ')}`
            .toLowerCase()
            .includes(search)),
    );
  }, [entries, category, availability, query, heroFilter]);
  const downloaded = entries?.filter((entry) => entry.status === 'downloaded').length ?? 0;
  const imageFiles = new Set(
    entries
      ?.filter((entry) => entry.status === 'downloaded' && entry.path)
      .map((entry) => entry.path),
  ).size;
  const total = entries?.length ?? 0;
  const pageCount = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  const safePage = Math.min(page, pageCount - 1);
  const visible = filtered.slice(safePage * PAGE_SIZE, (safePage + 1) * PAGE_SIZE);
  const selected = filtered.find((entry) => assetIdentity(entry) === selectedKey) ?? visible[0];
  const relatedHeroId =
    selected?.category === 'heroes' || selected?.category === 'hero-icons'
      ? (selected.hero_id ?? selected.id)
      : null;
  const relatedAbilities =
    relatedHeroId === null
      ? []
      : (entries ?? []).filter(
          (entry) => entry.category === 'abilities' && entry.hero_ids?.includes(relatedHeroId),
        );
  const relatedHeroes = (selected?.hero_ids ?? []).flatMap((id) => {
    const hero = entries?.find(
      (entry) => entry.category === 'heroes' && (entry.hero_id ?? entry.id) === id,
    );
    return hero ? [hero] : [];
  });
  function resetSelection() {
    setPage(0);
    setSelectedKey(null);
  }
  function resetFilters() {
    setCategory('heroes');
    setAvailability('downloaded');
    setQuery('');
    setHeroFilter(null);
    resetSelection();
  }
  function showRelated(entry: GameAsset) {
    setCategory(entry.category);
    setAvailability('all');
    setHeroFilter(null);
    setQuery(entry.key);
    setPage(0);
    setSelectedKey(assetIdentity(entry));
  }
  function refresh() {
    setLoading(true);
    setError('');
    setRevision((value) => value + 1);
  }
  return (
    <div className="page-content game-assets-page">
      <header className="page-heading">
        <div>
          <h1>游戏资源</h1>
          <p>浏览随程序保存的真实游戏图标，无需关联账号。</p>
        </div>
        <span className="status-pill">
          <Images size={15} aria-hidden="true" />
          本地资源
        </span>
      </header>
      {loading ? (
        <section className="panel asset-loading" role="status">
          <LoaderCircle className="spin" size={25} aria-hidden="true" />
          <h2>正在读取资源目录</h2>
          <p>图片按需显示，离线仍可浏览。</p>
        </section>
      ) : error ? (
        <section className="panel asset-error">
          <EmptyState
            title="游戏资源暂时不可用"
            icon="data"
            action={
              <button className="button" onClick={refresh}>
                <RefreshCw size={16} aria-hidden="true" />
                重新读取
              </button>
            }
          >
            {error}
          </EmptyState>
          <div className="asset-setup-hint">
            <p>
              请安装包含游戏资源的完整应用。源码运行时，在项目目录执行以下命令准备图标，完成后点击“重新读取”。
            </p>
            <code>python3 scripts/fetch-game-icons.py</code>
            <p className="hint">未下载或未提供的图片会保留缺失说明，不使用远程图片替代。</p>
          </div>
        </section>
      ) : (
        manifest && (
          <>
            {manifest.complete === false && (
              <div className="notice">
                <LoaderCircle size={17} aria-hidden="true" />
                <span>
                  资源目录尚未准备完成。未成功条目可能仍在等待下载；准备完成后点击“重新读取”。
                </span>
              </div>
            )}
            <section className="panel asset-toolbar">
              <div className="asset-inventory">
                <span>
                  目录条目 <strong>{total.toLocaleString()}</strong>
                </span>
                <span>
                  已下载条目 <strong>{downloaded.toLocaleString()}</strong>
                </span>
                <span>
                  本地图片 <strong>{imageFiles.toLocaleString()}</strong> 张
                </span>
                <span>
                  未提供 / 跳过 / 失败 <strong>{(total - downloaded).toLocaleString()}</strong>
                </span>
                <button className="text-button" onClick={refresh}>
                  <RefreshCw size={14} aria-hidden="true" />
                  重新读取
                </button>
              </div>
              <div className="asset-categories" role="group" aria-label="资源类别">
                {(['all', ...assetCategories] as const).map((item) => (
                  <button
                    key={item}
                    className={category === item ? 'selected' : ''}
                    aria-pressed={category === item}
                    onClick={() => {
                      setCategory(item);
                      setHeroFilter(null);
                      resetSelection();
                    }}
                  >
                    {item === 'all' ? '全部' : assetCategoryNames[item]}
                    <span>
                      {
                        (entries ?? []).filter(
                          (entry) =>
                            (item === 'all' || entry.category === item) &&
                            (availability === 'all' ||
                              (availability === 'downloaded'
                                ? entry.status === 'downloaded'
                                : entry.status !== 'downloaded')),
                        ).length
                      }
                    </span>
                  </button>
                ))}
              </div>
              <div className="filter-row">
                <label className="search-field">
                  <Search size={17} aria-hidden="true" />
                  <input
                    value={query}
                    onChange={(event) => {
                      setQuery(event.target.value);
                      resetSelection();
                    }}
                    placeholder="搜索名称、内部名称或 ID"
                    aria-label="搜索游戏资源"
                  />
                </label>
                <label className="field inline-field">
                  <span>资源状态</span>
                  <select
                    value={availability}
                    onChange={(event) => {
                      setAvailability(event.target.value as Availability);
                      resetSelection();
                    }}
                  >
                    <option value="downloaded">只看已下载</option>
                    <option value="all">全部条目</option>
                    <option value="missing">查看缺失说明</option>
                  </select>
                </label>
                <button className="text-button" onClick={resetFilters}>
                  重置筛选
                </button>
                <span className="filter-count" aria-live="polite">
                  {filtered.length.toLocaleString()} 条结果
                </span>
              </div>
              {heroFilter !== null && (
                <p className="hint">
                  仅显示英雄 ID {heroFilter} 的来源关联条目。
                  <button
                    className="text-button"
                    onClick={() => {
                      setHeroFilter(null);
                      resetSelection();
                    }}
                  >
                    清除英雄限定
                  </button>
                </p>
              )}
            </section>
            {filtered.length ? (
              <div className="asset-browser-layout">
                <section className="panel asset-results" aria-label="资源列表">
                  <div className="asset-grid">
                    {visible.map((entry) => (
                      <button
                        type="button"
                        key={assetIdentity(entry)}
                        className={`asset-card ${selected && assetIdentity(selected) === assetIdentity(entry) ? 'selected' : ''}`}
                        aria-label={`查看 ${entry.name || entry.key} ${assetCategoryNames[entry.category]}`}
                        aria-pressed={Boolean(
                          selected && assetIdentity(selected) === assetIdentity(entry),
                        )}
                        onClick={() => setSelectedKey(assetIdentity(entry))}
                      >
                        <AssetImage entry={entry} />
                        <span className="asset-card-name">{entry.name || entry.key}</span>
                        <small>
                          {entry.status === 'downloaded'
                            ? assetCategoryNames[entry.category]
                            : assetStatusNames[entry.status]}
                        </small>
                      </button>
                    ))}
                  </div>
                  <div className="pagination">
                    <button
                      className="button"
                      disabled={safePage === 0}
                      onClick={() => {
                        setPage(safePage - 1);
                        setSelectedKey(null);
                      }}
                    >
                      <ChevronLeft size={16} aria-hidden="true" />
                      上一页
                    </button>
                    <label className="field inline-field">
                      <span>页码</span>
                      <select
                        aria-label="资源页码"
                        value={safePage}
                        onChange={(event) => {
                          setPage(Number(event.target.value));
                          setSelectedKey(null);
                        }}
                      >
                        {Array.from({ length: pageCount }, (_, index) => (
                          <option key={index} value={index}>
                            {index + 1} / {pageCount}
                          </option>
                        ))}
                      </select>
                    </label>
                    <button
                      className="button"
                      disabled={safePage + 1 >= pageCount}
                      onClick={() => {
                        setPage(safePage + 1);
                        setSelectedKey(null);
                      }}
                    >
                      下一页
                      <ChevronRight size={16} aria-hidden="true" />
                    </button>
                    <span className="pagination-range">
                      第 {safePage * PAGE_SIZE + 1}—
                      {Math.min((safePage + 1) * PAGE_SIZE, filtered.length)} 条 / {filtered.length}{' '}
                      条
                    </span>
                  </div>
                </section>
                {selected && (
                  <aside className="panel asset-detail" aria-label="资源详情">
                    <AssetImage key={assetIdentity(selected)} entry={selected} detail />
                    <div className="asset-detail-body">
                      <span className="eyebrow">{assetCategoryNames[selected.category]}</span>
                      <h2>{selected.name || selected.key}</h2>
                      <p
                        className={`asset-status ${selected.status === 'downloaded' ? 'text-success' : 'text-muted'}`}
                      >
                        {assetStatusNames[selected.status]}
                        {selected.shared ? ' · 共享图标' : ''}
                      </p>
                      {selected.reason && <p className="asset-reason">{selected.reason}</p>}
                      <dl className="stats-list asset-metadata">
                        <div>
                          <dt>内部名称</dt>
                          <dd>
                            <code>{selected.key}</code>
                          </dd>
                        </div>
                        {(selected.id !== null || Boolean(selected.ids?.length)) && (
                          <div>
                            <dt>资源 ID</dt>
                            <dd>{selected.ids?.length ? selected.ids.join(' / ') : selected.id}</dd>
                          </div>
                        )}
                        {selected.width != null && selected.height != null && (
                          <div>
                            <dt>图像尺寸</dt>
                            <dd>
                              {selected.width} × {selected.height}
                            </dd>
                          </div>
                        )}
                        {selected.bytes != null && (
                          <div>
                            <dt>文件大小</dt>
                            <dd>{(selected.bytes / 1024).toFixed(1)} KB</dd>
                          </div>
                        )}
                      </dl>
                      {relatedAbilities.length > 0 && (
                        <section className="asset-related">
                          <h3>
                            关联技能 <span>{relatedAbilities.length}</span>
                          </h3>
                          <p className="hint">
                            按数据来源中的英雄关联显示，包含未提供独立图标的条目。
                          </p>
                          <button
                            className="text-button"
                            onClick={() => {
                              setCategory('abilities');
                              setHeroFilter(relatedHeroId);
                              setQuery('');
                              setAvailability('all');
                              resetSelection();
                            }}
                          >
                            筛选此英雄的技能
                          </button>
                          <div className="asset-related-grid">
                            {relatedAbilities.map((entry) => (
                              <button
                                key={assetIdentity(entry)}
                                onClick={() => showRelated(entry)}
                                title={entry.name || entry.key}
                              >
                                <AssetImage entry={entry} />
                                <span>{entry.name || entry.key}</span>
                              </button>
                            ))}
                          </div>
                        </section>
                      )}
                      {relatedHeroes.length > 0 && (
                        <section className="asset-related">
                          <h3>关联英雄</h3>
                          <div className="asset-related-heroes">
                            {relatedHeroes.map((entry) => (
                              <button
                                className="button"
                                key={assetIdentity(entry)}
                                onClick={() => showRelated(entry)}
                              >
                                {entry.name}
                              </button>
                            ))}
                          </div>
                        </section>
                      )}
                      <details className="asset-file-info">
                        <summary>来源与文件信息</summary>
                        <dl>
                          <dt>本地路径</dt>
                          <dd>{selected.path ?? '未保存图片'}</dd>
                          <dt>原始来源</dt>
                          <dd>{selected.source_url ?? '未提供独立来源地址'}</dd>
                          {selected.sha256 && (
                            <>
                              <dt>SHA-256</dt>
                              <dd>{selected.sha256}</dd>
                            </>
                          )}
                        </dl>
                      </details>
                    </div>
                  </aside>
                )}
              </div>
            ) : (
              <section className="panel">
                <EmptyState
                  title="没有符合筛选的资源"
                  action={
                    <button className="button" onClick={resetFilters}>
                      清除筛选
                    </button>
                  }
                >
                  尝试较短的名称或内部名称，或切换类别与资源状态。
                </EmptyState>
              </section>
            )}
            <footer className="panel asset-provenance">
              <span>
                目录来源 <strong>{manifest.source.name}</strong>
              </span>
              <span>生成时间 {manifest.generated_at}</span>
              {manifest.source.version && <span>版本 {manifest.source.version}</span>}
              {manifest.source.commit && (
                <span title={manifest.source.commit}>
                  来源提交 {manifest.source.commit.slice(0, 12)}
                </span>
              )}
              <p>数量按此份资源目录统计；未提供、占位条目和共享图片均保留来源状态。</p>
              <p>图像来自 Valve，权利归 Valve 及相应权利人；DPDA 为非官方产品。</p>
            </footer>
          </>
        )
      )}
    </div>
  );
}
