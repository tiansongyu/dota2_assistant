import { useEffect, useRef, useState } from 'react';
import { Download, ImagePlus, Trash2 } from 'lucide-react';
import { strToU8, zipSync } from 'fflate';

interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}
interface Screenshot {
  url: string;
  width: number;
  height: number;
  name: string;
}
const targets = [
  { value: 'draft_anchor', label: '选人界面独有标记（必填）' },
  ...Array.from({ length: 10 }, (_, index) => ({
    value: `portrait_${index}`,
    label: `${index < 5 ? '天辉' : '夜魇'}槽位 ${(index % 5) + 1} · 头像（必填）`,
  })),
  ...Array.from({ length: 10 }, (_, index) => ({
    value: `locked_${index}`,
    label: `${index < 5 ? '天辉' : '夜魇'}槽位 ${(index % 5) + 1} · 已锁定标记`,
  })),
  ...Array.from({ length: 10 }, (_, index) => ({
    value: `empty_${index}`,
    label: `${index < 5 ? '天辉' : '夜魇'}槽位 ${(index % 5) + 1} · 空槽标记`,
  })),
];
const sameRect = (a: Rect, b: Rect) =>
  a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
export function Calibration() {
  const [shot, setShot] = useState<Screenshot | null>(null);
  const [regions, setRegions] = useState<Record<string, Rect>>({});
  const [target, setTarget] = useState('draft_anchor');
  const [uiVersion, setUiVersion] = useState('');
  const [profileName, setProfileName] = useState('my-draft-layout');
  const [dpi, setDpi] = useState(96);
  const [threshold, setThreshold] = useState(0.92);
  const [error, setError] = useState('');
  const [exporting, setExporting] = useState(false);
  const imageRef = useRef<HTMLImageElement>(null);
  const start = useRef<{ x: number; y: number } | null>(null);
  const current = regions[target];
  useEffect(
    () => () => {
      if (shot) URL.revokeObjectURL(shot.url);
    },
    [shot],
  );
  async function load(file: File | undefined) {
    if (!file) return;
    if (
      !['image/png', 'image/jpeg', 'image/webp'].includes(file.type) ||
      file.size > 60 * 1024 * 1024
    ) {
      setError('请选择小于 60 MB 的 PNG、JPEG 或 WebP 游戏截图。');
      return;
    }
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.src = url;
    try {
      await img.decode();
      if (img.width * img.height > 40_000_000)
        throw new Error('截图超过 4000 万像素，请使用单个游戏窗口截图。');
      setShot({ url, width: img.width, height: img.height, name: file.name });
      setRegions({});
      setError('');
    } catch (cause) {
      URL.revokeObjectURL(url);
      setError(`无法读取截图：${String(cause)}`);
    }
  }
  function coordinates(event: React.PointerEvent<HTMLDivElement>) {
    const bounds = event.currentTarget.getBoundingClientRect();
    return {
      x: Math.round(
        Math.max(
          0,
          Math.min(shot!.width - 1, ((event.clientX - bounds.left) / bounds.width) * shot!.width),
        ),
      ),
      y: Math.round(
        Math.max(
          0,
          Math.min(shot!.height - 1, ((event.clientY - bounds.top) / bounds.height) * shot!.height),
        ),
      ),
    };
  }
  function draw(event: React.PointerEvent<HTMLDivElement>) {
    if (!start.current || !shot) return;
    const point = coordinates(event);
    const rect = {
      x: Math.min(start.current.x, point.x),
      y: Math.min(start.current.y, point.y),
      width: Math.max(1, Math.abs(start.current.x - point.x)),
      height: Math.max(1, Math.abs(start.current.y - point.y)),
    };
    setRegions((previous) => ({ ...previous, [target]: rect }));
  }
  async function exportProfile() {
    if (!shot || !imageRef.current) return;
    const required = [
      'draft_anchor',
      ...Array.from({ length: 10 }, (_, index) => `portrait_${index}`),
    ];
    if (!uiVersion.trim()) {
      setError('请填写此截图对应的 DOTA2 UI / 游戏版本。');
      return;
    }
    if (!Number.isFinite(dpi) || dpi < 48 || dpi > 768) {
      setError('DPI 应在 48—768 之间；Windows 100% 缩放通常为 96。');
      return;
    }
    if (required.some((key) => !regions[key])) {
      setError('请先标注选人界面标记和双方全部 10 个头像槽位。');
      return;
    }
    for (const [key, rect] of Object.entries(regions)) {
      if (
        ![rect.x, rect.y, rect.width, rect.height].every(Number.isInteger) ||
        rect.x < 0 ||
        rect.y < 0 ||
        rect.width < 1 ||
        rect.height < 1 ||
        rect.x + rect.width > shot.width ||
        rect.y + rect.height > shot.height
      ) {
        setError(`${targets.find((item) => item.value === key)?.label} 越界或尺寸无效。`);
        return;
      }
      if (key.startsWith('locked_') && sameRect(rect, regions[`portrait_${key.split('_')[1]}`])) {
        setError('已锁定标记必须独立于头像，不能用头像本身证明已锁定。');
        return;
      }
    }
    setExporting(true);
    setError('');
    try {
      const files: Record<string, Uint8Array> = {};
      for (const [key, rect] of Object.entries(regions)) {
        const canvas = document.createElement('canvas');
        canvas.width = rect.width;
        canvas.height = rect.height;
        const context = canvas.getContext('2d');
        if (!context) throw new Error('浏览器无法创建截图裁剪画布。');
        context.drawImage(
          imageRef.current,
          rect.x,
          rect.y,
          rect.width,
          rect.height,
          0,
          0,
          rect.width,
          rect.height,
        );
        const blob = await new Promise<Blob>((resolve, reject) =>
          canvas.toBlob(
            (result) => (result ? resolve(result) : reject(new Error('无法导出 PNG'))),
            'image/png',
          ),
        );
        files[`${key}.png`] = new Uint8Array(await blob.arrayBuffer());
      }
      const anchor = (key: string) =>
        regions[key]
          ? { region: regions[key], image_path: `${key}.png`, min_similarity: threshold }
          : null;
      const profile = {
        schema_version: 1,
        profile_name: profileName.trim() || 'my-draft-layout',
        ui_version: uiVersion.trim(),
        surface_width: shot.width,
        surface_height: shot.height,
        dpi,
        draft_anchor: anchor('draft_anchor'),
        slots: Array.from({ length: 10 }, (_, index) => ({
          slot_id: index,
          portrait: regions[`portrait_${index}`],
          locked_marker: anchor(`locked_${index}`),
          empty_marker: anchor(`empty_${index}`),
        })),
        not_validated: true,
        source_screenshot: shot.name,
      };
      files['profile.json'] = strToU8(JSON.stringify(profile, null, 2));
      files['README.txt'] = strToU8(
        '此配置由用户截图标注生成，尚未通过真实截图集和 Windows 游戏验收。\n解压到本地目录后，将 profile.json 绝对路径填入 profile_path。英雄模板 manifest.json 必须另外提供。\n截图尺寸、窗口边框、DPI、UI 版本必须与 WGC 捕获表面一致。\n未配置独立锁定标记的槽位只能产生候选，不能自动确认。仅在真实已锁定的槽位标注 locked_marker。\n空槽和锁定标记可为空；不要用纯色区域或头像本身作为锁定证据。\n',
      );
      const zipped = zipSync(files, { level: 0 });
      const url = URL.createObjectURL(
        new Blob([new Uint8Array(zipped)], { type: 'application/zip' }),
      );
      const link = document.createElement('a');
      link.href = url;
      link.download = `${profileName.replace(/[^\w-]/g, '_') || 'dpda-calibration'}.zip`;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (cause) {
      setError(`导出失败：${String(cause)}`);
    } finally {
      setExporting(false);
    }
  }
  return (
    <details className="calibration">
      <summary>
        可视化区域校准<span>从真实截图创建识别配置</span>
      </summary>
      <div className="calibration-content">
        <div className="notice">
          <ImagePlus size={17} aria-hidden="true" />
          <span>
            选择与 Windows
            捕获表面一致的截图，在图上拖动框选区域，也可用数值精调。只为真实已锁定槽位标注独立锁定特征；没有锁定特征的槽位仅作为候选。图片只在本机处理。
          </span>
        </div>
        <div className="calibration-top">
          <label className="field">
            <span>真实 DOTA2 截图</span>
            <input
              type="file"
              accept="image/png,image/jpeg,image/webp"
              onChange={(event) => {
                void load(event.target.files?.[0]);
              }}
            />
          </label>
          <label className="field">
            <span>配置名称</span>
            <input value={profileName} onChange={(event) => setProfileName(event.target.value)} />
          </label>
          <label className="field">
            <span>UI / 游戏版本</span>
            <input
              value={uiVersion}
              onChange={(event) => setUiVersion(event.target.value)}
              placeholder="填写截图对应版本"
            />
          </label>
          <label className="field">
            <span>Windows DPI</span>
            <input
              type="number"
              min="48"
              max="768"
              value={dpi}
              onChange={(event) => setDpi(Number(event.target.value))}
            />
          </label>
        </div>
        {shot && (
          <>
            <div className="calibration-tools">
              <label className="field">
                <span>当前标注区域</span>
                <select
                  aria-label="当前标注区域"
                  value={target}
                  onChange={(event) => setTarget(event.target.value)}
                >
                  {targets.map((item) => (
                    <option key={item.value} value={item.value}>
                      {regions[item.value] ? '✓ ' : ''}
                      {item.label}
                    </option>
                  ))}
                </select>
              </label>
              <div className="region-inputs">
                {(['x', 'y', 'width', 'height'] as const).map((key) => (
                  <label className="field" key={key}>
                    <span>{key}</span>
                    <input
                      type="number"
                      min={key === 'x' || key === 'y' ? 0 : 1}
                      value={current?.[key] ?? ''}
                      onChange={(event) =>
                        setRegions((previous) => ({
                          ...previous,
                          [target]: {
                            ...(previous[target] ?? { x: 0, y: 0, width: 1, height: 1 }),
                            [key]: Number(event.target.value),
                          },
                        }))
                      }
                    />
                  </label>
                ))}
              </div>
              <button
                className="icon-button"
                title="清除当前区域"
                aria-label="清除当前区域"
                disabled={!current}
                onClick={() =>
                  setRegions((previous) => {
                    const next = { ...previous };
                    delete next[target];
                    return next;
                  })
                }
              >
                <Trash2 size={17} />
              </button>
            </div>
            <div
              className="calibration-surface"
              onPointerDown={(event) => {
                if (event.button !== 0) return;
                event.currentTarget.setPointerCapture(event.pointerId);
                start.current = coordinates(event);
              }}
              onPointerMove={draw}
              onPointerUp={(event) => {
                draw(event);
                start.current = null;
              }}
              onPointerCancel={() => {
                start.current = null;
              }}
            >
              <img
                ref={imageRef}
                src={shot.url}
                width={shot.width}
                height={shot.height}
                alt="待校准的 DOTA2 真实截图"
                draggable={false}
              />
              <svg viewBox={`0 0 ${shot.width} ${shot.height}`} aria-hidden="true">
                {Object.entries(regions).map(([key, rect]) => (
                  <g key={key} className={key === target ? 'active-region' : ''}>
                    <rect x={rect.x} y={rect.y} width={rect.width} height={rect.height} />
                    <text x={rect.x + 3} y={Math.max(16, rect.y - 4)}>
                      {key}
                    </text>
                  </g>
                ))}
              </svg>
            </div>
            <div className="calibration-footer">
              <span className="text-muted">
                原图 {shot.width} × {shot.height} · 已标注 {Object.keys(regions).length} 个区域
              </span>
              <label className="field inline-field">
                <span>标记阈值</span>
                <input
                  type="number"
                  min="0.8"
                  max="1"
                  step="0.01"
                  value={threshold}
                  onChange={(event) =>
                    setThreshold(Math.max(0.8, Math.min(1, Number(event.target.value))))
                  }
                />
              </label>
              <button
                className="button primary"
                disabled={exporting}
                onClick={() => {
                  void exportProfile();
                }}
              >
                <Download size={16} aria-hidden="true" />
                {exporting ? '正在导出…' : '下载校准包'}
              </button>
            </div>
          </>
        )}
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        <p className="hint">
          校准包包含 profile.json 和实际裁剪 PNG。导出不等于识别验收；不同分辨率和 DPI
          应单独校准，不进行坐标线性缩放。
        </p>
      </div>
    </details>
  );
}
