import { describe, it, expect, vi } from 'vitest'
import {
  RealtimeBuffer,
  SignalHysteresis,
  StableList,
  classifyChange,
  shallowEqual
} from '../../src/lib/realtimeBuffer'

describe('realtimeBuffer invariants', () => {
  it('dedupe keeps only latest item for the same key', () => {
    const buffer = new RealtimeBuffer<{ id: string; px: number }>({
      flushIntervalMs: 1000,
      maxBufferSize: 10,
      dedupeKey: 'id'
    })

    const got: Array<Array<{ id: string; px: number }>> = []
    buffer.subscribe((items) => got.push(items))

    buffer.push({ id: 'A', px: 1 })
    buffer.push({ id: 'A', px: 2 })
    buffer.forceFlush()

    expect(got).toHaveLength(1)
    expect(got[0]).toHaveLength(1)
    expect(got[0][0]).toEqual({ id: 'A', px: 2 })
  })

  it('maxBufferSize is enforced and oldest entries are dropped', () => {
    const buffer = new RealtimeBuffer<{ id: string; px: number }>({
      flushIntervalMs: 1000,
      maxBufferSize: 3
    })
    const got: Array<Array<{ id: string; px: number }>> = []
    buffer.subscribe((items) => got.push(items))

    buffer.push({ id: '1', px: 1 })
    buffer.push({ id: '2', px: 2 })
    buffer.push({ id: '3', px: 3 })
    buffer.push({ id: '4', px: 4 })
    buffer.push({ id: '5', px: 5 })
    buffer.forceFlush()

    expect(got[0].map((x) => x.id)).toEqual(['3', '4', '5'])
  })

  it('start/stop controls timer-based flush lifecycle', () => {
    vi.useFakeTimers()
    try {
      const buffer = new RealtimeBuffer<{ id: string; px: number }>({
        flushIntervalMs: 100,
        maxBufferSize: 5
      })
      const got: Array<Array<{ id: string; px: number }>> = []
      buffer.subscribe((items) => got.push(items))

      buffer.start()
      buffer.push({ id: 'A', px: 1 })
      vi.advanceTimersByTime(100)
      expect(got).toHaveLength(1)

      buffer.stop()
      buffer.push({ id: 'B', px: 2 })
      vi.advanceTimersByTime(300)
      expect(got).toHaveLength(1)
      buffer.forceFlush()
      expect(got).toHaveLength(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('SignalHysteresis only emits updates after threshold crossing', () => {
    const h = new SignalHysteresis({ windowMs: 60_000, threshold: 10, minSamples: 3 })
    const a = h.addSample(100)
    const b = h.addSample(100)
    const c = h.addSample(100)

    expect(a.shouldUpdate).toBe(true)
    expect(b.shouldUpdate).toBe(true)
    expect(c.shouldUpdate).toBe(true)
    expect(c.stableValue).toBeCloseTo(100, 8)

    const d = h.addSample(104)
    expect(d.shouldUpdate).toBe(false)

    const e = h.addSample(130)
    const f = h.addSample(130)
    const g = h.addSample(130)
    expect([e.shouldUpdate, f.shouldUpdate, g.shouldUpdate].some(Boolean)).toBe(true)
    expect(h.getStableValue()).not.toBeNull()
  })

  it('StableList keeps maxItems cap and deterministic order', () => {
    const stable = new StableList<{ id: string; v: number }>({
      idKey: 'id',
      maxItems: 3,
      insertPosition: 'tail'
    })

    stable.update([
      { id: 'a', v: 1 },
      { id: 'b', v: 2 },
      { id: 'c', v: 3 }
    ])
    const after1 = stable.getItems()
    expect(after1.map((x) => x.id)).toEqual(['a', 'b', 'c'])

    stable.update([{ id: 'd', v: 4 }])
    const after2 = stable.getItems()
    expect(after2.map((x) => x.id)).toEqual(['b', 'c', 'd'])
  })

  it('classifyChange priority: state > grade > value > none', () => {
    const prev = { state: 'ok', level: 'L1', score: 10 }
    const next1 = { state: 'alert', level: 'L2', score: 20 }
    const next2 = { state: 'ok', level: 'L2', score: 20 }
    const next3 = { state: 'ok', level: 'L1', score: 11 }
    const next4 = { state: 'ok', level: 'L1', score: 10 }

    expect(classifyChange(prev, next1, ['level'], ['state']).type).toBe('state')
    expect(classifyChange(prev, next2, ['level'], ['state']).type).toBe('grade')
    expect(classifyChange(prev, next3, ['level'], ['state']).type).toBe('value')
    expect(classifyChange(prev, next4, ['level'], ['state']).type).toBe('none')
  })

  it('shallowEqual behaves consistently for flat objects', () => {
    expect(shallowEqual({ a: 1, b: 'x' }, { a: 1, b: 'x' })).toBe(true)
    expect(shallowEqual({ a: 1, b: 'x' }, { a: 2, b: 'x' })).toBe(false)
    expect(shallowEqual({ a: 1 }, { a: 1, b: 2 } as any)).toBe(false)
  })
})
