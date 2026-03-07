from __future__ import annotations

import argparse
import json
import ssl
import time
import urllib.parse
import urllib.request
from datetime import UTC, datetime, timedelta
from pathlib import Path
from urllib.error import HTTPError, URLError


ROOT = Path(r"D:\pre_trading")
DEFAULT_BASE_URLS = [
    "https://data-api.binance.vision",
    "https://api1.binance.com",
    "https://api2.binance.com",
]
SSL_CONTEXT = ssl.create_default_context()


def load_config(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def http_get_json(path: str, params: dict[str, object], base_urls: list[str]) -> object:
    query = urllib.parse.urlencode(params)
    last_error: Exception | None = None
    for base_url in base_urls:
        url = f"{base_url}{path}?{query}" if query else f"{base_url}{path}"
        for attempt in range(3):
            req = urllib.request.Request(
                url,
                headers={
                    "User-Agent": "pre_trading-calibration/1.0",
                    "Accept": "application/json",
                },
            )
            try:
                with urllib.request.urlopen(req, timeout=30, context=SSL_CONTEXT) as resp:
                    return json.loads(resp.read().decode("utf-8"))
            except HTTPError:
                raise
            except (URLError, TimeoutError, ConnectionError, ssl.SSLError) as exc:
                last_error = exc
                time.sleep(0.4 * (attempt + 1))
    if last_error is not None:
        raise last_error
    raise RuntimeError(f"request failed for path={path}")


def utc_ms(dt: datetime) -> int:
    return int(dt.timestamp() * 1000)


def download_agg_trades(symbol: str, start_ms: int, end_ms: int, limit: int, base_urls: list[str]) -> list[dict]:
    rows: list[dict] = []
    cursor = start_ms
    while cursor < end_ms:
        batch = http_get_json(
            "/api/v3/aggTrades",
            {
                "symbol": symbol,
                "startTime": cursor,
                "endTime": end_ms,
                "limit": limit,
            },
            base_urls,
        )
        if not isinstance(batch, list) or not batch:
            break
        rows.extend(batch)
        last_ts = int(batch[-1]["T"])
        next_cursor = last_ts + 1
        if next_cursor <= cursor:
            break
        cursor = next_cursor
        if len(batch) < limit:
            break
        time.sleep(0.15)
    return rows


def download_klines(symbol: str, interval: str, start_ms: int, end_ms: int, base_urls: list[str]) -> list[list]:
    rows: list[list] = []
    cursor = start_ms
    while cursor < end_ms:
        batch = http_get_json(
            "/api/v3/klines",
            {
                "symbol": symbol,
                "interval": interval,
                "startTime": cursor,
                "endTime": end_ms,
                "limit": 1000,
            },
            base_urls,
        )
        if not isinstance(batch, list) or not batch:
            break
        rows.extend(batch)
        last_open = int(batch[-1][0])
        next_cursor = last_open + 1
        if next_cursor <= cursor:
            break
        cursor = next_cursor
        if len(batch) < 1000:
            break
        time.sleep(0.15)
    return rows


def download_depth_snapshots(
    symbol: str, limit: int, snapshots: int, interval_sec: int, base_urls: list[str]
) -> list[dict]:
    rows: list[dict] = []
    for idx in range(snapshots):
        payload = http_get_json(
            "/api/v3/depth",
            {
                "symbol": symbol,
                "limit": limit,
            },
            base_urls,
        )
        if not isinstance(payload, dict):
            break
        rows.append(
            {
                "captured_at": datetime.now(UTC).isoformat(),
                "snapshot_index": idx,
                "payload": payload,
            }
        )
        if idx + 1 < snapshots:
            time.sleep(interval_sec)
    return rows


def write_json(path: Path, payload: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description="Download Binance Spot data for calibration.")
    parser.add_argument("--config", required=True, help="Path to calibration config JSON")
    args = parser.parse_args()

    config_path = (ROOT / args.config).resolve()
    config = load_config(config_path)
    base_urls = list(config.get("base_urls", DEFAULT_BASE_URLS))

    lookback = int(config["lookback_minutes"])
    end_dt = datetime.now(UTC)
    start_dt = end_dt - timedelta(minutes=lookback)
    start_ms = utc_ms(start_dt)
    end_ms = utc_ms(end_dt)

    output_dir = ROOT / config["output_dir"]
    profile_name = config["profile_name"]

    manifest = {
        "profile_name": profile_name,
        "venue": config["venue"],
        "downloaded_at": datetime.now(UTC).isoformat(),
        "base_urls": base_urls,
        "start_time_utc": start_dt.isoformat(),
        "end_time_utc": end_dt.isoformat(),
        "symbols": [],
    }

    for symbol in config["symbols"]:
        symbol_dir = output_dir / symbol
        agg_trades = download_agg_trades(
            symbol,
            start_ms,
            end_ms,
            int(config["agg_trade_page_limit"]),
            base_urls,
        )
        klines = download_klines(symbol, str(config["kline_interval"]), start_ms, end_ms, base_urls)
        depth_snapshots = download_depth_snapshots(
            symbol,
            int(config["depth_limit"]),
            int(config["depth_snapshots"]),
            int(config["depth_snapshot_interval_sec"]),
            base_urls,
        )

        write_json(symbol_dir / "agg_trades.json", agg_trades)
        write_json(symbol_dir / "klines.json", klines)
        write_json(symbol_dir / "depth_snapshots.json", depth_snapshots)

        manifest["symbols"].append(
            {
                "symbol": symbol,
                "agg_trades": len(agg_trades),
                "klines": len(klines),
                "depth_snapshots": len(depth_snapshots),
            }
        )

    write_json(output_dir / "manifest.json", manifest)
    print(f"downloaded profile={profile_name} into {output_dir}")


if __name__ == "__main__":
    main()
