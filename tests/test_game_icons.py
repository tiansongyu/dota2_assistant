"""Downloader contract tests using synthetic images and a loopback HTTP server.

These fixtures test file/transport safety, not real game recognition or CDN coverage.
"""
import contextlib
import hashlib
import http.server
import importlib.util
import io
import json
import os
from pathlib import Path
import signal
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.request
import zlib

SPEC = importlib.util.spec_from_file_location("game_icons", Path(__file__).resolve().parents[1] / "scripts/fetch-game-icons.py")
icons = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(icons)


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)


def png(scanline=b"\0\x20\x40\x60\xff"):
    return icons.PNG_SIGNATURE + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(scanline)) + chunk(b"IEND", b"")


SVG = b'<svg xmlns="http://www.w3.org/2000/svg" width="73" height="77" viewBox="0 0 73 77"><path d="M0 0L1 1" fill="white"/></svg>'


def catalog():
    prefix = "/apps/dota2/images/dota_react/"
    return {
        "heroes": {"7": {"id": 7, "name": "npc_dota_hero_test", "localized_name": "Test hero", "img": prefix + "heroes/test.png?", "icon": prefix + "heroes/icons/test.png?"}},
        "abilities": {"test_spell": {"dname": "Spell", "img": prefix + "abilities/test_spell.png"}, "special_bonus_test": {}, "generic_hidden": {}, "AbilityCastPoint": {}},
        "ability_ids": {"101": "test_spell", "102": "special_bonus_test", "103": "id_only_spell"},
        "items": {"blink": {"id": 1, "img": prefix + "items/blink.png"}, "recipe_a": {"id": 10, "img": prefix + "items/recipe.png"}},
        "item_ids": {"0": "ability_base", "1": "blink", "10": "recipe_a", "11": "recipe_b", "12": "old_item"},
        "hero_abilities": {"npc_dota_hero_test": {"abilities": ["test_spell", "generic_hidden"], "talents": [{"name": "special_bonus_test", "level": 1}], "facets": [{"id": 0, "icon": "mana", "deprecated": "true"}]}},
    }


class FakeLimiter:
    def __init__(self):
        self.calls = 0
        self.delays = []

    def wait(self):
        self.calls += 1

    def postpone(self, delay):
        self.delays.append(delay)


class TestImages(unittest.TestCase):
    def test_png_complete_structure_and_scanlines(self):
        self.assertEqual(icons.validate_png(png()), (1, 1))
        for bad in [b"<html>not a PNG</html>", png()[:24], png()[:-1], png() + b"junk", png(b"\x05\x20\x40\x60\xff"), png(b"\0" * 100)]:
            with self.subTest(length=len(bad)), self.assertRaises(ValueError):
                icons.validate_png(bad)

    def test_png_crc_and_deflate_are_checked(self):
        corrupt = bytearray(png())
        corrupt[-1] ^= 1
        with self.assertRaisesRegex(ValueError, "CRC"):
            icons.validate_png(bytes(corrupt))
        bad = icons.PNG_SIGNATURE + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0)) + chunk(b"IDAT", b"not zlib") + chunk(b"IEND", b"")
        with self.assertRaisesRegex(ValueError, "deflate"):
            icons.validate_png(bad)

    def test_svg_geometry(self):
        self.assertEqual(icons.validate_svg(SVG), (73, 77))

    def test_svg_rejects_active_and_external_content(self):
        attacks = [
            SVG.replace(b"<path", b'<script>bad()</script><path'),
            SVG.replace(b"<path", b'<foreignObject/><path'),
            SVG.replace(b'<path ', b'<path onclick="bad()" '),
            SVG.replace(b'<path ', b'<path href="https://example.invalid/image" '),
            SVG.replace(b'fill="white"', b'fill="url(https://example.invalid/image)"'),
            SVG.replace(b'<path ', b'<path style="fill: red" '),
            b'<!DOCTYPE svg [<!ENTITY x "bad">]>' + SVG,
            b'<?xml-stylesheet href="https://example.invalid/style"?>' + SVG,
            SVG.replace(b'width="73"', b'width="100%"'),
            SVG.replace(b'viewBox="0 0 73 77"', b'viewBox="0 0 NaN 77"'),
            b"<html><body>oops</body></html>",
        ]
        for bad in attacks:
            with self.subTest(data=bad[:90]), self.assertRaises(ValueError):
                icons.validate_svg(bad)

    def test_relative_paths_cannot_escape_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for path in ("../outside.png", "/tmp/outside.png", ""):
                with self.assertRaises(ValueError):
                    icons.safe_path(root, path)
            self.assertEqual(icons.safe_path(root, "heroes/1.png"), root / "heroes/1.png")


