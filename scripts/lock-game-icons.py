#!/usr/bin/env python3
"""Explicitly record the verified local Valve icon snapshot for future builds."""

import argparse
import datetime
import hashlib
import json
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]


def create_lock(directory):
    directory = directory.resolve(strict=True)
    manifest = json.loads((directory / "manifest.json").read_text(encoding="utf-8"))
    entries = manifest["entries"]
    if manifest.get("schema_version") != 1 or not manifest.get("complete"):
        raise ValueError("Download is incomplete; run fetch-game-icons.py first")
    if len(entries) != manifest["source"]["expected_entries"]:
        raise ValueError("Manifest entry count differs from its source catalog")
    files = {}
    counts = {}
    for entry in entries:
        status = entry["status"]
        if status not in {"downloaded", "not_found", "skipped"}:
            raise ValueError(f"Unresolved download: {entry['category']}/{entry['key']}")
        counts[status] = counts.get(status, 0) + 1
        if status != "downloaded":
            continue
        path = entry["path"]
        relative = PurePosixPath(path)
        if relative.is_absolute() or ".." in relative.parts or "\\" in path:
            raise ValueError(f"Unsafe asset path: {path}")
        target = (directory / relative).resolve(strict=True)
        if not target.is_relative_to(directory):
            raise ValueError(f"Asset resolves outside the icon directory: {path}")
        content = target.read_bytes()
        digest = hashlib.sha256(content).hexdigest()
        if digest != entry["sha256"] or len(content) != entry["bytes"]:
            raise ValueError(f"Corrupt or changed image: {path}")
        record = {"path": path, "sha256": digest, "bytes": len(content),
                  "source_url": entry["source_url"]}
        if path in files and files[path] != record:
            raise ValueError(f"Conflicting shared image: {path}")
        files[path] = record
    if not files:
        raise ValueError("The manifest has no downloaded images")
    return {"schema_version": 1,
            "source_commit": manifest["source"]["commit"],
            "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "files": [files[key] for key in sorted(files)],
            "counts": {**counts, "physical_files": len(files),
                       "bytes": sum(item["bytes"] for item in files.values())}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=ROOT / "assets/game-icons")
    parser.add_argument("--output", type=Path, default=ROOT / "assets/game-icons.lock.json")
    args = parser.parse_args()
    result = create_lock(args.input)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    temporary = args.output.with_suffix(args.output.suffix + ".part")
    temporary.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.replace(args.output)
    print(json.dumps({"lock": str(args.output), **result["counts"]}))


if __name__ == "__main__":
    main()
