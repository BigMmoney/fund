/**
 * 稳定 Signal Line 服务
 * 解决信号跳动、NaN 污染、展示不稳定问题
 */

import { clamp, safePercentChange } from '@/lib/safemath'

export interface SignalLineConfig {
  // 展示窗口
  displayWindow: {
    aggregationPeriod: '5m' | '15m' | '1h'  // 聚合周期
    smoothingFactor: number  // 0-1, 指数移动平均因子
    maxDisplayedChange: number  // 最大展示变化幅度 (%)
  }
  
  // 衰减策略
  decay: {
    enabled: boolean
    halfLifeHours: number  // 半衰期
    minWeight: number      // 最小权重
  }
  
  // Drift 控制
  driftControl: {
    enabled: boolean
    maxDriftPerUpdate: number  // 每次更新最大变化
    lockThreshold: number      // 超过此阈值锁定展示
  }
}

export const DEFAULT_SIGNAL_LINE_CONFIG: SignalLineConfig = {
  displayWindow: {
    aggregationPeriod: '5m',
    smoothingFactor: 0.3,
    maxDisplayedChange: 50
  },
  decay: {
    enabled: true,
    halfLifeHours: 24,
    minWeight: 0.1
  },
  driftControl: {
    enabled: true,
    maxDriftPerUpdate: 5,
    lockThreshold: 20
  }
}

interface HistoryEntry {
  value: number
  timestamp: Date
}

export interface SignalLineResult {
  value: number
  change: number | null
  changeDisplay: string
  status: 'normal' | 'locked' | 'stale' | 'insufficient'
  lastUpdated: Date | null
  sampleCount: number
}

export class StableSignalLine {
  private history: HistoryEntry[] = []
  private displayValue: number = 0
  private locked: boolean = false
  private lockReason: string | null = null
  private config: SignalLineConfig
  
  constructor(config: Partial<SignalLineConfig> = {}) {
    this.config = {
      ...DEFAULT_SIGNAL_LINE_CONFIG,
      ...config,
      displayWindow: { ...DEFAULT_SIGNAL_LINE_CONFIG.displayWindow, ...config.displayWindow },
      decay: { ...DEFAULT_SIGNAL_LINE_CONFIG.decay, ...config.decay },
      driftControl: { ...DEFAULT_SIGNAL_LINE_CONFIG.driftControl, ...config.driftControl }
    }
  }
  
  /**
   * 添加新的原始信号值
   */
  addRawValue(value: number, timestamp: Date = new Date()): boolean {
    // 🔴 规则 1：拒绝 NaN 和无效值
    if (!Number.isFinite(value)) {
      console.warn('[StableSignalLine] Rejected invalid value:', value)
      return false
    }
    
    this.history.push({ value, timestamp })
    this.pruneHistory()
    return true
  }
  
  /**
   * 批量添加历史数据
   */
  addBatch(entries: { value: number, timestamp: Date }[]): number {
    let added = 0
    for (const entry of entries) {
      if (this.addRawValue(entry.value, entry.timestamp)) {
        added++
      }
    }
    return added
  }
  
  /**
   * 获取稳定的展示值
   */
  getDisplayValue(): SignalLineResult {
    if (this.history.length === 0) {
      return { 
        value: 0, 
        change: null, 
        changeDisplay: '--',
        status: 'insufficient',
        lastUpdated: null,
        sampleCount: 0
      }
    }
    
    const now = new Date()
    const windowMs = this.getWindowMs()
    const windowStart = new Date(now.getTime() - windowMs)
    
    // 🔴 步骤 1：时间窗口过滤
    const windowedValues = this.history.filter(h => h.timestamp >= windowStart)
    
    if (windowedValues.length === 0) {
      return {
        value: this.displayValue,
        change: null,
        changeDisplay: '--',
        status: 'stale',
        lastUpdated: this.history[this.history.length - 1]?.timestamp || null,
        sampleCount: 0
      }
    }
    
    // 🔴 步骤 2：计算时间衰减加权平均
    const decayedAvg = this.calculateDecayedAverage(windowedValues, now)
    
    // 🔴 步骤 3：指数移动平均平滑
    const smoothed = this.applySmoothing(decayedAvg)
    
    // 🔴 步骤 4：Drift 控制
    const { finalValue, wasLocked } = this.applyDriftControl(smoothed)
    
    // 🔴 步骤 5：计算变化百分比
    const previousWindowStart = new Date(windowStart.getTime() - windowMs)
    const previousValues = this.history.filter(
      h => h.timestamp >= previousWindowStart && h.timestamp < windowStart
    )
    
    let change: number | null = null
    let changeDisplay = '--'
    
    if (previousValues.length > 0) {
      const previousAvg = this.calculateDecayedAverage(previousValues, windowStart)
      const rawChange = safePercentChange(finalValue, previousAvg)
      
      if (rawChange !== null) {
        // 🔴 限制展示变化幅度
        change = clamp(
          rawChange, 
          -this.config.displayWindow.maxDisplayedChange, 
          this.config.displayWindow.maxDisplayedChange
        )
        changeDisplay = this.formatChange(change, rawChange)
      }
    }
    
    this.displayValue = finalValue
    
    return {
      value: finalValue,
      change,
      changeDisplay,
      status: wasLocked ? 'locked' : 'normal',
      lastUpdated: windowedValues[windowedValues.length - 1].timestamp,
      sampleCount: windowedValues.length
    }
  }
  
