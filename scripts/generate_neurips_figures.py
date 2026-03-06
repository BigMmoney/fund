from __future__ import annotations

import json
from pathlib import Path


ROOT = Path(r"D:\pre_trading")
DATA_PATH = ROOT / "docs" / "benchmarks" / "simulator_multiseed_profile.json"
ABLATION_PATH = ROOT / "docs" / "benchmarks" / "simulator_ablation_profile.json"
AGENT_SWEEP_PATH = ROOT / "docs" / "benchmarks" / "simulator_agent_ablation_profile.json"
GRID_SWEEP_PATH = ROOT / "docs" / "benchmarks" / "simulator_parameter_grid_profile.json"
CUBE_SWEEP_PATH = ROOT / "docs" / "benchmarks" / "simulator_parameter_cube_profile.json"
HYPER_SWEEP_PATH = ROOT / "docs" / "benchmarks" / "simulator_parameter_hypercube_profile.json"
FIG_DIR = ROOT / "docs" / "neurips_track" / "figures"


def load_results() -> list[dict]:
    payload = json.loads(DATA_PATH.read_text(encoding="utf-8"))
    return payload["results"]


def load_named_results(path: Path) -> list[dict]:
    payload = json.loads(path.read_text(encoding="utf-8"))
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
        ".err{stroke:#dbeafe; stroke-width:1.5;}"
        "</style>\n"
        f'<rect class="bg" x="0" y="0" width="{width}" height="{height}" rx="18"/>\n'
        f"{body}\n</svg>\n"
    )


def bar_chart_with_ci(
    title: str,
    series: list[tuple[str, str, list[float], list[float]]],
    categories: list[str],
    out_path: Path,
    y_label: str,
) -> None:
    width, height = 980, 540
    left, right, top, bottom = 90, 40, 70, 90
    chart_w = width - left - right
    chart_h = height - top - bottom
    max_value = max(max(v + c for v, c in zip(values, cis)) for _, _, values, cis in series)
    min_value = min(min(v - c for v, c in zip(values, cis)) for _, _, values, cis in series)
    max_value = max(max_value * 1.12, 1.0)
    min_value = min(min_value * 1.12, 0.0)
    value_span = max(max_value - min_value, 1e-6)
    group_w = chart_w / len(categories)
    bar_w = group_w / (len(series) + 1)
    body = [f'<text class="title" x="{left}" y="38">{title}</text>']
    body.append(f'<text class="label" x="{left}" y="{height-20}">{y_label}</text>')

    for i in range(6):
        y = top + chart_h * i / 5
        body.append(f'<line class="grid" x1="{left}" y1="{y:.1f}" x2="{left+chart_w}" y2="{y:.1f}"/>')
        value = max_value - value_span * i / 5
        body.append(f'<text class="label" x="12" y="{y+4:.1f}">{value:.1f}</text>')

    body.append(f'<line class="axis" x1="{left}" y1="{top}" x2="{left}" y2="{top+chart_h}"/>')
    zero_y = top + chart_h - ((0 - min_value) / value_span) * chart_h
    body.append(f'<line class="axis" x1="{left}" y1="{zero_y:.1f}" x2="{left+chart_w}" y2="{zero_y:.1f}"/>')

    for idx, category in enumerate(categories):
        base_x = left + idx * group_w
        for s_idx, (_, color, values, cis) in enumerate(series):
            value = values[idx]
            ci = cis[idx]
            bar_h = abs(value) / value_span * chart_h
            x = base_x + bar_w * (s_idx + 0.5)
            if value >= 0:
                y = zero_y - bar_h
            else:
                y = zero_y
            cx = x + bar_w * 0.4
            body.append(f'<rect x="{x:.1f}" y="{y:.1f}" width="{bar_w*0.8:.1f}" height="{bar_h:.1f}" fill="{color}" rx="8"/>')
            hi_y = top + chart_h - (((value + ci) - min_value) / value_span) * chart_h
            lo_y = top + chart_h - (((value - ci) - min_value) / value_span) * chart_h
            body.append(f'<line class="err" x1="{cx:.1f}" y1="{hi_y:.1f}" x2="{cx:.1f}" y2="{lo_y:.1f}"/>')
            body.append(f'<line class="err" x1="{cx-7:.1f}" y1="{hi_y:.1f}" x2="{cx+7:.1f}" y2="{hi_y:.1f}"/>')
            body.append(f'<line class="err" x1="{cx-7:.1f}" y1="{lo_y:.1f}" x2="{cx+7:.1f}" y2="{lo_y:.1f}"/>')
            label_y = hi_y - 10 if value >= 0 else lo_y + 18
            label_y = max(top + 12, min(top + chart_h - 8, label_y))
            body.append(f'<text class="value" x="{cx:.1f}" y="{label_y:.1f}" text-anchor="middle">{value:.2f}</text>')
        body.append(f'<text class="label" x="{base_x+group_w/2:.1f}" y="{top+chart_h+28}" text-anchor="middle">{category}</text>')

    legend_x = width - 250
    for idx, (name, color, _, _) in enumerate(series):
        ly = 34 + idx * 22
        body.append(f'<rect x="{legend_x}" y="{ly-12}" width="14" height="14" fill="{color}" rx="3"/>')
        body.append(f'<text class="legend" x="{legend_x+22}" y="{ly}">{name}</text>')

    write_svg(out_path, wrap_svg(width, height, "\n".join(body)))


