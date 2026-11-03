"""License collector checks use clearly synthetic temporary package metadata."""
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "scripts" / "collect-licenses.py"
SPEC = importlib.util.spec_from_file_location("dpda_license_collector", SOURCE)
COLLECTOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COLLECTOR
SPEC.loader.exec_module(COLLECTOR)


class LicenseCollectionTests(unittest.TestCase):
    def package(self, root, name="synthetic-test", **kwargs):
        return COLLECTOR.Package(
            ecosystem="cargo", name=name, version="1.0.0",
            declared_license="MIT", repository="https://github.com/example/synthetic-test",
            source="registry+https://github.com/rust-lang/crates.io-index",
            root=root.resolve(), **kwargs,
        )

    def test_exact_dedup_keeps_package_provenance_and_archive_urls(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first, second = root / "one", root / "two"
            first.mkdir()
            second.mkdir()
            # Deliberately not a real license: test only byte preservation and provenance.
            # Write exact bytes so Windows newline translation cannot alter the fixture.
            text = "SYNTHETIC TEST TEXT — not a legal license\n"
            (first / "LICENSE").write_bytes(text.encode("utf-8"))
            (second / "copying.txt").write_bytes(text.encode("utf-8"))
            packages = [self.package(first, "one"), self.package(second, "two")]
            documents = COLLECTOR.collect_texts(packages)
            self.assertEqual(len(documents), 1)
            self.assertEqual(len(next(iter(documents.values())).uses), 2)
            rendered = COLLECTOR.render(packages, documents)
            self.assertIn("cargo:one@1.0.0", rendered)
            self.assertIn("cargo:two@1.0.0", rendered)
            self.assertEqual(rendered.count(text), 1)
            self.assertIn("https://crates.io/api/v1/crates/one/1.0.0/download", rendered)
            self.assertNotIn(str(root), rendered)

    def test_license_file_escape_is_not_read(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            root = parent / "package"
            root.mkdir()
            (parent / "not-a-license").write_text("PRIVATE SENTINEL", encoding="utf-8")
            package = self.package(root, license_file="../not-a-license")
            documents = COLLECTOR.collect_texts([package])
            self.assertFalse(documents)
            self.assertTrue(package.gaps)
            self.assertNotIn("PRIVATE SENTINEL", COLLECTOR.render([package], documents))

    def test_missing_text_stays_missing_instead_of_fabricating_mit(self):
        with tempfile.TemporaryDirectory() as directory:
            package = self.package(Path(directory))
            documents = COLLECTOR.collect_texts([package])
            self.assertFalse(documents)
            self.assertTrue(package.gaps)
            self.assertNotIn("Permission is hereby granted", COLLECTOR.render([package], documents))

    def test_supplement_requires_installed_commit_and_exact_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package_root = root / "package"
            package_root.mkdir()
            supplements = root / "supplements"
            supplements.mkdir()
            sha = "a" * 40
            (package_root / ".cargo_vcs_info.json").write_text(json.dumps({"git": {"sha1": sha}}))
            raw = b"SYNTHETIC NOTICE FOR COLLECTOR TEST\n"
            (supplements / "LICENSE").write_bytes(raw)
            entry = {
                "ecosystem": "cargo", "package": "synthetic-test", "version": "1.0.0",
                "kind": "exact-commit", "file": "LICENSE", "upstream_revision": sha,
                "source_url": f"https://raw.githubusercontent.com/example/synthetic-test/{sha}/LICENSE",
                "sha256": hashlib.sha256(raw).hexdigest(),
            }
            manifest = supplements / "supplements.json"
            manifest.write_text(json.dumps({"schema_version": 1, "licenses": [entry]}))
            valid = self.package(package_root)
            self.assertEqual(len(COLLECTOR.collect_texts([valid], supplements)), 1)
            self.assertFalse(valid.gaps)
            entry["upstream_revision"] = "b" * 40
            manifest.write_text(json.dumps({"schema_version": 1, "licenses": [entry]}))
            wrong_commit = self.package(package_root)
            self.assertFalse(COLLECTOR.collect_texts([wrong_commit], supplements))
            self.assertTrue(wrong_commit.gaps)
            entry["upstream_revision"] = sha
            entry["sha256"] = "0" * 64
            manifest.write_text(json.dumps({"schema_version": 1, "licenses": [entry]}))
            wrong_hash = self.package(package_root)
            self.assertFalse(COLLECTOR.collect_texts([wrong_hash], supplements))
            self.assertTrue(wrong_hash.gaps)

    def test_public_metadata_does_not_emit_local_paths_or_url_credentials(self):
        for value in ["/home/example/secret", "file:///private/license", "https://name:token@example.test/repo"]:
            result = COLLECTOR.public_url(value)
            self.assertNotIn("secret", result)
            self.assertNotIn("private", result)
            self.assertNotIn("token", result)
        self.assertEqual(COLLECTOR.public_url("https://github.com/example/repo?token=secret"), "https://github.com/example/repo")

    def test_dedicated_license_directory_and_atomic_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "LICENSES").mkdir()
            (root / "LICENSES" / "example.txt").write_bytes(b"SYNTHETIC\n")
            package = self.package(root)
            documents = COLLECTOR.collect_texts([package])
            output = root / "output" / "NOTICE.txt"
            COLLECTOR.write_atomic(output, COLLECTOR.render([package], documents))
            self.assertIn("LICENSES/example.txt", output.read_text(encoding="utf-8"))
            self.assertEqual(list(output.parent.iterdir()), [output])


if __name__ == "__main__":
    unittest.main()
