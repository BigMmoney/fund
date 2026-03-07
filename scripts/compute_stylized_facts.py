from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from statistics import mean, median


ROOT = Path(r"D:\pre_trading")


def percentile(values: list[float], q: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    idx = min(len(ordered) - 1, max(0, int(round((len(ordered) - 1) * q))))
    return float(ordered[idx])


def autocorr(values: list[float], lag: int) -> float:
    if len(values) <= lag:
        return 0.0
    left = values[:-lag]
    right = values[lag:]
    mean_left = mean(left)
    mean_right = mean(right)
    num = sum((a - mean_left) * (b - mean_right) for a, b in zip(left, right))
    den_left = sum((a - mean_left) ** 2 for a in left)
    den_right = sum((b - mean_right) ** 2 for b in right)
    if den_left <= 0 or den_right <= 0:
        return 0.0
    return num / math.sqrt(den_left * den_right)


def load_json(path: Path) -> object:
    return json.loads(path.read_text(encoding="utf-8"))


def summarize_symbol(symbol_dir: Path) -> dict:
    agg = load_json(symbol_dir / "agg_trades.json")
    klines = load_json(symbol_dir / "klines.json")
    depth = load_json(symbol_dir / "depth_snapshots.json")

    spreads: list[float] = []
    spread_bps: list[float] = []
    depth_profile: list[dict[str, float]] = []
    for snapshot in depth:
        payload = snapshot["payload"]
        bids = payload.get("bids", [])
        asks = payload.get("asks", [])
        if bids and asks:
            best_bid = float(bids[0][0])
            best_ask = float(asks[0][0])
            spread = best_ask - best_bid
            spreads.append(spread)
            mid = (best_bid + best_ask) / 2.0
            if mid > 0:
                spread_bps.append((spread / mid) * 10000.0)
            levels = min(5, len(bids), len(asks))
            for level in range(levels):
                if len(depth_profile) <= level:
                    depth_profile.append({"bid_qty": 0.0, "ask_qty": 0.0, "count": 0.0})
                depth_profile[level]["bid_qty"] += float(bids[level][1])
                depth_profile[level]["ask_qty"] += float(asks[level][1])
                depth_profile[level]["count"] += 1.0

    signs: list[float] = []
    inter_arrivals: list[float] = []
    impacts_source: list[tuple[float, float, float]] = []
    last_ts = None
    prices: list[float] = []
    for row in agg:
        sign = -1.0 if row["m"] else 1.0
        price = float(row["p"])
        qty = float(row["q"])
        ts = float(row["T"])
        signs.append(sign)
        prices.append(price)
        impacts_source.append((qty, price, sign))
        if last_ts is not None:
            inter_arrivals.append(ts - last_ts)
        last_ts = ts

    impact_curve: list[dict[str, float]] = []
    if len(impacts_source) >= 20:
        qtys = [q for q, _, _ in impacts_source]
        q25 = percentile(qtys, 0.25)
        q50 = percentile(qtys, 0.50)
        q75 = percentile(qtys, 0.75)
        buckets = {
            "q1": [],
            "q2": [],
            "q3": [],
            "q4": [],
        }
        horizon = 10
        for idx, (qty, price, sign) in enumerate(impacts_source[:-horizon]):
            future_price = impacts_source[idx + horizon][1]
            signed_impact = sign * (future_price - price)
            signed_impact_bps = (signed_impact / price) * 10000.0 if price > 0 else 0.0
            if qty <= q25:
                buckets["q1"].append((signed_impact, signed_impact_bps))
            elif qty <= q50:
                buckets["q2"].append((signed_impact, signed_impact_bps))
            elif qty <= q75:
                buckets["q3"].append((signed_impact, signed_impact_bps))
            else:
                buckets["q4"].append((signed_impact, signed_impact_bps))
        for bucket, values in buckets.items():
            raw_values = [value[0] for value in values]
            bps_values = [value[1] for value in values]
            impact_curve.append(
                {
                    "bucket": bucket,
                    "mean_signed_impact": mean(raw_values) if raw_values else 0.0,
                    "p90_signed_impact": percentile(raw_values, 0.90) if raw_values else 0.0,
                    "mean_signed_impact_bps": mean(bps_values) if bps_values else 0.0,
                    "p90_signed_impact_bps": percentile(bps_values, 0.90) if bps_values else 0.0,
                }
            )

    returns: list[float] = []
    for idx in range(1, len(klines)):
        prev_close = float(klines[idx - 1][4])
        close = float(klines[idx][4])
        if prev_close > 0:
            returns.append((close - prev_close) / prev_close)

    abs_returns = [abs(v) for v in returns]
    sq_returns = [v * v for v in returns]

    depth_summary = []
    first_bid_qty = 0.0
    first_ask_qty = 0.0
    for idx, level in enumerate(depth_profile, start=1):
        count = max(level["count"], 1.0)
        mean_bid_qty = level["bid_qty"] / count
        mean_ask_qty = level["ask_qty"] / count
        if idx == 1:
            first_bid_qty = max(mean_bid_qty, 1e-12)
            first_ask_qty = max(mean_ask_qty, 1e-12)
        depth_summary.append(
            {
                "level": idx,
                "mean_bid_qty": mean_bid_qty,
                "mean_ask_qty": mean_ask_qty,
                "bid_shape_ratio": mean_bid_qty / first_bid_qty if first_bid_qty > 0 else 0.0,
                "ask_shape_ratio": mean_ask_qty / first_ask_qty if first_ask_qty > 0 else 0.0,
            }
        )

    return {
        "symbol": symbol_dir.name,
        "trade_count": len(agg),
        "kline_count": len(klines),
        "depth_snapshot_count": len(depth),
        "spread_distribution": {
            "mean": mean(spreads) if spreads else 0.0,
            "median": median(spreads) if spreads else 0.0,
            "p90": percentile(spreads, 0.90) if spreads else 0.0,
        },
        "spread_distribution_bps": {
            "mean": mean(spread_bps) if spread_bps else 0.0,
            "median": median(spread_bps) if spread_bps else 0.0,
            "p90": percentile(spread_bps, 0.90) if spread_bps else 0.0,
        },
        "depth_profile": depth_summary,
        "order_sign_autocorrelation": {
            "lag_1": autocorr(signs, 1),
            "lag_5": autocorr(signs, 5),
            "lag_10": autocorr(signs, 10),
        },
        "inter_arrival_ms": {
            "mean": mean(inter_arrivals) if inter_arrivals else 0.0,
            "median": median(inter_arrivals) if inter_arrivals else 0.0,
            "p90": percentile(inter_arrivals, 0.90) if inter_arrivals else 0.0,
        },
        "volatility_clustering": {
            "abs_return_lag_1": autocorr(abs_returns, 1),
            "sq_return_lag_1": autocorr(sq_returns, 1),
        },
        "impact_curve": impact_curve,
    }


def profile_summary(symbols: list[dict]) -> dict:
    if not symbols:
        return {
            "symbol_count": 0,
            "trade_count_total": 0,
            "spread_mean_range": [0.0, 0.0],
            "spread_mean_bps_range": [0.0, 0.0],
            "order_sign_lag1_range": [0.0, 0.0],
            "inter_arrival_mean_ms_range": [0.0, 0.0],
            "volatility_abs_lag1_range": [0.0, 0.0],
            "top_bucket_mean_impact_range": [0.0, 0.0],
            "top_bucket_mean_impact_bps_range": [0.0, 0.0],
        }
    spread_means = [item["spread_distribution"]["mean"] for item in symbols]
    spread_means_bps = [item["spread_distribution_bps"]["mean"] for item in symbols]
    sign_lag1 = [item["order_sign_autocorrelation"]["lag_1"] for item in symbols]
    inter_arrival_means = [item["inter_arrival_ms"]["mean"] for item in symbols]
    vol_abs_lag1 = [item["volatility_clustering"]["abs_return_lag_1"] for item in symbols]
    top_bucket_impacts = [
        bucket["mean_signed_impact"]
        for item in symbols
        for bucket in item["impact_curve"]
        if bucket["bucket"] == "q4"
    ]
    top_bucket_impacts_bps = [
        bucket["mean_signed_impact_bps"]
        for item in symbols
        for bucket in item["impact_curve"]
        if bucket["bucket"] == "q4"
    ]
    return {
        "symbol_count": len(symbols),
        "trade_count_total": sum(item["trade_count"] for item in symbols),
        "spread_mean_range": [min(spread_means), max(spread_means)],
        "spread_mean_bps_range": [min(spread_means_bps), max(spread_means_bps)],
        "order_sign_lag1_range": [min(sign_lag1), max(sign_lag1)],
        "inter_arrival_mean_ms_range": [min(inter_arrival_means), max(inter_arrival_means)],
        "volatility_abs_lag1_range": [min(vol_abs_lag1), max(vol_abs_lag1)],
        "top_bucket_mean_impact_range": [
            min(top_bucket_impacts) if top_bucket_impacts else 0.0,
            max(top_bucket_impacts) if top_bucket_impacts else 0.0,
        ],
        "top_bucket_mean_impact_bps_range": [
            min(top_bucket_impacts_bps) if top_bucket_impacts_bps else 0.0,
            max(top_bucket_impacts_bps) if top_bucket_impacts_bps else 0.0,
        ],
    }


def write_outputs(profile_name: str, symbols: list[dict]) -> None:
    out_json = ROOT / "docs" / "benchmarks" / f"binance_spot_{profile_name}_facts.json"
    out_md = ROOT / "docs" / "benchmarks" / f"binance_spot_{profile_name}_facts.md"
    out_json.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "profile_name": profile_name,
        "summary": profile_summary(symbols),
        "symbols": symbols,
    }
    out_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

    summary = payload["summary"]
    lines = [
        f"# Binance Spot Stylized Facts ({profile_name})",
        "",
        "This artifact summarizes the first calibration bundle extracted from Binance Spot public market data.",
        "",
        "## Profile Summary",
        "",
        f"- symbols: `{summary['symbol_count']}`",
        f"- total trades: `{summary['trade_count_total']}`",
        f"- spread-mean range: `{summary['spread_mean_range'][0]:.6f}` -> `{summary['spread_mean_range'][1]:.6f}`",
        f"- spread-mean bps range: `{summary['spread_mean_bps_range'][0]:.4f}` -> `{summary['spread_mean_bps_range'][1]:.4f}`",
        f"- order-sign lag1 range: `{summary['order_sign_lag1_range'][0]:.4f}` -> `{summary['order_sign_lag1_range'][1]:.4f}`",
        f"- inter-arrival mean range ms: `{summary['inter_arrival_mean_ms_range'][0]:.2f}` -> `{summary['inter_arrival_mean_ms_range'][1]:.2f}`",
        f"- volatility abs-return lag1 range: `{summary['volatility_abs_lag1_range'][0]:.4f}` -> `{summary['volatility_abs_lag1_range'][1]:.4f}`",
        f"- top impact bucket mean range: `{summary['top_bucket_mean_impact_range'][0]:.8f}` -> `{summary['top_bucket_mean_impact_range'][1]:.8f}`",
        f"- top impact bucket mean bps range: `{summary['top_bucket_mean_impact_bps_range'][0]:.4f}` -> `{summary['top_bucket_mean_impact_bps_range'][1]:.4f}`",
        "",
    ]
    for item in symbols:
        lines.extend(
            [
                f"## {item['symbol']}",
                "",
                f"- trades: `{item['trade_count']}`",
                f"- klines: `{item['kline_count']}`",
                f"- depth snapshots: `{item['depth_snapshot_count']}`",
                f"- spread mean/median/p90: `{item['spread_distribution']['mean']:.6f}` / `{item['spread_distribution']['median']:.6f}` / `{item['spread_distribution']['p90']:.6f}`",
                f"- spread bps mean/median/p90: `{item['spread_distribution_bps']['mean']:.4f}` / `{item['spread_distribution_bps']['median']:.4f}` / `{item['spread_distribution_bps']['p90']:.4f}`",
                f"- order-sign autocorr lag1/lag5/lag10: `{item['order_sign_autocorrelation']['lag_1']:.4f}` / `{item['order_sign_autocorrelation']['lag_5']:.4f}` / `{item['order_sign_autocorrelation']['lag_10']:.4f}`",
                f"- inter-arrival mean/median/p90 ms: `{item['inter_arrival_ms']['mean']:.2f}` / `{item['inter_arrival_ms']['median']:.2f}` / `{item['inter_arrival_ms']['p90']:.2f}`",
                f"- volatility clustering abs/sq lag1: `{item['volatility_clustering']['abs_return_lag_1']:.4f}` / `{item['volatility_clustering']['sq_return_lag_1']:.4f}`",
                "",
                "| Depth Level | Mean Bid Qty | Mean Ask Qty | Bid Shape | Ask Shape |",
                "|---:|---:|---:|---:|---:|",
            ]
        )
        for level in item["depth_profile"]:
            lines.append(
                f"| {level['level']} | {level['mean_bid_qty']:.6f} | {level['mean_ask_qty']:.6f} | {level['bid_shape_ratio']:.4f} | {level['ask_shape_ratio']:.4f} |"
            )
        lines.append("")
        lines.append("| Impact Bucket | Mean Signed Impact | p90 Signed Impact | Mean Signed Impact (bps) | p90 Signed Impact (bps) |")
        lines.append("|---|---:|---:|---:|---:|")
        for bucket in item["impact_curve"]:
            lines.append(
                f"| {bucket['bucket']} | {bucket['mean_signed_impact']:.8f} | {bucket['p90_signed_impact']:.8f} | {bucket['mean_signed_impact_bps']:.4f} | {bucket['p90_signed_impact_bps']:.4f} |"
            )
        lines.append("")
    out_md.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description="Compute stylized facts from downloaded Binance Spot data.")
    parser.add_argument("--input-dir", required=True, help="Relative input directory under repo root")
    parser.add_argument("--profile-name", required=True, help="Profile name used for output artifacts")
    args = parser.parse_args()

    input_dir = (ROOT / args.input_dir).resolve()
    symbols = []
    for symbol_dir in sorted(p for p in input_dir.iterdir() if p.is_dir()):
        symbols.append(summarize_symbol(symbol_dir))
    write_outputs(args.profile_name, symbols)
    print(f"computed stylized facts for profile={args.profile_name}")


if __name__ == "__main__":
    main()