  /**
   * 获取原始历史数据（用于调试）
   */
  getHistory(): HistoryEntry[] {
    return [...this.history]
  }
  
  /**
   * 重置状态
   */
  reset(): void {
    this.history = []
    this.displayValue = 0
    this.locked = false
    this.lockReason = null
  }
  
  /**
   * 获取锁定状态
   */
  getLockStatus(): { locked: boolean, reason: string | null } {
    return {
      locked: this.locked,
      reason: this.lockReason
    }
  }
  
  private calculateDecayedAverage(values: HistoryEntry[], asOf: Date): number {
    if (!this.config.decay.enabled) {
      return values.reduce((sum, v) => sum + v.value, 0) / values.length
    }
    
    const halfLifeMs = this.config.decay.halfLifeHours * 60 * 60 * 1000
    let totalWeight = 0
    let weightedSum = 0
    
    for (const v of values) {
      const ageMs = asOf.getTime() - v.timestamp.getTime()
      const decayFactor = Math.pow(0.5, ageMs / halfLifeMs)
      const weight = Math.max(decayFactor, this.config.decay.minWeight)
      
      weightedSum += v.value * weight
      totalWeight += weight
    }
    
    return totalWeight > 0 ? weightedSum / totalWeight : 0
  }
  
  private applySmoothing(newValue: number): number {
    const alpha = this.config.displayWindow.smoothingFactor
    return alpha * newValue + (1 - alpha) * this.displayValue
  }
  
  private applyDriftControl(value: number): { finalValue: number, wasLocked: boolean } {
    if (!this.config.driftControl.enabled) {
      return { finalValue: value, wasLocked: false }
    }
    
    const drift = Math.abs(value - this.displayValue)
    
    // 如果变化太大，锁定并逐步过渡
    if (drift > this.config.driftControl.lockThreshold) {
      this.locked = true
      this.lockReason = `Large drift detected: ${drift.toFixed(1)}`
    }
    
    if (this.locked) {
      const maxStep = this.config.driftControl.maxDriftPerUpdate
      const step = Math.sign(value - this.displayValue) * Math.min(drift, maxStep)
      const finalValue = this.displayValue + step
      
      // 如果已经接近目标值，解锁
      if (Math.abs(finalValue - value) < 1) {
        this.locked = false
        this.lockReason = null
      }
      
      return { finalValue, wasLocked: true }
    }
    
    return { finalValue: value, wasLocked: false }
  }
  
  private getWindowMs(): number {
    const period = this.config.displayWindow.aggregationPeriod
    switch (period) {
      case '5m': return 5 * 60 * 1000
      case '15m': return 15 * 60 * 1000
      case '1h': return 60 * 60 * 1000
      default: return 5 * 60 * 1000
    }
  }
  
  private pruneHistory(): void {
    const maxAge = 24 * 60 * 60 * 1000 // 保留 24h
    const cutoff = new Date(Date.now() - maxAge)
    this.history = this.history.filter(h => h.timestamp >= cutoff)
  }
  
  private formatChange(displayChange: number, rawChange: number): string {
    const sign = displayChange >= 0 ? '+' : ''
    const isCapped = Math.abs(rawChange) > this.config.displayWindow.maxDisplayedChange
    return `${sign}${displayChange.toFixed(1)}%${isCapped ? '+' : ''}`
  }
}

/**
 * 创建带节流的 Signal Line 订阅
 */
export function createThrottledSignalLine(
  signalLine: StableSignalLine,
  throttleMs: number = 5000
): {
  subscribe: (callback: (result: SignalLineResult) => void) => () => void
  getValue: () => SignalLineResult
} {
  let lastEmit = 0
  let subscribers: ((result: SignalLineResult) => void)[] = []
  let intervalId: ReturnType<typeof setInterval> | null = null
  
  const emit = () => {
    const now = Date.now()
    if (now - lastEmit >= throttleMs) {
      const result = signalLine.getDisplayValue()
      subscribers.forEach(cb => cb(result))
      lastEmit = now
    }
  }
  
  return {
    subscribe: (callback) => {
      subscribers.push(callback)
      
      // 启动定时器
      if (intervalId === null) {
        intervalId = setInterval(emit, throttleMs)
        // 立即发送一次
        callback(signalLine.getDisplayValue())
      }
      
      // 返回取消订阅函数
      return () => {
        subscribers = subscribers.filter(cb => cb !== callback)
        if (subscribers.length === 0 && intervalId !== null) {
          clearInterval(intervalId)
          intervalId = null
        }
      }
    },
    getValue: () => signalLine.getDisplayValue()
  }
}

export default StableSignalLine
