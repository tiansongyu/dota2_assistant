"""Synthetic offline tests for snapshot integrity and installation safety, not game accuracy."""

import contextlib
import http.client
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock
import urllib.request
import warnings
import zipfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("icon_snapshot", ROOT / "scripts/fetch-locked-game-icons.py")
snapshot = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(snapshot)
COMMIT = "1" * 40
URL = "https://github.com/example/dpda/releases/download/assets-v1/icons.zip"


def png():
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(b"\0\x20\x40\x60\xff")) + chunk(b"IEND", b""))


def encoded(value):
    return (json.dumps(value, sort_keys=True) + "\n").encode()


class SnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        (self.source / "heroes").mkdir(parents=True)
        (self.source / "catalog").mkdir()
        image, catalog = png(), b'{"synthetic":true}\n'
        self.image = {"path": "heroes/test.png", "sha256": snapshot.digest(image), "bytes": len(image),
                      "source_url": "https://cdn.steamstatic.com/apps/dota2/images/dota_react/heroes/test.png"}
        self.manifest = {
            "schema_version": 1, "complete": True,
            "source": {"commit": COMMIT, "expected_entries": 2, "metadata_files": [
                {"path": "catalog/heroes.json", "bytes": len(catalog), "sha256": snapshot.digest(catalog)}]},
            "entries": [{**self.image, "key": "test", "category": "heroes", "status": "downloaded", "width": 1, "height": 1},
                        {"key": "unavailable", "category": "heroes", "status": "not_found"}],
        }
        (self.source / "heroes/test.png").write_bytes(image)
        (self.source / "catalog/heroes.json").write_bytes(catalog)
        (self.source / "manifest.json").write_bytes(encoded(self.manifest))
        self.lock = {"schema_version": 1, "source_commit": COMMIT, "files": [self.image]}
        self.lock_path = self.root / "game-icons.lock.json"
        self.lock_path.write_bytes(encoded(self.lock))
        self.zip_path = self.root / "icons.zip"
        self.output = self.root / "game-icons"
        self.descriptor_path = self.root / "game-icons.snapshot.json"
        self.repack()

    def repack(self, extra=()):
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            with zipfile.ZipFile(self.zip_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
                for path in sorted(self.source.rglob("*")):
                    if path.is_file():
                        archive.write(path, path.relative_to(self.source).as_posix())
                for name, data in extra:
                    archive.writestr(name, data)
        data = self.zip_path.read_bytes()
        self.descriptor = {"schema_version": 1, "source_commit": COMMIT,
                           "lock_sha256": snapshot.digest(self.lock_path.read_bytes()),
                           "manifest_sha256": snapshot.digest((self.source / "manifest.json").read_bytes()),
                           "archive": {"url": URL, "sha256": snapshot.digest(data), "bytes": len(data)}}
        self.descriptor_path.write_bytes(encoded(self.descriptor))

    def restore(self):
        with contextlib.redirect_stderr(io.StringIO()):
            return snapshot.restore(self.descriptor_path, self.lock_path, self.output, self.zip_path)

    def test_local_archive_restores_exact_files_and_truthful_missing_count(self):
        with mock.patch.object(snapshot.urllib.request, "build_opener", side_effect=AssertionError("network")):
            report = self.restore()
        self.assertEqual(report["status"], "restored")
        self.assertTrue(report["offline"])
        self.assertEqual(report["counts"], {"downloaded": 1, "not_found": 1})
        self.assertEqual((self.output / "heroes/test.png").read_bytes(), png())
        self.assertEqual(sorted(p.relative_to(self.output).as_posix() for p in self.output.rglob("*") if p.is_file()),
                         ["catalog/heroes.json", "heroes/test.png", "manifest.json"])
        self.assertFalse(any(p.name.startswith(".game-icons") for p in self.root.iterdir()))

    def test_valid_cache_checks_every_hash_and_skips_archive(self):
        self.restore()
        with mock.patch.object(snapshot, "acquire_archive", side_effect=AssertionError("download")):
            self.assertEqual(self.restore()["status"], "cached")
        (self.output / "heroes/test.png").write_bytes(b"changed")
        self.assertEqual(self.restore()["status"], "restored")
        self.assertEqual((self.output / "heroes/test.png").read_bytes(), png())

    def test_modified_manifest_and_catalog_cannot_bypass_cached_snapshot(self):
        self.restore()
        data = b'{"altered":true}\n'
        changed = json.loads((self.output / "manifest.json").read_text())
        changed["source"]["metadata_files"][0].update({"sha256": snapshot.digest(data), "bytes": len(data)})
        (self.output / "catalog/heroes.json").write_bytes(data)
        (self.output / "manifest.json").write_bytes(encoded(changed))
        self.assertEqual(self.restore()["status"], "restored")
        self.assertEqual((self.output / "manifest.json").read_bytes(), (self.source / "manifest.json").read_bytes())

    def test_descriptor_must_match_current_lock_even_with_valid_cache(self):
        self.restore()
        self.lock_path.write_bytes(encoded({**self.lock, "extra": "changed"}))
        with self.assertRaisesRegex(ValueError, "current.*lock"):
            self.restore()
        self.assertTrue((self.output / "manifest.json").is_file())

    def test_archive_hash_or_size_failure_keeps_old_directory(self):
        self.output.mkdir()
        sentinel = self.output / "old.txt"
        sentinel.write_text("keep")
        original = self.zip_path.read_bytes()
        for content in [original[:-1], original + b"x", b"X" + original[1:]]:
            with self.subTest(size=len(content)):
                self.zip_path.write_bytes(content)
                with self.assertRaises(ValueError):
                    self.restore()
                self.assertEqual(sentinel.read_text(), "keep")

    def test_zip_rejects_traversal_absolute_windows_names_and_encoded_paths(self):
        for name in ["../escape", "/absolute", "C:/drive", "a\\b", "a/../b", "a//b", "./x", "a/%2e%2e/x",
                     "CON.txt", "a/aux.png", "a/trailing.", "a/trailing ", "a/stream:ads"]:
            with self.subTest(name=name):
                self.repack([(name, b"bad")])
                with self.assertRaises(ValueError):
                    self.restore()
                self.assertFalse(self.output.exists())
        self.assertFalse((self.root / "escape").exists())

    def test_zip_rejects_duplicate_case_conflicting_and_file_directory_paths(self):
        for extra in [[("heroes/test.png", b"duplicate")], [("HEROES/test.png", b"case")],
                      [("Heroes/another.png", b"parent case")], [("heroes", b"file")],
                      [("one", b"file"), ("one/two", b"child")]]:
            with self.subTest(extra=extra):
                self.repack(extra)
                with self.assertRaises(ValueError):
                    self.restore()
                self.assertFalse(self.output.exists())

    def test_zip_rejects_links_and_special_files(self):
        for kind in [stat.S_IFLNK, stat.S_IFIFO, stat.S_IFCHR, stat.S_IFSOCK]:
            info = zipfile.ZipInfo("bad-link")
            info.create_system = 3
            info.external_attr = (kind | 0o777) << 16
            with self.subTest(kind=kind):
                self.repack([(info, b"../target")])
                with self.assertRaisesRegex(ValueError, "links or special"):
                    self.restore()
        info = zipfile.ZipInfo("windows-reparse")
        info.external_attr = 0x400
        self.repack([(info, b"target")])
        with self.assertRaisesRegex(ValueError, "links or special"):
            self.restore()

    def test_bounded_entry_size_total_and_entry_count(self):
        for name, value in [("MAX_ENTRIES", 2), ("MAX_FILE_BYTES", 5), ("MAX_EXPANDED_BYTES", 5)]:
            with self.subTest(limit=name), mock.patch.object(snapshot, name, value):
                with self.assertRaises(ValueError):
                    snapshot.extract_archive(self.zip_path, self.root / "extracted")
                self.assertFalse((self.root / "extracted").exists())
        self.descriptor["archive"]["bytes"] = snapshot.MAX_ARCHIVE_BYTES + 1
        self.descriptor_path.write_bytes(encoded(self.descriptor))
        with self.assertRaisesRegex(ValueError, "byte limit"):
            self.restore()

    def test_manifest_lock_and_image_validation_precede_replacement(self):
        self.output.mkdir()
        (self.output / "old.txt").write_text("old")
        for mutate in [lambda: self.manifest.update(complete=False),
                       lambda: self.manifest["entries"][0].update(status="failed"),
                       lambda: self.manifest["source"].update(commit="2" * 40),
                       lambda: self.manifest["entries"][0].update(width=2),
                       lambda: self.manifest["entries"].append(self.manifest["entries"][0])]:
            old = json.loads(json.dumps(self.manifest))
            mutate()
            (self.source / "manifest.json").write_bytes(encoded(self.manifest))
            self.repack()
            with self.assertRaises(ValueError):
                self.restore()
            self.assertEqual((self.output / "old.txt").read_text(), "old")
            self.manifest = old

    def test_new_physical_files_require_explicit_lock_update(self):
        changed = {**self.manifest["entries"][0], "path": "heroes/new.png", "key": "new"}
        self.manifest["entries"].append(changed)
        self.manifest["source"]["expected_entries"] += 1
        (self.source / "heroes/new.png").write_bytes(png())
        (self.source / "manifest.json").write_bytes(encoded(self.manifest))
        self.repack()
        with self.assertRaisesRegex(ValueError, "set differs from lock"):
            self.restore()

    def test_unlisted_files_rejected_even_if_archive_hash_matches(self):
        self.repack([("private.sqlite", b"not allowed")])
        with self.assertRaisesRegex(ValueError, "unlisted files"):
            self.restore()

    def test_corrupt_png_with_updated_archive_manifest_and_lock_is_rejected(self):
        image = b"this is not a PNG"
        (self.source / "heroes/test.png").write_bytes(image)
        self.image.update(sha256=snapshot.digest(image), bytes=len(image))
        self.manifest["entries"][0].update(self.image)
        (self.source / "manifest.json").write_bytes(encoded(self.manifest))
        self.lock_path.write_bytes(encoded(self.lock))
        self.repack()
        with self.assertRaises(ValueError):
            self.restore()

    @unittest.skipIf(sys.platform == "win32", "Creating symlinks can require Windows administrator privileges")
    def test_cache_symlinks_are_not_followed(self):
        self.restore()
        (self.output / "heroes/test.png").unlink()
        (self.output / "heroes/test.png").symlink_to(self.source / "heroes/test.png")
        with self.assertRaisesRegex(ValueError, "Symlink"):
            snapshot.verify_directory(self.output, self.lock, self.descriptor["manifest_sha256"])
        self.assertEqual(self.restore()["status"], "restored")
        self.assertFalse((self.output / "heroes/test.png").is_symlink())

    def test_windows_reparse_points_rejected_without_requiring_symlink_privileges(self):
        # Python on Windows can report a junction as a directory, not a symlink.
        metadata = types.SimpleNamespace(st_mode=stat.S_IFDIR | 0o755, st_file_attributes=0x400)
        with mock.patch.object(Path, "lstat", return_value=metadata):
            with self.assertRaisesRegex(ValueError, "reparse point"):
                snapshot.safe_stat(Path("synthetic-junction"))

    def test_failed_final_rename_restores_previous_directory(self):
        self.output.mkdir()
        (self.output / "old.txt").write_text("old")
        original = Path.rename

        def fail_staging(path, target):
            if path.name == "verified":
                raise OSError("synthetic install failure")
            return original(path, target)

        with mock.patch.object(Path, "rename", fail_staging), self.assertRaisesRegex(OSError, "synthetic"):
            self.restore()
        self.assertEqual((self.output / "old.txt").read_text(), "old")
        self.assertFalse((self.root / ".game-icons.snapshot-backup").exists())

    def test_interrupted_rename_backup_recovers_and_exclusive_install_rejects_overlap(self):
        backup = self.root / ".game-icons.snapshot-backup"
        shutil.copytree(self.source, backup)
        self.assertEqual(self.restore()["status"], "cached")
        self.assertFalse(backup.exists())
        marker = self.root / ".game-icons.snapshot-lock"
        marker.mkdir()
        with self.assertRaisesRegex(RuntimeError, "may be active"):
            self.restore()
        self.assertTrue(marker.exists())

    def test_leftover_backup_removed_only_after_valid_output_is_verified(self):
        self.restore()
        backup = self.root / ".game-icons.snapshot-backup"
        backup.mkdir()
        (backup / "old.txt").write_text("old")
        (self.output / "manifest.json").write_text("broken")
        with self.assertRaises(ValueError):
            self.restore()
        self.assertEqual((backup / "old.txt").read_text(), "old")
        (self.output / "manifest.json").write_bytes((self.source / "manifest.json").read_bytes())
        self.assertEqual(self.restore()["status"], "cached")
        self.assertFalse(backup.exists())

    def test_descriptor_requires_exact_release_url_and_pinned_manifest(self):
        for url in ["http://github.com/a/b/releases/download/v/a.zip", "https://github.com.evil.test/a.zip",
                    "https://github.com@evil.test/a.zip", "https://user@github.com/a/b/releases/download/v/a.zip",
                    "https://github.com:444/a/b/releases/download/v/a.zip", "https://github.com/a/b/archive/main.zip",
                    URL + "?token=secret", URL + "#fragment", "https://release-assets.githubusercontent.com/file.zip"]:
            with self.subTest(url=url):
                self.descriptor["archive"]["url"] = url
                self.descriptor_path.write_bytes(encoded(self.descriptor))
                with self.assertRaises(ValueError):
                    self.restore()
        self.descriptor["archive"]["url"] = URL
        self.descriptor.pop("manifest_sha256")
        self.descriptor_path.write_bytes(encoded(self.descriptor))
        with self.assertRaisesRegex(ValueError, "manifest_sha256"):
            self.restore()

    def test_redirects_only_allow_exact_https_github_asset_hosts(self):
        handler = snapshot.SnapshotRedirects()
        request = urllib.request.Request(URL)
        allowed = "https://release-assets.githubusercontent.com/github-production-release-asset/1/a?sig=opaque"
        self.assertEqual(handler.redirect_request(request, None, 302, "Found", {}, allowed).full_url, allowed)
        for target in ["http://release-assets.githubusercontent.com/a", "https://release-assets.githubusercontent.com.evil.test/a",
                       "https://evil.test/a", "https://user@release-assets.githubusercontent.com/a",
                       "https://release-assets.githubusercontent.com:444/a", "https://github.com/a#fragment"]:
            with self.subTest(target=target), self.assertRaises(ValueError):
                handler.redirect_request(request, None, 302, "Found", {}, target)

    def test_streamed_download_checks_headers_bytes_and_hash(self):
        content = self.zip_path.read_bytes()

        class Response(io.BytesIO):
            status = 200
            headers = {"Content-Length": str(len(content))}

            def geturl(self):
                return "https://release-assets.githubusercontent.com/asset?sig=opaque"

        destination = self.root / "download.zip"
        opener = mock.Mock()
        opener.open.return_value = Response(content)
        with mock.patch.object(snapshot.urllib.request, "build_opener", return_value=opener):
            snapshot.acquire_archive(self.descriptor["archive"], destination)
        self.assertEqual(destination.read_bytes(), content)
        opener.open.return_value = Response(content)
        opener.open.return_value.headers = {"Content-Length": "999999"}
        with mock.patch.object(snapshot.urllib.request, "build_opener", return_value=opener):
            with self.assertRaisesRegex(ValueError, "Content-Length"):
                snapshot.acquire_archive(self.descriptor["archive"], destination)

    def test_interrupted_stream_retries_from_zero_with_bounded_attempts(self):
        content = self.zip_path.read_bytes()

        class Response(io.BytesIO):
            status = 200
            headers = {}

            def geturl(self):
                return "https://release-assets.githubusercontent.com/asset?sig=opaque"

        class Broken(Response):
            def read(self, size=-1):
                if self.tell():
                    raise failure
                return super().read(min(size, 17))

        for failure in [http.client.IncompleteRead(b"partial", 100), ConnectionResetError("synthetic reset")]:
            with self.subTest(failure=type(failure).__name__):
                destination = self.root / "retry.zip"
                opener = mock.Mock()
                opener.open.side_effect = [Broken(content), Response(content)]
                with mock.patch.object(snapshot.urllib.request, "build_opener", return_value=opener), \
                        mock.patch.object(snapshot.time, "sleep") as sleep:
                    snapshot.acquire_archive(self.descriptor["archive"], destination)
                self.assertEqual(opener.open.call_count, 2)
                sleep.assert_called_once_with(1)
                self.assertEqual(destination.read_bytes(), content)

    def test_exhausted_stream_retries_report_a_sanitized_failure(self):
        opener = mock.Mock()
        opener.open.side_effect = [http.client.IncompleteRead(b"secret", 10) for _ in range(3)]
        with mock.patch.object(snapshot.urllib.request, "build_opener", return_value=opener), \
                mock.patch.object(snapshot.time, "sleep") as sleep:
            with self.assertRaisesRegex(RuntimeError, "Snapshot network request failed: IncompleteRead") as raised:
                snapshot.acquire_archive(self.descriptor["archive"], self.root / "failed.zip")
        self.assertEqual(opener.open.call_count, 3)
        self.assertEqual(sleep.call_args_list, [mock.call(1), mock.call(2)])
        self.assertNotIn("secret", str(raised.exception))

    def test_cli_offline_success_and_failure_exit_codes(self):
        command = [sys.executable, str(ROOT / "scripts/fetch-locked-game-icons.py"), "--snapshot", str(self.descriptor_path),
                   "--lock", str(self.lock_path), "--archive", str(self.zip_path), "--output", str(self.output)]
        result = subprocess.run(command, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["status"], "restored")
        self.descriptor_path.write_text("{bad", encoding="utf-8")
        result = subprocess.run(command, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 1)
        self.assertIn("Locked icon snapshot failed", result.stderr)
        self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
