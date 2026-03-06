import { describe, it, expect } from 'vitest'
import {
  clamp,
  formatPercentChange,
  safeDivide,
  safePercentChange,
  safeWeightedAverage,
  sanitizeOutput,
  toSafeNumber
} from '../../src/lib/safemath'

function seededRandom(seed: number): () => number {
  let state = seed >>> 0
  return () => {
    state = (1664525 * state + 1013904223) >>> 0
    return state / 0xffffffff
  }
}

describe('safemath invariants', () => {
  it('safeDivide always returns finite number when fallback is finite', () => {
    const rand = seededRandom(7)
    for (let i = 0; i < 500; i++) {
      const numerator = (rand() - 0.5) * 1e9
      const denominator = i % 10 === 0 ? 0 : (rand() - 0.5) * 1e6
      const value = safeDivide(numerator, denominator, 42)
      expect(Number.isFinite(value)).toBe(true)
    }
  })

  it('safePercentChange returns null when previous is near zero', () => {
    expect(safePercentChange(100, 0)).toBeNull()
    expect(safePercentChange(100, 1e-12)).toBeNull()
  })

  it('clamp output is always inside [min, max] and idempotent', () => {
    const rand = seededRandom(11)
    for (let i = 0; i < 300; i++) {
      const value = (rand() - 0.5) * 1e6
      const c = clamp(value, -50, 80)
      expect(c).toBeGreaterThanOrEqual(-50)
      expect(c).toBeLessThanOrEqual(80)
      expect(clamp(c, -50, 80)).toBe(c)
    }
  })

  it('safeWeightedAverage is invariant under uniform weight scaling', () => {
    const values = [10, 20, 30, 40]
    const weights = [1, 2, 3, 4]
    const scaled = weights.map((w) => w * 10)

    const a = safeWeightedAverage(values, weights, 2)
    const b = safeWeightedAverage(values, scaled, 2)

    expect(a.status).toBe('valid')
    expect(b.status).toBe('valid')
    expect(a.value).not.toBeNull()
    expect(b.value).not.toBeNull()
    expect(a.value!).toBeCloseTo(b.value!, 10)
  })

  it('safeWeightedAverage rejects insufficient samples', () => {
    const result = safeWeightedAverage([10], [1], 2)
    expect(result.status).toBe('insufficient')
    expect(result.value).toBeNull()
  })

  it('sanitizeOutput removes NaN/Infinity from nested payload', () => {
    const payload = {
      a: NaN,
      b: Infinity,
      c: -Infinity,
      d: { x: 12, y: NaN }
    }
    const sanitized = sanitizeOutput(payload) as any
    expect(sanitized.a).toBeNull()
    expect(sanitized.b).toBeNull()
    expect(sanitized.c).toBeNull()
    expect(sanitized.d.y).toBeNull()
    expect(sanitized.d.x).toBe(12)
  })

  it('formatPercentChange and toSafeNumber fallback behavior', () => {
    expect(formatPercentChange(null)).toBe('--')
    expect(formatPercentChange(Number.NaN)).toBe('--')
    expect(formatPercentChange(12.345, { decimals: 1 })).toBe('+12.3%')
    expect(toSafeNumber('abc', 9)).toBe(9)
    expect(toSafeNumber('12.8', 9)).toBe(12.8)
  })
})
