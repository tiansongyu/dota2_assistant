#!/usr/bin/env python3
"""Fetch official public portrait art into a user-local cache; never imply redistribution rights."""
import argparse
import concurrent.futures
import datetime
import hashlib
import json
from pathlib import Path
import re
import struct
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
BASE = "https://cdn.cloudflare.steamstatic.com/apps/dota2/images/dota_react/heroes/"
MAX_IMAGE_BYTES = 4 * 1024 * 1024


def validate_png(image):
    if len(image) > MAX_IMAGE_BYTES or len(image) < 24:
        raise ValueError("PNG image size outside the supported range")
    if image[:8] != b"\x89PNG\r\n\x1a\n" or image[12:16] != b"IHDR":
        raise ValueError("response is not a PNG image")
    width, height = struct.unpack(">II", image[16:24])
    if not (8 <= width <= 8192 and 8 <= height <= 8192):
        raise ValueError("PNG dimensions outside the supported range")


def download(url):
    for attempt in range(4):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "DPDA/0.1 personal-cache"})
            with urllib.request.urlopen(request, timeout=25) as response:
                image = response.read(MAX_IMAGE_BYTES + 1)
            validate_png(image)
            return image
        except urllib.error.HTTPError as exc:
            if exc.code not in (429, 500, 502, 503, 504) or attempt == 3:
                raise
            retry_after = exc.headers.get("Retry-After", "")
            if retry_after.isdigit() and int(retry_after) > 60:
                raise RuntimeError("CDN requests a long retry delay; rerun later") from exc
            time.sleep(int(retry_after) if retry_after.isdigit() else 2 ** attempt)
        except (urllib.error.URLError, TimeoutError):
            if attempt == 3:
                raise
            time.sleep(2 ** attempt)
    raise RuntimeError("download attempts exhausted")


def fetch(hero, output):
    name = hero["name"].removeprefix("npc_dota_hero_")
    if not re.fullmatch(r"[a-z0-9_]+", name) or not isinstance(hero["id"], int) or hero["id"] <= 0:
        raise ValueError("catalog contains an invalid hero ID or asset name")
    url = BASE + name + ".png"
    target = output / (str(hero["id"]) + ".png")
    if target.exists():
        image = target.read_bytes()
        validate_png(image)
    else:
        image = download(url)
        temporary = target.with_suffix(".part")
        temporary.write_bytes(image)
        temporary.replace(target)
    return {"hero_id": hero["id"], "hero_name": hero["name"], "localized_name": hero["localized_name"],
            "image_path": target.name, "template_version": "official-portrait-" + hashlib.sha256(image).hexdigest()[:16],
            "source_url": url, "sha256": hashlib.sha256(image).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path, help="User-owned local cache directory")
    parser.add_argument("--limit", type=int, help="Diagnostic request limit; omit for all catalogued heroes")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    heroes = json.loads((ROOT / "assets/heroes.json").read_text())
    if args.limit is not None:
        if args.limit <= 0:
            parser.error("--limit must be positive")
        heroes = heroes[:args.limit]
    manifest_path = args.output / "manifest.json"
    previous_templates = {}
    if manifest_path.exists():
        previous = json.loads(manifest_path.read_text(encoding="utf-8"))
        if previous.get("source") != BASE:
            parser.error("existing manifest uses a different source; choose a separate cache directory")
        previous_templates = {item["hero_id"]: item for item in previous.get("templates", [])}
    templates, failures = [], []
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
        jobs = {executor.submit(fetch, hero, args.output): hero for hero in heroes}
        for job in concurrent.futures.as_completed(jobs):
            try:
                templates.append(job.result())
            except Exception as exc:
                failures.append({"hero_id": jobs[job]["id"], "error": str(exc)})
    # A diagnostic --limit run must not discard other already cached templates.
    if args.limit is not None:
        for template in templates:
            previous_templates[template["hero_id"]] = template
        templates = list(previous_templates.values())
    manifest = {"schema_version": 1, "source": BASE,
                "retrieved_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                "license_notice": "Dota 2 artwork belongs to Valve. Download for local analysis; distribution permission is not established by the dotaconstants MIT license.",
                "validation": "Public hero portraits only. These are not labelled draft screenshots or proof of in-game recognition accuracy.",
                "templates": sorted(templates, key=lambda item: item["hero_id"]), "failures": failures}
    temporary_manifest = manifest_path.with_suffix(".json.part")
    temporary_manifest.write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding="utf-8")
    temporary_manifest.replace(manifest_path)
    print(json.dumps({"downloaded_or_cached": len(templates), "failed": len(failures), "manifest": str(args.output / "manifest.json")}))
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
