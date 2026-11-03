#!/usr/bin/env python3
"""Start the real Linux desktop with an isolated copy of an existing DPDA database.

Python standard library only. Initial preparation uses SQLite backup from a
read-only source connection. Resume never copies or resets the session database.
"""

import argparse
import contextlib
import datetime
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time

if sys.platform.startswith("linux"):
    import fcntl

ROOT = Path(__file__).resolve().parents[1]
APP_ID = "io.dpda.desktop"
STEAM64_BASE = 76561197960265728
SESSION_FORMAT = 1


class LaunchError(Exception):
    """Actionable input or environment error without printing private records."""


def account_id(value):
    # Never route Steam64 through a floating-point number.
    if not isinstance(value, str) or not re.fullmatch(r"[0-9]{1,20}", value):
        raise LaunchError("--account 必须是十进制 Steam32 或 Steam64 字符串。")
    number = int(value)
    if number >= STEAM64_BASE:
        number -= STEAM64_BASE
    if not 0 < number < 4294967295:
        raise LaunchError("--account 不在有效 Steam32 / 个人 Steam64 范围内。")
    return number


def database_path(session):
    return session / "data" / APP_ID / "dpda.sqlite"


def checked_database(connection, selected=None):
    version = connection.execute("PRAGMA user_version").fetchone()[0]
    if version not in (1, 2):
        raise LaunchError("数据库 Schema 不受此启动脚本支持（当前支持版本 1、2）。")
    if connection.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
        raise LaunchError("数据库完整性检查失败；未启动应用。")
    tables = {row[0] for row in connection.execute(
        "SELECT name FROM sqlite_master WHERE type='table'")}
    if not {"profiles", "matches", "settings", "match_heroes", "sync_jobs"} <= tables:
        raise LaunchError("指定文件不是完整的 DPDA 数据库。")
    if version == 2 and "role_evidence" not in tables:
        raise LaunchError("数据库 Schema 2 缺少 role_evidence 表。")
    if selected is not None:
        profile = connection.execute(
            "SELECT steam64 FROM profiles WHERE account_id=?", (selected,)).fetchone()
        if profile is None:
            raise LaunchError("指定账号不存在于数据库中；请核对 --account，或先完成导入。")
        if profile[0] != str(STEAM64_BASE + selected):
            raise LaunchError("数据库中的 Steam32 / Steam64 不一致；未修改源文件。")