class TestCatalog(unittest.TestCase):
    def test_real_composite_id_key_and_duplicate_names_preserve_all_ids(self):
        # This exact composite key occurs in dotaconstants commit 5c0242df... .
        mapping = {"3060,1617": "templar_assassin_hidden_gates", "4001": "same_name", "4002": "same_name"}
        self.assertEqual(icons.index_ids(mapping), {"templar_assassin_hidden_gates": [1617, 3060], "same_name": [4001, 4002]})
        data = catalog()
        data["ability_ids"].update(mapping)
        rows = {(r["category"], r["key"]): r for r in icons.build_entries(data)}
        row = rows["abilities", "templar_assassin_hidden_gates"]
        self.assertEqual(row["ids"], [1617, 3060])
        self.assertIsNone(row["id"])
        self.assertEqual(rows["abilities", "same_name"]["ids"], [4001, 4002])

    def test_stable_ids_real_associations_and_explicit_shared_assets(self):
        rows = {(r["category"], r["key"]): r for r in icons.build_entries(catalog())}
        hero = rows["heroes", "test"]
        self.assertEqual(hero["id"], 7)
        self.assertEqual(icons.target_path(hero), "heroes/7.png")
        self.assertEqual(rows["abilities", "test_spell"]["hero_ids"], [7])
        self.assertEqual(rows["abilities", "test_spell"]["id"], 101)
        self.assertEqual(rows["abilities", "id_only_spell"]["id"], 103)
        talent = rows["abilities", "special_bonus_test"]
        self.assertTrue(talent["shared"])
        self.assertEqual(talent["hero_ids"], [7])
        self.assertEqual(icons.target_path(talent), "ui/talents.svg")
        self.assertEqual(rows["abilities", "generic_hidden"]["status"], "skipped")
        self.assertEqual(rows["abilities", "AbilityCastPoint"]["status"], "skipped")
        self.assertEqual(rows["items", "recipe_a"]["source_url"], rows["items", "recipe_b"]["source_url"])
        self.assertEqual(icons.target_path(rows["items", "recipe_b"]), "items/recipe.png")
        self.assertEqual(rows["items", "recipe_b"]["id"], 11)
        self.assertTrue(rows["items", "recipe_b"]["shared"])
        self.assertEqual(rows["facets", "mana"]["hero_ids"], [7])
        self.assertIsNone(rows["facets", "mana"]["id"])

    def test_reject_unsafe_names_urls_and_id_disagreement(self):
        data = catalog()
        data["heroes"]["7"]["id"] = 8
        with self.assertRaisesRegex(ValueError, "ID mismatch"):
            icons.build_entries(data)
        data = catalog()
        data["items"]["blink"]["id"] = 99
        with self.assertRaisesRegex(ValueError, "ID mismatch"):
            icons.build_entries(data)
        data = catalog()
        data["abilities"]["../escape"] = {}
        with self.assertRaisesRegex(ValueError, "unsafe"):
            icons.build_entries(data)
        for url in ("https://evil.invalid/a.png", "//evil.invalid/a.png", "/apps/dota2/images/dota_react/../bad.png", "/other/tree/a.png"):
            with self.assertRaises(ValueError):
                icons.image_url(url)

    def test_summary_counts_physical_files_once(self):
        rows = [dict(category="items", key=str(i), status="downloaded", path="items/recipe.png", bytes=50) for i in range(2)]
        result = icons.summarize(rows)
        self.assertEqual(result["statuses"], {"downloaded": 2})
        self.assertEqual(result["unique_files"], 1)
        self.assertEqual(result["unique_bytes"], 50)


