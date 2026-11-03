"""Synthetic SQLite fixtures only; never reads an account database or starts a UI."""

import contextlib
import hashlib
import importlib.util
import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("linux_debug_launcher", ROOT / "scripts/start-linux-debug.py")
launcher = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(launcher)


@unittest.skipUnless(sys.platform.startswith("linux"), "Linux launcher uses POSIX XDG permissions and flock")
class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="dpda-launcher-test-")
        self.root = Path(self.temp.name)
        self.source = self.root / "synthetic source.sqlite"
        with contextlib.closing(sqlite3.connect(self.source)) as db:
            db.executescript((ROOT / "crates/persistence/migrations/001_initial.sql").read_text())
            db.execute("PRAGMA user_version=1")
            db.execute("INSERT INTO profiles VALUES(?,?,?)", (42, str(launcher.STEAM64_BASE + 42), 1))
            db.execute("INSERT INTO matches(account_id,match_id,hero_id,start_time,source,fetched_at) "
                       "VALUES(42,'synthetic-test',1,1,'synthetic-test',1)")
            db.execute("INSERT INTO settings VALUES('pool:42',?,1)", (json.dumps({"synthetic": True}),))
            db.commit()

    def tearDown(self):
        self.temp.cleanup()

    def prepare(self):
        return launcher.prepare(self.source, 42, self.root / "sessions")

    def test_exact_steam_identifiers(self):
        self.assertEqual(launcher.account_id("42"), 42)
        self.assertEqual(launcher.account_id(str(launcher.STEAM64_BASE + 42)), 42)
        self.assertEqual(launcher.account_id(str(launcher.STEAM64_BASE + 4294967294)), 4294967294)
        for value in ["0", "4294967295", "-1", "1.0", "4e1", " 42", "四十二", "76561197960265728", "9" * 5000]:
            with self.subTest(value=value), self.assertRaises(launcher.LaunchError):
                launcher.account_id(value)

    def test_backup_keeps_source_and_all_records(self):
        before = hashlib.sha256(self.source.read_bytes()).digest()
        session = self.prepare()
        self.assertEqual(session.stat().st_mode & 0o777, 0o700)
        self.assertEqual(launcher.database_path(session).stat().st_mode & 0o777, 0o600)
        with contextlib.closing(sqlite3.connect(launcher.database_path(session))) as db:
            self.assertEqual(db.execute("SELECT COUNT(*) FROM matches").fetchone()[0], 1)
            active = json.loads(db.execute("SELECT value FROM settings WHERE key='active_profile'").fetchone()[0])
            self.assertEqual(active, {"account_id": 42, "steam64": str(launcher.STEAM64_BASE + 42)})
            self.assertEqual(db.execute("SELECT value FROM settings WHERE key='app_settings'").fetchone()[0], "{}")
            self.assertEqual(json.loads(db.execute("SELECT value FROM settings WHERE key='pool:42'").fetchone()[0]),
                             {"synthetic": True})
        self.assertEqual(hashlib.sha256(self.source.read_bytes()).digest(), before)

    def test_wal_backup_includes_committed_wal_rows(self):
        with contextlib.closing(sqlite3.connect(self.source)) as writer:
            writer.execute("PRAGMA journal_mode=WAL")
            writer.execute("INSERT INTO matches(account_id,match_id,hero_id,start_time,source,fetched_at) "
                           "VALUES(42,'synthetic-wal',2,2,'synthetic-test',2)")
            writer.commit()
            session = self.prepare()
            with contextlib.closing(sqlite3.connect(launcher.database_path(session))) as copied:
                self.assertEqual(copied.execute("SELECT COUNT(*) FROM matches").fetchone()[0], 2)

    def test_resume_preserves_modified_settings_and_works_without_source(self):
        session = self.prepare()
        with contextlib.closing(sqlite3.connect(launcher.database_path(session))) as db:
            db.execute("UPDATE settings SET value=? WHERE key='app_settings'", ('{"theme":"light"}',))
            db.commit()
        self.source.unlink()
        self.assertEqual(launcher.resume(session), session)
        with contextlib.closing(sqlite3.connect(launcher.database_path(session))) as db:
            self.assertEqual(json.loads(db.execute("SELECT value FROM settings WHERE key='app_settings'").fetchone()[0]),
                             {"theme": "light"})

    def test_new_preparation_never_reuses_or_overwrites_session(self):
        first = self.prepare()
        original = launcher.database_path(first).read_bytes()
        second = self.prepare()
        self.assertNotEqual(first, second)
        self.assertEqual(launcher.database_path(first).read_bytes(), original)

    def test_missing_account_is_rejected_before_copy(self):
        with self.assertRaisesRegex(launcher.LaunchError, "不存在"):
            launcher.prepare(self.source, 43, self.root / "sessions")
        self.assertEqual(list((self.root / "sessions").iterdir()), [])

    def test_future_schema_is_not_migrated_or_copied(self):
        with contextlib.closing(sqlite3.connect(self.source)) as db:
            db.execute("PRAGMA user_version=3")
        with self.assertRaisesRegex(launcher.LaunchError, "Schema"):
            self.prepare()
        self.assertEqual(list((self.root / "sessions").iterdir()), [])

    def test_schema_two_role_evidence_is_preserved(self):
        with contextlib.closing(sqlite3.connect(self.source)) as db:
            db.executescript((ROOT / "crates/persistence/migrations/002_role_evidence.sql").read_text())
            db.execute("PRAGMA user_version=2")
            db.execute("INSERT INTO role_evidence(account_id,match_id,evidence_json,fetched_at) "
                       "VALUES(42,'synthetic-test','{}',1)")
            db.commit()
        session = self.prepare()
        with contextlib.closing(sqlite3.connect(launcher.database_path(session))) as db:
            self.assertEqual(db.execute("PRAGMA user_version").fetchone()[0], 2)
            self.assertEqual(db.execute("SELECT COUNT(*) FROM role_evidence").fetchone()[0], 1)

    def test_invalid_resume_does_not_create_database(self):
        with self.assertRaises(launcher.LaunchError):
            launcher.resume(self.root)
        self.assertFalse((self.root / "data").exists())
        (self.root / "session.json").write_text("[]")
        with self.assertRaises(launcher.LaunchError):
            launcher.resume(self.root)

    def test_xdg_isolation_preserves_display_runtime_and_bus(self):
        session = self.root / "session"
        original = {"XDG_DATA_HOME": "/ordinary/data", "XDG_RUNTIME_DIR": "/run/user/123",
                    "WAYLAND_DISPLAY": "wayland-0", "DBUS_SESSION_BUS_ADDRESS": "unix:path=/bus"}
        env = launcher.environment(session, original)
        self.assertEqual(env["XDG_DATA_HOME"], str(session / "data"))
        self.assertEqual(env["XDG_CONFIG_HOME"], str(session / "config"))
        self.assertEqual(env["XDG_CACHE_HOME"], str(session / "cache"))
        self.assertEqual(env["XDG_STATE_HOME"], str(session / "state"))
        self.assertEqual(env["XDG_RUNTIME_DIR"], original["XDG_RUNTIME_DIR"])
        self.assertEqual(env["DBUS_SESSION_BUS_ADDRESS"], original["DBUS_SESSION_BUS_ADDRESS"])
        self.assertEqual(original["XDG_DATA_HOME"], "/ordinary/data")

    def test_headless_error_and_prepare_only(self):
        with self.assertRaisesRegex(launcher.LaunchError, "DISPLAY"):
            launcher.require_display({})
        with mock.patch.dict("os.environ", {}, clear=True), mock.patch("builtins.print") as printed:
            self.assertEqual(launcher.main(["--db", str(self.source), "--account", "42", "--prepare-only",
                                           "--session-root", str(self.root / "sessions")]), 0)
            result = json.loads(printed.call_args.args[0])
        self.assertEqual(result["status"], "prepared")
        self.assertIn("--resume", result["resume_command"])
        self.assertFalse(result["source_database_modified"])

    def test_missing_binary_and_shared_library_error(self):
        with self.assertRaisesRegex(launcher.LaunchError, "--build"):
            launcher.check_binary(self.root / "missing")
        fake = self.root / "not-a-real-application"
        fake.write_text("synthetic-test")
        fake.chmod(0o700)
        with mock.patch.object(launcher.shutil, "which", return_value="/usr/bin/ldd"), \
                mock.patch.object(launcher.subprocess, "run") as run:
            run.return_value.returncode = 0
            run.return_value.stdout = "libSynthetic.so => not found\n"
            with self.assertRaisesRegex(launcher.LaunchError, "libSynthetic.so"):
                launcher.check_binary(fake)


if __name__ == "__main__":
    unittest.main()
