#!/usr/bin/env python3
"""Restore the exact, reviewed game-icon snapshot; never fall back to a live CDN."""

import argparse
import collections
import contextlib
import hashlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import stat
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
MAX_ARCHIVE_BYTES = 256 * 1024 * 1024
MAX_EXPANDED_BYTES = 512 * 1024 * 1024
MAX_FILE_BYTES = 32 * 1024 * 1024
MAX_ENTRIES = 20_000
CHUNK_BYTES = 64 * 1024
DOWNLOAD_HOSTS = {"github.com", "release-assets.githubusercontent.com"}
CATEGORIES = {"heroes", "hero-icons", "abilities", "items", "facets", "ui"}
STATUSES = {"downloaded", "not_found", "skipped"}
SPEC = importlib.util.spec_from_file_location("dpda_snapshot_images", ROOT / "scripts/fetch-game-icons.py")
IMAGES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(IMAGES)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def positive_integer(value, limit):
    return type(value) is int and 0 < value <= limit


def digest(data):
    return hashlib.sha256(data).hexdigest()


def safe_stat(path):
    result = path.lstat()
    # Windows junctions are reparse points but are not always reported as symlinks.
    require(not stat.S_ISLNK(result.st_mode)
            and not (getattr(result, "st_file_attributes", 0) & 0x400),
            f"Symlink or Windows reparse point is forbidden: {path.name}")
    return result


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "Duplicate JSON key: " + key)
        result[key] = value
    return result


def parse_json(data):
    return json.loads(data.decode("utf-8"), object_pairs_hook=unique_object)


def read_json(path, limit=MAX_FILE_BYTES):
    require(stat.S_ISREG(safe_stat(path).st_mode), f"Expected a regular JSON file: {path}")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    require(len(data) <= limit, f"JSON exceeds size limit: {path}")
    return parse_json(data), data


def safe_relative(value):
    require(isinstance(value, str) and value, "Missing relative path")
    pieces = value.split("/")
    for piece in pieces:
        require(re.fullmatch(r"[A-Za-z0-9_-][A-Za-z0-9_.-]*", piece) is not None
                and not piece.endswith("."), f"Unsafe relative path: {value}")
        require(re.fullmatch(r"CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9]",
                             piece.split(".")[0], re.IGNORECASE) is None,
                f"Windows reserved path: {value}")
    return value


def https_url(url, hosts=None):
    require(isinstance(url, str) and not re.search(r"[\x00-\x20\\\x7f]", url), "Unsafe HTTPS URL")
    parsed = urllib.parse.urlsplit(url)
    require(parsed.scheme == "https" and parsed.hostname and parsed.username is None
            and parsed.password is None and parsed.port in (None, 443) and not parsed.fragment,
            "Only credential-free HTTPS on port 443 is allowed")
    if hosts is not None:
        require(parsed.hostname in hosts, f"Untrusted snapshot host: {parsed.hostname}")
    return parsed


def read_contract(snapshot_path, lock_path):
    snapshot, _ = read_json(snapshot_path, 64 * 1024)
    lock, lock_bytes = read_json(lock_path)
    require(isinstance(snapshot, dict) and snapshot.get("schema_version") == 1,
            "Unsupported snapshot schema")
    require(re.fullmatch(r"[a-f0-9]{40}", snapshot.get("source_commit", "")) is not None,
            "Snapshot must pin a source commit")
    require(snapshot.get("lock_sha256") == digest(lock_bytes),
            "Snapshot does not match the current game-icons.lock.json bytes")
    require(re.fullmatch(r"[a-f0-9]{64}", snapshot.get("manifest_sha256", "")) is not None,
            "Snapshot must pin manifest_sha256 for cache verification")
    require(isinstance(lock, dict) and lock.get("schema_version") == 1
            and lock.get("source_commit") == snapshot["source_commit"],
            "Lock source commit differs from the snapshot")
    archive = snapshot.get("archive")
    require(isinstance(archive, dict), "Missing snapshot archive")
    parsed = https_url(archive.get("url"), {"github.com"})
    require(not parsed.query and re.fullmatch(
        r"/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/releases/download/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\.zip",
        parsed.path) is not None, "Snapshot must reference an exact GitHub release ZIP asset")
    require(positive_integer(archive.get("bytes"), MAX_ARCHIVE_BYTES), "Invalid archive byte limit")
    require(re.fullmatch(r"[a-f0-9]{64}", archive.get("sha256", "")) is not None,
            "Invalid archive SHA-256")
    files = lock.get("files")
    require(isinstance(files, list) and 0 < len(files) <= MAX_ENTRIES, "Invalid lock files")
    paths = set()
    for entry in files:
        require(isinstance(entry, dict), "Invalid lock entry")
        relative = safe_relative(entry.get("path"))
        require(relative.casefold() not in paths, "Duplicate or case-conflicting lock path")
        paths.add(relative.casefold())
        require(positive_integer(entry.get("bytes"), 8 * 1024 * 1024)
                and re.fullmatch(r"[a-f0-9]{64}", entry.get("sha256", "")) is not None,
                f"Invalid locked size/hash: {relative}")
        https_url(entry.get("source_url"))
    return snapshot, lock