class TestTrustedRedirects(unittest.TestCase):
    def redirect(self, original, target):
        return icons.TrustedRedirects().redirect_request(
            urllib.request.Request(original), None, 302, "Found", {}, target)

    def test_same_asset_https_redirect_and_metadata_same_host(self):
        original = icons.IMAGE_BASE + "items/arcane_boots.png"
        self.assertEqual(self.redirect(original, original).full_url, original)
        metadata = "https://raw.githubusercontent.com/odota/dotaconstants/" + icons.REVISION + "/build/items.json"
        self.assertEqual(self.redirect(metadata, metadata).full_url, metadata)

    def test_metadata_does_not_inherit_image_cdn_allowlist(self):
        metadata = "https://raw.githubusercontent.com/odota/dotaconstants/" + icons.REVISION + "/build/items.json"
        for host in icons.IMAGE_CDN_HOSTS:
            with self.subTest(host=host), self.assertRaises(ValueError):
                self.redirect(metadata, "https://" + host + icons.DOTA_IMAGE_PATH + "items/arcane_boots.png")

    def test_verified_valve_cdns_can_redirect_only_the_same_asset(self):
        original = icons.IMAGE_BASE + "items/arcane_boots.png"
        for host in icons.IMAGE_CDN_HOSTS:
            for port in ("", ":443"):
                target = "https://" + host + port + icons.DOTA_IMAGE_PATH + "items/arcane_boots.png"
                with self.subTest(target=target):
                    self.assertEqual(self.redirect(original, target).full_url, target)
            wrong_path = "https://" + host + icons.DOTA_IMAGE_PATH + "items/different_item.png"
            with self.assertRaises(ValueError):
                self.redirect(original, wrong_path)

    def test_rejection_diagnostics_never_expose_credentials_or_query(self):
        original = icons.IMAGE_BASE + "items/arcane_boots.png?token=original_secret"
        targets = ["https://user:password_secret@evil.invalid/private?token=query_secret",
                   "https://user:password_secret@℀.invalid/private?token=query_secret"]
        for index, target in enumerate(targets):
            with self.subTest(target=target), self.assertRaises(ValueError) as raised:
                self.redirect(original, target)
            diagnostic = str(raised.exception)
            if index == 0:
                self.assertIn("https://cdn.cloudflare.steamstatic.com", diagnostic)
                self.assertIn("https://evil.invalid", diagnostic)
            for sensitive in ("original_secret", "password_secret", "query_secret", "token", "/private", "userinfo@"):
                self.assertNotIn(sensitive, diagnostic)

    def test_untrusted_image_redirect_destinations_are_rejected(self):
        original = icons.IMAGE_BASE + "items/arcane_boots.png"
        path = "/apps/dota2/images/dota_react/items/arcane_boots.png"
        targets = [
            "http://cdn.cloudflare.steamstatic.com" + path,
            "https://cdn.cloudflare.steamstatic.com.evil.invalid" + path,
            "https://arbitrary.steamstatic.com" + path,
            "https://user:password@cdn.cloudflare.steamstatic.com" + path,
            "https://cdn.cloudflare.steamstatic.com:8443" + path,
            "https://cdn.cloudflare.steamstatic.com:invalid" + path,
            icons.IMAGE_BASE + "items/different_item.png",
        ]
        for target in targets:
            with self.subTest(target=target), self.assertRaises(ValueError):
                self.redirect(original, target)


