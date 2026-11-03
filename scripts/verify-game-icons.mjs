#!/usr/bin/env node
// Offline build gate. Explicit --sync restores the pinned archive, never the live CDN.
import { createHash } from "node:crypto";
import {
  existsSync,
  lstatSync,
  readFileSync,
  readdirSync,
  realpathSync,
} from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const repository = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const categories = new Set([
  "heroes",
  "hero-icons",
  "abilities",
  "items",
  "facets",
  "ui",
]);
const statuses = ["downloaded", "not_found", "skipped", "failed"];
const guidance =
  "请执行 npm --prefix apps/desktop run assets:sync 恢复固定资源快照，再重试。";

function ensure(condition, message) {
  if (!condition) throw new Error(message);
}

function object(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function relativePath(value) {
  ensure(typeof value === "string" && value.length > 0, "资源 path 不能为空");
  ensure(
    !/[\\:\u0000-\u001f?#%]/u.test(value) && !path.posix.isAbsolute(value),
    `资源路径不安全：${value}`,
  );
  const pieces = value.split("/");
  ensure(
    pieces.every((piece) => piece && piece !== "." && piece !== ".."),
    `资源路径不能穿越目录：${value}`,
  );
  return value;
}

function readJson(filename, label) {
  ensure(existsSync(filename), `缺少 ${label}：${filename}。${guidance}`);
  try {
    return JSON.parse(readFileSync(filename, "utf8"));
  } catch (error) {
    throw new Error(`${label} JSON 无法读取：${error.message}`);
  }
}

function walk(directory, prefix = "") {
  const result = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
    ensure(
      !entry.isSymbolicLink(),
      `公开静态目录不能包含符号链接：${relative}`,
    );
    if (entry.isDirectory())
      result.push(...walk(path.join(directory, entry.name), relative));
    else {
      ensure(entry.isFile(), `不支持的静态文件类型：${relative}`);
      result.push(relative);
    }
  }
  return result;
}

function checkedFile(root, entry, label) {
  const relative = relativePath(entry.path);
  const filename = path.join(root, ...relative.split("/"));
  ensure(existsSync(filename), `${label} 文件缺失：${relative}。${guidance}`);
  ensure(
    lstatSync(filename).isFile() && !lstatSync(filename).isSymbolicLink(),
    `${label} 必须是普通文件：${relative}`,
  );
  const rootReal = realpathSync(root);
  ensure(
    realpathSync(filename).startsWith(rootReal + path.sep),
    `${label} 超出资源目录：${relative}`,
  );
  ensure(
    Number.isSafeInteger(entry.bytes) && entry.bytes > 0,
    `${label} bytes 无效：${relative}`,
  );
  ensure(
    typeof entry.sha256 === "string" && /^[a-f0-9]{64}$/u.test(entry.sha256),
    `${label} SHA-256 无效：${relative}`,
  );
  const data = readFileSync(filename);
  ensure(data.length === entry.bytes, `${label} 字节数与清单不符：${relative}`);
  ensure(
    createHash("sha256").update(data).digest("hex") === entry.sha256,
    `${label} SHA-256 与清单不符：${relative}；请重新下载，更新资源时显式刷新锁文件。`,
  );
  return data;
}

function attribute(tag, name) {
  return tag.match(
    new RegExp(`(?:^|\\s)${name}\\s*=\\s*(["'])(.*?)\\1`, "iu"),
  )?.[2];
}

function svgDimensions(data, relative) {
  const svg = new TextDecoder("utf-8", { fatal: true }).decode(data);
  ensure(
    !/<!DOCTYPE|<!ENTITY|<\s*(?:script|foreignObject|iframe|object|embed|animate|set|audio|video)\b|\bon[a-z]+\s*=|@import/iu.test(
      svg,
    ),
    `SVG 包含不允许的活动内容：${relative}`,
  );
  for (const match of svg.matchAll(
    /(?:\b(?:href|src))\s*=\s*(["'])(.*?)\1/giu,
  )) {
    ensure(
      /^#[a-z0-9_.:-]+$/iu.test(match[2]),
      `SVG 含外部或嵌入资源：${relative}`,
    );
  }
  for (const match of svg.matchAll(/url\s*\((.*?)\)/giu)) {
    ensure(
      /^["']?#[a-z0-9_.:-]+["']?$/iu.test(match[1].trim()),
      `SVG 含外部 CSS URL：${relative}`,
    );
  }
  const opening = svg.match(/<svg\b([^>]*)>/iu);
  ensure(opening && /<\/svg\s*>/iu.test(svg), `SVG 缺少根元素：${relative}`);
  const number = (value) =>
    value && /^\d+(?:\.\d+)?(?:px)?$/u.test(value)
      ? Number(value.replace(/px$/u, ""))
      : undefined;
  const viewBox = attribute(opening[1], "viewBox")
    ?.trim()
    .split(/[\s,]+/u)
    .map(Number);
  return [
    number(attribute(opening[1], "width")) ?? viewBox?.[2],
    number(attribute(opening[1], "height")) ?? viewBox?.[3],
  ];
}

function imageDimensions(data, relative) {
  if (relative.endsWith(".svg")) return svgDimensions(data, relative);
  ensure(
    relative.endsWith(".png") &&
      data.length >= 33 &&
      data
        .subarray(0, 8)
        .equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10])) &&
      data.readUInt32BE(8) === 13 &&
      data.toString("ascii", 12, 16) === "IHDR",
    `图片不是受支持的 PNG / SVG：${relative}`,
  );
  return [data.readUInt32BE(16), data.readUInt32BE(20)];
}

function verifyPublicDirectory(publicRoot) {
  const allowed = new Set([
    "game-icons",
    "game-icons.lock.json",
    "game-icons.snapshot.json",
    "heroes.json",
    "patches.json",
    "data-provenance.json",
    "licenses",
    "hero-templates",
  ]);
  for (const entry of readdirSync(publicRoot)) {
    ensure(
      allowed.has(entry),
      `assets 将公开打包，发现未批准的顶层内容：${entry}`,
    );
  }
  for (const relative of walk(publicRoot)) {
    rejectSensitivePath(relative);
  }
}

function rejectSensitivePath(relative) {
  ensure(
    !/(?:^|\/)(?:\.[^/]+|session\.json|snapshot[^/]*\.json)|\.(?:sqlite[\w-]*|db(?:-(?:wal|shm))?|log|part|tmp|pem|key)$/iu.test(
      relative,
    ),
    `公开静态目录中发现私人、临时或凭证文件：${relative}`,
  );
}

export function verifyGameIcons({
  root = path.join(repository, "assets/game-icons"),
  lockPath = path.join(repository, "assets/game-icons.lock.json"),
  publicRoot = path.join(repository, "assets"),
  requireLock = false,
} = {}) {
  const manifest = readJson(path.join(root, "manifest.json"), "图标 manifest");
  ensure(
    object(manifest) && manifest.schema_version === 1,
    "图标 manifest schema_version 必须为 1",
  );
  ensure(
    manifest.complete === true,
    `图标下载尚未完整结束（complete != true）。${guidance}`,
  );
  ensure(
    object(manifest.source) &&
      /^[a-f0-9]{40}$/u.test(manifest.source.commit ?? ""),
    "manifest.source.commit 必须固定为上游 40 位 commit",
  );
  ensure(
    Array.isArray(manifest.entries) &&
      Number.isSafeInteger(manifest.source.expected_entries) &&
      manifest.source.expected_entries > 0 &&
      manifest.entries.length === manifest.source.expected_entries,
    "图标条目数不等于 source.expected_entries；不能把诊断或部分下载作为完整资源包",
  );
  ensure(
    Array.isArray(manifest.source.metadata_files) &&
      manifest.source.metadata_files.length > 0,
    "缺少固定来源的 metadata_files",
  );
  const declared = new Set(["manifest.json"]);
  for (const metadata of manifest.source.metadata_files) {
    ensure(object(metadata), "metadata_files 条目无效");
    const relative = relativePath(metadata.path);
    ensure(
      relative.startsWith("catalog/") && relative.endsWith(".json"),
      `元数据路径不正确：${relative}`,
    );
    ensure(!declared.has(relative), `重复元数据路径：${relative}`);
    JSON.parse(checkedFile(root, metadata, "来源元数据").toString("utf8"));
    declared.add(relative);
  }
  const counts = Object.fromEntries(statuses.map((status) => [status, 0]));
  const byCategory = Object.fromEntries(
    [...categories].map((category) => [
      category,
      Object.fromEntries(statuses.map((status) => [status, 0])),
    ]),
  );
  const unavailable = [];
  const physical = new Map();
  const caseNames = new Map();
  const keys = new Set();
  for (const entry of manifest.entries) {
    ensure(
      object(entry) &&
        categories.has(entry.category) &&
        statuses.includes(entry.status),
      "图标条目含未知 category / status",
    );
    ensure(
      typeof entry.key === "string" && entry.key.length > 0,
      "图标条目缺少 key",
    );
    const key = `${entry.category}:${entry.key}`;
    ensure(!keys.has(key), `重复图标键：${key}`);
    keys.add(key);
    counts[entry.status]++;
    byCategory[entry.category][entry.status]++;
    if (entry.status !== "downloaded") {
      unavailable.push({
        category: entry.category,
        key: entry.key,
        status: entry.status,
      });
      continue;
    }
    const relative = relativePath(entry.path);
    ensure(
      typeof entry.source_url === "string" &&
        entry.source_url.startsWith("https://"),
      `图片缺少 HTTPS source_url：${relative}`,
    );
    ensure(
      !caseNames.has(relative.toLowerCase()) ||
        caseNames.get(relative.toLowerCase()) === relative,
      `Windows 不区分大小写的路径冲突：${relative}`,
    );
    caseNames.set(relative.toLowerCase(), relative);
    const previous = physical.get(relative);
    if (previous) {
      ensure(
        ["sha256", "bytes", "source_url", "width", "height"].every(
          (field) => previous[field] === entry[field],
        ),
        `共享图片的元数据不一致：${relative}`,
      );
    } else {
      ensure(
        entry.bytes <= 8 * 1024 * 1024,
        `图片超过 8 MiB 上限：${relative}`,
      );
      const data = checkedFile(root, entry, "图片");
      const [width, height] = imageDimensions(data, relative);
      ensure(
        [width, height].every(
          (value) => Number.isFinite(value) && value > 0 && value <= 8192,
        ) &&
          width === entry.width &&
          height === entry.height,
        `图片尺寸与清单不符：${relative}`,
      );
      physical.set(relative, entry);
      declared.add(relative);
    }
  }
  ensure(
    counts.failed === 0,
    `图标清单含 ${counts.failed} 个 failed 条目；请先修复下载错误。${guidance}`,
  );
  ensure(physical.size > 0, `资源包没有任何已下载图片。${guidance}`);
  for (const relative of walk(root)) {
    ensure(
      declared.has(relative),
      `资源目录存在未列入清单的文件，不能打包：${relative}`,
    );
  }
  let lockVerified = false;
  if (lockPath && existsSync(lockPath)) {
    const lock = readJson(lockPath, "图标来源锁");
    ensure(
      object(lock) &&
        lock.schema_version === 1 &&
        lock.source_commit === manifest.source.commit &&
        Array.isArray(lock.files),
      "图标锁版本或 source_commit 与 manifest 不一致；需显式刷新锁",
    );
    const locked = new Set();
    for (const entry of lock.files) {
      ensure(object(entry), "图标锁 files 条目无效");
      const relative = relativePath(entry.path);
      ensure(!locked.has(relative), `图标锁含重复物理路径：${relative}`);
      locked.add(relative);
      const current = physical.get(relative);
      ensure(
        current &&
          ["sha256", "bytes", "source_url"].every(
            (field) => current[field] === entry[field],
          ),
        `图片与来源锁不一致：${relative}；需审查变更后显式刷新锁。`,
      );
    }
    ensure(
      locked.size === physical.size,
      "图标物理文件集与来源锁不一致；存在新增/缺失，需显式刷新锁",
    );
    lockVerified = true;
  } else
    ensure(
      !requireLock,
      "CI / 发布构建需要已跟踪的 assets/game-icons.lock.json",
    );
  if (publicRoot) verifyPublicDirectory(publicRoot);
  return {
    status: "verified",
    offline: true,
    source_commit: manifest.source.commit,
    expected_entries: manifest.source.expected_entries,
    counts,
    by_category: byCategory,
    physical_files: physical.size,
    bytes: [...physical.values()].reduce((sum, entry) => sum + entry.bytes, 0),
    lock_verified: lockVerified,
    unavailable,
    note: "not_found / skipped 为明确缺失，不计为已下载或 100% 图像覆盖。",
  };
}

export function syncWithPython({
  run = spawnSync,
  environment = process.env,
  platform = process.platform,
} = {}) {
  const candidates = environment.DPDA_PYTHON
    ? [[environment.DPDA_PYTHON]]
    : platform === "win32"
      ? [["py", "-3"], ["python"], ["python3"]]
      : [["python3"], ["python"]];
  for (const [command, ...prefix] of candidates) {
    const probe = run(
      command,
      [
        ...prefix,
        "-c",
        "import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 1)",
      ],
      { cwd: repository, stdio: "ignore", windowsHide: true, env: environment },
    );
    if (probe.error || probe.status !== 0) continue;
    const download = run(
      command,
      [...prefix, path.join(repository, "scripts/fetch-locked-game-icons.py")],
      {
        cwd: repository,
        stdio: "inherit",
        env: { ...environment, PYTHONUTF8: "1" },
        windowsHide: true,
      },
    );
    ensure(
      !download.error && download.status === 0,
      `固定资源快照恢复失败（退出码 ${download.status ?? "未启动"}），请查看下载器输出；不会回退到实时 CDN。`,
    );
    return;
  }
  throw new Error(
    "找不到 Python 3.10+；请安装 Python，或通过 DPDA_PYTHON 指定解释器可执行文件。",
  );
}

export function verifyBuiltGameIcons({
  assetsRoot = path.join(repository, "assets"),
  distRoot = path.join(repository, "apps/desktop/dist"),
  requireLock = false,
} = {}) {
  const lockPath = path.join(assetsRoot, "game-icons.lock.json");
  const source = verifyGameIcons({
    root: path.join(assetsRoot, "game-icons"),
    lockPath,
    publicRoot: assetsRoot,
    requireLock,
  });
  ensure(
    existsSync(path.join(distRoot, "index.html")),
    "缺少前端 dist/index.html；请先执行 npm --prefix apps/desktop run build。",
  );
  verifyGameIcons({
    root: path.join(distRoot, "game-icons"),
    lockPath,
    publicRoot: null,
    requireLock,
  });
  const expected = walk(assetsRoot);
  for (const relative of expected) {
    const built = path.join(distRoot, ...relative.split("/"));
    ensure(existsSync(built), `Vite dist 缺少静态资源：${relative}`);
    ensure(
      readFileSync(path.join(assetsRoot, ...relative.split("/"))).equals(
        readFileSync(built),
      ),
      `Vite dist 静态资源与源文件不同：${relative}；请重新构建。`,
    );
  }
  for (const relative of walk(distRoot)) rejectSensitivePath(relative);
  return {
    ...source,
    dist_verified: true,
    copied_public_files: expected.length,
  };
}

function main() {
  const args = process.argv.slice(2);
  ensure(
    args.every((arg) =>
      ["--sync", "--require-lock", "--check-dist"].includes(arg),
    ),
    "用法：node scripts/verify-game-icons.mjs [--sync] [--require-lock] [--check-dist]",
  );
  if (args.includes("--sync")) syncWithPython();
  const verify = args.includes("--check-dist")
    ? verifyBuiltGameIcons
    : verifyGameIcons;
  const report = verify({
    requireLock: args.includes("--require-lock") || args.includes("--sync"),
  });
  const { unavailable, ...summary } = report;
  console.log(
    JSON.stringify({ ...summary, unavailable_entries: unavailable.length }),
  );
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    main();
  } catch (error) {
    console.error(`图标资源校验失败：${error.message}`);
    process.exitCode = 1;
  }
}
