#!/usr/bin/env python3
"""Fetch a pinned Dota constants catalog and public Valve artwork, with honest coverage."""
from __future__ import annotations

import argparse
import collections
import concurrent.futures
import datetime
import email.utils
import hashlib
import json
import math
import os
from pathlib import Path
import re
import struct
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
import zlib

ROOT = Path(__file__).resolve().parents[1]
REVISION = "5c0242df9057848fa2036ca3fafe9f2bd4508163"
CDN = "https://cdn.cloudflare.steamstatic.com"
IMAGE_BASE = CDN + "/apps/dota2/images/dota_react/"
CATALOG_NAMES = ("heroes", "abilities", "ability_ids", "items", "item_ids", "hero_abilities")
MAX_IMAGE_BYTES = 4 * 1024 * 1024
MAX_JSON_BYTES = 16 * 1024 * 1024
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
SAFE_KEY = re.compile(r"[A-Za-z0-9_]+\Z")


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def atomic_write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=path.name + ".", suffix=".part", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def write_json(path, value):
    atomic_write(path, (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode())


def safe_path(root, relative):
    if not isinstance(relative, str) or not relative or Path(relative).is_absolute():
        raise ValueError("invalid relative asset path")
    target = root / relative
    if not target.resolve().is_relative_to(root.resolve()):
        raise ValueError("asset path escapes output directory")
    return target


def validate_png(data):
    """Check complete PNG chunks, CRC, bounded deflate stream, and scanline structure."""
    if not 45 <= len(data) <= MAX_IMAGE_BYTES or data[:8] != PNG_SIGNATURE:
        raise ValueError("response is not a bounded PNG image")
    offset, dimensions, compressed, palette, ended = 8, None, [], False, False
    seen_idat, closed_idat = False, False
    while offset < len(data):
        if offset + 12 > len(data):
            raise ValueError("truncated PNG chunk")
        length = struct.unpack_from(">I", data, offset)[0]
        kind = data[offset + 4:offset + 8]
        end = offset + length + 12
        if end > len(data):
            raise ValueError("truncated PNG payload")
        payload = data[offset + 8:offset + 8 + length]
        crc = struct.unpack_from(">I", data, offset + length + 8)[0]
        if zlib.crc32(kind + payload) & 0xffffffff != crc:
            raise ValueError("PNG CRC mismatch")
        if dimensions is None and kind != b"IHDR":
            raise ValueError("PNG must begin with IHDR")
        if kind == b"IHDR":
            if dimensions is not None or length != 13:
                raise ValueError("invalid PNG IHDR")
            width, height, depth, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", payload)
            allowed_depths = {0: (1, 2, 4, 8, 16), 2: (8, 16), 3: (1, 2, 4, 8), 4: (8, 16), 6: (8, 16)}
            if not (1 <= width <= 8192 and 1 <= height <= 8192 and width * height <= 16_777_216):
                raise ValueError("PNG dimensions outside supported bounds")
            if depth not in allowed_depths.get(color, ()) or compression or filtering or interlace not in (0, 1):
                raise ValueError("unsupported PNG header")
            dimensions = width, height, depth, color, interlace
        elif kind == b"PLTE":
            if seen_idat or length == 0 or length % 3 or length > 768:
                raise ValueError("invalid PNG palette")
            palette = True
        elif kind == b"IDAT":
            if closed_idat:
                raise ValueError("noncontiguous PNG IDAT")
            seen_idat = True
            compressed.append(payload)
        elif kind == b"IEND":
            if length or not seen_idat or end != len(data):
                raise ValueError("invalid PNG end")
            ended = True
        elif kind[0] & 32 == 0:
            raise ValueError("unknown critical PNG chunk")
        if seen_idat and kind != b"IDAT":
            closed_idat = True
        offset = end
    if not ended or dimensions is None:
        raise ValueError("incomplete PNG")
    width, height, depth, color, interlace = dimensions
    if color == 3 and not palette:
        raise ValueError("indexed PNG is missing palette")
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color]
    passes = [(0, 0, 1, 1)] if interlace == 0 else [
        (0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4),
        (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
    rows = []
    for x, y, dx, dy in passes:
        columns = max(0, (width - x + dx - 1) // dx)
        count = max(0, (height - y + dy - 1) // dy)
        if columns and count:
            rows.extend([1 + (columns * channels * depth + 7) // 8] * count)
    expected = sum(rows)
    if expected > 128 * 1024 * 1024:
        raise ValueError("decompressed PNG exceeds supported bounds")
    inflater = zlib.decompressobj()
    try:
        pixels = inflater.decompress(b"".join(compressed), expected + 1)
    except zlib.error as error:
        raise ValueError("invalid PNG deflate stream") from error
    if len(pixels) != expected or not inflater.eof or inflater.unused_data or inflater.unconsumed_tail:
        raise ValueError("PNG scanline size or deflate termination mismatch")
    position = 0
    for size in rows:
        if pixels[position] > 4:
            raise ValueError("invalid PNG scanline filter")
        position += size
    return width, height


def validate_svg(data):
    """Accept inert, self-contained SVG geometry; never execute downloaded XML."""
    if not data or len(data) > MAX_IMAGE_BYTES:
        raise ValueError("SVG byte size outside bounds")
    text = data.decode("utf-8")
    if re.search(r"<!DOCTYPE|<!ENTITY|<\?", text, re.IGNORECASE):
        raise ValueError("SVG declarations/entities/processing instructions rejected")
    try:
        root = ET.fromstring(text)
    except ET.ParseError as error:
        raise ValueError("invalid SVG XML") from error
    namespace = "{http://www.w3.org/2000/svg}"
    allowed = {"svg", "g", "path", "circle", "ellipse", "rect", "line", "polyline", "polygon", "title", "desc", "defs", "linearGradient", "radialGradient", "stop", "clipPath", "mask"}
    if root.tag != namespace + "svg":
        raise ValueError("response is not an SVG image")
    for node in root.iter():
        if not node.tag.startswith(namespace) or node.tag[len(namespace):] not in allowed:
            raise ValueError("active or unsupported SVG element rejected")
        for attribute, value in node.attrib.items():
            local = attribute.rsplit("}", 1)[-1].lower()
            if local.startswith("on") or local in ("href", "style", "src", "base"):
                raise ValueError("active SVG attribute rejected")
            if re.search(r"url\s*\(|https?:|javascript:|data:", value, re.IGNORECASE):
                raise ValueError("SVG external/resource reference rejected")
    def dimension(value):
        if value is None or not re.fullmatch(r"(?:\d+(?:\.\d*)?|\.\d+)(?:px)?", value):
            raise ValueError("SVG requires explicit numeric dimensions")
        size = float(value.removesuffix("px"))
        if not math.isfinite(size) or not 0 < size <= 8192:
            raise ValueError("SVG dimensions outside bounds")
        return int(size) if size.is_integer() else size
    width, height = dimension(root.get("width")), dimension(root.get("height"))
    viewbox = root.get("viewBox")
    if viewbox:
        numbers = [float(v) for v in re.split(r"[\s,]+", viewbox.strip())]
        if len(numbers) != 4 or not all(math.isfinite(v) for v in numbers) or min(numbers[2:]) <= 0:
            raise ValueError("invalid SVG viewBox")
    return width, height


def validate_image(data, relative):
    if relative.endswith(".png"):
        return validate_png(data)
    if relative.endswith(".svg"):
        return validate_svg(data)
    raise ValueError("unsupported image extension")


class DownloadCancelled(Exception):
    """A cooperative stop; it must never trigger a retry or a new HTTP request."""


class RateLimiter:
    def __init__(self, rate, clock=time.monotonic, sleep=None, stop_event=None):
        self.interval = 1 / rate
        self.clock, self.sleep = clock, sleep
        self.stop_event = stop_event or threading.Event()
        self.next_request = 0.0
        self.lock = threading.Lock()

    def wait(self):
        # Recheck after sleeping: a concurrent 429 may have postponed all workers.
        while True:
            if self.stop_event.is_set():
                raise DownloadCancelled("download cancelled")
            with self.lock:
                now = self.clock()
                delay = self.next_request - now
                if delay <= 0:
                    self.next_request = now + self.interval
                    return
            if self.sleep is not None:
                self.sleep(delay)  # Deterministic clocks used by offline unit tests.
            elif self.stop_event.wait(delay):
                raise DownloadCancelled("download cancelled")

    def postpone(self, delay):
        with self.lock:
            self.next_request = max(self.next_request, self.clock() + delay)


def retry_delay(value, fallback, now=None):
    if value:
        try:
            numeric = float(value)
            return max(0.0, numeric) if math.isfinite(numeric) else fallback
        except ValueError:
            try:
                parsed = email.utils.parsedate_to_datetime(value)
                return max(0.0, parsed.timestamp() - (time.time() if now is None else now))
            except (ValueError, TypeError, OverflowError):
                pass
    return fallback


class TrustedRedirects(urllib.request.HTTPRedirectHandler):
    def __init__(self, stop_event=None):
        super().__init__()
        self.stop_event = stop_event

    def redirect_request(self, request, fp, code, message, headers, newurl):
        if self.stop_event is not None and self.stop_event.is_set():
            raise DownloadCancelled("download cancelled before redirect")
        original = urllib.parse.urlsplit(request.full_url)
        target = urllib.parse.urlsplit(newurl)
        if target.scheme != "https" or target.hostname != original.hostname:
            raise ValueError("cross-host or insecure redirect rejected")
        return super().redirect_request(request, fp, code, message, headers, newurl)


class Downloader:
    def __init__(self, rate=4, attempts=4, timeout=25, opener=None, limiter=None, stop_event=None):
        if not 0 < timeout <= 25:
            raise ValueError("HTTP timeout must be >0 and <=25 seconds")
        self.stop_event = stop_event or threading.Event()
        self.limiter = limiter or RateLimiter(rate, stop_event=self.stop_event)
        self.attempts, self.timeout = attempts, timeout
        self.opener = opener or urllib.request.build_opener(TrustedRedirects(self.stop_event))

    def cancel(self):
        self.stop_event.set()
        if hasattr(self.limiter, "stop_event"):
            self.limiter.stop_event.set()

    def check_cancelled(self):
        if self.stop_event.is_set():
            raise DownloadCancelled("download cancelled")

    def get(self, url, limit):
        for attempt in range(self.attempts):
            self.check_cancelled()
            self.limiter.wait()
            self.check_cancelled()
            request = urllib.request.Request(url, headers={"User-Agent": "DPDA/0.1 asset-cache", "Accept": "image/png,application/json;q=0.9"})
            try:
                with self.opener.open(request, timeout=self.timeout) as response:
                    chunks, size = [], 0
                    read_chunk = getattr(response, "read1", response.read)
                    # read1 performs at most one underlying read, so a slow stream
                    # cannot hide cancellation inside one large buffered read().
                    while True:
                        self.check_cancelled()
                        block = read_chunk(min(64 * 1024, limit + 1 - size))
                        if not block:
                            break
                        chunks.append(block)
                        size += len(block)
                        if size > limit:
                            raise ValueError("response exceeds byte limit")
                    data = b"".join(chunks)
                    if len(data) > limit:
                        raise ValueError("response exceeds byte limit")
                    return data
            except urllib.error.HTTPError as error:
                self.check_cancelled()
                if error.code not in (429, 500, 502, 503, 504) or attempt + 1 == self.attempts:
                    raise
                delay = retry_delay(error.headers.get("Retry-After"), 2 ** attempt)
                if delay > 60:
                    raise RuntimeError("server requested a retry delay above 60s; rerun later") from error
                self.limiter.postpone(delay)
            except (urllib.error.URLError, TimeoutError, ConnectionError) as error:
                self.check_cancelled()
                if attempt + 1 == self.attempts:
                    raise
                self.limiter.postpone(2 ** attempt)
        raise RuntimeError("download attempts exhausted")


def image_url(path):
    parsed = urllib.parse.urlsplit(path)
    expected = "/apps/dota2/images/dota_react/"
    if parsed.scheme or parsed.netloc or not parsed.path.startswith(expected):
        raise ValueError("catalog image URL is outside the trusted Valve image tree")
    if ".." in parsed.path.split("/") or not parsed.path.endswith(".png"):
        raise ValueError("invalid catalog image path")
    return CDN + parsed.path


def entry(category, key, identifier, name, url=None, **extra):
    if not SAFE_KEY.fullmatch(key):
        raise ValueError("unsafe catalog key: " + key)
    return {"category": category, "key": key, "id": identifier, "name": name or key,
            "path": None, "status": "failed", "source_url": url, **extra}


def index_ids(mapping):
    """Preserve all IDs, including actual constants keys such as '3060,1617'."""
    grouped = collections.defaultdict(set)
    for identifiers, key in mapping.items():
        parts = identifiers.split(",")
        if not parts or not all(re.fullmatch(r"\d+", part.strip()) for part in parts):
            raise ValueError("invalid catalog numeric ID list: " + identifiers)
        grouped[key].update(int(part.strip()) for part in parts)
    return {key: sorted(values) for key, values in grouped.items()}


def build_entries(catalog):
    heroes = catalog["heroes"]
    hero_ids = {row["name"]: row["id"] for row in heroes.values()}
    associations = collections.defaultdict(set)
    facet_heroes = collections.defaultdict(set)
    for name, row in catalog["hero_abilities"].items():
        if name not in hero_ids:
            continue
        for ability in row.get("abilities", []) + [t["name"] for t in row.get("talents", [])]:
            associations[ability].add(hero_ids[name])
        for facet in row.get("facets", []):
            if facet.get("icon"):
                facet_heroes[facet["icon"]].add(hero_ids[name])
    result = []
    for identifier, hero in heroes.items():
        if not isinstance(hero["id"], int) or hero["id"] <= 0 or str(hero["id"]) != identifier:
            raise ValueError("hero ID mismatch")
        key = hero["name"].removeprefix("npc_dota_hero_")
        for category, field in (("heroes", "img"), ("hero-icons", "icon")):
            result.append(entry(category, key, hero["id"], hero.get("localized_name", key),
                                image_url(hero[field]), hero_id=hero["id"]))
    ability_ids = index_ids(catalog["ability_ids"])
    for key in sorted(set(catalog["abilities"]) | set(ability_ids) | set(associations)):
        meta = catalog["abilities"].get(key, {})
        identifiers = ability_ids.get(key, [])
        row = entry("abilities", key, identifiers[0] if len(identifiers) == 1 else None,
                    meta.get("dname", key), ids=identifiers, hero_ids=sorted(associations[key]))
        if key.startswith("special_bonus"):
            row.update(source_url=IMAGE_BASE + "icons/talents.svg", shared=True,
                       reason="shared talent-tree symbol; no independent talent portrait")
        elif key in ("dota_base_ability", "dota_empty_ability", "generic_hidden", "default_attack", "attribute_bonus") or re.search(r"(?:^|_)empty(?:\d|_|$)", key):
            row.update(status="skipped", reason="base ability or empty/hidden slot: no independent portrait requested")
        elif key in ("AbilityCastPoint", "AbilityCastAnimation"):
            row.update(status="skipped", reason="metadata field, not a mapped game ability")
        else:
            row["source_url"] = image_url(meta.get("img", "/apps/dota2/images/dota_react/abilities/" + key + ".png"))
        result.append(row)
    item_ids = index_ids(catalog["item_ids"])
    for key in sorted(set(catalog["items"]) | set(item_ids)):
        meta = catalog["items"].get(key, {})
        identifiers = item_ids.get(key, [])
        metadata_id = meta.get("id")
        if metadata_id is not None and identifiers and metadata_id not in identifiers:
            raise ValueError("item ID mismatch: " + key)
        if metadata_id is not None and not identifiers:
            identifiers = [metadata_id]
        row = entry("items", key, identifiers[0] if len(identifiers) == 1 else None, meta.get("dname", key), ids=identifiers)
        if key == "ability_base":
            row.update(status="skipped", reason="internal item base definition")
        else:
            # Catalogued recipes explicitly use this shared image. ID-only recipes
            # are marked as shared too; they are not asserted to be current shop items.
            default = "recipe" if key.startswith("recipe_") else key
            row["source_url"] = image_url(meta.get("img", "/apps/dota2/images/dota_react/items/" + default + ".png"))
            if key.startswith("recipe_"):
                row.update(shared=True, reason="shared recipe artwork; not an independent item portrait")
        result.append(row)
    for key, ids in sorted(facet_heroes.items()):
        result.append(entry("facets", key, None, key, IMAGE_BASE + "icons/facets/" + key + ".png", hero_ids=sorted(ids), shared=True))
    for key in ("hero_strength", "hero_agility", "hero_intelligence", "hero_universal", "innate_icon"):
        result.append(entry("ui", key, None, key, IMAGE_BASE + "icons/" + key + ".png"))
    result.append(entry("ui", "talents", None, "Talent tree", IMAGE_BASE + "icons/talents.svg", shared=True))
    return sorted(result, key=lambda row: (row["category"], row["key"]))


def target_path(row):
    if row["category"] in ("heroes", "hero-icons"):
        return f"{row['category']}/{row['id']}.png"
    filename = Path(urllib.parse.urlsplit(row["source_url"]).path).name
    if not re.fullmatch(r"[A-Za-z0-9_]+\.(?:png|svg)", filename):
        raise ValueError("unsafe asset filename")
    if row["source_url"] == IMAGE_BASE + "icons/talents.svg":
        return "ui/talents.svg"
    return row["category"] + "/" + filename


def load_catalog(output, revision, downloader, previous):
    files, catalog = [], {}
    previous_files = {row["path"]: row for row in previous.get("source", {}).get("metadata_files", [])}
    same_revision = previous.get("source", {}).get("commit") == revision
    for name in (*CATALOG_NAMES, "package"):
        upstream = "package.json" if name == "package" else "build/" + name + ".json"
        url = f"https://raw.githubusercontent.com/odota/dotaconstants/{revision}/{upstream}"
        relative = f"catalog/{name}.json"
        path = safe_path(output, relative)
        data = path.read_bytes() if path.exists() and same_revision else None
        old = previous_files.get(relative, {})
        if data is None or sha256(data) != old.get("sha256") or old.get("source_url") != url:
            data = downloader.get(url, MAX_JSON_BYTES)
            parsed = json.loads(data)
            if not isinstance(parsed, dict):
                raise ValueError("catalog must contain JSON objects")
            atomic_write(path, data)
        catalog[name] = json.loads(data)
        files.append({"path": relative, "source_url": url, "sha256": sha256(data), "bytes": len(data)})
    source = {"name": "odota/dotaconstants", "commit": revision,
              "version": catalog["package"].get("version"), "metadata_files": files}
    return catalog, source


def fetch_asset(output, rows, old_rows, downloader, retry_missing=False, same_revision=True):
    downloader.check_cancelled()
    relative = target_path(rows[0])
    path = safe_path(output, relative)
    previous = next((old_rows.get((row["category"], row["key"])) for row in rows
                     if old_rows.get((row["category"], row["key"]), {}).get("status") == "downloaded"), None)
    checked = utc_now()
    if same_revision and previous and previous.get("path") == relative and previous.get("source_url") == rows[0]["source_url"]:
        try:
            data = path.read_bytes()
            width, height = validate_image(data, relative)
            if sha256(data) == previous.get("sha256"):
                return {"path": relative, "status": "downloaded", "sha256": sha256(data), "bytes": len(data),
                        "width": width, "height": height, "downloaded_at": previous.get("downloaded_at", checked), "checked_at": checked}, "cached"
        except (OSError, ValueError):
            pass
    prior_missing = [old_rows.get((row["category"], row["key"]), {}) for row in rows]
    if same_revision and not retry_missing and all(old.get("status") == "not_found" and old.get("source_url") == row["source_url"] for old, row in zip(prior_missing, rows)):
        return {"path": None, "status": "not_found", "reason": "HTTP 404 (cached; use --retry-missing to probe again)", "checked_at": prior_missing[0].get("checked_at")}, "cached_missing"
    try:
        data = downloader.get(rows[0]["source_url"], MAX_IMAGE_BYTES)
        width, height = validate_image(data, relative)
        atomic_write(path, data)
        return {"path": relative, "status": "downloaded", "sha256": sha256(data), "bytes": len(data),
                "width": width, "height": height, "downloaded_at": utc_now(), "checked_at": checked}, "downloaded"
    except urllib.error.HTTPError as error:
        status = "not_found" if error.code == 404 else "failed"
        return {"path": None, "status": status, "reason": "HTTP " + str(error.code), "checked_at": checked}, status
    except (OSError, ValueError, RuntimeError) as error:
        return {"path": None, "status": "failed", "reason": str(error), "checked_at": checked}, "failed"


def summarize(entries):
    statuses = collections.Counter(row["status"] for row in entries)
    categories = {}
    physical = {}
    for row in entries:
        categories.setdefault(row["category"], collections.Counter())[row["status"]] += 1
        if row["status"] == "downloaded":
            physical[row["path"]] = row.get("bytes", 0)
    return {"entries": len(entries), "statuses": dict(sorted(statuses.items())),
            "categories": {key: dict(sorted(value.items())) for key, value in sorted(categories.items())},
            "unique_files": len(physical), "unique_bytes": sum(physical.values())}


def verify(output):
    manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
    if manifest.get("schema_version") != 1:
        raise ValueError("unsupported manifest schema")
    errors, checked = [], {}
    if manifest.get("complete") is not True or manifest.get("source", {}).get("expected_entries") != len(manifest.get("entries", [])):
        errors.append({"error": "manifest is incomplete or expected entry count does not match"})
    if len(manifest.get("source", {}).get("metadata_files", [])) != len(CATALOG_NAMES) + 1:
        errors.append({"error": "missing source catalog files"})
    for row in manifest.get("source", {}).get("metadata_files", []):
        try:
            data = safe_path(output, row["path"]).read_bytes()
            if sha256(data) != row["sha256"] or len(data) != row["bytes"]:
                raise ValueError("catalog hash/size mismatch")
            json.loads(data)
        except (OSError, ValueError, KeyError) as error:
            errors.append({"path": row.get("path"), "error": str(error)})
    seen = set()
    for row in manifest["entries"]:
        identity = row["category"], row["key"]
        if identity in seen:
            errors.append({"entry": identity, "error": "duplicate category/key"})
        seen.add(identity)
        if row["status"] == "failed":
            errors.append({"entry": identity, "error": row.get("reason", "download failed")})
        elif row["status"] == "downloaded":
            try:
                relative = row["path"]
                if relative not in checked:
                    data = safe_path(output, relative).read_bytes()
                    checked[relative] = sha256(data), len(data), validate_image(data, relative)
                digest, size, dimensions = checked[relative]
                if (digest, size, dimensions) != (row["sha256"], row["bytes"], (row["width"], row["height"])):
                    raise ValueError("image hash/size/dimensions mismatch")
            except (OSError, ValueError, KeyError) as error:
                errors.append({"entry": identity, "error": str(error)})
        elif row["status"] not in ("not_found", "skipped") or row.get("path") is not None:
            errors.append({"entry": identity, "error": "invalid unavailable-asset state"})
    print(json.dumps({"verification": "passed" if not errors else "failed", "summary": summarize(manifest["entries"]), "errors": errors}, ensure_ascii=False), flush=True)
    return 1 if errors else 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "assets/game-icons")
    parser.add_argument("--revision", default=REVISION, help="Immutable dotaconstants Git commit (40 hexadecimal digits)")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--rate", type=float, default=4, help="Global HTTP requests/second; maximum 8")
    parser.add_argument("--retry-missing", action="store_true", help="Retry previously recorded HTTP 404 images")
    parser.add_argument("--verify", action="store_true", help="Verify all manifest files without making network requests")
    args = parser.parse_args(argv)
    if not re.fullmatch(r"[0-9a-f]{40}", args.revision) or not 1 <= args.workers <= 8 or not 0 < args.rate <= 8:
        parser.error("revision must be a full lowercase commit; workers 1–8 and rate >0–8 are supported")
    output = args.output.resolve()
    if args.verify:
        return verify(output)
    output.mkdir(parents=True, exist_ok=True)
    manifest_path = output / "manifest.json"
    previous = json.loads(manifest_path.read_text(encoding="utf-8")) if manifest_path.exists() else {}
    if previous and previous.get("schema_version") != 1:
        raise ValueError("unsupported existing manifest schema")
    downloader = Downloader(rate=args.rate)
    catalog, source = load_catalog(output, args.revision, downloader, previous)
    entries = build_entries(catalog)
    source["expected_entries"] = len(entries)
    manifest = {"schema_version": 1, "complete": False, "source": source, "generated_at": utc_now(),
                "license_notice": "Dota 2 artwork belongs to Valve. Public CDN availability and dotaconstants MIT code license do not establish artwork redistribution rights.",
                "coverage_notice": "Catalog includes historical/internal definitions. Download success does not prove current in-game availability. Empty/base slots are explicitly skipped. Talents use the shared talent-tree symbol; recipe and facet art can be shared.",
                "entries": entries, "summary": summarize(entries)}
    old_rows = {(row["category"], row["key"]): row for row in previous.get("entries", [])}
    same_revision = previous.get("source", {}).get("commit") == args.revision
    groups = collections.defaultdict(list)
    for row in entries:
        if row["status"] != "skipped":
            groups[(row["source_url"], target_path(row))].append(row)
            old = old_rows.get((row["category"], row["key"]), {})
            if same_revision and old.get("source_url") == row["source_url"]:
                for field in ("status", "path", "sha256", "bytes", "width", "height", "downloaded_at", "checked_at"):
                    if field in old:
                        row[field] = old[field]
    for rows in groups.values():
        if len(rows) > 1:
            for row in rows:
                row["shared"] = True
    write_json(manifest_path, manifest)
    activity = collections.Counter()
    print(json.dumps({"catalog_commit": args.revision, "entries": len(entries), "asset_jobs": len(groups), "workers": args.workers, "rate": args.rate}), flush=True)
    completed, interrupted, finished = 0, False, False
    jobs, processed = {}, set()
    executor = concurrent.futures.ThreadPoolExecutor(max_workers=args.workers)

    def collect_result(future):
        if future in processed or future.cancelled():
            return
        rows = jobs[future]
        try:
            result, action = future.result()
        except DownloadCancelled:
            # Retain a previously verified cache entry if it has not been touched.
            # New/unverified entries already remain failed and will resume later.
            return
        except Exception as error:  # Do not lose other completed downloads.
            result, action = {"status": "failed", "path": None, "reason": str(error), "checked_at": utc_now()}, "failed"
        for row in rows:
            if result["status"] != "downloaded":
                for field in ("sha256", "bytes", "width", "height", "downloaded_at"):
                    row.pop(field, None)
            row.update(result)
        activity[action] += 1
        processed.add(future)

    def checkpoint(complete=False):
        manifest.update(generated_at=utc_now(), summary=summarize(entries), complete=complete)
        write_json(manifest_path, manifest)

    try:
        for rows in groups.values():
            future = executor.submit(fetch_asset, output, rows, old_rows, downloader, args.retry_missing, same_revision)
            jobs[future] = rows
        for index, future in enumerate(concurrent.futures.as_completed(jobs), 1):
            collect_result(future)
            if index % 50 == 0 or index == len(jobs):
                checkpoint()
                print(json.dumps({"completed": index, "total": len(jobs), "activity": dict(activity), "summary": manifest["summary"]["statuses"]}), flush=True)
        finished = True
    except KeyboardInterrupt:
        interrupted = True
        downloader.cancel()
        for future in jobs:
            future.cancel()
        checkpoint()
        print(json.dumps({"interrupted": True, "completed": len(processed), "total": len(groups),
                          "message": "Queued requests cancelled; waiting only for active HTTP operations (timeout <=25s)."}), flush=True)
    finally:
        # Do not use the executor context manager: its __exit__ waits for every
        # queued download before an outer KeyboardInterrupt handler can run.
        if not finished:
            downloader.cancel()
        executor.shutdown(wait=True, cancel_futures=not finished)
        for future in jobs:
            if future.done():
                collect_result(future)
        completed = len(processed)
        complete = finished and completed == len(groups) and not summarize(entries)["statuses"].get("failed", 0)
        checkpoint(complete)
    print(json.dumps({"manifest": str(manifest_path), "summary": manifest["summary"]}), flush=True)
    if interrupted:
        return 130
    return 1 if manifest["summary"]["statuses"].get("failed", 0) else 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, RuntimeError) as error:
        print(json.dumps({"error": str(error)}, ensure_ascii=False), flush=True)
        raise SystemExit(1) from error
    except KeyboardInterrupt:
        # An interruption before workers exist (e.g. catalog retrieval).
        print(json.dumps({"interrupted": True, "complete": False}), flush=True)
        raise SystemExit(130)
