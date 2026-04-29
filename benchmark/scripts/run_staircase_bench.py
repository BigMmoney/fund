import json
import os
import subprocess
import sys
import urllib.request
from datetime import datetime, timezone
from pathlib import Path


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def read_secret() -> str:
    secret = os.getenv("BENCH_SECRET", "").strip()
    if secret:
        return secret
    secret_file = os.getenv(
        "BENCH_SECRET_FILE",
        "/var/run/secrets/exchange/internal_auth.secret",
    )
    return Path(secret_file).read_text(encoding="utf-8").strip()


def build_users(prefix: str, count: int) -> list[str]:
    return [f"{prefix}-{idx:02d}" for idx in range(1, count + 1)]


def fetch_text(url: str) -> str:
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read().decode("utf-8")


def fetch_json(url: str) -> dict:
    return json.loads(fetch_text(url))


def fetch_optional_json(url: str) -> dict:
    try:
        return fetch_json(url)
    except Exception as exc:
        return {"fetch_error": str(exc), "url": url}


def fetch_optional_text(url: str) -> str:
    try:
        return fetch_text(url)
    except Exception as exc:
        return "# fetch_error\n" + str(exc) + "\n"


def compact_text(value: str, limit: int = 4000) -> str:
    text = value.strip()
    if len(text) <= limit:
        return text
    return text[:limit] + "\n...[truncated]..."


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")


def run_command(args: list[str], stdout_path: Path, stderr_path: Path) -> str:
    completed = subprocess.run(args, capture_output=True, text=True)
    stdout_path.write_text(completed.stdout, encoding="utf-8")
    stderr_path.write_text(completed.stderr, encoding="utf-8")
    if completed.stderr.strip():
        print(completed.stderr.strip(), file=sys.stderr)
    if completed.returncode != 0:
        raise RuntimeError(
            "command failed with exit code {0}: {1}\nstdout:\n{2}\nstderr:\n{3}".format(
                completed.returncode,
                " ".join(args),
                compact_text(completed.stdout) or "<empty>",
                compact_text(completed.stderr) or "<empty>",
            )
        )
    return completed.stdout


def classify_turning_point(previous_run: dict | None, current_run: dict) -> dict | None:
    if current_run["http_429_count"] > 0:
        return {
            "reason": "http_429_detected",
            "total_requests": current_run["total_requests"],
            "client_p99_us": current_run["client_latency_us"]["p99"],
        }
    if current_run["http_4xx_count"] > 0:
        return {
            "reason": "http_4xx_detected",
            "total_requests": current_run["total_requests"],
            "client_p99_us": current_run["client_latency_us"]["p99"],
        }
    if current_run["success_rate"] < 99.9:
        return {
            "reason": "success_rate_drop",
            "total_requests": current_run["total_requests"],
            "client_p99_us": current_run["client_latency_us"]["p99"],
        }
    if previous_run is None:
        return None
    previous_p99 = previous_run["client_latency_us"]["p99"]
    current_p99 = current_run["client_latency_us"]["p99"]
    if previous_p99 > 0 and current_p99 >= previous_p99 * 1.5:
        return {
            "reason": "client_p99_jump",
            "total_requests": current_run["total_requests"],
            "previous_p99_us": previous_p99,
            "current_p99_us": current_p99,
        }
    previous_roundtrip = previous_run["http_roundtrip_us"]["p99"]
    current_roundtrip = current_run["http_roundtrip_us"]["p99"]
    if previous_roundtrip > 0 and current_roundtrip >= previous_roundtrip * 1.5:
        return {
            "reason": "http_roundtrip_p99_jump",
            "total_requests": current_run["total_requests"],
            "previous_roundtrip_p99_us": previous_roundtrip,
            "current_roundtrip_p99_us": current_roundtrip,
        }
    return None


