// Synthetic integrity fixtures only; these are not game images or vision evidence.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  appendFileSync,
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import {
  syncWithPython,
  verifyBuiltGameIcons,
  verifyGameIcons,
} from "../scripts/verify-game-icons.mjs";

const png = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jKXcAAAAASUVORK5CYII=",
  "base64",
);
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");

function fixture(t) {
  const temporary = mkdtempSync(
    path.join(os.tmpdir(), "dpda-icon-build-test-"),
  );
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const root = path.join(temporary, "assets/game-icons");
  const lockPath = path.join(temporary, "assets/game-icons.lock.json");
  const metadata = Buffer.from('{"synthetic_test_only":true}');
  mkdirSync(path.join(root, "catalog"), { recursive: true });
  mkdirSync(path.join(root, "heroes"));
  writeFileSync(path.join(root, "catalog/heroes.json"), metadata);
  writeFileSync(path.join(root, "heroes/synthetic.png"), png);
  const entry = {
    category: "heroes",
    key: "synthetic",
    id: 1,
    name: "synthetic test only",
    status: "downloaded",
    path: "heroes/synthetic.png",
    sha256: hash(png),
    bytes: png.length,
    width: 1,
    height: 1,
    source_url: "https://example.invalid/synthetic.png",
  };
  const manifest = {
    schema_version: 1,
    complete: true,
    source: {
      name: "synthetic-test-only",
      commit: "a".repeat(40),
      expected_entries: 1,
      metadata_files: [
        {
          path: "catalog/heroes.json",
          sha256: hash(metadata),
          bytes: metadata.length,
          source_url: "https://example.invalid/synthetic-catalog.json",
        },
      ],
    },
    entries: [entry],
  };
  function save() {
    manifest.source.expected_entries = manifest.entries.length;
    writeFileSync(path.join(root, "manifest.json"), JSON.stringify(manifest));
  }
  save();
  return {
    temporary,
    root,
    lockPath,
    entry,
    manifest,
    save,
    verify: (options = {}) =>
      verifyGameIcons({ root, lockPath, publicRoot: null, ...options }),
    lock: () =>
      writeFileSync(
        lockPath,
        JSON.stringify({
          schema_version: 1,
          source_commit: manifest.source.commit,
          generated_at: "synthetic-test-only",
          files: [entry],
        }),
      ),
  };
}

test("offline gate reports missing/skipped entries without claiming image coverage", (t) => {
  const f = fixture(t);
  f.manifest.entries.push(
    {
      category: "abilities",
      key: "synthetic-missing",
      status: "not_found",
      path: null,
    },
    {
      category: "abilities",
      key: "synthetic-skipped",
      status: "skipped",
      path: null,
    },
  );
  f.save();
  const report = f.verify();
  assert.equal(report.offline, true);
  assert.equal(report.physical_files, 1);
  assert.deepEqual(report.counts, {
    downloaded: 1,
    not_found: 1,
    skipped: 1,
    failed: 0,
  });
  assert.equal(report.unavailable.length, 2);
});

test("missing manifest gives an actionable download command", (t) => {
  const f = fixture(t);
  rmSync(path.join(f.root, "manifest.json"));
  assert.throws(() => f.verify(), /assets:sync/u);
});

test("in-progress and diagnostic partial downloads cannot build", (t) => {
  const f = fixture(t);
  f.manifest.complete = false;
  f.save();
  assert.throws(() => f.verify(), /complete/u);
  f.manifest.complete = true;
  f.manifest.source.expected_entries = 2;
  writeFileSync(path.join(f.root, "manifest.json"), JSON.stringify(f.manifest));
  assert.throws(() => f.verify(), /expected_entries/u);
});

test("failed status and a zero-image package are rejected", (t) => {
  const f = fixture(t);
  f.manifest.entries.push({
    category: "items",
    key: "synthetic-error",
    status: "failed",
  });
  f.save();
  assert.throws(() => f.verify(), /failed/u);
  f.manifest.entries = [
    { category: "items", key: "missing", status: "not_found" },
  ];
  f.save();
  assert.throws(() => f.verify(), /没有任何已下载/u);
});

test("same-size image tampering and catalog tampering are detected", (t) => {
  const f = fixture(t);
  const damaged = Buffer.from(png);
  damaged[damaged.length - 1] ^= 1;
  writeFileSync(path.join(f.root, f.entry.path), damaged);
  assert.throws(() => f.verify(), /SHA-256/u);
  writeFileSync(path.join(f.root, f.entry.path), png);
  appendFileSync(path.join(f.root, "catalog/heroes.json"), " ");
  assert.throws(() => f.verify(), /字节数/u);
});

