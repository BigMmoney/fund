from __future__ import annotations

import json
from pathlib import Path
from typing import Iterable


ROOT = Path(r"D:\pre_trading")
DATA_PATH = ROOT / "docs" / "benchmarks" / "simulator_multiseed_profile.json"
FIG_DIR = ROOT / "docs" / "neurips_track" / "figures"


def load_results() -> list[dict]:
    payload = json.loads(DATA_PATH.read_text(encoding="utf-8"))
    return payload["results"]


def write_svg(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def wrap_svg(width: int, height: int, body: str) -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}" role="img">\n'
        "<style>"
        ".title{font:700 20px Arial; fill:#f3f7ff;}"
        ".label{font:12px Arial; fill:#cbd8f0;}"
        ".value{font:11px Arial; fill:#eef5ff;}"
        ".axis{stroke:#4d6387; stroke-width:1;}"
        ".grid{stroke:#20324d; stroke-width:1; opacity:.8;}"
        ".legend{font:12px Arial; fill:#d7e7ff;}"
        ".bg{fill:#071423;}"
        "</style>\n"
        f'<rect class="bg" x="0" y="0" width="{width}" height="{height}" rx="18"/>\n'
        f"{body}\n</svg>\n"
    )


def bar_chart(
    title: str,
    series: list[tuple[str, str, list[float]]],
    categories: list[str],
    out_path: Path,
    y_label: str,
) -> None:
    width, height = 960, 520
    left, right, top, bottom = 90, 40, 70, 90
    chart_w = width - left - right
    chart_h = height - top - bottom
    max_value = max(max(values) for _, _, values in series)
    max_value *= 1.12
    group_w = chart_w / len(categories)
    bar_w = group_w / (len(series) + 1)
    body = [f'<text class="title" x="{left}" y="38">{title}</text>']
    body.append(f'<text class="label" x="{left}" y="{height-20}">{y_label}</text>')

    for i in range(6):
        y = top + chart_h * i / 5
        body.append(f'<line class="grid" x1="{left}" y1="{y:.1f}" x2="{left+chart_w}" y2="{y:.1f}"/>')
        value = max_value * (5 - i) / 5
        body.append(f'<text class="label" x="18" y="{y+4:.1f}">{value:.0f}</text>')

    body.append(f'<line class="axis" x1="{left}" y1="{top}" x2="{left}" y2="{top+chart_h}"/>')
    body.append(f'<line class="axis" x1="{left}" y1="{top+chart_h}" x2="{left+chart_w}" y2="{top+chart_h}"/>')

    for idx, category in enumerate(categories):
        base_x = left + idx * group_w
        for s_idx, (_, color, values) in enumerate(series):
            value = values[idx]
            bar_h = (value / max_value) * chart_h
            x = base_x + bar_w * (s_idx + 0.5)
            y = top + chart_h - bar_h
            body.append(
                f'<rect x="{x:.1f}" y="{y:.1f}" width="{bar_w*0.8:.1f}" height="{bar_h:.1f}" fill="{color}" rx="8"/>'
            )
            body.append(
                f'<text class="value" x="{x+bar_w*0.4:.1f}" y="{max(y-8, top+12):.1f}" text-anchor="middle">{value:.1f}</text>'
            )
        body.append(
            f'<text class="label" x="{base_x+group_w/2:.1f}" y="{top+chart_h+28}" text-anchor="middle">{category}</text>'
        )

    legend_x = width - 230
    for idx, (name, color, _) in enumerate(series):
        ly = 34 + idx * 22
        body.append(f'<rect x="{legend_x}" y="{ly-12}" width="14" height="14" fill="{color}" rx="3"/>')
        body.append(f'<text class="legend" x="{legend_x+22}" y="{ly}">{name}</text>')

    write_svg(out_path, wrap_svg(width, height, "\n".join(body)))