def line_chart_with_ci(
    title: str,
    series: list[tuple[str, str, list[float], list[float]]],
    categories: list[str],
    out_path: Path,
    y_label: str,
) -> None:
    width, height = 980, 540
    left, right, top, bottom = 90, 40, 70, 90
    chart_w = width - left - right
    chart_h = height - top - bottom
    max_value = max(max(v + c for v, c in zip(values, cis)) for _, _, values, cis in series) * 1.12
    step_x = chart_w / (len(categories) - 1)
    body = [f'<text class="title" x="{left}" y="38">{title}</text>']
    body.append(f'<text class="label" x="{left}" y="{height-20}">{y_label}</text>')

    for i in range(6):
        y = top + chart_h * i / 5
        body.append(f'<line class="grid" x1="{left}" y1="{y:.1f}" x2="{left+chart_w}" y2="{y:.1f}"/>')
        value = max_value * (5 - i) / 5
        body.append(f'<text class="label" x="12" y="{y+4:.1f}">{value:.0f}</text>')

    body.append(f'<line class="axis" x1="{left}" y1="{top}" x2="{left}" y2="{top+chart_h}"/>')
    body.append(f'<line class="axis" x1="{left}" y1="{top+chart_h}" x2="{left+chart_w}" y2="{top+chart_h}"/>')

    for idx, category in enumerate(categories):
        x = left + idx * step_x
        body.append(f'<text class="label" x="{x:.1f}" y="{top+chart_h+28}" text-anchor="middle">{category}</text>')

    legend_x = width - 250
    for idx, (name, color, values, cis) in enumerate(series):
        points = []
        for v_idx, value in enumerate(values):
            x = left + v_idx * step_x
            y = top + chart_h - (value / max_value) * chart_h
            points.append(f"{x:.1f},{y:.1f}")
        body.append(f'<polyline fill="none" stroke="{color}" stroke-width="4" points="{" ".join(points)}"/>')
        for v_idx, value in enumerate(values):
            ci = cis[v_idx]
            x = left + v_idx * step_x
            y = top + chart_h - (value / max_value) * chart_h
            hi_y = top + chart_h - ((value + ci) / max_value) * chart_h
            lo_y = top + chart_h - ((max(value - ci, 0)) / max_value) * chart_h
            body.append(f'<line class="err" x1="{x:.1f}" y1="{hi_y:.1f}" x2="{x:.1f}" y2="{lo_y:.1f}"/>')
            body.append(f'<line class="err" x1="{x-7:.1f}" y1="{hi_y:.1f}" x2="{x+7:.1f}" y2="{hi_y:.1f}"/>')
            body.append(f'<line class="err" x1="{x-7:.1f}" y1="{lo_y:.1f}" x2="{x+7:.1f}" y2="{lo_y:.1f}"/>')
            body.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="5" fill="{color}"/>')
            body.append(f'<text class="value" x="{x:.1f}" y="{max(hi_y-10, top+12):.1f}" text-anchor="middle">{value:.1f}</text>')
        ly = 34 + idx * 22
        body.append(f'<line x1="{legend_x}" y1="{ly-6}" x2="{legend_x+16}" y2="{ly-6}" stroke="{color}" stroke-width="4"/>')
        body.append(f'<text class="legend" x="{legend_x+24}" y="{ly}">{name}</text>')

    write_svg(out_path, wrap_svg(width, height, "\n".join(body)))