class SnapshotRedirects(urllib.request.HTTPRedirectHandler):
    max_redirections = 5

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        https_url(req.full_url, DOWNLOAD_HOSTS)
        https_url(newurl, DOWNLOAD_HOSTS)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def copy_archive(stream, destination, expected):
    size, checksum = 0, hashlib.sha256()
    with destination.open("wb") as output:
        while True:
            block = stream.read(min(CHUNK_BYTES, expected["bytes"] + 1 - size))
            if not block:
                break
            size += len(block)
            require(size <= expected["bytes"], "Archive exceeds its pinned byte count")
            checksum.update(block)
            output.write(block)
        output.flush()
        os.fsync(output.fileno())
    require(size == expected["bytes"], "Archive is truncated or has a different byte count")
    require(checksum.hexdigest() == expected["sha256"], "Archive SHA-256 mismatch")


def acquire_archive(expected, destination, local_archive=None):
    if local_archive is not None:
        require(stat.S_ISREG(safe_stat(local_archive).st_mode),
                "--archive must name a regular local ZIP file")
        with local_archive.open("rb") as stream:
            copy_archive(stream, destination, expected)
        return
    https_url(expected["url"], {"github.com"})
    opener = urllib.request.build_opener(SnapshotRedirects())
    request = urllib.request.Request(expected["url"], headers={
        "User-Agent": "DPDA/0.1 locked-asset-snapshot", "Accept": "application/octet-stream",
        "Accept-Encoding": "identity",
    })
    for attempt in range(3):
        try:
            with opener.open(request, timeout=30) as response:
                https_url(response.geturl(), DOWNLOAD_HOSTS)
                require(response.status == 200, "Snapshot download did not return HTTP 200")
                length = response.headers.get("Content-Length")
                require(length is None or length == str(expected["bytes"]),
                        "HTTP Content-Length differs from the pinned archive size")
                copy_archive(response, destination, expected)
            return
        except urllib.error.HTTPError as error:
            if error.code not in (408, 429, 500, 502, 503, 504) or attempt == 2:
                raise RuntimeError(f"Snapshot download failed: HTTP {error.code}") from None
        except (urllib.error.URLError, http.client.HTTPException, ConnectionError, TimeoutError) as error:
            if attempt == 2:
                raise RuntimeError(f"Snapshot network request failed: {type(error).__name__}") from None
        time.sleep(attempt + 1)


def extract_archive(archive_path, destination):
    """Never use extractall: preflight every name/type/size before writing any member."""
    with zipfile.ZipFile(archive_path) as archive:
        entries = archive.infolist()
        require(0 < len(entries) <= MAX_ENTRIES, "Archive entry count exceeds limit")
        seen, kinds, total = {}, {}, 0
        for entry in entries:
            require(entry.orig_filename == entry.filename, "ZIP entry contains a NUL")
            relative = safe_relative(entry.filename[:-1] if entry.is_dir() else entry.filename)
            folded = relative.casefold()
            require(folded not in seen, f"Duplicate or case-conflicting ZIP path: {relative}")
            seen[folded] = relative
            kinds[relative] = entry.is_dir()
            mode = stat.S_IFMT(entry.external_attr >> 16)
            expected_type = stat.S_IFDIR if entry.is_dir() else stat.S_IFREG
            require(mode in (0, expected_type) and not (entry.external_attr & 0x400),
                    f"ZIP links or special files are forbidden: {relative}")
            require(not (entry.flag_bits & 1), "Encrypted ZIP members are forbidden")
            require(entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
                    "Unsupported ZIP compression")
            require(0 <= entry.file_size <= MAX_FILE_BYTES, "ZIP member exceeds size limit")
            require(not entry.is_dir() or entry.file_size == 0, "ZIP directory contains data")
            total += entry.file_size
        require(total <= MAX_EXPANDED_BYTES, "Expanded ZIP exceeds size limit")
        # Case checks include implicit parents, not only full file names.
        parents = dict(seen)
        for relative in kinds:
            pieces = relative.split("/")
            for index in range(1, len(pieces)):
                parent = "/".join(pieces[:index])
                folded = parent.casefold()
                require(folded not in parents or parents[folded] == parent,
                        "Case-conflicting ZIP directory")
                require(parent not in kinds or kinds[parent], "ZIP file is also a directory")
                parents[folded] = parent
        destination.mkdir()
        for entry in entries:
            relative = entry.filename[:-1] if entry.is_dir() else entry.filename
            target = destination.joinpath(*relative.split("/"))
            if entry.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            size = 0
            with archive.open(entry) as source, target.open("xb") as output:
                while True:
                    block = source.read(CHUNK_BYTES)
                    if not block:
                        break
                    size += len(block)
                    require(size <= entry.file_size, "ZIP member exceeded declared size")
                    output.write(block)
            require(size == entry.file_size, "Truncated ZIP member")


