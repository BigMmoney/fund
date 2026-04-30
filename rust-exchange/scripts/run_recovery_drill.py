import argparse
import json
import subprocess
import sys
import time
from pathlib import Path


def run_step(name: str, command: list[str], workdir: Path) -> dict:
    started = time.time()
    result = subprocess.run(
        command,
        cwd=workdir,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    elapsed = round(time.time() - started, 3)
    return {
        "name": name,
        "command": command,
        "exit_code": result.returncode,
        "elapsed_seconds": elapsed,
        "stdout": result.stdout,
        "stderr": result.stderr,
        "ok": result.returncode == 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run recovery / restore drill checks for the Rust exchange backend.",
    )
    parser.add_argument(
        "--output",
        help="Optional path to write the JSON drill report.",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[1]

    steps = [
        (
            "critical_config_tests",
            ["cargo", "test", "-p", "api", "config::tests", "--", "--nocapture"],
        ),
        (
            "rollback_guard_tests",
            ["cargo", "test", "-p", "api", "rollback::tests", "--", "--nocapture"],
        ),
        (
            "wal_crc_corruption_detection",
            ["cargo", "test", "-p", "persistence", "crc32_detects_corruption", "--", "--nocapture"],
        ),
        (
            "wal_best_effort_recovery",
            ["cargo", "test", "-p", "persistence", "best_effort_recovery_skips_corrupt_entries", "--", "--nocapture"],
        ),
        (
            "wal_backup_rotation",
            ["cargo", "test", "-p", "persistence", "wal_rotation_creates_backup", "--", "--nocapture"],
        ),
        (
            "matching_crash_recovery_example",
            ["cargo", "run", "-q", "-p", "matching", "--example", "crash_recovery_drill"],
        ),
    ]

    results = [run_step(name, command, repo_root) for name, command in steps]

    report = {
        "generated_at_epoch": int(time.time()),
        "repo_root": str(repo_root),
        "steps": results,
        "ok": all(step["ok"] for step in results),
    }

    rendered = json.dumps(report, indent=2, ensure_ascii=False)

    if args.output:
        output_path = Path(args.output)
        if not output_path.is_absolute():
            output_path = repo_root / output_path
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(rendered + "\n", encoding="utf-8")

    print(rendered)

    if report["ok"]:
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
