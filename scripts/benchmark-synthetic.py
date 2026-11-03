#!/usr/bin/env python3
"""Run bounded SYNTHETIC workload diagnostics. Never use a player database or API."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "artifacts/synthetic-performance")
    parser.add_argument("--rounds", type=int, default=5, choices=range(3, 11))
    parser.add_argument("--skip-build", action="store_true", help="Use an already built release example")
    args = parser.parse_args()
    if platform.system() != "Linux" or not Path("/usr/bin/time").is_file():
        parser.error("This measurement wrapper requires Linux and /usr/bin/time")
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--locked", "--release", "-p", "dpda-debug", "--example", "synthetic_performance"], cwd=ROOT, check=True)
    executable = ROOT / "target/release/examples/synthetic_performance"
    if not executable.is_file():
        parser.error("release example is missing; omit --skip-build")
    args.output.mkdir(parents=True, exist_ok=True)
    production_paths = ["Cargo.lock", "crates/persistence/src/lib.rs", "crates/hero-pool/src/lib.rs", "crates/recommender/src/lib.rs", "crates/statistics/src/lib.rs", "crates/debug-cli/examples/synthetic_performance.rs"]
    cpu = re.search(r"^model name\s*:\s*(.+)$", Path("/proc/cpuinfo").read_text(), re.MULTILINE)
    environment = {
        "provenance": "synthetic",
        "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "kernel": platform.platform(),
        "cpu": cpu.group(1) if cpu else None,
        "logical_cpus": os.cpu_count(),
        "rustc": command("rustc", "--version"),
        "cargo": command("cargo", "--version"),
        "git_head": command("git", "rev-parse", "HEAD"),
        "workspace_dirty": bool(command("git", "status", "--porcelain")),
        "source_sha256": {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in production_paths},
        "executable_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
        "profile": "release",
        "cache_policy": "OS caches not cleared; temporary database generated in measured process",
        "measurement_isolation": "normal shared host, no CPU pinning or load isolation",
    }
    (args.output / "environment.json").write_text(json.dumps(environment, indent=2) + "\n")
    environment_variables = os.environ.copy()
    environment_variables["LC_ALL"] = "C"
    summary = {"provenance": "synthetic", "runs": []}
    for count in (10_000, 50_000):
        report_path = args.output / f"{count}.json"
        time_path = args.output / f"{count}.time.txt"
        with report_path.open("w") as output:
            subprocess.run(["/usr/bin/time", "-v", "-o", str(time_path), str(executable), "--matches", str(count), "--rounds", str(args.rounds)], cwd=ROOT, env=environment_variables, stdout=output, check=True, timeout=180)
        report = json.loads(report_path.read_text())
        time_text = time_path.read_text()
        rss = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", time_text)
        if not rss or report.get("provenance") != "synthetic":
            raise RuntimeError("missing RSS measurement or synthetic provenance")
        summary["runs"].append({"matches": count, "peak_rss_kib": int(rss.group(1)), "peak_rss_scope": "entire process including generation/insertion; not steady-state", "measurements": report["measurements"], "database_bytes": report["database_bytes"]})
        print(f"SYNTHETIC {count} matches: {report_path}, peak RSS {rss.group(1)} KiB", flush=True)
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")


if __name__ == "__main__":
    main()
