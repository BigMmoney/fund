# Benchmark to Resume Translation

## One-line Statements (CV/Application Ready)

1. Achieved **123.6M ops/sec** for core numeric safety primitive (`safeDivide`) under a **200k-iteration micro-batch benchmark**.
2. Sustained **17.8k ops/sec** in `StableList.update` with **128-item burst batches**, validating deterministic throughput under event spikes.
3. Demonstrated **~1.41M ops/sec** on hysteresis-based stream smoothing (`SignalHysteresis.addSample`), maintaining stable processing under noisy high-frequency updates.

## Source of Truth

- `docs/benchmarks/benchmark-latest.json`
- `docs/benchmarks/benchmark-latest.svg`

## Usage Guidance

- Keep one line in bullets for resume.
- Keep full metric + workload shape together (`ops/sec` + batch size/iterations).
- Only quote numbers present in the latest benchmark artifact.