def heat_color(value: float, min_value: float, max_value: float) -> str:
    if max_value <= min_value:
        ratio = 0.5
    else:
        ratio = (value - min_value) / (max_value - min_value)
    ratio = max(0.0, min(1.0, ratio))
    r = int(22 + ratio * 220)
    g = int(74 + (1 - ratio) * 110)
    b = int(120 + (1 - ratio) * 80)
    return f"#{r:02x}{g:02x}{b:02x}"


def heatmap_chart(
    title: str,
    values: dict[tuple[int, int], tuple[float, float]],
    x_values: list[int],
    y_values: list[int],
    out_path: Path,
    footer: str,
) -> None:
    width, height = 880, 540
    left, top = 150, 90
    cell_w, cell_h = 150, 90
    metrics = [metric for metric, _ in values.values()]
    min_metric = min(metrics)
    max_metric = max(metrics)
    body = [f'<text class="title" x="{left}" y="38">{title}</text>']
    body.append(f'<text class="label" x="{left}" y="{height-20}">{footer}</text>')
    for idx, x_val in enumerate(x_values):
        x = left + idx * cell_w
        body.append(f'<text class="legend" x="{x + cell_w/2:.1f}" y="{top-18}" text-anchor="middle">Maker x{x_val}</text>')
    for idy, y_val in enumerate(y_values):
        y = top + idy * cell_h
        body.append(f'<text class="legend" x="{left-20}" y="{y + cell_h/2 + 4:.1f}" text-anchor="end">Arb x{y_val}</text>')
        for idx, x_val in enumerate(x_values):
            x = left + idx * cell_w
            mean, ci = values[(y_val, x_val)]
            color = heat_color(mean, min_metric, max_metric)
            body.append(f'<rect x="{x}" y="{y}" width="{cell_w-8}" height="{cell_h-8}" fill="{color}" rx="12" opacity="0.88"/>')
            body.append(f'<text class="value" x="{x + (cell_w-8)/2:.1f}" y="{y + 35:.1f}" text-anchor="middle">{mean:.1f}</text>')
            body.append(f'<text class="label" x="{x + (cell_w-8)/2:.1f}" y="{y + 58:.1f}" text-anchor="middle">+/- {ci:.1f}</text>')
    write_svg(out_path, wrap_svg(width, height, "\n".join(body)))


