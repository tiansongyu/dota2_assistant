#!/usr/bin/env python3
"""Package verified game icons into a reproducible ZIP and pinned release descriptor."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import stat
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "artifacts/game-icons-snapshot")
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,79}", args.release_tag):
        parser.error("release tag must contain only ASCII letters, digits, dots, underscores or hyphens")
    directory = ROOT / "assets/game-icons"
    output = args.output.resolve()
    if output.is_relative_to((ROOT / "assets").resolve()):
        parser.error("archive output must be outside public assets")
    subprocess.run(["node", str(ROOT / "scripts/verify-game-icons.mjs"), "--require-lock"], check=True)
    lock_bytes = (ROOT / "assets/game-icons.lock.json").read_bytes()
    lock_hash = hashlib.sha256(lock_bytes).hexdigest()
    lock = json.loads(lock_bytes)
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"game-icons-{lock_hash[:12]}.zip"
    temporary = archive.with_suffix(".zip.part")
    try:
        with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as zipped:
            for file in sorted(directory.rglob("*")):
                if not file.is_file():
                    continue
                info = zipfile.ZipInfo(file.relative_to(directory).as_posix(), date_time=(1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.external_attr = (stat.S_IFREG | 0o644) << 16
                zipped.writestr(info, file.read_bytes(), compress_type=zipfile.ZIP_DEFLATED, compresslevel=6)
        temporary.replace(archive)
    finally:
        temporary.unlink(missing_ok=True)
    descriptor = {
        "schema_version": 1,
        "source_commit": lock["source_commit"],
        "lock_sha256": lock_hash,
        "manifest_sha256": hashlib.sha256((directory / "manifest.json").read_bytes()).hexdigest(),
        "archive": {
            "url": f"https://github.com/tiansongyu/dota2_assistant/releases/download/{args.release_tag}/{archive.name}",
            "sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
            "bytes": archive.stat().st_size,
        },
    }
    descriptor_path = ROOT / "assets/game-icons.snapshot.json"
    descriptor_path.write_text(json.dumps(descriptor, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"archive": str(archive), "snapshot": str(descriptor_path), **descriptor["archive"]}))


if __name__ == "__main__":
    main()
