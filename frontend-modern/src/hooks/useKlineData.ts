/**
 * 专业K线数据管理Hook
 * Professional Candlestick Data Management Hook
 * 
 * 核心原则：
 * 1. 数据稳定引用 - 使用 useRef 保存 candles 数组，避免 React 状态触发全量重绘
 * 2. 增量更新 - 只更新最后一根未收盘 candle，历史 candle 不可修改
 * 3. 时间对齐 - endTs 必须对齐到 timeframe 边界（整点/00:00）
 * 4. 缓存机制 - symbol+timeframe 独立缓存，切换不重新生成
 */

import { useRef, useCallback, useEffect, useState } from 'react'

// ============================================================================
// K线数据结构 - Candle Data Structure
// ============================================================================
export interface Candle {
  t: number       // 开盘时间戳 (毫秒, UTC)
  o: number       // 开盘价 Open
  h: number       // 最高价 High
  l: number       // 最低价 Low
  c: number       // 收盘价 Close
  v: number       // 成交量 Volume
  closed: boolean // 是否已收盘（时间边界已过）
}

// ============================================================================
// Timeframe 配置表 - 严格按规范定义
// ============================================================================
export interface TimeframeConfig {
  key: string          // 显示名称
  intervalMs: number   // K线间隔(毫秒)
  count: number        // 显示的K线数量
  labelFormat: string  // X轴标签格式
}

export const TIMEFRAME_CONFIGS: Record<string, TimeframeConfig> = {
  '1m':  { key: '1m',  intervalMs: 60_000,      count: 300, labelFormat: 'HH:mm' },
  '5m':  { key: '5m',  intervalMs: 300_000,     count: 300, labelFormat: 'HH:mm' },
  '15m': { key: '15m', intervalMs: 900_000,     count: 200, labelFormat: 'HH:mm' },
  '1h':  { key: '1h',  intervalMs: 3_600_000,   count: 120, labelFormat: 'HH:mm' },
  '1d':  { key: '1d',  intervalMs: 86_400_000,  count: 90,  labelFormat: 'MM/DD' },
}

// 兼容旧的时间周期名称映射
const TIMEFRAME_MAPPING: Record<string, string> = {
  '1M': '1m', '5M': '5m', '15M': '15m', '1H': '1h', '4H': '1h', '1D': '1d'
}

// ============================================================================
// 时间对齐工具函数
// ============================================================================

/**
 * 将时间戳向下取整到 timeframe 边界
 * 1h: 对齐到整点 (13:00, 14:00)
 * 1d: 对齐到 00:00 UTC
 */
export const floorToInterval = (ts: number, intervalMs: number): number => {
  return Math.floor(ts / intervalMs) * intervalMs
}

/**
 * 格式化时间标签
 */
export const formatTimeLabel = (ts: number, format: string): string => {
  const d = new Date(ts)
  const hh = d.getUTCHours().toString().padStart(2, '0')
  const mm = d.getUTCMinutes().toString().padStart(2, '0')
  const MM = (d.getUTCMonth() + 1).toString().padStart(2, '0')
  const DD = d.getUTCDate().toString().padStart(2, '0')
  
  switch (format) {
    case 'HH:mm':
      return `${hh}:${mm}`
    case 'MM/DD':
      return `${MM}/${DD}`
    case 'MM/DD HH:mm':
      return `${MM}/${DD} ${hh}:${mm}`
    default:
      return `${hh}:${mm}`
  }
}

// ============================================================================
// K线缓存结构
// ============================================================================
interface KlineCache {
  candles: Candle[]      // K线数据数组（稳定引用）
  lastUpdateTs: number   // 最后更新时间
  config: TimeframeConfig
}

// ============================================================================
// 模拟价格生成器 (用于演示，实际应替换为真实数据源)
// ============================================================================
class PriceSimulator {
  private basePrice: number
  private tickCount: number = 0
  
  constructor(basePrice: number) {
    this.basePrice = basePrice
  }
  
