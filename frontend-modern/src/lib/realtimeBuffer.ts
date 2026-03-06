/**
 * 实时数据缓冲层
 * 解决：WS 高频 setState 导致的 UI 闪烁
 * 
 * 架构：WS → Buffer → Window Aggregation → Store → UI
 */

// ============== A. 数据缓冲器 ==============

export interface BufferConfig {
  flushIntervalMs: number      // 刷新间隔（UI 层建议 500-1000ms）
  maxBufferSize: number        // 最大缓冲条数
  dedupeKey?: string           // 去重字段
  aggregationWindow?: number   // 聚合窗口（ms）
}

export interface BufferedItem<T> {
  data: T
  timestamp: number
  sequence: number
}

export class RealtimeBuffer<T extends Record<string, any>> {
  private buffer: BufferedItem<T>[] = []
  private flushTimer: ReturnType<typeof setInterval> | null = null
  private lastFlush: number = 0
  private sequence: number = 0
  private subscribers: Set<(items: T[]) => void> = new Set()
  private config: BufferConfig

  constructor(config: BufferConfig) {
    this.config = config
  }

  /**
   * 启动缓冲器
   */
  start(): void {
    if (this.flushTimer) return
    
    this.flushTimer = setInterval(() => {
      this.flush()
    }, this.config.flushIntervalMs)
  }

  /**
   * 停止缓冲器
   */
  stop(): void {
    if (this.flushTimer) {
      clearInterval(this.flushTimer)
      this.flushTimer = null
    }
  }

  /**
   * 接收数据（来自 WS onmessage）
   * ⚠️ 这里不直接触发 UI 更新
   */
  push(item: T): void {
    this.sequence++
    
    // 去重检查
    if (this.config.dedupeKey) {
      const key = item[this.config.dedupeKey]
      const existingIdx = this.buffer.findIndex(
        b => b.data[this.config.dedupeKey!] === key
      )
      if (existingIdx !== -1) {
        // 更新已存在的项
        this.buffer[existingIdx] = {
          data: item,
          timestamp: Date.now(),
          sequence: this.sequence
        }
        return
      }
    }

    this.buffer.push({
      data: item,
      timestamp: Date.now(),
      sequence: this.sequence
    })

    // 防止内存溢出
    if (this.buffer.length > this.config.maxBufferSize) {
      this.buffer = this.buffer.slice(-this.config.maxBufferSize)
    }
  }

  /**
   * 批量刷新到订阅者
   * ✅ 这是唯一触发 UI 更新的地方
   */
  private flush(): void {
    if (this.buffer.length === 0) return

    const now = Date.now()
    
    // 聚合窗口内的数据
    let itemsToFlush: T[]
    if (this.config.aggregationWindow) {
      const windowStart = now - this.config.aggregationWindow
      itemsToFlush = this.buffer
        .filter(b => b.timestamp >= windowStart)
        .map(b => b.data)
    } else {
      itemsToFlush = this.buffer.map(b => b.data)
    }

    // 清空缓冲区
    this.buffer = []
    this.lastFlush = now

    // 通知所有订阅者（批量更新）
    if (itemsToFlush.length > 0) {
      this.subscribers.forEach(callback => {
        callback(itemsToFlush)
      })
    }
  }

  /**
   * 订阅批量更新
   */
  subscribe(callback: (items: T[]) => void): () => void {
    this.subscribers.add(callback)
    return () => {
      this.subscribers.delete(callback)
    }
  }

  /**
   * 强制立即刷新
   */
  forceFlush(): void {
    this.flush()
  }

  /**
   * 获取缓冲区统计
   */
  getStats() {
    return {
      bufferSize: this.buffer.length,
      lastFlush: this.lastFlush,
      sequence: this.sequence,
      subscriberCount: this.subscribers.size
    }
  }
}

// ============== B. 信号迟钝化（Hysteresis） ==============

export interface HysteresisConfig {
  windowMs: number             // 观察窗口
  threshold: number            // 变化阈值（%）
  minSamples: number           // 最小样本数
}

export class SignalHysteresis {
  private samples: { value: number; timestamp: number }[] = []
  private lastEmittedValue: number | null = null
  private config: HysteresisConfig

  constructor(config: Partial<HysteresisConfig> = {}) {
    this.config = {
      windowMs: 5000,          // 5秒窗口
      threshold: 5,            // 5% 变化才更新
      minSamples: 3,           // 至少3个样本
      ...config
    }
  }

  /**
   * 添加样本，返回是否应该更新 UI
   */
  addSample(value: number): { shouldUpdate: boolean; stableValue: number } {
    const now = Date.now()
    
    // 清理过期样本
    this.samples = this.samples.filter(
      s => now - s.timestamp < this.config.windowMs
    )
    
    // 添加新样本
    this.samples.push({ value, timestamp: now })

    // 样本不足，使用当前值但不更新
    if (this.samples.length < this.config.minSamples) {
      return {
        shouldUpdate: this.lastEmittedValue === null,
        stableValue: value
      }
    }

    // 计算窗口内平均值
    const avg = this.samples.reduce((sum, s) => sum + s.value, 0) / this.samples.length

    // 首次发射
    if (this.lastEmittedValue === null) {
      this.lastEmittedValue = avg
      return { shouldUpdate: true, stableValue: avg }
    }

    // 检查变化是否超过阈值
    const changePercent = Math.abs(avg - this.lastEmittedValue) / 
      (Math.abs(this.lastEmittedValue) || 1) * 100

    if (changePercent >= this.config.threshold) {
      this.lastEmittedValue = avg
      return { shouldUpdate: true, stableValue: avg }
    }

    // 未达阈值，返回上次稳定值
    return { shouldUpdate: false, stableValue: this.lastEmittedValue }
  }

