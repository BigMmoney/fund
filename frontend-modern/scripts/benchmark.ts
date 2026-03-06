import fs from 'node:fs'
import path from 'node:path'
import { performance } from 'node:perf_hooks'

import {
  safeDivide,
  safeWeightedAverage,
  clamp,
  formatPercentChange
} from '../src/lib/safemath'
import {
  StableList,
  SignalHysteresis,
  classifyChange
} from '../src/lib/realtimeBuffer'

type BenchResult = {
  name: string
  iterations: number
  rounds: number
  meanMs: number
  p95Ms: number
  opsPerSec: number
}

function percentile(values: number[], p: number): number {
  if (values.length === 0) return 0
  const sorted = [...values].sort((a, b) => a - b)
  const idx = Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * p))
  return sorted[idx]
}

function runCase(
  name: string,
  iterations: number,
  rounds: number,
  fn: (i: number) => void
): BenchResult {
  for (let i = 0; i < Math.min(2000, iterations); i++) fn(i)

  const samples: number[] = []
  for (let r = 0; r < rounds; r++) {
    const start = performance.now()
    for (let i = 0; i < iterations; i++) fn(i)
    samples.push(performance.now() - start)
  }

  const meanMs = samples.reduce((acc, cur) => acc + cur, 0) / samples.length
  const p95Ms = percentile(samples, 0.95)
  const opsPerSec = iterations / (meanMs / 1000)

  return { name, iterations, rounds, meanMs, p95Ms, opsPerSec }
}

function formatOps(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`
  return n.toFixed(0)
}

function renderSvg(results: BenchResult[]): string {
  const width = 1100
  const marginLeft = 290
  const marginRight = 60
  const rowHeight = 56
  const headerHeight = 90
  const footerHeight = 60
  const chartWidth = width - marginLeft - marginRight
  const height = headerHeight + results.length * rowHeight + footerHeight

  const maxOps = Math.max(...results.map((r) => r.opsPerSec), 1)

  const bars = results
    .map((r, idx) => {
      const y = headerHeight + idx * rowHeight
      const barWidth = Math.max(1, (r.opsPerSec / maxOps) * chartWidth)
      const barY = y + 14
      const textY = y + 33

      return `
  <text x="20" y="${textY}" fill="#D8E8FF" font-size="16" font-family="Segoe UI, Arial">${r.name}</text>
  <rect x="${marginLeft}" y="${barY}" width="${barWidth}" height="22" rx="6" fill="url(#barGradient)"/>
  <text x="${marginLeft + barWidth + 12}" y="${textY}" fill="#9CC4FF" font-size="14" font-family="Consolas, Menlo, monospace">${formatOps(r.opsPerSec)} ops/s</text>`
    })
    .join('\n')

  const generatedAt = new Date().toISOString()

  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#071126"/>
      <stop offset="100%" stop-color="#0D1D38"/>
    </linearGradient>
    <linearGradient id="barGradient" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#28C7FA"/>
      <stop offset="100%" stop-color="#1A9BFF"/>
    </linearGradient>
  </defs>
  <rect x="0" y="0" width="${width}" height="${height}" fill="url(#bg)"/>
  <text x="20" y="36" fill="#FFFFFF" font-size="28" font-family="Segoe UI, Arial" font-weight="700">Benchmark Results</text>
  <text x="20" y="62" fill="#9CC4FF" font-size="14" font-family="Segoe UI, Arial">frontend-modern | market infrastructure / systems profile</text>
${bars}
  <text x="20" y="${height - 20}" fill="#7EA3D9" font-size="12" font-family="Consolas, Menlo, monospace">generated: ${generatedAt}</text>
</svg>`
}

function runBenchmarks(): BenchResult[] {
  const stableList = new StableList<{ id: string; value: number; level: number }>({
    idKey: 'id',
    maxItems: 512,
    insertPosition: 'tail'
  })

  const hysteresis = new SignalHysteresis({
    windowMs: 8_000,
    threshold: 2,
    minSamples: 4
  })

  const weights = [1, 2, 3, 4, 5, 6, 7, 8]
  const baseValues = [12, 34, 56, 78, 90, 45, 23, 67]

  return [
    runCase('safeDivide hot path', 200_000, 3, (i) => {
      safeDivide(i + 101, 37.3, 0)
    }),
    runCase('safeWeightedAverage small vector', 30_000, 3, (i) => {
      const values = baseValues.map((v, idx) => (v + (i % (idx + 3))) % 100)
      safeWeightedAverage(values, weights, 2)
    }),
    runCase('classifyChange state + grade + value', 80_000, 3, (i) => {
      classifyChange(
        { status: 'A', grade: 'L1', score: i },
        { status: i % 5 === 0 ? 'B' : 'A', grade: i % 2 ? 'L1' : 'L2', score: i + 1 },
        ['grade'],
        ['status']
      )
    }),
    runCase('StableList.update (batch=128)', 400, 3, (i) => {
      const batch = Array.from({ length: 128 }, (_, k) => ({
        id: `${i % 512}-${k}`,
        value: clamp((i + k) % 200, 0, 100),
        level: (i + k) % 5
      }))
      stableList.update(batch)
    }),
    runCase('SignalHysteresis.addSample', 80_000, 3, (i) => {
      if (i % 200 === 0) {
        hysteresis.reset()
      }
      hysteresis.addSample(100 + Math.sin(i / 50) * 3 + (i % 7))
    }),
    runCase('formatPercentChange', 200_000, 3, (i) => {
      formatPercentChange((i % 1000) - 500, { decimals: 2, showPlus: true })
    })
  ]
}

function printTable(results: BenchResult[]): void {
  const rows = results.map((r) => ({
    case: r.name,
    opsPerSec: Number(r.opsPerSec.toFixed(0)),
    meanMs: Number(r.meanMs.toFixed(3)),
    p95Ms: Number(r.p95Ms.toFixed(3)),
    iterations: r.iterations
  }))
  console.table(rows)
}

function assertCIThresholds(results: BenchResult[]): void {
  const thresholds: Record<string, number> = {
    'safeDivide hot path': 50_000,
    'safeWeightedAverage small vector': 8_000,
    'classifyChange state + grade + value': 30_000,
    'StableList.update (batch=128)': 120,
    'SignalHysteresis.addSample': 20_000,
    formatPercentChange: 60_000
  }

  const failures: string[] = []

  for (const result of results) {
    const min = thresholds[result.name]
    if (min && result.opsPerSec < min) {
      failures.push(`${result.name}: ${Math.floor(result.opsPerSec)} < ${min} ops/s`)
    }
  }

  if (failures.length > 0) {
    throw new Error(`Benchmark regression detected:\n${failures.join('\n')}`)
  }
}

function main(): void {
  const ciMode = process.argv.includes('--ci')
  const results = runBenchmarks()
  printTable(results)

  const outputDir = path.resolve(process.cwd(), 'docs/benchmarks')
  fs.mkdirSync(outputDir, { recursive: true })

  const payload = {
    generatedAt: new Date().toISOString(),
    ciMode,
    nodeVersion: process.version,
    platform: process.platform,
    arch: process.arch,
    results
  }

  fs.writeFileSync(
    path.join(outputDir, 'benchmark-latest.json'),
    `${JSON.stringify(payload, null, 2)}\n`,
    'utf8'
  )
  fs.writeFileSync(
    path.join(outputDir, 'benchmark-latest.svg'),
    renderSvg(results),
    'utf8'
  )

  if (ciMode) {
    assertCIThresholds(results)
  }

  console.log(`Benchmark artifacts written to ${outputDir}`)
}

main()