def multi_line_chart(
    title: str,
    series: list[tuple[str, str, list[float]]],
    categories: list[str],
    out_path: Path,
    y_label: str,
) -> None:
    width, height = 960, 520
    left, right, top, bottom = 90, 40, 70, 90
    chart_w = width - left - right
    chart_h = height - top - bottom
    max_value = max(max(values) for _, _, values in series) * 1.12
    step_x = chart_w / (len(categories) - 1)
    body = [f'<text class="title" x="{left}" y="38">{title}</text>']
    body.append(f'<text class="label" x="{left}" y="{height-20}">{y_label}</text>')

    for i in range(6):
        y = top + chart_h * i / 5
        body.append(f'<line class="grid" x1="{left}" y1="{y:.1f}" x2="{left+chart_w}" y2="{y:.1f}"/>')
        value = max_value * (5 - i) / 5
        body.append(f'<text class="label" x="18" y="{y+4:.1f}">{value:.0f}</text>')

    body.append(f'<line class="axis" x1="{left}" y1="{top}" x2="{left}" y2="{top+chart_h}"/>')
    body.append(f'<line class="axis" x1="{left}" y1="{top+chart_h}" x2="{left+chart_w}" y2="{top+chart_h}"/>')

    for idx, category in enumerate(categories):
        x = left + idx * step_x
        body.append(f'<text class="label" x="{x:.1f}" y="{top+chart_h+28}" text-anchor="middle">{category}</text>')

    legend_x = width - 250
    for idx, (name, color, values) in enumerate(series):
        points = []
        for v_idx, value in enumerate(values):
            x = left + v_idx * step_x
            y = top + chart_h - (value / max_value) * chart_h
            points.append(f"{x:.1f},{y:.1f}")
        body.append(f'<polyline fill="none" stroke="{color}" stroke-width="4" points="{" ".join(points)}"/>')
        for v_idx, value in enumerate(values):
            x = left + v_idx * step_x
            y = top + chart_h - (value / max_value) * chart_h
            body.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="5" fill="{color}"/>')
            body.append(f'<text class="value" x="{x:.1f}" y="{max(y-10, top+12):.1f}" text-anchor="middle">{value:.1f}</text>')
        ly = 34 + idx * 22
        body.append(f'<line x1="{legend_x}" y1="{ly-6}" x2="{legend_x+16}" y2="{ly-6}" stroke="{color}" stroke-width="4"/>')
        body.append(f'<text class="legend" x="{legend_x+24}" y="{ly}">{name}</text>')

    write_svg(out_path, wrap_svg(width, height, "\n".join(body)))


def generate() -> None:
    results = load_results()
    categories = [r["name"].replace("Immediate-Surrogate", "Immediate").replace("FBA-", "").replace("-Stress", " Stress") for r in results]

    bar_chart(
        "Throughput Comparison",
        [
            ("Orders/s", "#2dd4bf", [r["mean_orders_per_sec"] for r in results]),
            ("Fills/s", "#38bdf8", [r["mean_fills_per_sec"] for r in results]),
        ],
        categories,
        FIG_DIR / "throughput.svg",
        "Mean throughput",
    )

    multi_line_chart(
        "Latency Profile",
        [
            ("p50", "#34d399", [r["mean_p50_latency_ms"] for r in results]),
            ("p95", "#f59e0b", [r["mean_p95_latency_ms"] for r in results]),
            ("p99", "#fb7185", [r["mean_p99_latency_ms"] for r in results]),
        ],
        categories,
        FIG_DIR / "latency.svg",
        "Latency (ms)",
    )

    bar_chart(
        "Fairness Proxy Comparison",
        [
            ("Queue Advantage x1000", "#a78bfa", [r["mean_queue_priority_advantage"] * 1000 for r in results]),
            ("Arb Profit / 10", "#f472b6", [r["mean_latency_arbitrage_profit"] / 10 for r in results]),
        ],
        categories,
        FIG_DIR / "fairness.svg",
        "Scaled proxy value",
    )


if __name__ == "__main__":
    generate()