class TestTransport(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.responses = {}
        cls.requests = []
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass
            def do_GET(self):
                cls.requests.append(self.path)
                queue = cls.responses[self.path]
                code, headers, payload = queue.pop(0) if len(queue) > 1 else queue[0]
                self.send_response(code)
                for key, value in headers.items():
                    self.send_header(key, value)
                self.end_headers()
                self.wfile.write(payload)
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base = "http://127.0.0.1:" + str(cls.server.server_port)

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()

    def setUp(self):
        self.responses.clear()
        self.requests.clear()
        self.limiter = FakeLimiter()
        self.downloader = icons.Downloader(limiter=self.limiter)

    def row(self, key="test"):
        return icons.entry("abilities", key, 101, "Test", self.base + "/" + key + ".png")

    def test_429_retries_respect_retry_after(self):
        self.responses["/retry.png"] = [(429, {"Retry-After": "3"}, b"slow down"), (200, {}, png())]
        self.assertEqual(self.downloader.get(self.base + "/retry.png", 1000), png())
        self.assertEqual(self.limiter.delays, [3])
        self.assertEqual(len(self.requests), 2)

    def test_long_retry_after_stops_without_extra_request(self):
        self.responses["/wait.png"] = [(429, {"Retry-After": "120"}, b"slow down")]
        with self.assertRaisesRegex(RuntimeError, "above 60s"):
            self.downloader.get(self.base + "/wait.png", 1000)
        self.assertEqual(len(self.requests), 1)

    def test_404_cached_and_retry_missing_is_explicit(self):
        self.responses["/test.png"] = [(404, {}, b"not here")]
        row = self.row()
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            result, action = icons.fetch_asset(output, [row], {}, self.downloader)
            self.assertEqual(action, "not_found")
            old = {(row["category"], row["key"]): {**row, **result}}
            _, action = icons.fetch_asset(output, [row], old, self.downloader)
            self.assertEqual(action, "cached_missing")
            self.assertEqual(len(self.requests), 1)
            icons.fetch_asset(output, [row], old, self.downloader, retry_missing=True)
            self.assertEqual(len(self.requests), 2)
            self.assertFalse((output / "abilities/test.png").exists())

    def test_cache_is_hashed_and_corrupt_file_is_repaired_atomically(self):
        self.responses["/test.png"] = [(200, {"Content-Type": "image/png"}, png())]
        row = self.row()
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            result, _ = icons.fetch_asset(output, [row], {}, self.downloader)
            old = {(row["category"], row["key"]): {**row, **result}}
            _, action = icons.fetch_asset(output, [row], old, self.downloader)
            self.assertEqual(action, "cached")
            self.assertEqual(len(self.requests), 1)
            target = output / result["path"]
            target.write_bytes(b"corruption")
            _, action = icons.fetch_asset(output, [row], old, self.downloader)
            self.assertEqual(action, "downloaded")
            self.assertEqual(target.read_bytes(), png())
            self.assertEqual(len(self.requests), 2)
            self.assertEqual(list(output.rglob("*.part")), [])

    def test_200_html_is_failure_and_never_saved_as_png(self):
        self.responses["/test.png"] = [(200, {"Content-Type": "image/png"}, b"<html>CDN error</html>")]
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            result, action = icons.fetch_asset(output, [self.row()], {}, self.downloader)
            self.assertEqual(action, "failed")
            self.assertIsNone(result["path"])
            self.assertEqual(list(output.rglob("*.png")), [])

    def test_response_size_limit(self):
        self.responses["/big.png"] = [(200, {}, b"x" * 100)]
        with self.assertRaisesRegex(ValueError, "byte limit"):
            self.downloader.get(self.base + "/big.png", 10)

    def test_cancelled_downloader_never_starts_request(self):
        self.downloader.cancel()
        with self.assertRaises(icons.DownloadCancelled):
            self.downloader.get(self.base + "/not-requested.png", 1000)
        self.assertEqual(self.requests, [])

    def test_cancellation_wakes_retry_after_without_retrying(self):
        self.responses["/backoff.png"] = [(429, {"Retry-After": "30"}, b"slow down")]
        downloader = icons.Downloader(rate=8)
        errors = []
        def run():
            try:
                downloader.get(self.base + "/backoff.png", 1000)
            except Exception as error:
                errors.append(error)
        thread = threading.Thread(target=run)
        thread.start()
        deadline = time.monotonic() + 2
        while not self.requests and time.monotonic() < deadline:
            time.sleep(.005)
        downloader.cancel()
        thread.join(1)
        self.assertFalse(thread.is_alive(), "Retry-After wait did not wake on cancellation")
        self.assertEqual(len(self.requests), 1)
        self.assertEqual(len(errors), 1)
        self.assertIsInstance(errors[0], icons.DownloadCancelled)


class TestInterruptAndResume(unittest.TestCase):
    @unittest.skipUnless(os.name == "posix", "Subprocess SIGINT assertion uses POSIX signals")
    def test_sigint_cancels_queued_http_and_resume_reuses_completed_asset(self):
        # Synthetic catalog/images and a loopback server: no production assets or
        # public APIs are accessed by this real subprocess interruption test.
        requests, second_started, release_second = [], threading.Event(), threading.Event()
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass
            def do_GET(self):
                requests.append(self.path)
                if self.path == "/test2.png" and not release_second.is_set():
                    second_started.set()
                    release_second.wait(5)
                try:
                    self.send_response(200)
                    self.send_header("Content-Length", str(len(png())))
                    self.end_headers()
                    self.wfile.write(png())
                except (BrokenPipeError, ConnectionResetError):
                    pass  # The cancelled request is expected to time out.
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        child = None
        try:
            with tempfile.TemporaryDirectory() as temporary:
                output = Path(temporary)
                script = str(Path(icons.__file__).resolve())
                base = "http://127.0.0.1:" + str(server.server_port)
                code = f'''
import importlib.util
from pathlib import Path
spec = importlib.util.spec_from_file_location("icons", {script!r})
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
real_downloader = m.Downloader
m.Downloader = lambda rate: real_downloader(rate=rate, timeout=0.4)
def load_catalog(output, revision, downloader, previous):
    files = []
    for name in (*m.CATALOG_NAMES, "package"):
        relative = "catalog/" + name + ".json"
        m.atomic_write(output / relative, b"{{}}")
        files.append({{"path": relative, "sha256": m.sha256(b"{{}}"), "bytes": 2}})
    return {{}}, {{"name": "synthetic cancellation fixture", "commit": revision, "version": "test", "metadata_files": files}}
m.load_catalog = load_catalog
m.build_entries = lambda catalog: [m.entry("abilities", "test" + str(i), i, "Synthetic test", {base!r} + "/test" + str(i) + ".png") for i in range(1, 7)]
raise SystemExit(m.main(["--output", {str(output)!r}, "--workers", "1", "--rate", "8"]))
'''
                child = subprocess.Popen([sys.executable, "-c", code], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                self.assertTrue(second_started.wait(5), "second loopback request never started")
                start = time.monotonic()
                child.send_signal(signal.SIGINT)
                stdout, stderr = child.communicate(timeout=5)
                self.assertEqual(child.returncode, 130, stdout + stderr)
                self.assertLess(time.monotonic() - start, 5)
                self.assertEqual(requests, ["/test1.png", "/test2.png"], "queued tasks continued issuing requests after Ctrl-C")
                partial = json.loads((output / "manifest.json").read_text())
                self.assertFalse(partial["complete"])
                self.assertEqual(len(partial["entries"]), 6)
                first = partial["entries"][0]
                self.assertEqual(first["status"], "downloaded")
                digest_before = icons.sha256((output / first["path"]).read_bytes())
                self.assertEqual(digest_before, first["sha256"])
                release_second.set()
                resumed = subprocess.run([sys.executable, "-c", code], capture_output=True, text=True, timeout=5)
                self.assertEqual(resumed.returncode, 0, resumed.stdout + resumed.stderr)
                complete = json.loads((output / "manifest.json").read_text())
                self.assertTrue(complete["complete"])
                self.assertTrue(all(row["status"] == "downloaded" for row in complete["entries"]))
                self.assertEqual(requests.count("/test1.png"), 1, "resume downloaded an already completed file again")
                self.assertEqual(icons.sha256((output / first["path"]).read_bytes()), digest_before)
                self.assertEqual(list(output.rglob("*.part")), [])
                with contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(icons.verify(output), 0)
        finally:
            release_second.set()
            if child is not None and child.poll() is None:
                child.kill()
                child.communicate()
            server.shutdown()
            server.server_close()
            thread.join()


class TestOfflineVerification(unittest.TestCase):
    def make_manifest(self, output):
        row = icons.entry("heroes", "test", 7, "Test", icons.IMAGE_BASE + "heroes/test.png")
        row.update(path="heroes/7.png", status="downloaded", sha256=hashlib.sha256(png()).hexdigest(), bytes=len(png()), width=1, height=1)
        icons.atomic_write(output / row["path"], png())
        files = []
        for name in (*icons.CATALOG_NAMES, "package"):
            path = "catalog/" + name + ".json"
            icons.atomic_write(output / path, b"{}")
            files.append({"path": path, "sha256": icons.sha256(b"{}"), "bytes": 2})
        manifest = {"schema_version": 1, "complete": True, "source": {"metadata_files": files, "expected_entries": 1}, "entries": [row]}
        icons.write_json(output / "manifest.json", manifest)
        return manifest

    def test_verify_offline_detects_corruption_and_incomplete_manifest(self):
        with tempfile.TemporaryDirectory() as temporary, contextlib.redirect_stdout(io.StringIO()):
            output = Path(temporary)
            manifest = self.make_manifest(output)
            self.assertEqual(icons.verify(output), 0)
            (output / "heroes/7.png").write_bytes(b"broken")
            self.assertEqual(icons.verify(output), 1)
            icons.atomic_write(output / "heroes/7.png", png())
            manifest["complete"] = False
            icons.write_json(output / "manifest.json", manifest)
            self.assertEqual(icons.verify(output), 1)
            manifest["complete"] = True
            icons.write_json(output / "manifest.json", manifest)
            (output / "catalog/heroes.json").write_bytes(b'{"changed":true}')
            self.assertEqual(icons.verify(output), 1)

    def test_verify_missing_art_is_honest_but_not_network_failure(self):
        with tempfile.TemporaryDirectory() as temporary, contextlib.redirect_stdout(io.StringIO()):
            output = Path(temporary)
            manifest = self.make_manifest(output)
            manifest["entries"] += [dict(category="abilities", key="old", status="not_found", path=None), dict(category="abilities", key="empty", status="skipped", path=None)]
            manifest["source"]["expected_entries"] = 3
            icons.write_json(output / "manifest.json", manifest)
            self.assertEqual(icons.verify(output), 0)
            manifest["entries"][1]["status"] = "failed"
            icons.write_json(output / "manifest.json", manifest)
            self.assertEqual(icons.verify(output), 1)


class TestRateLimits(unittest.TestCase):
    def test_cancellation_wakes_shared_rate_wait(self):
        limiter = icons.RateLimiter(4)
        limiter.postpone(30)
        errors = []
        started = threading.Event()
        def run():
            started.set()
            try:
                limiter.wait()
            except Exception as error:
                errors.append(error)
        thread = threading.Thread(target=run)
        thread.start()
        self.assertTrue(started.wait(1))
        limiter.stop_event.set()
        thread.join(1)
        self.assertFalse(thread.is_alive(), "shared limiter did not wake on cancellation")
        self.assertEqual(len(errors), 1)
        self.assertIsInstance(errors[0], icons.DownloadCancelled)

    def test_shared_rate_spacing_and_postponement(self):
        now = [0.0]
        sleeps = []
        def sleep(delay):
            sleeps.append(delay)
            now[0] += delay
        limiter = icons.RateLimiter(4, clock=lambda: now[0], sleep=sleep)
        limiter.wait()
        limiter.wait()
        limiter.postpone(3)
        limiter.wait()
        self.assertEqual(sleeps, [0.25, 3])

    def test_retry_after_date_and_invalid_value(self):
        self.assertEqual(icons.retry_delay("3", 1), 3)
        self.assertEqual(icons.retry_delay("Thu, 01 Jan 1970 00:00:05 GMT", 1, now=2), 3)
        self.assertEqual(icons.retry_delay("not a date", 2), 2)
        self.assertEqual(icons.retry_delay("NaN", 2), 2)
        self.assertEqual(icons.retry_delay("Infinity", 2), 2)


if __name__ == "__main__":
    unittest.main()
