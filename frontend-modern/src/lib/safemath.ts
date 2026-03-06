/**
 * 安全数学运算库
 * 防止 NaN 污染和数值溢出
 */

/**
 * 安全除法 - 防止除以零和 NaN
 */
export function safeDivide(
  numerator: number, 
  denominator: number, 
  fallback: number = 0
): number {
  if (!Number.isFinite(numerator)) return fallback
  if (!Number.isFinite(denominator)) return fallback
  if (Math.abs(denominator) < 1e-10) return fallback  // 分母保护
  
  const result = numerator / denominator
  return Number.isFinite(result) ? result : fallback
}

/**
 * 安全百分比变化计算
 * 返回 null 表示不可计算（而非 NaN）
 */
export function safePercentChange(
  current: number, 
  previous: number
): number | null {
  if (!Number.isFinite(current) || !Number.isFinite(previous)) return null
  if (Math.abs(previous) < 1e-10) {
    // 前值接近 0，百分比变化无意义
    return null
  }
  const result = ((current - previous) / Math.abs(previous)) * 100
  return Number.isFinite(result) ? result : null
}

/**
 * Clamp 函数 - 将值限制在范围内
 * NaN/Infinity 返回最小值
 */
export function clamp(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min
  return Math.max(min, Math.min(max, value))
}

/**
 * 安全加权平均
 */
export function safeWeightedAverage(
  values: number[], 
  weights: number[],
  minSamples: number = 2
): { value: number | null, status: 'valid' | 'insufficient' | 'invalid' } {
  
  // 过滤有效数据
  const validPairs = values
    .map((v, i) => ({ value: v, weight: weights[i] ?? 0 }))
    .filter(p => Number.isFinite(p.value) && Number.isFinite(p.weight) && p.weight > 0)
  
  if (validPairs.length < minSamples) {
    return { value: null, status: 'insufficient' }
  }
  
  const totalWeight = validPairs.reduce((sum, p) => sum + p.weight, 0)
  if (totalWeight < 1e-10) {
    return { value: null, status: 'invalid' }
  }
  
  const weightedSum = validPairs.reduce((sum, p) => sum + p.value * p.weight, 0)
  const result = weightedSum / totalWeight
  
  return { 
    value: Number.isFinite(result) ? clamp(result, 0, 100) : null,
    status: Number.isFinite(result) ? 'valid' : 'invalid'
  }
}

/**
 * 检查值是否为有效数字
 */
export function isValidNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value)
}

/**
 * 安全数字转换
 */
export function toSafeNumber(value: unknown, fallback: number = 0): number {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value
  }
  if (typeof value === 'string') {
    const parsed = parseFloat(value)
    if (Number.isFinite(parsed)) {
      return parsed
    }
  }
  return fallback
}

/**
 * 递归清理对象中的 NaN 值
 */
export function sanitizeOutput<T>(obj: T): T {
  return JSON.parse(JSON.stringify(obj, (_key, value) => {
    if (typeof value === 'number') {
      if (!Number.isFinite(value)) return null  // NaN/Infinity → null
    }
    return value
  }))
}

/**
 * 格式化百分比变化为显示字符串
 */
export function formatPercentChange(
  change: number | null, 
  options: {
    maxDisplay?: number
    decimals?: number
    showPlus?: boolean
  } = {}
): string {
  const { maxDisplay = 999, decimals = 1, showPlus = true } = options
  
  if (change === null || !Number.isFinite(change)) {
    return '--'
  }
  
  const displayChange = clamp(change, -maxDisplay, maxDisplay)
  const sign = showPlus && displayChange >= 0 ? '+' : ''
  const isCapped = Math.abs(change) > maxDisplay
  
  return `${sign}${displayChange.toFixed(decimals)}%${isCapped ? '+' : ''}`
}

/**
 * 格式化数字为显示字符串
 */
export function formatNumber(
  value: number | null | undefined,
  options: {
    decimals?: number
    fallback?: string
    prefix?: string
    suffix?: string
  } = {}
): string {
  const { decimals = 2, fallback = '--', prefix = '', suffix = '' } = options
  
  if (value === null || value === undefined || !Number.isFinite(value)) {
    return fallback
  }
  
  return `${prefix}${value.toFixed(decimals)}${suffix}`
}

/**
 * 计算指数移动平均 (EMA)
 */
export function calculateEMA(
  values: number[],
  alpha: number = 0.3
): number | null {
  const validValues = values.filter(v => Number.isFinite(v))
  if (validValues.length === 0) return null
  
  let ema = validValues[0]
  for (let i = 1; i < validValues.length; i++) {
    ema = alpha * validValues[i] + (1 - alpha) * ema
  }
  
  return Number.isFinite(ema) ? ema : null
}

/**
 * 计算带时间衰减的加权值
 */
export function calculateDecayedValue(
  value: number,
  ageMs: number,
  halfLifeMs: number,
  minWeight: number = 0.1
): { value: number, weight: number } {
  if (!Number.isFinite(value)) {
    return { value: 0, weight: 0 }
  }
  
  const decayFactor = Math.pow(0.5, ageMs / halfLifeMs)
  const weight = Math.max(decayFactor, minWeight)
  
  return {
    value: value * weight,
    weight
  }
}

export default {
  safeDivide,
  safePercentChange,
  clamp,
  safeWeightedAverage,
  isValidNumber,
  toSafeNumber,
  sanitizeOutput,
  formatPercentChange,
  formatNumber,
  calculateEMA,
  calculateDecayedValue
}