test("unsafe and duplicate case-insensitive paths cannot enter Windows bundles", (t) => {
  const f = fixture(t);
  f.entry.path = "../outside.png";
  f.save();
  assert.throws(() => f.verify(), /穿越/u);
  f.entry.path = "C:\\outside.png";
  f.save();
  assert.throws(() => f.verify(), /不安全/u);
  f.entry.path = "heroes/synthetic.png";
  f.manifest.entries.push({
    ...f.entry,
    key: "other",
    path: "heroes/Synthetic.png",
  });
  f.save();
  assert.throws(() => f.verify(), /大小写/u);
});

test("lock requires exact source and physical file set", (t) => {
  const f = fixture(t);
  assert.throws(
    () => f.verify({ requireLock: true }),
    /来源锁|game-icons.lock/u,
  );
  f.lock();
  assert.equal(f.verify({ requireLock: true }).lock_verified, true);
  const lock = JSON.parse(readFileSync(f.lockPath, "utf8"));
  lock.files[0].source_url = "https://example.invalid/different-origin.png";
  writeFileSync(f.lockPath, JSON.stringify(lock));
  assert.throws(() => f.verify(), /来源锁不一致/u);
  lock.files = [];
  writeFileSync(f.lockPath, JSON.stringify(lock));
  assert.throws(() => f.verify(), /物理文件集/u);
  lock.source_commit = "b".repeat(40);
  writeFileSync(f.lockPath, JSON.stringify(lock));
  assert.throws(() => f.verify(), /source_commit/u);
});

test("shared safe SVG is verified once and can have multiple stable keys", (t) => {
  const f = fixture(t);
  mkdirSync(path.join(f.root, "ui"));
  const svg = Buffer.from(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M0 0 L24 24"/></svg>',
  );
  writeFileSync(path.join(f.root, "ui/talents.svg"), svg);
  const entry = {
    ...f.entry,
    category: "ui",
    key: "talents",
    path: "ui/talents.svg",
    bytes: svg.length,
    width: 24,
    height: 24,
    sha256: hash(svg),
    source_url: "https://example.invalid/synthetic.svg",
  };
  f.manifest.entries.push(entry, {
    ...entry,
    category: "abilities",
    key: "special_bonus_synthetic",
    ids: [1, 2],
  });
  f.save();
  const result = f.verify();
  assert.equal(result.counts.downloaded, 3);
  assert.equal(result.physical_files, 2);
});

test("SVG scripts and remote resources fail even when the supplied hash matches", (t) => {
  const f = fixture(t);
  mkdirSync(path.join(f.root, "ui"));
  const entry = {
    ...f.entry,
    category: "ui",
    key: "unsafe",
    path: "ui/unsafe.svg",
    width: 10,
    height: 10,
  };
  f.manifest.entries.push(entry);
  for (const content of [
    "<script>alert(1)</script>",
    '<image href="https://example.invalid/image.png"/>',
    "<style>.x{fill:url(data:image/svg+xml,foo)}</style>",
    '<path onclick="alert(1)"/>',
    '<!ENTITY unsafe SYSTEM "file:///private">',
  ]) {
    const data = Buffer.from(`<svg width="10" height="10">${content}</svg>`);
    writeFileSync(path.join(f.root, entry.path), data);
    entry.bytes = data.length;
    entry.sha256 = hash(data);
    f.save();
    assert.throws(() => f.verify(), /SVG/u);
  }
});

test("private or undeclared files are blocked from the public directory", (t) => {
  const f = fixture(t);
  writeFileSync(path.join(f.root, "unlisted.png"), png);
  assert.throws(() => f.verify(), /未列入清单/u);
  rmSync(path.join(f.root, "unlisted.png"));
  const publicRoot = path.dirname(f.root);
  assert.equal(f.verify({ publicRoot }).status, "verified");
  writeFileSync(
    path.join(publicRoot, "game-icons.snapshot.json"),
    '{"synthetic_descriptor":true}',
  );
  assert.equal(f.verify({ publicRoot }).status, "verified");
  writeFileSync(path.join(publicRoot, "history.sqlite"), "synthetic-test-only");
  assert.throws(() => f.verify({ publicRoot }), /未批准/u);
});