def checked_content(directory, entry, limit):
    relative = safe_relative(entry.get("path"))
    require(positive_integer(entry.get("bytes"), limit), f"Invalid file size: {relative}")
    require(re.fullmatch(r"[a-f0-9]{64}", entry.get("sha256", "")) is not None,
            f"Invalid file hash: {relative}")
    path = directory.joinpath(*relative.split("/"))
    require(stat.S_ISREG(safe_stat(path).st_mode), f"Missing regular file: {relative}")
    with path.open("rb") as stream:
        data = stream.read(entry["bytes"] + 1)
    require(len(data) == entry["bytes"] and digest(data) == entry["sha256"],
            f"File hash/size mismatch: {relative}")
    return data


def verify_directory(directory, lock, manifest_sha256):
    require(stat.S_ISDIR(safe_stat(directory).st_mode), "Icon directory is missing or linked")
    # Reject links before opening any manifest-declared files, including linked parent directories.
    actual, actual_dirs, case_names = set(), set(), {}
    for parent, dirs, files in os.walk(directory, followlinks=False):
        for name in dirs + files:
            path = Path(parent) / name
            relative = safe_relative(path.relative_to(directory).as_posix())
            metadata = safe_stat(path)
            require(relative.casefold() not in case_names, "Case-conflicting icon directory")
            case_names[relative.casefold()] = relative
            if name in dirs:
                actual_dirs.add(relative)
            else:
                require(stat.S_ISREG(metadata.st_mode), f"Non-regular icon file: {relative}")
                actual.add(relative)
    manifest, manifest_bytes = read_json(directory / "manifest.json")
    require(digest(manifest_bytes) == manifest_sha256, "Manifest differs from the pinned snapshot")
    require(isinstance(manifest, dict) and manifest.get("schema_version") == 1
            and manifest.get("complete") is True, "Manifest is incomplete or unsupported")
    source, entries = manifest.get("source"), manifest.get("entries")
    require(isinstance(source, dict) and source.get("commit") == lock["source_commit"],
            "Manifest source commit differs from lock")
    require(isinstance(entries, list) and positive_integer(source.get("expected_entries"), MAX_ENTRIES)
            and len(entries) == source["expected_entries"], "Manifest entry count mismatch")
    metadata = source.get("metadata_files")
    require(isinstance(metadata, list) and metadata, "Missing source metadata")
    declared, physical, keys, counts = {"manifest.json"}, {}, set(), collections.Counter()
    for entry in metadata:
        require(isinstance(entry, dict), "Invalid metadata entry")
        relative = safe_relative(entry.get("path"))
        require(relative.startswith("catalog/") and relative.endswith(".json")
                and relative not in declared, "Duplicate or invalid metadata path")
        parse_json(checked_content(directory, entry, MAX_FILE_BYTES))
        declared.add(relative)
    for entry in entries:
        require(isinstance(entry, dict) and entry.get("category") in CATEGORIES
                and entry.get("status") in STATUSES, "Manifest has a failed or unknown entry")
        require(isinstance(entry.get("key"), str) and entry["key"], "Missing manifest key")
        key = (entry["category"], entry["key"])
        require(key not in keys, "Duplicate manifest key")
        keys.add(key)
        counts[entry["status"]] += 1
        if entry["status"] != "downloaded":
            continue
        relative = safe_relative(entry.get("path"))
        https_url(entry.get("source_url"))
        if relative in physical:
            require(all(entry.get(field) == physical[relative].get(field)
                        for field in ("sha256", "bytes", "source_url", "width", "height")),
                    "Conflicting shared image")
            continue
        require(relative not in declared, "Image conflicts with metadata")
        data = checked_content(directory, entry, 8 * 1024 * 1024)
        dimensions = IMAGES.validate_image(data, relative)
        require(dimensions == (entry.get("width"), entry.get("height")), "Image dimensions mismatch")
        physical[relative] = entry
        declared.add(relative)
    locked = {entry["path"]: entry for entry in lock["files"]}
    require(physical and physical.keys() == locked.keys(), "Physical image set differs from lock")
    for relative, entry in physical.items():
        require(all(entry.get(field) == locked[relative].get(field)
                    for field in ("sha256", "bytes", "source_url")), f"Image differs from lock: {relative}")
    require(actual == declared, "Icon directory has missing or unlisted files")
    declared_dirs = {"/".join(relative.split("/")[:index]) for relative in declared
                     for index in range(1, len(relative.split("/")))}
    require(actual_dirs <= declared_dirs, "Icon directory has unlisted directories")
    return {"physical_files": len(physical), "bytes": sum(row["bytes"] for row in physical.values()),
            "counts": dict(sorted(counts.items())), "source_commit": source["commit"]}


