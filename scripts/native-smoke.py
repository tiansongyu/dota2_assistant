#!/usr/bin/env python3
"""Exercise a real Linux Tauri binary through W3C WebDriver with isolated app data.

No IPC mocks or external API requests are used. An optional existing SQLite
database is copied with SQLite backup; the source is opened read-only.
"""
import argparse
import base64
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

STEAM64_BASE = 76561197960265728


def unused_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def executable(value):
    found = shutil.which(value) if not Path(value).is_file() else str(Path(value).resolve())
    if not found:
        raise ValueError(f"Executable not found: {value}")
    return found


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


class Driver:
    def __init__(self, base):
        self.base = base
        self.session = None
        self.capabilities = {}
        self.calls = []

    def request(self, method, path, payload=None, timeout=20):
        body = json.dumps(payload).encode() if payload is not None else None
        req = urllib.request.Request(self.base + path, data=body, method=method,
                                     headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=timeout) as response:
                result = json.load(response)
        except urllib.error.HTTPError as exc:
            message = exc.read().decode("utf-8", "replace")
            raise RuntimeError(f"WebDriver HTTP {exc.code}: {message[:1200]}") from exc
        value = result.get("value")
        if isinstance(value, dict) and value.get("error") and "ok" not in value:
            raise RuntimeError(f"WebDriver: {value['error']}: {value.get('message', '')}")
        return value

    def start(self, application):
        value = self.request("POST", "/session", {"capabilities": {"alwaysMatch": {
            "browserName": "wry", "tauri:options": {"application": str(application)}
        }}}, timeout=45)
        self.session = value["sessionId"]
        self.capabilities = value.get("capabilities", {})
        self.request("POST", self.path("timeouts"), {"script": 15000, "pageLoad": 30000, "implicit": 0})

    def path(self, suffix):
        return f"/session/{self.session}/{suffix}"

    def script(self, script, args=None):
        return self.request("POST", self.path("execute/sync"), {"script": script, "args": args or []})

    def invoke(self, command, args=None, should_fail=False):
        script = """
        const done = arguments[arguments.length - 1];
        const timer = setTimeout(() => done({ok: false, timeout: true, error: 'IPC timeout'}), 10000);
        window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1]).then(value => {
          clearTimeout(timer); done({ok: true, value});
        }).catch(error => { clearTimeout(timer); done({ok: false, error: String(error)}); });
        """
        started = time.monotonic()
        result = self.request("POST", self.path("execute/async"),
                              {"script": script, "args": [command, args or {}]})
        elapsed_ms = (time.monotonic() - started) * 1000
        self.calls.append({"command": command, "ok": result.get("ok", False),
                           "elapsed_ms": round(elapsed_ms, 3), "expected_error": should_fail})
        if result.get("timeout"):
            raise RuntimeError(f"{command}: IPC did not respond")
        if should_fail:
            if result.get("ok"):
                raise AssertionError(f"{command}: invalid input was accepted")
            return result.get("error", "")
        if not result.get("ok"):
            raise RuntimeError(f"{command}: {result.get('error')}")
        return result.get("value")

    def wait_script(self, script, args=None, seconds=10):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            value = self.script(script, args)
            if value:
                return value
            time.sleep(0.1)
        raise AssertionError(f"Renderer condition did not become true: {script[:160]}")

    def screenshot(self, output):
        encoded = self.request("GET", self.path("screenshot"))
        output.write_bytes(base64.b64decode(encoded, validate=True))

    def stop(self):
        if self.session:
            self.request("DELETE", f"/session/{self.session}")
            self.session = None