def main() -> None:
    artifact_root = Path(os.getenv("ARTIFACT_ROOT", "/artifacts"))
    raw_dir = artifact_root / "raw"
    artifact_root.mkdir(parents=True, exist_ok=True)
    raw_dir.mkdir(parents=True, exist_ok=True)

    base_url = os.getenv("BENCH_BASE_URL", "http://exchange.exchange.svc.cluster.local:3030").rstrip("/")
    market = os.getenv("BENCH_MARKET", "btc-usdt")
    admin_subject = os.getenv("BENCH_ADMIN_SUBJECT", "bench-admin")
    total_requests_steps = [
        int(value.strip())
        for value in os.getenv("BENCH_TOTAL_REQUEST_STEPS", "10000,50000,100000").split(",")
        if value.strip()
    ]
    buyer_count = int(os.getenv("BENCH_BUYER_COUNT", "12"))
    seller_count = int(os.getenv("BENCH_SELLER_COUNT", "12"))
    pair_concurrency = int(os.getenv("BENCH_PAIR_CONCURRENCY", "5"))
    rate_limit_per_second = int(os.getenv("BENCH_RATE_LIMIT_PER_SECOND", "60"))
    request_stagger_ms = int(os.getenv("BENCH_REQUEST_STAGGER_MS", "5"))
    rate_limit_retry_max = int(os.getenv("BENCH_RATE_LIMIT_RETRY_MAX", "2"))
    rate_limit_backoff_ms = int(os.getenv("BENCH_RATE_LIMIT_BACKOFF_MS", "150"))
    base_price = int(os.getenv("BENCH_BASE_PRICE", "50000"))
    amount = int(os.getenv("BENCH_AMOUNT", "1"))
    disable_keep_alives = os.getenv("BENCH_DISABLE_KEEP_ALIVES", "false").strip().lower()
    seed_users = os.getenv("BENCH_SEED_USERS", "false").strip().lower() in {"1", "true", "yes", "on"}
    seed_cash_amount = int(os.getenv("BENCH_SEED_CASH_AMOUNT", "500000000"))
    seed_position_amount = int(os.getenv("BENCH_SEED_POSITION_AMOUNT", "200000"))
    seed_retry_max = int(os.getenv("BENCH_SEED_RETRY_MAX", "6"))
    seed_retry_backoff_ms = int(os.getenv("BENCH_SEED_RETRY_BACKOFF_MS", "200"))
    seed_request_delay_ms = int(os.getenv("BENCH_SEED_REQUEST_DELAY_MS", "25"))
    buyers = build_users(os.getenv("BENCH_BUYER_PREFIX", "job-buyer"), buyer_count)
    sellers = build_users(os.getenv("BENCH_SELLER_PREFIX", "job-seller"), seller_count)
    secret = read_secret()

    started_at = now_iso()
    health_before = fetch_optional_json(f"{base_url}/health")
    ready_before = fetch_optional_json(f"{base_url}/ready")

    runs: list[dict] = []
    turning_point = None
    previous_run = None
    failed_step = None
    failed_error = None

    for index, total_requests in enumerate(total_requests_steps, start=1):
        pair_count = max(1, total_requests // 2)
        prefix = f"stair-{index}-{total_requests}"
        step_started_at = now_iso()
        stdout_path = raw_dir / f"{prefix}.stdout.log"
        stderr_path = raw_dir / f"{prefix}.stderr.log"
        args = [
            "/usr/local/bin/exchange_http_bench",
            "--base-url",
            base_url,
            "--secret",
            secret,
            "--market",
            market,
            "--buyers",
            ",".join(buyers),
            "--sellers",
            ",".join(sellers),
            "--pair-count",
            str(pair_count),
            "--pair-concurrency",
            str(pair_concurrency),
            "--rate-limit-per-second",
            str(rate_limit_per_second),
            "--request-stagger-ms",
            str(request_stagger_ms),
            "--rate-limit-retry-max",
            str(rate_limit_retry_max),
            "--rate-limit-backoff-ms",
            str(rate_limit_backoff_ms),
            "--base-price",
            str(base_price),
            "--amount",
            str(amount),
            "--disable-keep-alives=" + disable_keep_alives,
            "--admin-subject",
            admin_subject,
            "--prefix",
            prefix,
        ]
        if seed_users and index == 1:
            args.extend(
                [
                    "--seed-users",
                    "--seed-cash-amount",
                    str(seed_cash_amount),
                    "--seed-position-amount",
                    str(seed_position_amount),
                    "--seed-retry-max",
                    str(seed_retry_max),
                    "--seed-retry-backoff-ms",
                    str(seed_retry_backoff_ms),
                    "--seed-request-delay-ms",
                    str(seed_request_delay_ms),
                ]
            )

        try:
            step_stdout = run_command(args, stdout_path=stdout_path, stderr_path=stderr_path)
            step_finished_at = now_iso()
            step_result = json.loads(step_stdout)
            step_result["target_total_requests"] = total_requests
            step_result["pair_count"] = pair_count
            step_result["started_at"] = step_started_at
            step_result["finished_at"] = step_finished_at
            step_result["stdout_log"] = str(stdout_path)
            step_result["stderr_log"] = str(stderr_path)
            write_json(raw_dir / f"{total_requests}.json", step_result)
            runs.append(step_result)

            if turning_point is None:
                turning_point = classify_turning_point(previous_run, step_result)
            previous_run = step_result
        except Exception as exc:
            failed_step = {
                "index": index,
                "prefix": prefix,
                "target_total_requests": total_requests,
                "pair_count": pair_count,
                "started_at": step_started_at,
                "failed_at": now_iso(),
                "command": args,
                "stdout_log": str(stdout_path),
                "stderr_log": str(stderr_path),
            }
            failed_error = str(exc)
            write_json(
                raw_dir / f"{prefix}.failure.json",
                {
                    "error": failed_error,
                    "failed_step": failed_step,
                },
            )
            break

    metrics_path = artifact_root / "metrics.prometheus.txt"
    health_path = artifact_root / "health.json"
    ready_path = artifact_root / "ready.json"
    health_after = fetch_optional_json(f"{base_url}/health")
    ready_after = fetch_optional_json(f"{base_url}/ready")
    metrics_path.write_text(fetch_optional_text(f"{base_url}/metrics/prometheus"), encoding="utf-8")
    write_json(health_path, health_after)
    write_json(ready_path, ready_after)

    completed_at = now_iso()
    summary = {
        "run_mode": "k8s_staircase_benchmark_job",
        "status": "failed" if failed_step else "completed",
        "started_at": started_at,
        "completed_at": completed_at,
        "base_url": base_url,
        "market": market,
        "buyer_count": buyer_count,
        "seller_count": seller_count,
        "pair_concurrency": pair_concurrency,
        "rate_limit_per_second": rate_limit_per_second,
        "request_stagger_ms": request_stagger_ms,
        "rate_limit_retry_max": rate_limit_retry_max,
        "rate_limit_backoff_ms": rate_limit_backoff_ms,
        "target_total_request_steps": total_requests_steps,
        "buyers": buyers,
        "sellers": sellers,
        "seed_users": seed_users,
        "seed_retry_max": seed_retry_max,
        "seed_retry_backoff_ms": seed_retry_backoff_ms,
        "seed_request_delay_ms": seed_request_delay_ms,
        "health_before": health_before,
        "ready_before": ready_before,
        "health_after": health_after,
        "ready_after": ready_after,
        "runs": runs,
        "failed_step": failed_step,
        "failure_error": failed_error,
        "turning_point": turning_point
        or {
            "reason": "not_reached_within_test_range",
            "tested_steps": total_requests_steps,
        },
        "artifacts": {
            "summary_json": str(artifact_root / "summary.json"),
            "summary_md": str(artifact_root / "summary.md"),
            "metrics_prometheus": str(metrics_path),
            "health_json": str(health_path),
            "ready_json": str(ready_path),
            "raw_dir": str(raw_dir),
        },
    }

    summary_json_path = artifact_root / "summary.json"
    summary_md_path = artifact_root / "summary.md"
    write_json(summary_json_path, summary)

    lines = [
        "# K8s Staircase Benchmark",
        "",
        f"- Status: {summary['status']}",
        f"- Started At: {started_at}",
        f"- Completed At: {completed_at}",
        f"- Base URL: {base_url}",
        f"- Market: {market}",
        f"- Buyer / Seller Count: {buyer_count} / {seller_count}",
        f"- Pair Concurrency / Rate / Stagger: {pair_concurrency} / {rate_limit_per_second} / {request_stagger_ms}",
        f"- Turning Point: {json.dumps(summary['turning_point'], ensure_ascii=True)}",
        "",
    ]
    if failed_step is not None:
        lines.extend(
            [
                "## Failure",
                "",
                f"- Failed Step: {failed_step['prefix']}",
                f"- Error: {failed_error}",
                f"- Stdout Log: {failed_step['stdout_log']}",
                f"- Stderr Log: {failed_step['stderr_log']}",
                "",
            ]
        )
    lines.extend(
        [
            "## Steps",
            "",
        ]
    )
    for run in runs:
        lines.append(
            "- {0} req: success={1}/{2} ({3}%), client_p99_us={4}, roundtrip_p99_us={5}, retry_backoff_p99_us={6}, matching_core_p99_us={7}".format(
                run["target_total_requests"],
                run["success_count"],
                run["total_requests"],
                run["success_rate"],
                run["client_latency_us"]["p99"],
                run["http_roundtrip_us"]["p99"],
                run["retry_backoff_us"]["p99"],
                run["matching_core_us"]["p99"],
            )
        )
    summary_md_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    print(json.dumps(summary, indent=2))
    if failed_step is not None:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