@contextlib.contextmanager
def exclusive_install(output):
    marker = output.parent / ("." + output.name + ".snapshot-lock")
    try:
        marker.mkdir()
    except FileExistsError:
        raise RuntimeError(f"Another snapshot install may be active. If it has stopped, remove {marker}") from None
    try:
        yield
    finally:
        marker.rmdir()


def restore_backup(output, backup, lock, manifest_sha256):
    if not os.path.lexists(backup):
        return
    require(stat.S_ISDIR(safe_stat(backup).st_mode), "Unsafe snapshot backup")
    if output.exists():
        verify_directory(output, lock, manifest_sha256)
        shutil.rmtree(backup)
    else:
        backup.rename(output)


def install_directory(staging, output, backup):
    require(not os.path.lexists(backup), "Snapshot backup already exists")
    had_previous = output.exists()
    if had_previous:
        require(stat.S_ISDIR(safe_stat(output).st_mode), "Unsafe existing icon directory")
        output.rename(backup)
    try:
        staging.rename(output)
    except BaseException:
        if had_previous and not output.exists():
            backup.rename(output)
        raise
    if had_previous:
        shutil.rmtree(backup)


def restore(snapshot_path, lock_path, output, archive_path=None):
    snapshot, lock = read_contract(snapshot_path, lock_path)
    output = output.absolute()
    require(output.name not in ("", ".", ".."), "Unsafe output directory")
    if os.path.lexists(output):
        require(stat.S_ISDIR(safe_stat(output).st_mode), "Unsafe output directory")
    output.parent.mkdir(parents=True, exist_ok=True)
    safe_stat(output.parent)
    backup = output.parent / ("." + output.name + ".snapshot-backup")
    with exclusive_install(output):
        restore_backup(output, backup, lock, snapshot["manifest_sha256"])
        try:
            report = verify_directory(output, lock, snapshot["manifest_sha256"])
        except (OSError, ValueError, KeyError, TypeError) as error:
            print(f"Snapshot cache unavailable: {error}", file=sys.stderr)
        else:
            return {"status": "cached", "offline": True, **report}
        with tempfile.TemporaryDirectory(prefix="." + output.name + "-snapshot-", dir=output.parent) as temporary:
            workspace = Path(temporary)
            archive = workspace / "snapshot.zip"
            acquire_archive(snapshot["archive"], archive, archive_path)
            staging = workspace / "verified"
            extract_archive(archive, staging)
            report = verify_directory(staging, lock, snapshot["manifest_sha256"])
            install_directory(staging, output, backup)
        return {"status": "restored", "offline": archive_path is not None, **report}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", type=Path, default=ROOT / "assets/game-icons.snapshot.json")
    parser.add_argument("--lock", type=Path, default=ROOT / "assets/game-icons.lock.json")
    parser.add_argument("--output", type=Path, default=ROOT / "assets/game-icons")
    parser.add_argument("--archive", type=Path, help="Use a local, SHA-256-pinned ZIP without network access")
    args = parser.parse_args()
    try:
        report = restore(args.snapshot, args.lock, args.output, args.archive)
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, zipfile.BadZipFile) as error:
        print(f"Locked icon snapshot failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