test("explicit sync uses only the fixed snapshot loader and a shell-free Python executable", () => {
  const calls = [];
  const interpreter = "C:\\Synthetic Python\\python.exe";
  syncWithPython({
    environment: { DPDA_PYTHON: interpreter, SYNTHETIC_SETTING: "preserved" },
    platform: "win32",
    run(command, args, options) {
      calls.push({ command, args, options });
      return { status: 0 };
    },
  });
  assert.equal(calls.length, 2);
  assert.equal(calls[0].command, interpreter);
  assert.deepEqual(calls[0].args.slice(0, 1), ["-c"]);
  assert.match(calls[0].args[1], /sys\.version_info >= \(3, 10\)/u);
  assert.equal(calls[1].command, interpreter);
  assert.equal(path.basename(calls[1].args[0]), "fetch-locked-game-icons.py");
  assert.equal(calls[1].args.length, 1);
  assert.equal(calls[1].options.env.PYTHONUTF8, "1");
  assert.equal(calls[1].options.env.SYNTHETIC_SETTING, "preserved");
  assert.notEqual(calls[1].options.shell, true);
});

test("Windows launcher prefix is retained and Python candidates are probed before running", () => {
  const calls = [];
  syncWithPython({
    environment: {},
    platform: "win32",
    run(command, args) {
      calls.push({ command, args });
      return { status: 0 };
    },
  });
  assert.equal(calls[0].command, "py");
  assert.deepEqual(calls[0].args.slice(0, 2), ["-3", "-c"]);
  assert.equal(calls[1].command, "py");
  assert.equal(calls[1].args[0], "-3");
  assert.equal(path.basename(calls[1].args[1]), "fetch-locked-game-icons.py");
  const fallback = [];
  syncWithPython({
    environment: {},
    platform: "linux",
    run(command, args) {
      fallback.push({ command, args });
      return { status: command === "python3" ? 1 : 0 };
    },
  });
  assert.deepEqual(
    fallback.map((call) => call.command),
    ["python3", "python", "python"],
  );
  assert.equal(
    path.basename(fallback[2].args[0]),
    "fetch-locked-game-icons.py",
  );
});

test("snapshot failure is terminal and does not fall back to another interpreter or live downloader", () => {
  const calls = [];
  assert.throws(
    () =>
      syncWithPython({
        environment: {},
        platform: "win32",
        run(command, args) {
          calls.push({ command, args });
          return { status: calls.length === 1 ? 0 : 1 };
        },
      }),
    /固定资源快照恢复失败.*不会回退/u,
  );
  assert.equal(calls.length, 2);
  assert.equal(path.basename(calls[1].args[1]), "fetch-locked-game-icons.py");
  assert.throws(
    () =>
      syncWithPython({
        environment: { DPDA_PYTHON: "synthetic-missing-python" },
        run() {
          return { error: new Error("synthetic ENOENT"), status: null };
        },
      }),
    /找不到 Python 3.10/u,
  );
});

test("desktop pre-start and pre-build gates stay offline; network restore remains explicit", () => {
  const packageJson = JSON.parse(
    readFileSync(
      new URL("../apps/desktop/package.json", import.meta.url),
      "utf8",
    ),
  );
  for (const lifecycle of ["predev", "prebuild", "prepreview"])
    assert.equal(packageJson.scripts[lifecycle], "npm run assets:verify");
  assert.equal(
    packageJson.scripts["assets:sync"],
    "node ../../scripts/verify-game-icons.mjs --sync",
  );
  assert.equal(
    packageJson.scripts["assets:verify"],
    "node ../../scripts/verify-game-icons.mjs",
  );
});

test(
  "symlinks to files outside assets are refused",
  { skip: process.platform === "win32" },
  (t) => {
    const f = fixture(t);
    const filename = path.join(f.root, f.entry.path);
    rmSync(filename);
    const external = path.join(f.temporary, "external.png");
    writeFileSync(external, png);
    symlinkSync(external, filename);
    assert.throws(() => f.verify(), /普通文件/u);
  },
);

test("production dist contains identical locked assets and no private files", (t) => {
  const f = fixture(t);
  f.lock();
  const distRoot = path.join(f.temporary, "dist");
  const assetsRoot = path.dirname(f.root);
  cpSync(assetsRoot, distRoot, { recursive: true });
  writeFileSync(
    path.join(distRoot, "index.html"),
    "<!doctype html><title>Synthetic build test</title>",
  );
  const verify = () =>
    verifyBuiltGameIcons({ assetsRoot, distRoot, requireLock: true });
  assert.equal(verify().dist_verified, true);
  appendFileSync(path.join(distRoot, "game-icons/manifest.json"), " ");
  assert.throws(verify, /与源文件不同/u);
  cpSync(assetsRoot, distRoot, { recursive: true });
  writeFileSync(
    path.join(distRoot, "snapshot-private.json"),
    '{"synthetic":true}',
  );
  assert.throws(verify, /私人/u);
});