  /**
   * 重置状态
   */
  reset(): void {
    this.samples = []
    this.lastEmittedValue = null
  }

  /**
   * 获取当前稳定值
   */
  getStableValue(): number | null {
    return this.lastEmittedValue
  }
}

// ============== C. 列表稳定器（防止 key 变化导致 unmount） ==============

export interface StableListConfig<T> {
  idKey: keyof T               // 唯一标识字段
  maxItems: number             // 最大保留条数
  insertPosition: 'head' | 'tail'  // 新项插入位置
}

export class StableList<T extends Record<string, any>> {
  private items: Map<string, T> = new Map()
  private order: string[] = []
  private config: StableListConfig<T>

  constructor(config: StableListConfig<T>) {
    this.config = config
  }

  /**
   * 批量更新，保持已有项的引用稳定
   */
  update(newItems: T[]): T[] {
    for (const item of newItems) {
      const id = String(item[this.config.idKey])
      
      if (this.items.has(id)) {
        // 已存在：浅合并更新，保持对象引用（如果内容相同）
        const existing = this.items.get(id)!
        const merged = { ...existing, ...item }
        
        // 只有真正变化才替换
        if (!shallowEqual(existing, merged)) {
          this.items.set(id, merged)
        }
      } else {
        // 新项：插入
        this.items.set(id, item)
        if (this.config.insertPosition === 'head') {
          this.order.unshift(id)
        } else {
          this.order.push(id)
        }
      }
    }

    // 限制数量
    while (this.order.length > this.config.maxItems) {
      const removedId = this.config.insertPosition === 'head' 
        ? this.order.pop()! 
        : this.order.shift()!
      this.items.delete(removedId)
    }

    // 返回有序数组
    return this.order.map(id => this.items.get(id)!)
  }

  /**
   * 获取当前列表
   */
  getItems(): T[] {
    return this.order.map(id => this.items.get(id)!)
  }

  /**
   * 清空
   */
  clear(): void {
    this.items.clear()
    this.order = []
  }
}

// ============== D. 变化类型分类器 ==============

export type ChangeType = 'value' | 'grade' | 'state' | 'none'

export interface ChangeClassification<T> {
  type: ChangeType
  field: keyof T | null
  oldValue: any
  newValue: any
  shouldAnimate: boolean
  shouldFlash: boolean
}

/**
 * 分类变化类型，决定 UI 行为
 */
export function classifyChange<T extends Record<string, any>>(
  prev: T,
  next: T,
  gradeFields: (keyof T)[],   // 等级字段（如 severity, level）
  stateFields: (keyof T)[]    // 状态字段（如 state, status）
): ChangeClassification<T> {
  // 检查状态变化（最高优先级）
  for (const field of stateFields) {
    if (prev[field] !== next[field]) {
      return {
        type: 'state',
        field,
        oldValue: prev[field],
        newValue: next[field],
        shouldAnimate: true,   // 状态变化需要动画
        shouldFlash: false     // 但不需要闪烁
      }
    }
  }

  // 检查等级变化
  for (const field of gradeFields) {
    if (prev[field] !== next[field]) {
      return {
        type: 'grade',
        field,
        oldValue: prev[field],
        newValue: next[field],
        shouldAnimate: true,   // 等级变化需要动画
        shouldFlash: true      // 短暂闪烁提示
      }
    }
  }

  // 检查数值变化
  for (const key of Object.keys(next) as (keyof T)[]) {
    if (typeof next[key] === 'number' && prev[key] !== next[key]) {
      return {
        type: 'value',
        field: key,
        oldValue: prev[key],
        newValue: next[key],
        shouldAnimate: false,  // 数值变化平滑过渡
        shouldFlash: false     // 不闪烁
      }
    }
  }

  return {
    type: 'none',
    field: null,
    oldValue: null,
    newValue: null,
    shouldAnimate: false,
    shouldFlash: false
  }
}

// ============== E. 工具函数 ==============

/**
 * 浅比较
 */
export function shallowEqual<T extends Record<string, any>>(a: T, b: T): boolean {
  const keysA = Object.keys(a)
  const keysB = Object.keys(b)
  
  if (keysA.length !== keysB.length) return false
  
  for (const key of keysA) {
    if (a[key] !== b[key]) return false
  }
  
  return true
}

/**
 * 创建稳定的 key 生成器
 */
export function createStableKeyGenerator() {
  const keyMap = new Map<string, number>()
  let counter = 0

  return {
    getKey(id: string): string {
      if (!keyMap.has(id)) {
        keyMap.set(id, counter++)
      }
      return `stable-${keyMap.get(id)}`
    },
    
    hasKey(id: string): boolean {
      return keyMap.has(id)
    },

    clear(): void {
      keyMap.clear()
      counter = 0
    }
  }
}

// ============== F. 导出单例 ==============

// 全局缓冲器实例
export const newsBuffer = new RealtimeBuffer<any>({
  flushIntervalMs: 500,
  maxBufferSize: 200,
  dedupeKey: 'id'
})

export const alertBuffer = new RealtimeBuffer<any>({
  flushIntervalMs: 1000,
  maxBufferSize: 50,
  dedupeKey: 'id'
})

// 信号迟钝化实例
export const signalHysteresis = {
  score: new SignalHysteresis({ windowMs: 5000, threshold: 5, minSamples: 3 }),
  velocity: new SignalHysteresis({ windowMs: 10000, threshold: 10, minSamples: 5 }),
  sentiment: new SignalHysteresis({ windowMs: 5000, threshold: 3, minSamples: 3 })
}