def generate() -> None:
    results = load_results()
    ablations = load_named_results(ABLATION_PATH)
    agent_sweeps = load_named_results(AGENT_SWEEP_PATH)
    grid_sweep = load_named_results(GRID_SWEEP_PATH)
    cube_sweep = load_named_results(CUBE_SWEEP_PATH)
    hyper_sweep = load_named_results(HYPER_SWEEP_PATH)
    categories = []
    for r in results:
        label = r["name"].replace("Immediate-Surrogate", "Immediate")
        label = label.replace("SpeedBump-", "SpeedBump ")
        label = label.replace("Adaptive-", "Adaptive ")
        label = label.replace("FBA-", "")
        label = label.replace("-Stress", " Stress")
        categories.append(label)

    bar_chart_with_ci(
        "Throughput Comparison (95% CI)",
        [
            ("Orders/s", "#2dd4bf", [r["mean_orders_per_sec"] for r in results], [r["ci95_orders_per_sec"] for r in results]),
            ("Fills/s", "#38bdf8", [r["mean_fills_per_sec"] for r in results], [r["ci95_fills_per_sec"] for r in results]),
        ],
        categories,
        FIG_DIR / "throughput.svg",
        "Mean throughput",
    )

    line_chart_with_ci(
        "Latency Profile (95% CI)",
        [
            ("p50", "#34d399", [r["mean_p50_latency_ms"] for r in results], [r["ci95_p50_latency_ms"] for r in results]),
            ("p95", "#f59e0b", [r["mean_p95_latency_ms"] for r in results], [r["ci95_p95_latency_ms"] for r in results]),
            ("p99", "#fb7185", [r["mean_p99_latency_ms"] for r in results], [r["ci95_p99_latency_ms"] for r in results]),
        ],
        categories,
        FIG_DIR / "latency.svg",
        "Latency (ms)",
    )

    bar_chart_with_ci(
        "Fairness Proxy Comparison (95% CI)",
        [
            (
                "Queue Advantage x1000",
                "#a78bfa",
                [r["mean_queue_priority_advantage"] * 1000 for r in results],
                [r["ci95_queue_priority_advantage"] * 1000 for r in results],
            ),
            (
                "Arb Profit / 10",
                "#f472b6",
                [r["mean_latency_arbitrage_profit"] / 10 for r in results],
                [r["ci95_latency_arbitrage_profit"] / 10 for r in results],
            ),
        ],
        categories,
        FIG_DIR / "fairness.svg",
        "Scaled proxy value",
    )

    bar_chart_with_ci(
        "Welfare and Behavior Metrics (95% CI)",
        [
            (
                "Retail Surplus / unit",
                "#34d399",
                [r["mean_retail_surplus_per_unit"] for r in results],
                [r["ci95_retail_surplus_per_unit"] for r in results],
            ),
            (
                "Retail Adverse Rate",
                "#f59e0b",
                [r["mean_retail_adverse_selection_rate"] for r in results],
                [r["ci95_retail_adverse_selection_rate"] for r in results],
            ),
        ],
        categories,
        FIG_DIR / "welfare.svg",
        "Per-unit welfare and adverse-selection rate",
    )

    bar_chart_with_ci(
        "Mechanism Ablation Snapshot",
        [
            (
                "Orders/s",
                "#2dd4bf",
                [r["mean_orders_per_sec"] for r in ablations],
                [0.0 for _ in ablations],
            ),
            (
                "Arb Profit / 10",
                "#f472b6",
                [r["mean_latency_arbitrage_profit"] / 10 for r in ablations],
                [0.0 for _ in ablations],
            ),
        ],
        [r["name"].replace("Ablation-", "") for r in ablations],
        FIG_DIR / "ablation.svg",
        "Orders/s and scaled arb profit",
    )

    bar_chart_with_ci(
        "Agent and Workload Sweep Snapshot",
        [
            (
                "Orders/s",
                "#38bdf8",
                [r["mean_orders_per_sec"] for r in agent_sweeps],
                [0.0 for _ in agent_sweeps],
            ),
            (
                "p99 / 2",
                "#f59e0b",
                [r["mean_p99_latency_ms"] / 2 for r in agent_sweeps],
                [0.0 for _ in agent_sweeps],
            ),
        ],
        [
            r["name"]
            .replace("AgentAblation-", "")
            .replace("AgentSweep-", "")
            for r in agent_sweeps
        ],
        FIG_DIR / "agent_sweeps.svg",
        "Orders/s and scaled p99 latency",
    )

    grid_p99 = {
        (r["arbitrageur_intensity_multiplier"], r["maker_quote_width_multiplier"]): (
            r["mean_p99_latency_ms"],
            r["ci95_p99_latency_ms"],
        )
        for r in grid_sweep
    }
    grid_arb = {
        (r["arbitrageur_intensity_multiplier"], r["maker_quote_width_multiplier"]): (
            r["mean_latency_arbitrage_profit"],
            r["ci95_latency_arbitrage_profit"],
        )
        for r in grid_sweep
    }
    x_values = sorted({r["maker_quote_width_multiplier"] for r in grid_sweep})
    y_values = sorted({r["arbitrageur_intensity_multiplier"] for r in grid_sweep})

    heatmap_chart(
        "Parameter Grid: p99 Latency Heatmap",
        grid_p99,
        x_values,
        y_values,
        FIG_DIR / "grid_p99_heatmap.svg",
        "Rows: arbitrageur intensity, columns: maker quote width",
    )

    heatmap_chart(
        "Parameter Grid: Arbitrage Profit Heatmap",
        grid_arb,
        x_values,
        y_values,
        FIG_DIR / "grid_arb_heatmap.svg",
        "Rows: arbitrageur intensity, columns: maker quote width",
    )

    retail_levels = sorted({r["retail_intensity_multiplier"] for r in cube_sweep})
    informed_levels = sorted({r["informed_intensity_multiplier"] for r in cube_sweep})
    maker_levels = sorted({r["maker_quote_width_multiplier"] for r in cube_sweep})
    for retail in retail_levels:
        cube_slice = [r for r in cube_sweep if r["retail_intensity_multiplier"] == retail]
        cube_p99 = {
            (r["informed_intensity_multiplier"], r["maker_quote_width_multiplier"]): (
                r["mean_p99_latency_ms"],
                r["ci95_p99_latency_ms"],
            )
            for r in cube_slice
        }
        cube_arb = {
            (r["informed_intensity_multiplier"], r["maker_quote_width_multiplier"]): (
                r["mean_latency_arbitrage_profit"],
                r["ci95_latency_arbitrage_profit"],
            )
            for r in cube_slice
        }
        heatmap_chart(
            f"Parameter Cube: p99 Heatmap (Retail x{retail})",
            cube_p99,
            maker_levels,
            informed_levels,
            FIG_DIR / f"cube_p99_retail{retail}.svg",
            "Rows: informed-flow intensity, columns: maker quote width",
        )
        heatmap_chart(
            f"Parameter Cube: Arbitrage Heatmap (Retail x{retail})",
            cube_arb,
            maker_levels,
            informed_levels,
            FIG_DIR / f"cube_arb_retail{retail}.svg",
            "Rows: informed-flow intensity, columns: maker quote width",
        )

    hyper_arb_levels = sorted({r["arbitrageur_intensity_multiplier"] for r in hyper_sweep})
    hyper_retail_levels = sorted({r["retail_intensity_multiplier"] for r in hyper_sweep})
    hyper_maker_levels = sorted({r["maker_quote_width_multiplier"] for r in hyper_sweep})
    fixed_informed = 2
    for arb in hyper_arb_levels:
        hyper_slice = [
            r for r in hyper_sweep
            if r["arbitrageur_intensity_multiplier"] == arb and r["informed_intensity_multiplier"] == fixed_informed
        ]
        hyper_p99 = {
            (r["retail_intensity_multiplier"], r["maker_quote_width_multiplier"]): (
                r["mean_p99_latency_ms"],
                r["ci95_p99_latency_ms"],
            )
            for r in hyper_slice
        }
        hyper_welfare = {
            (r["retail_intensity_multiplier"], r["maker_quote_width_multiplier"]): (
                r["mean_surplus_transfer_gap"],
                r["ci95_surplus_transfer_gap"],
            )
            for r in hyper_slice
        }
        heatmap_chart(
            f"Hypercube: p99 Heatmap (Arb x{arb}, Informed x{fixed_informed})",
            hyper_p99,
            hyper_maker_levels,
            hyper_retail_levels,
            FIG_DIR / f"hyper_p99_arb{arb}.svg",
            "Rows: retail-flow intensity, columns: maker quote width",
        )
        heatmap_chart(
            f"Hypercube: Welfare Gap (Arb x{arb}, Informed x{fixed_informed})",
            hyper_welfare,
            hyper_maker_levels,
            hyper_retail_levels,
            FIG_DIR / f"hyper_welfare_arb{arb}.svg",
            "Rows: retail-flow intensity, columns: maker quote width",
        )


if __name__ == "__main__":
    generate()
