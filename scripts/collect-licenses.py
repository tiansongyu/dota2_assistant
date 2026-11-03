#!/usr/bin/env python3
"""Collect installed dependency license texts without inferring legal compatibility.

Uses only the Python standard library. Cargo metadata determines crate directories;
npm ls determines installed production dependency directories. No source or license
text is downloaded separately, and missing notices are reported rather than invented.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from urllib.parse import quote, urlsplit

LICENSE_PREFIXES = ("LICENSE", "LICENCE", "COPYING", "NOTICE")


@dataclass
class Package:
    ecosystem: str
    name: str
    version: str
    declared_license: str
    repository: str
    source: str
    root: Path = field(repr=False)
    license_file: str | None = field(default=None, repr=False)
    texts: list[tuple[str, str]] = field(default_factory=list)
    gaps: list[str] = field(default_factory=list)
    normal_targets: list[str] = field(default_factory=list)

    @property
    def label(self) -> str:
        return f"{self.ecosystem}:{self.name}@{self.version}"


@dataclass
class Document:
    raw: bytes = field(repr=False)
    uses: set[tuple[str, str]] = field(default_factory=set)


def single_line(value: object) -> str:
    return " ".join(str(value).splitlines()).strip()


def public_url(value: object) -> str:
    """Keep public metadata only; never include filesystem paths or URL credentials."""
    if isinstance(value, dict):
        value = value.get("url", "")
    if not isinstance(value, str) or not value.strip():
        return "未提供"
    value = single_line(value)
    parsed = urlsplit(value.removeprefix("git+"))
    if parsed.scheme not in ("https", "http", "git", "ssh") or not parsed.hostname:
        return "非公开 URL，已省略"
    if parsed.username or parsed.password:
        return "含用户信息的 URL，已省略"
    # Metadata URL queries can contain temporary credentials. They are not provenance.
    return value.split("?", 1)[0]


def source_label(value: object) -> str:
    if not isinstance(value, str):
        return "本地依赖，路径已省略"
    for prefix in ("registry+", "sparse+"):
        if value.startswith(prefix):
            return prefix + public_url(value[len(prefix):])
    if value.startswith("git+"):
        return public_url(value)
    return "本地依赖，路径已省略"


def license_declaration(value: object) -> str:
    if isinstance(value, str) and value.strip():
        # A license declaration is metadata, not a substitute for its legal text.
        return single_line(value)
    if isinstance(value, dict):
        return single_line(value.get("type", "未声明"))
    if isinstance(value, list):
        return "; ".join(license_declaration(item) for item in value) or "未声明"
    return "未声明"


def command(args: list[str], cwd: Path) -> str:
    try:
        result = subprocess.run(
            args, cwd=cwd, capture_output=True, text=True, encoding="utf-8", check=False
        )
    except OSError as error:
        raise RuntimeError(f"无法执行 {Path(args[0]).name}：{error.strerror}") from error
    if result.returncode:
        # Upstream stderr may include absolute paths or environment-specific URLs.
        # Ask the operator to run the recorded command instead of copying it here.
        name = "cargo metadata" if Path(args[0]).name.startswith("cargo") else "npm ls"
        raise RuntimeError(f"{name} 失败（退出码 {result.returncode}）；请单独运行该命令检查依赖安装状态")
    return result.stdout


def cargo_packages(workspace: Path) -> list[Package]:
    raw = command(["cargo", "metadata", "--format-version", "1", "--locked"], workspace)
    metadata = json.loads(raw)
    own = set(metadata["workspace_members"])
    result = []
    for item in metadata["packages"]:
        if item["id"] in own:
            continue
        result.append(Package(
            ecosystem="cargo", name=item["name"], version=item["version"],
            declared_license=license_declaration(item.get("license")),
            repository=public_url(item.get("repository")),
            source=source_label(item.get("source")),
            root=Path(item["manifest_path"]).parent.resolve(),
            license_file=item.get("license_file"),
        ))
    return result


def npm_packages(workspace: Path) -> list[Package]:
    desktop = (workspace / "apps" / "desktop").resolve()
    npm = shutil.which("npm.cmd" if os.name == "nt" else "npm")
    if npm is None:
        raise RuntimeError("找不到 npm；请安装 Node.js 并先执行 npm ci")
    npm_command = [npm]
    if os.name == "nt":
        node = shutil.which("node.exe") or shutil.which("node")
        npm_cli = Path(npm).parent / "node_modules" / "npm" / "bin" / "npm-cli.js"
        if node is None or not npm_cli.is_file():
            raise RuntimeError("无法定位 Windows Node.js 的 npm CLI；请使用官方 Node.js 或 setup-node 安装布局")
        npm_command = [node, str(npm_cli)]
    raw = command(npm_command + ["ls", "--omit=dev", "--parseable", "--all"], desktop)
    modules = desktop / "node_modules"
    result = []
    for line in raw.splitlines():
        if not line.strip():
            continue
        directory = Path(line.strip()).resolve()
        if directory == desktop:
            continue
        # Only installed packages under this app's node_modules are read.
        if not directory.is_relative_to(modules):
            raise RuntimeError("npm 返回应用 node_modules 之外的依赖；为避免读取其他目录，已停止收集")
        try:
            item = json.loads((directory / "package.json").read_text(encoding="utf-8"))
        except (OSError, UnicodeError, json.JSONDecodeError) as error:
            raise RuntimeError("无法读取 npm 生产依赖的 package.json；请重新执行 npm ci") from error
        declared = license_declaration(item.get("license", item.get("licenses")))
        match = re.fullmatch(r"SEE LICEN[CS]E IN (.+)", declared, flags=re.IGNORECASE)
        result.append(Package(
            ecosystem="npm", name=item["name"], version=item["version"],
            declared_license=declared,
            repository=public_url(item.get("repository")),
            source="npm 已安装的生产依赖树（含传递和 peer 依赖）",
            root=directory, license_file=match.group(1) if match else None,
        ))
    return result


def find_files(package: Package) -> list[tuple[str, Path]]:
    """Read package-root notices and dedicated notice directories only."""
    found: dict[Path, str] = {}
    root = package.root

    def add(path: Path, label: str) -> None:
        try:
            resolved = path.resolve()
            if not resolved.is_relative_to(root):
                package.gaps.append(f"{label} 指向包目录之外，未读取")
            elif not resolved.is_file():
                package.gaps.append(f"{label} 不存在或不是文件")
            else:
                found.setdefault(resolved, label)
        except OSError:
            package.gaps.append(f"{label} 无法读取")

    if package.license_file:
        explicit = Path(package.license_file)
        candidate = explicit if explicit.is_absolute() else root / explicit
        label = "声明的 license_file"
        if candidate.is_relative_to(root):
            label = candidate.relative_to(root).as_posix()
        add(candidate, label)

    try:
        children = sorted(root.iterdir(), key=lambda path: path.name.casefold())
    except OSError:
        package.gaps.append("无法枚举包根目录中的许可证文件")
        return []
    for child in children:
        if not child.name.upper().startswith(LICENSE_PREFIXES):
            continue
        if child.is_symlink():
            add(child, child.name)
        elif child.is_file():
            add(child, child.name)
        elif child.is_dir():
            # LICENSES/ may contain separate original license/notice files.
            for current, directories, files in os.walk(child, followlinks=False):
                directories[:] = sorted(name for name in directories if not (Path(current) / name).is_symlink())
                for name in sorted(files):
                    candidate = Path(current) / name
                    add(candidate, candidate.relative_to(root).as_posix())
    return sorted(((label, path) for path, label in found.items()), key=lambda item: item[0])


def attach_normal_targets(packages: list[Package], workspace: Path) -> None:
    """Annotate the two requested binaries without calling this a linked-file audit."""
    targets = [("dpda-desktop", "x86_64-pc-windows-msvc"),
               ("dpda-debug", "x86_64-unknown-linux-gnu")]
    for name, target in targets:
        raw = command(["cargo", "tree", "-p", name, "--target", target,
                       "--edges", "normal", "--prefix", "none", "--format", "{p}", "--locked"], workspace)
        keys = set()
        for line in raw.splitlines():
            parts = line.split()
            if len(parts) >= 2 and parts[1].startswith("v"):
                keys.add((parts[0], parts[1][1:]))
        for package in packages:
            if package.ecosystem == "cargo" and (package.name, package.version) in keys:
                package.normal_targets.append(f"{name} ({target})")


def supplemental_texts(package: Package, directory: Path | None, entries: list[dict]) -> list[tuple[str, bytes]]:
    if directory is None:
        return []
    result = []
    for entry in entries:
        if (entry.get("ecosystem"), entry.get("package"), entry.get("version")) != (package.ecosystem, package.name, package.version):
            continue
        try:
            path = (directory / entry["file"]).resolve()
            if not path.is_relative_to(directory.resolve()):
                raise ValueError("补充文件超出 assets/licenses")
            raw = path.read_bytes()
            if hashlib.sha256(raw).hexdigest() != entry["sha256"]:
                raise ValueError("补充文件 SHA-256 不匹配")
            kind = entry.get("kind")
            if kind == "exact-commit":
                vcs = json.loads((package.root / ".cargo_vcs_info.json").read_text(encoding="utf-8"))
                if vcs.get("git", {}).get("sha1") != entry.get("upstream_revision"):
                    raise ValueError("补充文件 commit 与安装包 .cargo_vcs_info.json 不一致")
                repository = package.repository.removesuffix(".git").removeprefix("https://github.com/")
                expected = f"https://raw.githubusercontent.com/{repository}/{entry['upstream_revision']}/"
                if not entry["source_url"].startswith(expected):
                    raise ValueError("补充文件 URL 与包仓库或固定 commit 不一致")
            elif kind == "standard-license":
                # Only the explicitly reviewed standard MPL text is accepted here.
                # This does not invent or substitute MIT/BSD copyright statements.
                if entry.get("license_expression") != "MPL-2.0" or package.declared_license != "MPL-2.0":
                    raise ValueError("标准文本仅支持已核对的 MPL-2.0 声明")
            else:
                raise ValueError("未知的补充许可证证据类型")
            label = f"upstream supplement: {entry['file']} | {public_url(entry['source_url'])}"
            if kind == "standard-license":
                label += f" | official: {public_url(entry.get('official_license_url'))}"
            result.append((label, raw))
        except (KeyError, OSError, ValueError, json.JSONDecodeError) as error:
            reason = str(error) if isinstance(error, ValueError) and not isinstance(error, json.JSONDecodeError) else type(error).__name__
            package.gaps.append(f"补充许可证未通过校验：{reason}")
    return result


def collect_texts(packages: list[Package], supplements: Path | None = None) -> dict[str, Document]:
    documents: dict[str, Document] = {}
    entries = []
    if supplements is not None and (supplements / "supplements.json").is_file():
        manifest = json.loads((supplements / "supplements.json").read_text(encoding="utf-8"))
        if manifest.get("schema_version") != 1:
            raise RuntimeError("补充许可证映射版本不支持")
        entries = manifest["licenses"]
    for package in packages:
        contents = []
        for label, path in find_files(package):
            try:
                contents.append((label, path.read_bytes()))
            except OSError:
                package.gaps.append(f"{label} 无法读取")
        contents.extend(supplemental_texts(package, supplements, entries))
        for label, raw in contents:
            if not raw.strip():
                package.gaps.append(f"{label} 内容为空")
                continue
            if b"\x00" in raw:
                package.gaps.append(f"{label} 包含 NUL 字节，未当作许可证文本输出")
                continue
            digest = hashlib.sha256(raw).hexdigest()
            document = documents.setdefault(digest, Document(raw))
            document.uses.add((package.label, label))
            package.texts.append((label, digest))
        if not package.texts:
            package.gaps.append("已安装包和已核验补充文件均未提供可读取的许可证原文")
        package.gaps = sorted(set(package.gaps))
    return documents


def render(packages: list[Package], documents: dict[str, Document]) -> str:
    packages = sorted(packages, key=lambda package: (package.ecosystem, package.name, package.version, package.source))
    ids = {digest: f"T{index:04d}" for index, digest in enumerate(sorted(documents), 1)}
    cargo_count = sum(package.ecosystem == "cargo" for package in packages)
    npm_count = sum(package.ecosystem == "npm" for package in packages)
    gaps = [package for package in packages if package.gaps]
    lines = [
        "DPDA — THIRD-PARTY LICENSE TEXTS / 第三方许可证原文", "",
        "本文件汇集已安装依赖的许可证和声明原文，不是许可证兼容性或履约结论。",
        "依赖元数据中的 license 字段与许可证原文分别保留；未找到的原文不会补写。",
        "Cargo 范围为锁文件解析图，跳过本项目 workspace 成员，可能包含开发、构建及其他平台依赖。",
        "npm 范围为已安装的生产依赖树，包含传递依赖和生产 peer 依赖。",
        "这些集合不等同于某个安装包实际包含的文件清单。外部系统库、SDK、游戏素材及另行下载的二进制不在本次扫描范围。",
        "Normal scope 来自 Windows dpda-desktop 和 Linux dpda-debug 的 cargo tree --edges normal；不覆盖构建时生成代码或外部 DLL。",
        "缺失包内文本时，仅使用 assets/licenses 中经过 SHA-256 和证据校验的补充原文；固定 commit 必须匹配包的 VCS 记录。",
        "相同字节内容按 SHA-256 去重；每份原文仍保留其包名、版本和包内相对文件来源。",
        "许可证原文中的 URL、版权和措辞未根据本项目的许可证修改。", "",
        "收集命令：", "  cargo metadata --format-version 1 --locked",
        "  npm ls --omit=dev --parseable --all  （apps/desktop）", "",
        f"依赖：Cargo {cargo_count}，npm {npm_count}；唯一原文 {len(documents)}；存在收集缺口的依赖 {len(gaps)}。",
        "", "=" * 78, "收集缺口 / COLLECTION GAPS", "=" * 78,
    ]
    if not gaps:
        lines.append("本次已安装包范围内，未发现许可证原文收集缺口。")
    for package in gaps:
        lines.extend([f"[{package.label}]", f"声明：{package.declared_license}", f"仓库：{package.repository}", f"Normal scope: {'; '.join(package.normal_targets) or '不在本次两个二进制的 normal 依赖树中'}"])
        lines.extend(f"  - {gap}" for gap in package.gaps)
        lines.append("")
    lines.extend(["", "=" * 78, "依赖清单 / PACKAGE INDEX", "=" * 78])
    for package in packages:
        lines.extend([f"[{package.label}]", f"License declaration: {package.declared_license}",
                      f"Repository: {package.repository}", f"Source: {package.source}"])
        if package.ecosystem == "cargo":
            lines.append(f"Normal scope: {'; '.join(package.normal_targets) or '不在本次两个二进制的 normal 依赖树中（可能属于构建或其他平台依赖）'}")
            if "crates.io" in package.source:
                lines.append(f"Source archive: https://crates.io/api/v1/crates/{quote(package.name, safe='')}/{quote(package.version, safe='')}/download")
        for label, digest in package.texts:
            lines.append(f"Text: {ids[digest]} — {label}")
        if not package.texts:
            lines.append("Text: 未找到；见收集缺口")
        lines.append("")
    lines.extend(["=" * 78, "许可证原文 / ORIGINAL TEXTS", "=" * 78])
    for digest in sorted(documents):
        document = documents[digest]
        try:
            text = document.raw.decode("utf-8")
            encoding = "UTF-8"
        except UnicodeDecodeError:
            # Latin-1 preserves every byte; never replace invalid bytes silently.
            text = document.raw.decode("latin-1")
            encoding = "ISO-8859-1 byte-preserving fallback (source encoding unknown)"
        lines.extend(["", f"[{ids[digest]}]", f"SHA-256 (original bytes): {digest}", f"Text decoding: {encoding}", "Sources:"])
        lines.extend(f"  - {package}: {label}" for package, label in sorted(document.uses))
        lines.extend(["----- BEGIN ORIGINAL TEXT -----", text, "----- END ORIGINAL TEXT -----"])
    return "\n".join(lines) + "\n"


def write_atomic(output: Path, text: str) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", newline="\n", dir=output.parent, delete=False) as handle:
            temporary = Path(handle.name)
            handle.write(text)
        os.replace(temporary, output)
        output.chmod(0o644)
    finally:
        if temporary and temporary.exists():
            temporary.unlink()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="输出许可证文本清单的位置")
    parser.add_argument("--workspace", type=Path, default=Path(__file__).resolve().parent.parent,
                        help="Rust workspace 根目录（默认为脚本所在项目）")
    parser.add_argument("--strict", action="store_true", help="存在收集缺口时仍输出清单，但以退出码 2 结束")
    parser.add_argument("--strict-normal", action="store_true", help="已审阅的 normal 依赖树或 npm 生产依赖存在缺口时以退出码 2 结束")
    args = parser.parse_args(argv)
    try:
        workspace = args.workspace.resolve()
        candidates = cargo_packages(workspace) + npm_packages(workspace)
        # Identical name/version from different origins must remain separate.
        unique: dict[tuple[str, str, str, str], Package] = {}
        for package in candidates:
            key = (package.ecosystem, package.name, package.version, package.source)
            previous = unique.get(key)
            if previous is None:
                unique[key] = package
            elif previous.root != package.root:
                # npm may install the same version at several locations. Read both;
                # the notice documents themselves are deduplicated by exact bytes.
                key = (*key[:3], f"{package.source} (additional installed copy)")
                while key in unique:
                    key = (*key[:3], key[3] + " +")
                package.source = key[3]
                unique[key] = package
        packages = list(unique.values())
        attach_normal_targets(packages, workspace)
        documents = collect_texts(packages, workspace / "assets" / "licenses")
        write_atomic(args.output, render(packages, documents))
        missing = sum(bool(package.gaps) for package in packages)
        print(f"Collected {len(packages)} packages, {len(documents)} unique texts; {missing} packages have collection gaps.")
        normal_missing = sum(bool(package.gaps) and (bool(package.normal_targets) or package.ecosystem == "npm") for package in packages)
        print(f"Audited normal/production dependency gaps: {normal_missing}")
        print(f"Output: {args.output.name}")
        return 2 if (args.strict and missing) or (args.strict_normal and normal_missing) else 0
    except (RuntimeError, KeyError, ValueError, OSError) as error:
        # Avoid including command output, manifests, or absolute paths in reports.
        message = str(error) if isinstance(error, RuntimeError) else type(error).__name__
        print(f"License collection failed: {message}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