def run(args, work):
    application = Path(args.application).resolve(strict=True)
    account = int(args.account)
    if account > 4294967295:
        account -= STEAM64_BASE
    if not 0 < account < 4294967295:
        raise ValueError("--account must be a valid Steam32 or individual Steam64 ID")
    data = work / "data" / "io.dpda.desktop"
    data.mkdir(parents=True)
    database_path = data / "dpda.sqlite"
    expected_count = 0
    if args.fixture_db:
        source_path = Path(args.fixture_db).resolve(strict=True)
        with sqlite3.connect(source_path.as_uri() + "?mode=ro", uri=True) as source:
            with sqlite3.connect(database_path) as destination:
                source.backup(destination)
                # Clean only the isolated copy's preferences to exercise onboarding.
                destination.execute("DELETE FROM settings")
                expected_count = destination.execute(
                    "SELECT COUNT(*) FROM matches WHERE account_id=?", (account,)).fetchone()[0]
        if not expected_count:
            raise ValueError("The fixture database has no matches for --account")

    env = os.environ.copy()
    for name, folder in [("XDG_DATA_HOME", work / "data"), ("XDG_CONFIG_HOME", work / "config"),
                         ("XDG_CACHE_HOME", work / "cache"), ("XDG_RUNTIME_DIR", work / "runtime")]:
        folder.mkdir(exist_ok=True)
        folder.chmod(0o700)
        env[name] = str(folder)
    env.update({"GTK_USE_PORTAL": "0", "GDK_BACKEND": "x11", "NO_AT_BRIDGE": "1",
                "GIO_USE_VFS": "local", "GSETTINGS_BACKEND": "memory",
                "WEBKIT_DISABLE_COMPOSITING_MODE": "1", "WEBKIT_DISABLE_DMABUF_RENDERER": "1"})
    port, native_port = unused_port(), unused_port()
    while native_port == port:
        native_port = unused_port()
    command = [executable("xvfb-run"), "-a", "-s", "-screen 0 1600x1000x24",
               executable("dbus-run-session"), "--", executable(args.tauri_driver),
               "--native-driver", executable(args.native_driver), "--port", str(port),
               "--native-port", str(native_port)]
    report = {"status": "running", "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "platform": platform.platform(), "application": str(application),
              "binary_sha256": sha256_file(application),
              "isolated_directory": str(work), "fixture_matches": expected_count,
              "fixture_account_id": account, "external_api_requests": 0,
              "notes": ["Hero position/pool values are explicit test configuration, not inferred player positions.",
                        "Linux native integration does not validate Windows capture, global hotkeys or game FPS."],
              "checks": []}
    driver = Driver(f"http://127.0.0.1:{port}")
    process = None

    def check(name, condition, detail=None):
        report["checks"].append({"name": name, "passed": bool(condition), "detail": detail})
        if not condition:
            raise AssertionError(name)

    try:
        with (work / "driver.log").open("w") as output:
            process = subprocess.Popen(command, env=env, stdout=output, stderr=subprocess.STDOUT,
                                       start_new_session=True)
        deadline = time.monotonic() + 10
        while True:
            if process.poll() is not None:
                raise RuntimeError("tauri-driver exited before becoming ready; inspect driver.log")
            try:
                if driver.request("GET", "/status", timeout=1).get("ready"):
                    break
            except (urllib.error.URLError, TimeoutError, ConnectionError):
                pass
            if time.monotonic() >= deadline:
                raise TimeoutError("tauri-driver did not become ready within 10 seconds")
            time.sleep(0.1)
        driver.start(application)
        report["capabilities"] = driver.capabilities
        driver.wait_script("return !!document.querySelector('nav') && typeof window.__TAURI_INTERNALS__?.invoke === 'function';")
        initial = driver.invoke("get_snapshot")
        check("real_local_tauri_origin", driver.script("return location.href;").startswith("tauri://localhost"))
        check("fresh_preferences", initial["profile"] is None and not initial["matches"])
        check("current_snapshot_contract", isinstance(initial.get("automatic_pool"), list)
              and isinstance(initial.get("pool_candidates"), list))
        if args.game_assets:
            asset_result = driver.request("POST", driver.path("execute/async"), {
                "script": """
                const done = arguments[arguments.length - 1];
                (async () => {
                  const response = await fetch('/game-icons/manifest.json');
                  if (!response.ok) throw new Error('manifest HTTP ' + response.status);
                  const manifest = await response.json();
                  const categories = ['heroes', 'hero-icons', 'abilities', 'items', 'facets', 'ui'];
                  const samples = categories.map(category => manifest.entries.find(
                    entry => entry.category === category && entry.status === 'downloaded'));
                  const talent = manifest.entries.find(entry => entry.path === 'ui/talents.svg');
                  if (talent) samples.push(talent);
                  const images = await Promise.all(samples.map(entry => new Promise((resolve, reject) => {
                    if (!entry) { reject(new Error('missing downloaded category')); return; }
                    const img = new Image();
                    img.onload = () => resolve({category: entry.category, path: entry.path,
                      width: img.naturalWidth, height: img.naturalHeight,
                      expected_width: entry.width, expected_height: entry.height});
                    img.onerror = () => reject(new Error('cannot decode ' + entry.path));
                    img.src = '/game-icons/' + entry.path;
                  })));
                  done({ok: true, complete: manifest.complete, entries: manifest.entries.length,
                    source_commit: manifest.source.commit, images});
                })().catch(error => done({ok: false, error: String(error)}));
                """, "args": []})
            check("bundled_manifest_available_under_native_csp",
                  asset_result.get("ok") and asset_result.get("complete"), asset_result)
            check("bundled_png_and_svg_decode_in_webkit", all(
                image["width"] == image["expected_width"] and image["height"] == image["expected_height"]
                for image in asset_result["images"]))
            driver.script("Array.from(document.querySelectorAll('nav button')).find(b => b.textContent.trim() === '游戏资源').click(); return true;")
            driver.wait_script("return document.querySelector('.asset-grid img')?.naturalWidth > 0;")
            check("native_resource_browser_without_account",
                  driver.script("return document.body.innerText.includes('本地图片') && document.documentElement.scrollWidth <= innerWidth;"))
            driver.screenshot(work / "game-assets.png")
        driver.invoke("save_account", {"input": "0"}, should_fail=True)
        check("invalid_account_does_not_mutate_profile", driver.invoke("get_snapshot")["profile"] is None)
        if args.steam_discovery:
            discovered = driver.invoke("discover_steam_accounts")
            candidates = discovered["candidates"]
            check("native_steam_discovery_contract", discovered["status"] in {
                "found", "partial", "not_installed", "no_records", "permission_denied",
                "invalid_format", "io_error", "unsupported_platform"}, {
                    "status": discovered["status"], "candidate_count": len(candidates),
                    "locations_checked": discovered["locations_checked"],
                })
            check("discovery_does_not_change_active_account",
                  driver.invoke("get_snapshot")["profile"] is None)
            check("discovered_steam64_preserves_precision", all(
                isinstance(candidate["steam64"], str)
                and candidate["steam64"] == str(STEAM64_BASE + candidate["account_id"])
                for candidate in candidates))
            if candidates:
                candidate = candidates[0]
                driver.script("Array.from(document.querySelectorAll('nav button')).find(b => b.textContent.trim() === '总览').click(); return true;")
                driver.wait_script("return !!document.querySelector('input[name=\"steam-account\"]');")
                driver.script("Array.from(document.querySelectorAll('input[name=\"steam-account\"]')).find(e => e.value === arguments[0]).click(); return true;",
                              [str(candidate["account_id"])])
                driver.wait_script("return Array.from(document.querySelectorAll('button')).some(b => b.textContent.trim() === '使用此账号' && !b.disabled);")
                driver.script("Array.from(document.querySelectorAll('button')).find(b => b.textContent.trim() === '使用此账号').click(); return true;")
                driver.wait_script("return document.body.innerText.includes(arguments[0]);",
                                   [f"Account {candidate['account_id']}"])
                check("native_detected_account_selected_without_typing",
                      driver.invoke("get_snapshot")["profile"]["steam64"] == candidate["steam64"])
            # The actual discovered identity is never written into this report or screenshots.
            # Restore the explicit public test account below before the remaining UI checks.
        driver.invoke("save_account", {"input": str(STEAM64_BASE + account)})
        snapshot = driver.invoke("get_snapshot")
        check("steam64_is_exact_string", snapshot["profile"] == {
            "account_id": account, "steam64": str(STEAM64_BASE + account)})
        check("real_sqlite_history_loaded", len(snapshot["matches"]) == expected_count,
              {"expected": expected_count, "actual": len(snapshot["matches"])})
        driver.wait_script("return document.body.innerText.includes(arguments[0]);", [f"Account {account}"])
        check("backend_event_refreshes_renderer", True)
        ids = list(dict.fromkeys([m["hero_id"] for m in snapshot["matches"]] + [h["id"] for h in snapshot["heroes"]]))[:3]
        first, opponent, excluded = ids
        pool = {"mode": "manual", "included": ids, "excluded": [excluded],
                "roles": {str(hero): [2] for hero in ids}, "priorities": {}}
        driver.invoke("save_pool", {"pool": pool})
        driver.invoke("save_pool", {"pool": {**pool, "included": [4294967294]}}, should_fail=True)
        check("invalid_pool_does_not_mutate_settings", driver.invoke("get_snapshot")["pool"] == pool)
        draft = {"radiant": [None] * 5, "dire": [opponent, None, None, None, None],
                 "own_team": "radiant", "banned": []}
        driver.invoke("update_draft", {"draft": draft})
        snapshot = driver.invoke("get_snapshot")
        check("pool_selected_and_excluded_constraints", [r["hero_id"] for r in snapshot["recommendations"]] == [first])
        driver.invoke("update_draft", {"draft": {**draft, "radiant": [opponent, None, None, None, None]}}, should_fail=True)
        check("duplicate_draft_rejection_is_atomic", driver.invoke("get_snapshot")["draft"] == draft)
        driver.invoke("update_draft", {"draft": {**draft, "banned": [first]}})
        check("banned_candidate_removed", not driver.invoke("get_snapshot")["recommendations"])
        driver.invoke("update_draft", {"draft": draft})
        settings = {**snapshot["settings"], "theme": "light", "overlay_compact": True,
                    "top_k": 3, "capture_hz": 0.5}
        driver.invoke("save_settings", {"settings": settings})
        driver.invoke("save_settings", {"settings": {**settings, "top_k": 0}}, should_fail=True)
        check("invalid_settings_are_atomic", driver.invoke("get_snapshot")["settings"] == settings)
        error = driver.invoke("start_capture", should_fail=True)
        check("linux_capture_rejection_is_explicit", "Windows" in error)
        driver.script("Array.from(document.querySelectorAll('button')).find(b => b.textContent.trim() === 'BP 推荐').click(); return true;")
        driver.wait_script("return document.body.innerText.includes('BP 工作台');")
        if args.game_assets:
            driver.script("document.querySelector('.hero-mark img')?.scrollIntoView({block: 'center'}); return true;")
            driver.wait_script("return Array.from(document.querySelectorAll('.hero-mark img')).some(img => img.complete && img.naturalWidth > 0);")
            check("native_hero_marks_use_local_images", driver.script("return Array.from(document.querySelectorAll('.hero-mark img')).every(img => new URL(img.src).origin === location.origin);"))
        check("main_window_no_horizontal_overflow", driver.script("return document.documentElement.scrollWidth <= innerWidth;"))
        driver.screenshot(work / "main.png")
        main_handle = driver.request("GET", driver.path("window"))
        driver.invoke("open_overlay")
        driver.invoke("open_overlay")
        handles = driver.request("GET", driver.path("window/handles"))
        check("overlay_creation_is_idempotent", len(handles) == 2, {"windows": len(handles)})
        overlay = next(handle for handle in handles if handle != main_handle)
        driver.request("POST", driver.path("window"), {"handle": overlay})
        driver.wait_script("return document.querySelector('.overlay-content.compact') !== null;")
        check("overlay_uses_real_recommendations", driver.script("return location.search.includes('overlay=1') && document.body.innerText.includes('推荐分');"))
        driver.wait_script("return innerHeight === 420;")
        check("compact_overlay_window_height", driver.request("GET", driver.path("window/rect"))["height"] == 420)
        driver.screenshot(work / "overlay.png")
        # Exercise an actual user-sized native window, then ensure a mode toggle preserves width.
        driver.request("POST", driver.path("window/rect"), {"width": 480, "height": 420})
        driver.invoke("save_settings", {"settings": {**settings, "overlay_compact": False}})
        driver.wait_script("return innerHeight === 620 && innerWidth === 480;")
        check("expanded_overlay_height_preserves_user_width", True)
        driver.invoke("save_settings", {"settings": settings})
        driver.wait_script("return innerHeight === 420 && innerWidth === 480;")
        check("compact_toggle_restores_height_without_resetting_width", True)
        check("overlay_no_horizontal_overflow", driver.script("return document.documentElement.scrollWidth <= innerWidth;"))
        driver.request("POST", driver.path("window"), {"handle": main_handle})
        # A safe rejection is valid if the isolated desktop cannot register the recovery shortcut.
        try:
            driver.invoke("save_settings", {"settings": {**settings, "overlay_click_through": True}})
            check("click_through_remains_recoverable_from_main", True, "enabled then disabled through main-window IPC")
        except RuntimeError as exc:
            if "恢复快捷键未注册" not in str(exc):
                raise
            check("click_through_requires_recovery_shortcut", True, str(exc))
        finally:
            driver.invoke("save_settings", {"settings": settings})
        driver.stop()
        driver.start(application)
        driver.wait_script("return typeof window.__TAURI_INTERNALS__?.invoke === 'function';")
        restored = driver.invoke("get_snapshot")
        check("account_pool_settings_survive_restart", restored["profile"]["account_id"] == account
              and restored["pool"] == pool and restored["settings"] == settings)
        check("history_survives_restart", len(restored["matches"]) == expected_count)
        check("transient_draft_does_not_survive_restart", not any(restored["draft"]["radiant"] + restored["draft"]["dire"]))
        driver.stop()
        with sqlite3.connect(database_path.as_uri() + "?mode=ro", uri=True) as connection:
            integrity = connection.execute("PRAGMA quick_check").fetchone()[0]
        check("sqlite_integrity_after_native_commands", integrity == "ok")
        report["status"] = "passed"
    except Exception as exc:
        report["status"] = "failed"
        report["error"] = f"{type(exc).__name__}: {exc}"
    finally:
        try:
            driver.stop()
        except Exception:
            pass
        if process and process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)
        report["ipc_calls"] = driver.calls
        report["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        (work / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--application", required=True, help="Built Tauri binary, including frontend assets")
    parser.add_argument("--tauri-driver", default="tauri-driver")
    parser.add_argument("--native-driver", default="WebKitWebDriver")
    parser.add_argument("--account", required=True, help="Explicit public test account ID; never a user default")
    parser.add_argument("--fixture-db", help="Optional existing database with real imported matches; opened read-only")
    parser.add_argument("--game-assets", action="store_true", help="Verify bundled PNG/SVG icons, local manifest CSP, and the resource browser")
    parser.add_argument("--steam-discovery", action="store_true", help="Check actual local Steam discovery and account selection; omit discovered identities from reports")
    parser.add_argument("--output", type=Path, help="New output directory; must not already exist")
    args = parser.parse_args()
    if platform.system() != "Linux":
        parser.error("This Xvfb/WebKitWebDriver harness currently supports Linux only")
    if args.output:
        work = args.output.resolve()
        work.mkdir(parents=True, exist_ok=False)
    else:
        work = Path(tempfile.mkdtemp(prefix="dpda-native-smoke-"))
    try:
        report = run(args, work)
    except Exception as exc:
        report = {"status": "failed", "error": f"{type(exc).__name__}: {exc}", "checks": []}
        (work / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "checks": len(report["checks"]),
                      "report": str(work / "report.json"), "error": report.get("error")}, ensure_ascii=False))
    raise SystemExit(0 if report["status"] == "passed" else 1)


if __name__ == "__main__":
    main()