def prepare(source_path, selected, session_root):
    source_path = source_path.expanduser().resolve(strict=True)
    if not source_path.is_file():
        raise LaunchError("--db 必须指向已有 SQLite 文件。")
    session_root = session_root.expanduser().resolve()
    session_root.mkdir(mode=0o700, parents=True, exist_ok=True)
    session = None
    try:
        with contextlib.closing(sqlite3.connect(
                source_path.as_uri() + "?mode=ro", uri=True, timeout=10)) as source:
            source.execute("PRAGMA query_only=ON")
            checked_database(source, selected)
            session = Path(tempfile.mkdtemp(prefix="session-", dir=session_root))
            for directory in [session / name for name in ("data", "config", "cache", "state")]:
                directory.mkdir(mode=0o700)
            database_path(session).parent.mkdir(mode=0o700)
            deadline = time.monotonic() + 60

            def progress(_status, _remaining, _total):
                if time.monotonic() >= deadline:
                    raise LaunchError("SQLite 备份超过 60 秒；请暂停写入源库后重试。")

            with contextlib.closing(sqlite3.connect(database_path(session))) as destination:
                source.backup(destination, pages=256, progress=progress, sleep=0.05)
                checked_database(destination, selected)
                now = int(time.time())
                with destination:
                    destination.execute(
                        "INSERT INTO settings(key,value,updated_at) VALUES(?,?,?) "
                        "ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
                        ("active_profile", json.dumps({"account_id": selected,
                                                      "steam64": str(STEAM64_BASE + selected)}), now))
                    # Rust's #[serde(default)] supplies current application defaults.
                    # Preserve every existing preference and hero-pool entry.
                    destination.execute(
                        "INSERT OR IGNORE INTO settings(key,value,updated_at) VALUES(?,?,?)",
                        ("app_settings", "{}", now))
            database_path(session).chmod(0o600)
        metadata = {"format_version": SESSION_FORMAT, "app_identifier": APP_ID,
                    "created_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    "source_database": str(source_path)}
        (session / "session.json").write_text(
            json.dumps(metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        (session / "session.json").chmod(0o600)
        return session
    except BaseException:
        # Only the freshly-created directory is removable here, never --resume.
        if session is not None:
            shutil.rmtree(session)
        raise


def resume(session_path):
    session = session_path.expanduser().resolve(strict=True)
    try:
        metadata = json.loads((session / "session.json").read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise LaunchError("--resume 目录缺少有效 session.json。") from exc
    if (not isinstance(metadata, dict) or metadata.get("format_version") != SESSION_FORMAT
            or metadata.get("app_identifier") != APP_ID):
        raise LaunchError("--resume 目录不是受支持的 DPDA 调试会话。")
    for directory in ("data", "config", "cache", "state"):
        if not (session / directory).is_dir():
            raise LaunchError("调试会话目录不完整；未重新复制或覆盖任何文件。")
    with contextlib.closing(sqlite3.connect(
            database_path(session).as_uri() + "?mode=ro", uri=True, timeout=10)) as database:
        checked_database(database)
    return session


def environment(session, original=None):
    env = dict(os.environ if original is None else original)
    for key, directory in [("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"),
                           ("XDG_CACHE_HOME", "cache"), ("XDG_STATE_HOME", "state")]:
        env[key] = str(session / directory)
    # Retain XDG_RUNTIME_DIR, DISPLAY and D-Bus: Wayland uses its existing socket.
    return env


def require_display(env):
    if not (env.get("DISPLAY") or env.get("WAYLAND_DISPLAY")):
        raise LaunchError("未找到 DISPLAY / WAYLAND_DISPLAY。请在 Linux 图形桌面终端启动；"
                          "仅准备数据库可加 --prepare-only。")
    if env.get("WAYLAND_DISPLAY") and not env.get("DISPLAY"):
        display = Path(env["WAYLAND_DISPLAY"])
        if not display.is_absolute():
            runtime = env.get("XDG_RUNTIME_DIR")
            if not runtime:
                raise LaunchError("Wayland 环境缺少 XDG_RUNTIME_DIR。")
            display = Path(runtime) / display
        if not display.exists():
            raise LaunchError("WAYLAND_DISPLAY 指向的显示服务不存在。")


def check_binary(binary):
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise LaunchError("未找到可运行的 target/debug/dpda-desktop；请加 --build。")
    if shutil.which("ldd") is None:
        raise LaunchError("缺少 ldd，无法检查 Linux 动态库；请安装 libc-bin。")
    result = subprocess.run(["ldd", str(binary)], capture_output=True, text=True, check=False)
    missing = [line.strip() for line in result.stdout.splitlines() if "not found" in line]
    if result.returncode or missing:
        detail = "; ".join(missing) if missing else "无法解析此 Linux 可执行文件"
        raise LaunchError("桌面运行依赖缺失：" + detail
                          + "。需要 GTK 3、WebKitGTK 4.1 和构建时使用的 OpenCV 动态库。")


def build():
    for command in ("npm", "cargo", "pkg-config"):
        if shutil.which(command) is None:
            raise LaunchError(f"构建依赖缺失：{command}。")
    if not (ROOT / "apps/desktop/node_modules/.bin/tauri").is_file():
        raise LaunchError("前端构建依赖未安装；先执行 npm --prefix apps/desktop ci。")
    for package in ("gtk+-3.0", "webkit2gtk-4.1", "opencv4"):
        result = subprocess.run(["pkg-config", "--exists", package], check=False)
        if result.returncode:
            raise LaunchError(f"缺少 {package} 开发库；安装 Linux 桌面/OpenCV 构建依赖后重试。")
    # Tauri CLI embeds built frontend assets; plain cargo build can use devUrl.
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(ROOT / "target")
    env.pop("CARGO_BUILD_TARGET", None)
    result = subprocess.run(["npm", "--prefix", "apps/desktop", "run", "tauri", "--",
                             "build", "--debug", "--no-bundle"], cwd=ROOT, env=env, check=False)
    if result.returncode:
        raise LaunchError("Tauri debug 构建失败；请查看上方构建错误。")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", type=Path, help="已有 DPDA SQLite；只读备份，不覆盖源库")
    parser.add_argument("--account", help="精确 Steam32 / Steam64，必须已存在于 --db")
    parser.add_argument("--session-root", type=Path,
                        help="新会话的父目录；每次创建随机子目录，已有目录不覆盖")
    parser.add_argument("--resume", type=Path, help="继续已有会话，保留全部数据和界面设置")
    parser.add_argument("--prepare-only", action="store_true", help="仅准备/检查会话，输出 JSON 与启动命令")
    parser.add_argument("--build", action="store_true", help="通过 Tauri CLI 重新构建嵌入前端的 debug 应用")
    args = parser.parse_args(argv)
    if args.resume:
        if args.db is not None or args.account is not None or args.session_root is not None:
            parser.error("--resume 不能与 --db、--account、--session-root 同用")
    elif args.db is None or args.account is None:
        parser.error("首次启动需要同时提供 --db 和 --account；继续已有会话使用 --resume")
    try:
        if not sys.platform.startswith("linux"):
            raise LaunchError("此调试启动脚本只支持 Linux。")
        selected = account_id(args.account) if args.account is not None else None
        if not args.prepare_only:
            require_display(os.environ)
        if args.build:
            build()
        binary = ROOT / "target/debug/dpda-desktop"
        if not args.prepare_only:
            check_binary(binary)
        session = resume(args.resume) if args.resume else prepare(
            args.db, selected,
            args.session_root or Path.home() / ".local/share/dpda-debug-sessions")
        resume_command = shlex.join([sys.executable, str(Path(__file__).resolve()), "--resume", str(session)])
        print(json.dumps({"status": "prepared" if args.prepare_only else "starting",
                          "session": str(session), "database": str(database_path(session)),
                          "source_database_modified": False,
                          "resume_command": resume_command}, ensure_ascii=False, indent=2), flush=True)
        if args.prepare_only:
            return 0
        # Retain an advisory lock across exec, preventing this launcher from
        # opening the same debug session twice. Different sessions are independent.
        lock = os.open(session / "launch.lock", os.O_CREAT | os.O_RDWR, 0o600)
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            os.close(lock)
            raise LaunchError("该调试会话仍在运行；请关闭原窗口后重试。") from exc
        os.set_inheritable(lock, True)
        os.execve(binary, [str(binary)], environment(session))
    except (LaunchError, OSError, sqlite3.Error) as exc:
        print(f"启动失败：{exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