  nextTick(): { price: number; volume: number } {
    this.tickCount++
    const t = this.tickCount * 0.1
    
    // 多周期叠加模拟真实市场
    const trend = Math.sin(t * 0.05) * this.basePrice * 0.02
    const cycle = Math.sin(t * 0.2) * this.basePrice * 0.005
    const noise = (Math.random() - 0.5) * this.basePrice * 0.001
    
    const price = this.basePrice + trend + cycle + noise
    const volume = 100000 + Math.random() * 500000
    
    return { price, volume }
  }
  
  setBasePrice(price: number) {
    this.basePrice = price
  }
}

// ============================================================================
// useKlineData Hook
// ============================================================================
export interface UseKlineDataOptions {
  symbol: string
  timeframe: string
  basePrice: number
  onUpdate?: (candles: Candle[]) => void
}

export interface UseKlineDataReturn {
  candles: Candle[]
  currentCandle: Candle | null
  isLoading: boolean
  error: string | null
  // 手动触发方法
  updateWithTick: (price: number, volume: number) => void
  refresh: () => void
}

export function useKlineData(options: UseKlineDataOptions): UseKlineDataReturn {
  const { symbol, timeframe, basePrice, onUpdate } = options
  
  // 规范化 timeframe
  const normalizedTf = TIMEFRAME_MAPPING[timeframe] || timeframe.toLowerCase()
  const config = TIMEFRAME_CONFIGS[normalizedTf] || TIMEFRAME_CONFIGS['1m']
  
  // 使用 useRef 保存缓存，避免触发 React 重渲染
  const cacheRef = useRef<Record<string, KlineCache>>({})
  const simulatorRef = useRef<PriceSimulator>(new PriceSimulator(basePrice))
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  
  // 渲染版本号 - 仅用于触发必要的 UI 更新
  const [renderVersion, setRenderVersion] = useState(0)
  // 加载状态 - 保留接口但暂未使用
  const [isLoading] = useState(false)
  const [error] = useState<string | null>(null)
  
  // 获取缓存键
  const getCacheKey = useCallback((sym: string, tf: string) => `${sym}_${tf}`, [])
  
  // ============================================================================
  // 构建初始K线数据 - 时间对齐到边界
  // ============================================================================
  const buildInitialCandles = useCallback((cfg: TimeframeConfig): Candle[] => {
    const now = Date.now()
    const { intervalMs, count } = cfg
    
    // 关键：endTs 对齐到 timeframe 边界
    const endTs = floorToInterval(now, intervalMs)
    const startTs = endTs - (count - 1) * intervalMs
    
    console.log(`[Kline] 构建初始数据:`)
    console.log(`  timeframe=${cfg.key}, interval=${intervalMs}ms, count=${count}`)
    console.log(`  startTs=${new Date(startTs).toISOString()}`)
    console.log(`  endTs=${new Date(endTs).toISOString()}`)
    
    const candles: Candle[] = []
    let price = basePrice
    
    for (let i = 0; i < count; i++) {
      const candleTs = startTs + i * intervalMs
      const isClosed = candleTs + intervalMs <= now
      
      // 模拟价格变化
      const volatility = Math.sqrt(intervalMs / 60000) * 0.001
      const change = (Math.sin(i * 0.15) + Math.sin(i * 0.05) * 0.5) * basePrice * volatility
      
      const open = price
      const close = basePrice + change
      const high = Math.max(open, close) * (1 + Math.random() * volatility)
      const low = Math.min(open, close) * (1 - Math.random() * volatility)
      const volume = (100000 + Math.random() * 500000) * (intervalMs / 60000)
      
      candles.push({
        t: candleTs,
        o: open,
        h: high,
        l: low,
        c: close,
        v: volume,
        closed: isClosed
      })
      
      price = close
    }
    
    // 验证：检查相邻 candle 的 t 差值
    if (candles.length >= 2) {
      const diff = candles[1].t - candles[0].t
      console.log(`[Kline] 验证: 相邻K线时间差=${diff}ms, 期望=${intervalMs}ms, 匹配=${diff === intervalMs}`)
      console.log(`[Kline] 首根: t=${new Date(candles[0].t).toISOString()}`)
      console.log(`[Kline] 末根: t=${new Date(candles[candles.length - 1].t).toISOString()}`)
    }
    
    return candles
  }, [basePrice])
  
  // ============================================================================
  // 获取或初始化缓存
  // ============================================================================
  const getOrCreateCache = useCallback((sym: string, cfg: TimeframeConfig): KlineCache => {
    const key = getCacheKey(sym, cfg.key)
    
    if (!cacheRef.current[key]) {
      console.log(`[Kline] 创建新缓存: ${key}`)
      cacheRef.current[key] = {
        candles: buildInitialCandles(cfg),
        lastUpdateTs: Date.now(),
        config: cfg
      }
    }
    
    return cacheRef.current[key]
  }, [getCacheKey, buildInitialCandles])
  
  // ============================================================================
  // 增量更新最后一根K线 (in-place mutation)
  // ============================================================================
  const updateLastCandleInPlace = useCallback((cache: KlineCache, price: number, volume: number) => {
    const { candles, config } = cache
    if (candles.length === 0) return
    
    const now = Date.now()
    const { intervalMs } = config
    const currentBucketTs = floorToInterval(now, intervalMs)
    const lastCandle = candles[candles.length - 1]
    
    // 检查是否需要创建新K线
    if (currentBucketTs > lastCandle.t) {
      // 时间边界已过，关闭旧K线，创建新K线
      lastCandle.closed = true
      
      // 移除最旧的K线，保持数组长度
      candles.shift()
      
      // 创建新K线
      const newCandle: Candle = {
        t: currentBucketTs,
        o: price,
        h: price,
        l: price,
        c: price,
        v: volume,
        closed: false
      }
      candles.push(newCandle)
      
      console.log(`[Kline] 新K线: t=${new Date(currentBucketTs).toISOString()}`)
    } else {
      // 同一根K线内，原地更新
      lastCandle.c = price
      lastCandle.h = Math.max(lastCandle.h, price)
      lastCandle.l = Math.min(lastCandle.l, price)
      lastCandle.v += volume * 0.01 // 累加成交量的一小部分
    }
    
    cache.lastUpdateTs = now
  }, [])
  
  // ============================================================================
  // 外部调用：更新tick数据
  // ============================================================================
  const updateWithTick = useCallback((price: number, volume: number) => {
    const cache = getOrCreateCache(symbol, config)
    updateLastCandleInPlace(cache, price, volume)
    onUpdate?.(cache.candles)
  }, [symbol, config, getOrCreateCache, updateLastCandleInPlace, onUpdate])
  
  // ============================================================================
  // 刷新数据
  // ============================================================================
  const refresh = useCallback(() => {
    const key = getCacheKey(symbol, config.key)
    delete cacheRef.current[key]
    setRenderVersion(v => v + 1)
  }, [symbol, config.key, getCacheKey])
  
  // ============================================================================
  // 自动更新循环
  // ============================================================================
  useEffect(() => {
    simulatorRef.current.setBasePrice(basePrice)
    
    // 获取或创建缓存
    const cache = getOrCreateCache(symbol, config)
    
    // 根据 timeframe 设置更新频率
    const updateInterval = config.intervalMs <= 60000 ? 200 :
                          config.intervalMs <= 300000 ? 500 :
                          config.intervalMs <= 900000 ? 1000 :
                          config.intervalMs <= 3600000 ? 2000 : 5000
    
    // 启动更新循环
    intervalRef.current = setInterval(() => {
      const { price, volume } = simulatorRef.current.nextTick()
      updateLastCandleInPlace(cache, price, volume)
      
      // 低频率触发 React 渲染
      setRenderVersion(v => v + 1)
    }, updateInterval)
    
    // 初始渲染
    setRenderVersion(v => v + 1)
    
    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current)
      }
    }
  }, [symbol, config, basePrice, getOrCreateCache, updateLastCandleInPlace])
  
  // 获取当前缓存的数据
  const cache = cacheRef.current[getCacheKey(symbol, config.key)]
  const candles = cache?.candles || []
  const currentCandle = candles.length > 0 ? candles[candles.length - 1] : null
  
  // 抑制 unused variable 警告
  void renderVersion
  
  return {
    candles,
    currentCandle,
    isLoading,
    error,
    updateWithTick,
    refresh
  }
}

// ============================================================================
// 导出工具函数
// ============================================================================
export { PriceSimulator }
