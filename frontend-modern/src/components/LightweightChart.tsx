/**
 * TradingView Lightweight Charts 组件
 * 使用Canvas渲染，无闪烁，专为实时金融数据设计
 * 支持所有图表类型：Area, K线, 综合, 斐波那契, 量价, 资金流, 趋势, 热力图, MACD, RSI, KDJ, 深度, 成交量
 */
import { useEffect, useRef, useState, useCallback } from 'react'
import {
  createChart,
  CandlestickSeries,
  HistogramSeries,
  LineSeries,
  AreaSeries,
  BaselineSeries,
  ColorType,
  CrosshairMode,
  IChartApi,
  ISeriesApi,
  LineData,
  Time,
  MouseEventParams
} from 'lightweight-charts'

// ============================================================================
// 类型定义
// ============================================================================
interface CandleData {
  time: Time
  open: number
  high: number
  low: number
  close: number
  volume?: number
  changePercent?: number
  // 移动平均线
  ma5?: number
  ma10?: number
  ma20?: number
  ma60?: number
  ema12?: number
  ema26?: number
  // 动量指标
  rsi?: number
  macd?: number
  signal?: number
  histogram?: number
  k?: number
  d?: number
  j?: number
  // 波动率指标
  upperBB?: number
  middleBB?: number
  lowerBB?: number
  atr?: number
  // 成交量指标
  obv?: number
  vwap?: number
  // 其他专业指标
  williamsR?: number
  cci?: number
  stochRsi?: number
  adx?: number
  // 资金流
  inflow?: number
  outflow?: number
  netFlow?: number
  mfi?: number  // Money Flow Index
}

interface LightweightChartProps {
  symbol: string
  basePrice: number
  chartType?: string
  timeframe?: string
  height?: number
  showVolume?: boolean
  showMA?: boolean
}

// 悬浮信息类型
interface HoverInfo {
  time: string
  open: number
  high: number
  low: number
  close: number
  volume: number
  change: number
  changePercent: number
  // 移动平均线
  ma5?: number
  ma10?: number
  ma20?: number
  ma60?: number
  ema12?: number
  ema26?: number
  // 动量指标
  rsi?: number
  macd?: number
  signal?: number
  histogram?: number
  k?: number
  d?: number
  j?: number
  stochRsi?: number
  // 波动率
  upperBB?: number
  middleBB?: number
  lowerBB?: number
  atr?: number
  // 成交量指标
  obv?: number
  vwap?: number
  // 其他专业指标
  williamsR?: number
  cci?: number
  adx?: number
  mfi?: number
  // 资金流
  inflow?: number
  outflow?: number
  netFlow?: number
}

// ============================================================================
// 技术指标计算函数
// ============================================================================
const calculateRSI = (prices: number[], period: number = 14): number => {
  if (prices.length < period + 1) return 50
  let gains = 0, losses = 0
  for (let i = prices.length - period; i < prices.length; i++) {
    const diff = prices[i] - prices[i - 1]
    if (diff > 0) gains += diff
    else losses -= diff
  }
  const avgGain = gains / period
  const avgLoss = losses / period
  if (avgLoss === 0) return 100
  return 100 - (100 / (1 + avgGain / avgLoss))
}

const calculateMACD = (prices: number[]): { macd: number; signal: number; histogram: number } => {
  if (prices.length < 26) return { macd: 0, signal: 0, histogram: 0 }
  const ema12 = prices.slice(-12).reduce((a, b) => a + b, 0) / 12
  const ema26 = prices.slice(-26).reduce((a, b) => a + b, 0) / 26
  const macd = ema12 - ema26
  const signal = macd * 0.8
  return { macd, signal, histogram: macd - signal }
}

const calculateKDJ = (highs: number[], lows: number[], closes: number[], period: number = 9): { k: number; d: number; j: number } => {
  const len = closes.length
  if (len < period) return { k: 50, d: 50, j: 50 }
  const high9 = Math.max(...highs.slice(-period))
  const low9 = Math.min(...lows.slice(-period))
  const rsv = high9 !== low9 ? ((closes[len - 1] - low9) / (high9 - low9)) * 100 : 50
  const k = rsv, d = k * 0.67 + 33, j = 3 * k - 2 * d
  return { k: Math.max(0, Math.min(100, k)), d: Math.max(0, Math.min(100, d)), j: Math.max(-20, Math.min(120, j)) }
}

const calculateBollingerBands = (prices: number[], period: number = 20): { upper: number; middle: number; lower: number } => {
  if (prices.length < period) {
    const avg = prices.reduce((a, b) => a + b, 0) / prices.length
    return { upper: avg * 1.02, middle: avg, lower: avg * 0.98 }
  }
  const slice = prices.slice(-period)
  const middle = slice.reduce((a, b) => a + b, 0) / period
  const variance = slice.reduce((acc, p) => acc + Math.pow(p - middle, 2), 0) / period
  const std = Math.sqrt(variance)
  return { upper: middle + std * 2, middle, lower: middle - std * 2 }
}

// ATR - Average True Range (波动率指标)
const calculateATR = (highs: number[], lows: number[], closes: number[], period: number = 14): number => {
  if (closes.length < period + 1) return 0
  let trSum = 0
  for (let i = closes.length - period; i < closes.length; i++) {
    const high = highs[i], low = lows[i], prevClose = closes[i - 1] || closes[i]
    const tr = Math.max(high - low, Math.abs(high - prevClose), Math.abs(low - prevClose))
    trSum += tr
  }
  return trSum / period
}

// OBV - On Balance Volume (能量潮)
const calculateOBV = (closes: number[], volumes: number[]): number => {
  if (closes.length < 2) return 0
  let obv = 0
  for (let i = 1; i < closes.length; i++) {
    if (closes[i] > closes[i - 1]) obv += volumes[i]
    else if (closes[i] < closes[i - 1]) obv -= volumes[i]
  }
  return obv
}

// Williams %R
const calculateWilliamsR = (highs: number[], lows: number[], closes: number[], period: number = 14): number => {
  if (closes.length < period) return -50
  const highN = Math.max(...highs.slice(-period))
  const lowN = Math.min(...lows.slice(-period))
  const close = closes[closes.length - 1]
  return highN !== lowN ? ((highN - close) / (highN - lowN)) * -100 : -50
}

// CCI - Commodity Channel Index
const calculateCCI = (highs: number[], lows: number[], closes: number[], period: number = 20): number => {
  if (closes.length < period) return 0
  const tps: number[] = []
  for (let i = closes.length - period; i < closes.length; i++) {
    tps.push((highs[i] + lows[i] + closes[i]) / 3)
  }
  const smaTP = tps.reduce((a, b) => a + b, 0) / period
  const meanDeviation = tps.reduce((acc, tp) => acc + Math.abs(tp - smaTP), 0) / period
  return meanDeviation !== 0 ? (tps[tps.length - 1] - smaTP) / (0.015 * meanDeviation) : 0
}

// ADX - Average Directional Index (趋势强度)
const calculateADX = (highs: number[], lows: number[], closes: number[], period: number = 14): number => {
  if (closes.length < period * 2) return 25
  // 简化的ADX计算
  const atr = calculateATR(highs, lows, closes, period)
  if (atr === 0) return 25
  let dmPlusSum = 0, dmMinusSum = 0
  for (let i = closes.length - period; i < closes.length; i++) {
    const upMove = highs[i] - (highs[i - 1] || highs[i])
    const downMove = (lows[i - 1] || lows[i]) - lows[i]
    if (upMove > downMove && upMove > 0) dmPlusSum += upMove
    if (downMove > upMove && downMove > 0) dmMinusSum += downMove
  }
  const diPlus = (dmPlusSum / period) / atr * 100
  const diMinus = (dmMinusSum / period) / atr * 100
  const diSum = diPlus + diMinus
  return diSum !== 0 ? Math.abs(diPlus - diMinus) / diSum * 100 : 25
}

// MFI - Money Flow Index (资金流量指标)
const calculateMFI = (highs: number[], lows: number[], closes: number[], volumes: number[], period: number = 14): number => {
  if (closes.length < period + 1) return 50
  let posFlow = 0, negFlow = 0
  for (let i = closes.length - period; i < closes.length; i++) {
    const tp = (highs[i] + lows[i] + closes[i]) / 3
    const prevTp = i > 0 ? (highs[i-1] + lows[i-1] + closes[i-1]) / 3 : tp
    const mf = tp * volumes[i]
    if (tp > prevTp) posFlow += mf
    else negFlow += mf
  }
  return negFlow !== 0 ? 100 - (100 / (1 + posFlow / negFlow)) : 100
}

// Stochastic RSI
const calculateStochRSI = (prices: number[], period: number = 14): number => {
  if (prices.length < period * 2) return 50
  const rsiValues: number[] = []
  for (let i = period; i < prices.length; i++) {
    const slice = prices.slice(i - period, i + 1)
    let gains = 0, losses = 0
    for (let j = 1; j < slice.length; j++) {
      const diff = slice[j] - slice[j - 1]
      if (diff > 0) gains += diff
      else losses -= diff
    }
    const avgGain = gains / period, avgLoss = losses / period
    rsiValues.push(avgLoss === 0 ? 100 : 100 - (100 / (1 + avgGain / avgLoss)))
  }
  if (rsiValues.length < period) return 50
  const minRSI = Math.min(...rsiValues.slice(-period))
  const maxRSI = Math.max(...rsiValues.slice(-period))
  const currentRSI = rsiValues[rsiValues.length - 1]
  return maxRSI !== minRSI ? ((currentRSI - minRSI) / (maxRSI - minRSI)) * 100 : 50
}

// EMA - Exponential Moving Average
const calculateEMA = (prices: number[], period: number): number => {
  if (prices.length === 0) return 0
  if (prices.length < period) return prices.reduce((a, b) => a + b, 0) / prices.length
  const multiplier = 2 / (period + 1)
  let ema = prices.slice(0, period).reduce((a, b) => a + b, 0) / period
  for (let i = period; i < prices.length; i++) {
    ema = (prices[i] - ema) * multiplier + ema
  }
  return ema
}

// VWAP - Volume Weighted Average Price
const calculateVWAP = (highs: number[], lows: number[], closes: number[], volumes: number[]): number => {
  if (closes.length === 0) return 0
  let tpv = 0, totalVol = 0
  for (let i = 0; i < closes.length; i++) {
    const tp = (highs[i] + lows[i] + closes[i]) / 3
    tpv += tp * volumes[i]
    totalVol += volumes[i]
  }
  return totalVol !== 0 ? tpv / totalVol : closes[closes.length - 1]
}

const calculateFibonacci = (data: CandleData[]): { levels: number[] } => {
  if (data.length < 10) return { levels: [] }
  const high = Math.max(...data.map(d => d.high))
  const low = Math.min(...data.map(d => d.low))
  const range = high - low
  return { levels: [high, high - range * 0.236, high - range * 0.382, high - range * 0.5, high - range * 0.618, high - range * 0.786, low] }
}

// ============================================================================
// 稳定的伪随机数生成器 (基于种子)
// ============================================================================
const seededRandom = (seed: number): number => {
  const x = Math.sin(seed * 9999) * 10000
  return x - Math.floor(x)
}

// 生成平滑的价格走势 (使用确定性算法)
const generateSmoothPrice = (basePrice: number, index: number, totalPoints: number, volatility: number): number => {
  // 使用多个正弦波叠加产生自然的价格走势
  const t = index / totalPoints
  const trend = Math.sin(t * Math.PI * 2) * volatility * 0.4          // 主趋势
  const cycle1 = Math.sin(t * Math.PI * 8) * volatility * 0.25        // 中周期
  const cycle2 = Math.sin(t * Math.PI * 20) * volatility * 0.15       // 短周期
  const cycle3 = Math.sin(t * Math.PI * 50) * volatility * 0.08       // 噪声
  const noise = (seededRandom(index * 7919) - 0.5) * volatility * 0.12 // 确定性噪声
  
  return basePrice * (1 + trend + cycle1 + cycle2 + cycle3 + noise)
}

// ============================================================================
// 时间周期配置 - 严格对齐时间边界
// ============================================================================
const TIMEFRAME_CONFIG: Record<string, { 
  intervalMs: number      // 每根K线的时间间隔(毫秒)
  points: number          // 显示多少根K线
  label: string           // 中文标签
  tickInterval: number    // 时间轴刻度间隔(秒)
  priceDecimals: number   // 价格小数位数
}> = {
  '1min': { 
    intervalMs: 1000,           // 每秒1根K线 (秒级数据)
    points: 60,                 // 显示60秒 = 1分钟
    label: '1分钟', 
    tickInterval: 10,           // 每10秒一个刻度
    priceDecimals: 2
  },
  '1h': { 
    intervalMs: 60 * 1000,      // 每分钟1根K线
    points: 60,                 // 显示60分钟 = 1小时
    label: '1小时', 
    tickInterval: 300,          // 每5分钟一个刻度
    priceDecimals: 2
  },
  '24h': { 
    intervalMs: 60 * 1000,      // 每分钟1根K线
    points: 1440,               // 显示1440分钟 = 24小时
    label: '24小时', 
    tickInterval: 3600,         // 每小时一个刻度
    priceDecimals: 2
  },
  '1month': { 
    intervalMs: 60 * 60 * 1000, // 每小时1根K线
    points: 720,                // 显示720小时 = 30天
    label: '1个月', 
    tickInterval: 86400,        // 每天一个刻度
    priceDecimals: 0
  },
  '1year': { 
    intervalMs: 24 * 60 * 60 * 1000, // 每天1根K线
    points: 365,                // 显示365天 = 1年
    label: '1年', 
    tickInterval: 2592000,      // 每月一个刻度
    priceDecimals: 0
  },
}

// 将时间戳对齐到周期边界
const floorToInterval = (timestamp: number, intervalMs: number): number => {
  return Math.floor(timestamp / intervalMs) * intervalMs
}

// 基于symbol生成唯一种子
const getSymbolSeed = (symbol: string): number => {
  let seed = 0
  for (let i = 0; i < symbol.length; i++) {
    seed = ((seed << 5) - seed) + symbol.charCodeAt(i)
    seed = seed & seed
  }
  return Math.abs(seed)
}

// ============================================================================
// 生成初始数据 - 严格对齐时间区间 + 稳定数据生成 + 每个币种独立走势
// ============================================================================
const generateInitialData = (basePrice: number, timeframe: string = '1h', symbol: string = 'BTC'): CandleData[] => {
  const config = TIMEFRAME_CONFIG[timeframe] || TIMEFRAME_CONFIG['1h']
  const intervalSec = config.intervalMs / 1000
  const points = config.points
  
  // 基于symbol生成独特的价格走势种子
  const symbolSeed = getSymbolSeed(symbol)
  
  const data: CandleData[] = []
  const now = floorToInterval(Date.now(), config.intervalMs) / 1000 // 对齐到周期边界
  
  // 根据时间周期调整波动率 + 每个币种有不同的基础波动特性
  const symbolVolatilityFactor = 0.8 + (symbolSeed % 100) / 200 // 0.8-1.3
  const volatility = (timeframe === '1min' ? 0.003 : 
                     timeframe === '1h' ? 0.008 : 
                     timeframe === '24h' ? 0.025 : 
                     timeframe === '1month' ? 0.08 : 0.15) * symbolVolatilityFactor
  
  // 收集历史数据用于指标计算
  const closes: number[] = [], highs: number[] = [], lows: number[] = [], volumes: number[] = []

  for (let i = 0; i < points; i++) {
    const time = (now - (points - i) * intervalSec) as Time
    
    // 使用稳定的价格生成算法 - 带有symbol种子偏移
    const close = generateSmoothPrice(basePrice, i + symbolSeed, points, volatility)
    const prevClose = i > 0 ? closes[i - 1] : close
    const open = prevClose
    
    // 生成稳定的高低价 - 带有symbol种子偏移
    const range = Math.abs(close - open) + basePrice * volatility * 0.1 * (0.5 + seededRandom((i + symbolSeed) * 1009))
    const high = Math.max(open, close) + range * (0.3 + seededRandom((i + symbolSeed) * 2003) * 0.4)
    const low = Math.min(open, close) - range * (0.3 + seededRandom((i + symbolSeed) * 3001) * 0.4)
    
    // 生成稳定的成交量 - 带有symbol种子偏移
    const volumeBase = 500000 + basePrice * 10
    const volume = volumeBase * (0.5 + seededRandom((i + symbolSeed) * 4007) + Math.abs(close - open) / basePrice * 20)

    closes.push(close); highs.push(high); lows.push(low); volumes.push(volume)
    
    // 计算所有技术指标
    const ma5 = i >= 4 ? closes.slice(-5).reduce((a, b) => a + b, 0) / 5 : close
    const ma10 = i >= 9 ? closes.slice(-10).reduce((a, b) => a + b, 0) / 10 : close
    const ma20 = i >= 19 ? closes.slice(-20).reduce((a, b) => a + b, 0) / 20 : close
    const ma60 = i >= 59 ? closes.slice(-60).reduce((a, b) => a + b, 0) / 60 : close
    const ema12 = calculateEMA(closes, 12)
    const ema26 = calculateEMA(closes, 26)
    
    // 动量指标
    const rsi = calculateRSI(closes)
    const { macd, signal, histogram } = calculateMACD(closes)
    const { k, d, j } = calculateKDJ(highs, lows, closes)
    const stochRsi = calculateStochRSI(closes)
    
    // 波动率指标
    const bb = calculateBollingerBands(closes)
    const atr = calculateATR(highs, lows, closes)
    
    // 成交量指标
    const obv = calculateOBV(closes, volumes)
    const vwap = calculateVWAP(highs, lows, closes, volumes)
    
    // 其他专业指标
    const williamsR = calculateWilliamsR(highs, lows, closes)
    const cci = calculateCCI(highs, lows, closes)
    const adx = calculateADX(highs, lows, closes)
    const mfi = calculateMFI(highs, lows, closes, volumes)
    
    // 资金流
    const isUp = close > open
    const inflow = volume * (isUp ? 0.6 : 0.35)
    const outflow = volume * (isUp ? 0.35 : 0.6)
    const netFlow = inflow - outflow

    data.push({ 
      time, open, high, low, close, volume,
      ma5, ma10, ma20, ma60, ema12, ema26,
      rsi, macd, signal, histogram, k, d, j, stochRsi,
      upperBB: bb.upper, middleBB: bb.middle, lowerBB: bb.lower, atr,
      obv, vwap, williamsR, cci, adx, mfi,
      inflow, outflow, netFlow,
      changePercent: i > 0 ? ((close - closes[0]) / closes[0]) * 100 : 0
    })
  }
  return data
}

const calculateMA = (data: CandleData[], period: number): LineData[] => {
  const result: LineData[] = []
  for (let i = period - 1; i < data.length; i++) {
    let sum = 0
    for (let j = 0; j < period; j++) sum += data[i - j].close
    result.push({ time: data[i].time, value: sum / period })
  }
  return result
}

// ============================================================================
// 主组件
// ============================================================================
export function LightweightChart({
  symbol,
  basePrice,
  chartType = 'candle',
  timeframe = '1H',
  height = 300
}: LightweightChartProps) {
  const chartContainerRef = useRef<HTMLDivElement>(null)
  const chartRef = useRef<IChartApi | null>(null)
  const seriesRefs = useRef<Map<string, ISeriesApi<any>>>(new Map())
  const dataRef = useRef<CandleData[]>([])
  const tickCountRef = useRef(0)
  const lastSymbolRef = useRef('')
  const lastChartTypeRef = useRef('')
  const lastTimeframeRef = useRef('')

  const [currentPrice, setCurrentPrice] = useState(basePrice)
  const [priceChange, setPriceChange] = useState(0)
  const [hoverInfo, setHoverInfo] = useState<HoverInfo | null>(null)
  const [mousePos, setMousePos] = useState({ x: 0, y: 0 })
  
  // 风险指标状态
  const [riskMetrics, setRiskMetrics] = useState({
    dayPnL: 3650,
    dayDD: -1120,
    weekPnL: 28500,
    weekMaxDD: -9800,
    varPercent: 2.5,
    riskPercent: 15,
    openExposure: 68,
    sharpeRatio: 1.85,
    winRate: 62.5,
    profitFactor: 1.92
  })
  const [showRiskPanel, setShowRiskPanel] = useState(false)
  // Position at top-right, after ONLINE status area
  const [riskPanelPos, setRiskPanelPos] = useState({ x: 950, y: 50 })
  const [isDraggingRisk, setIsDraggingRisk] = useState(false)
  const [dragOffset, setDragOffset] = useState({ x: 0, y: 0 })
  
  // 数据质量状态
  const [dataQuality, setDataQuality] = useState({
    feedStatus: 'connected' as 'connected' | 'delayed' | 'stale' | 'disconnected',
    packetLoss: 0.02,
    lastUpdate: Date.now(),
    outageCount: 0,
    latencyMs: 12
  })
  const [showDataPanel, setShowDataPanel] = useState(false)
  // Position at top-right, next to Risk panel
  const [dataPanelPos, setDataPanelPos] = useState({ x: 1140, y: 50 })
  const [isDraggingData, setIsDraggingData] = useState(false)
  
  // 事件标记 - 增强版含8+2判断依据
  interface EventMarker {
    time: number
    type: 'news' | 'liq' | 'spike' | 'halt'
    label: string
    // 详细信息
    source: string           // 信息来源
    confidence: number       // 可信度 0-100
    // 8项基本判断依据
    basicReasons: {
      name: string
      value: string
      passed: boolean
    }[]
    // 2项专业判断
    expertReasons: {
      name: string
      analysis: string
      confidence: number
    }[]
    impact: 'HIGH' | 'MEDIUM' | 'LOW'  // 影响程度
    relatedAssets?: string[] // 关联资产
    timestamp: string        // 时间戳
    verified: boolean        // 是否已验证
  }
  
  const [eventMarkers, setEventMarkers] = useState<EventMarker[]>([])
  const [hoveredEvent, setHoveredEvent] = useState<EventMarker | null>(null)
  
  // 拖动处理
  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (isDraggingRisk) {
      setRiskPanelPos({ x: e.clientX - dragOffset.x, y: e.clientY - dragOffset.y })
    }
    if (isDraggingData) {
      setDataPanelPos({ x: e.clientX - dragOffset.x, y: e.clientY - dragOffset.y })
    }
  }, [isDraggingRisk, isDraggingData, dragOffset])
  
  const handleMouseUp = useCallback(() => {
    setIsDraggingRisk(false)
    setIsDraggingData(false)
  }, [])
  
  // 模拟风险数据更新
  useEffect(() => {
    const interval = setInterval(() => {
      setRiskMetrics(prev => ({
        ...prev,
        dayPnL: prev.dayPnL + (Math.random() - 0.48) * 100,
        dayDD: Math.min(prev.dayDD, prev.dayDD - Math.random() * 50),
        openExposure: Math.max(0, Math.min(100, prev.openExposure + (Math.random() - 0.5) * 5)),
        riskPercent: Math.max(0, Math.min(100, prev.riskPercent + (Math.random() - 0.5) * 2))
      }))
      setDataQuality(prev => ({
        ...prev,
        lastUpdate: Date.now(),
        latencyMs: Math.max(5, Math.min(100, prev.latencyMs + (Math.random() - 0.5) * 10)),
        packetLoss: Math.max(0, Math.min(5, prev.packetLoss + (Math.random() - 0.5) * 0.1))
      }))
      // 随机生成事件标记（模拟）- 增强版含8+2判断依据
      if (Math.random() < 0.05) {
        const eventTypes: Array<'news' | 'liq' | 'spike' | 'halt'> = ['news', 'liq', 'spike', 'halt']
        const eventLabels = {
          news: ['Fed公告', 'CPI数据', '财报发布', '政策变动'],
          liq: ['大单成交', '流动性冲击', '机构扫货'],
          spike: ['异常波动', '价格突破', '闪崩预警'],
          halt: ['临时停牌', '熔断机制', '系统维护']
        }
        const eventSources = {
          news: ['Bloomberg Terminal', 'Reuters', 'Fed官网', '财经日历'],
          liq: ['交易所数据', '大宗交易监控', '链上分析', 'Whale Alert'],
          spike: ['价格算法检测', '波动率监控', '技术指标系统'],
          halt: ['交易所公告', '监管机构', '系统监控']
        }
        
        // 8项基本判断依据模板
        const basicReasonTemplates = {
          news: [
            { name: '官方时间匹配', getValue: () => `${Math.floor(Math.random() * 5)}秒偏差` },
            { name: '多源验证', getValue: () => `${Math.floor(2 + Math.random() * 4)}家媒体` },
            { name: '市场反应', getValue: () => `${(Math.random() * 2).toFixed(2)}%波动` },
            { name: '历史模式匹配', getValue: () => `${Math.floor(70 + Math.random() * 30)}%相似` },
            { name: '权威来源确认', getValue: () => Math.random() > 0.3 ? '已确认' : '待确认' },
            { name: '时间窗口验证', getValue: () => Math.random() > 0.2 ? '符合预期' : '意外发布' },
            { name: '机构资金流向', getValue: () => `${Math.random() > 0.5 ? '+' : '-'}$${(Math.random() * 500).toFixed(0)}M` },
            { name: '舆情热度指数', getValue: () => `${Math.floor(60 + Math.random() * 40)}/100` }
          ],
          liq: [
            { name: '成交量倍数', getValue: () => `${(5 + Math.random() * 15).toFixed(1)}x日均` },
            { name: '价格滑点', getValue: () => `${(Math.random() * 0.5).toFixed(3)}%` },
            { name: 'VWAP偏离', getValue: () => `${(Math.random() * 1.5).toFixed(2)}%` },
            { name: '订单簿深度', getValue: () => `减少${Math.floor(30 + Math.random() * 50)}%` },
            { name: '多所同步性', getValue: () => `${Math.floor(2 + Math.random() * 5)}家交易所` },
            { name: '链上转账', getValue: () => `${Math.floor(Math.random() * 5000)}枚大额` },
            { name: '暗池流量', getValue: () => `${Math.floor(20 + Math.random() * 40)}%占比` },
            { name: '期权OI变化', getValue: () => `${Math.random() > 0.5 ? '+' : '-'}${Math.floor(Math.random() * 30)}%` }
          ],
          spike: [
            { name: 'ATR倍数', getValue: () => `${(2 + Math.random() * 3).toFixed(1)}倍标准差` },
            { name: 'RSI极值', getValue: () => `${Math.floor(Math.random() > 0.5 ? 75 + Math.random() * 25 : Math.random() * 25)}` },
            { name: '布林带状态', getValue: () => Math.random() > 0.5 ? '上轨突破' : '下轨突破' },
            { name: '成交量放大', getValue: () => `${(3 + Math.random() * 7).toFixed(1)}x` },
            { name: '多周期共振', getValue: () => `${Math.floor(2 + Math.random() * 3)}个周期` },
            { name: '流动性真空', getValue: () => Math.random() > 0.4 ? '检测到' : '正常' },
            { name: '止损密集区', getValue: () => `$${(Math.random() * 1000 + 50000).toFixed(0)}附近` },
            { name: '杠杆清算风险', getValue: () => `$${(Math.random() * 200).toFixed(0)}M待清算` }
          ],
          halt: [
            { name: '官方通知', getValue: () => Math.random() > 0.2 ? '已发布' : '未发布' },
            { name: '涨跌幅触发', getValue: () => `${Math.random() > 0.5 ? '+' : '-'}${Math.floor(5 + Math.random() * 15)}%` },
            { name: '熔断机制', getValue: () => Math.random() > 0.3 ? 'Level 1' : 'Level 2' },
            { name: '监管状态', getValue: () => Math.random() > 0.5 ? '正常' : '关注中' },
            { name: 'API连接', getValue: () => `延迟${Math.floor(Math.random() * 500)}ms` },
            { name: '数据源状态', getValue: () => `${Math.floor(80 + Math.random() * 20)}%可用` },
            { name: '流动性评估', getValue: () => Math.random() > 0.3 ? '充足' : '不足' },
            { name: '系统负载', getValue: () => `${Math.floor(40 + Math.random() * 50)}%` }
          ]
        }
        
        // 2项专业判断模板
        const expertReasonTemplates = {
          news: [
            { name: '宏观经济分析师', getAnalysis: () => Math.random() > 0.5 ? '政策转向信号明确，建议关注利率路径' : '符合市场预期，影响有限' },
            { name: '量化策略系统', getAnalysis: () => Math.random() > 0.5 ? '历史相似事件回测显示正向收益概率68%' : '波动率预计上升，建议降低杠杆' }
          ],
          liq: [
            { name: '大宗交易专家', getAnalysis: () => Math.random() > 0.5 ? '疑似机构建仓行为，后续或有跟风盘' : '可能为对冲平仓，短期影响' },
            { name: '链上数据分析', getAnalysis: () => Math.random() > 0.5 ? '巨鲸地址活跃，长期持有意图明显' : '交易所流入增加，抛压风险' }
          ],
          spike: [
            { name: '技术面分析师', getAnalysis: () => Math.random() > 0.5 ? '关键支撑/阻力位突破，趋势延续概率高' : '假突破风险存在，等待确认' },
            { name: '风险控制系统', getAnalysis: () => Math.random() > 0.5 ? '波动率处于历史极值，建议减仓观望' : '杠杆清算链可能触发，注意风控' }
          ],
          halt: [
            { name: '合规监控系统', getAnalysis: () => Math.random() > 0.5 ? '常规性停牌，预计短时恢复' : '异常情况待核实，保持警惕' },
            { name: '流动性管理', getAnalysis: () => Math.random() > 0.5 ? '备用流动性通道可用' : '建议分散至其他交易所' }
          ]
        }
        
        const type = eventTypes[Math.floor(Math.random() * eventTypes.length)]
        const labels = eventLabels[type]
        const sources = eventSources[type]
        
        // 生成8项基本判断依据
        const basicTemplates = basicReasonTemplates[type]
        const basicReasons = basicTemplates.map(t => {
          const passed = Math.random() > 0.25 // 75%通过率
          return {
            name: t.name,
            value: t.getValue(),
            passed
          }
        })
        
        // 生成2项专业判断
        const expertTemplates = expertReasonTemplates[type]
        const expertReasons = expertTemplates.map(t => ({
          name: t.name,
          analysis: t.getAnalysis(),
          confidence: Math.floor(60 + Math.random() * 35)
        }))
        
        // 计算可信度 = 基本依据通过率 * 0.6 + 专业判断平均置信度 * 0.4
        const basicPassRate = basicReasons.filter(r => r.passed).length / 8
        const expertAvgConfidence = expertReasons.reduce((sum, r) => sum + r.confidence, 0) / 2
        const confidence = Math.floor(basicPassRate * 60 + expertAvgConfidence * 0.4)
        
        // 验证状态：可信度>75且基本依据通过>6项
        const verified = confidence >= 75 && basicReasons.filter(r => r.passed).length >= 6
        
        const newEvent: EventMarker = {
          time: Date.now(),
          type,
          label: labels[Math.floor(Math.random() * labels.length)],
          source: sources[Math.floor(Math.random() * sources.length)],
          confidence,
          basicReasons,
          expertReasons,
          impact: confidence >= 80 ? 'HIGH' : confidence >= 60 ? 'MEDIUM' : 'LOW',
          relatedAssets: ['BTC', 'ETH', 'SPY'].slice(0, Math.floor(Math.random() * 3) + 1),
          timestamp: new Date().toLocaleString('zh-CN'),
          verified
        }
        
        setEventMarkers(prev => [...prev.slice(-5), newEvent])
      }
    }, 2000)
    return () => clearInterval(interval)
  }, [])

  // 根据timeframe格式化时间 - 严格对应时间区间
  const formatTime = (time: Time): string => {
    const date = new Date((time as number) * 1000)
    
    switch (timeframe) {
      case '1min':
        // 1分钟: 显示 时:分:秒
        return date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit' })
      case '1h':
        // 1小时: 显示 时:分
        return date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
      case '24h':
        // 24小时: 显示 日 时:分
        return date.toLocaleDateString('zh-CN', { day: '2-digit' }) + ' ' + 
               date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
      case '1month':
        // 1个月: 显示 月/日 时
        return date.toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit' }) + ' ' +
               date.toLocaleTimeString('zh-CN', { hour: '2-digit' }) + '时'
      case '1year':
        // 1年: 显示 年/月/日
        return date.toLocaleDateString('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit' })
      default:
        return date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
    }
  }

  const handleCrosshairMove = useCallback((param: MouseEventParams) => {
    if (!param.time || !param.point) { setHoverInfo(null); return }
    // 更新鼠标位置用于悬浮卡片
    if (param.point) {
      setMousePos({ x: param.point.x, y: param.point.y })
    }
    const candleData = dataRef.current.find(d => d.time === param.time)
    if (candleData) {
      const change = candleData.close - candleData.open
      setHoverInfo({
        time: formatTime(candleData.time), 
        open: candleData.open, high: candleData.high, low: candleData.low, close: candleData.close,
        volume: candleData.volume || 0, change, 
        changePercent: candleData.open !== 0 ? (change / candleData.open) * 100 : 0,
        // 移动平均线
        ma5: candleData.ma5, ma10: candleData.ma10, ma20: candleData.ma20, ma60: candleData.ma60,
        ema12: candleData.ema12, ema26: candleData.ema26,
        // 动量指标
        rsi: candleData.rsi, macd: candleData.macd, signal: candleData.signal, histogram: candleData.histogram,
        k: candleData.k, d: candleData.d, j: candleData.j, stochRsi: candleData.stochRsi,
        // 波动率
        upperBB: candleData.upperBB, middleBB: candleData.middleBB, lowerBB: candleData.lowerBB, atr: candleData.atr,
        // 成交量指标
        obv: candleData.obv, vwap: candleData.vwap,
        // 其他专业指标
        williamsR: candleData.williamsR, cci: candleData.cci, adx: candleData.adx, mfi: candleData.mfi,
        // 资金流
        inflow: candleData.inflow, outflow: candleData.outflow, netFlow: candleData.netFlow
      })
    }
  }, [timeframe])

  const clearSeries = useCallback(() => {
    seriesRefs.current.forEach((series) => { try { chartRef.current?.removeSeries(series) } catch {} })
    seriesRefs.current.clear()
  }, [])

  // 创建图表
  useEffect(() => {
    if (!chartContainerRef.current) return
    
    // 根据时间周期设置不同的时间轴显示
    const showSeconds = timeframe === '1min'
    // 优化K线宽度: 增大间距使K线更清晰可读
    const barSpacing = timeframe === '1year' ? 6 : timeframe === '1month' ? 8 : timeframe === '24h' ? 5 : 12
    
    const chart = createChart(chartContainerRef.current, {
      width: chartContainerRef.current.clientWidth, 
      height: height,
      layout: { 
        background: { type: ColorType.Solid, color: '#0a0a0a' }, 
        textColor: '#888', 
        fontFamily: 'SF Mono, Monaco, monospace', 
        fontSize: 10,
        attributionLogo: false  // 隐藏 TradingView logo
      },
      grid: { vertLines: { color: '#1a1a1a' }, horzLines: { color: '#1a1a1a' } },
      crosshair: { mode: CrosshairMode.Normal, vertLine: { color: '#555', width: 1, style: 3, labelBackgroundColor: '#333' }, horzLine: { color: '#555', width: 1, style: 3, labelBackgroundColor: '#333' } },
      rightPriceScale: { 
        borderColor: '#333', 
        scaleMargins: { top: 0.1, bottom: 0.2 }, 
        autoScale: true,
        mode: 0, // 正常模式
      },
      timeScale: { 
        borderColor: '#333', 
        timeVisible: true, 
        secondsVisible: showSeconds, 
        rightOffset: 3, 
        barSpacing: barSpacing, 
        minBarSpacing: 1, 
        fixLeftEdge: true, 
        fixRightEdge: false 
      },
      handleScroll: { mouseWheel: true, pressedMouseMove: true, horzTouchDrag: true, vertTouchDrag: false },
      handleScale: { axisPressedMouseMove: true, mouseWheel: true, pinch: true },
      autoSize: true
    })
    chartRef.current = chart
    chart.subscribeCrosshairMove(handleCrosshairMove)
    const handleResize = () => { if (chartContainerRef.current) chart.applyOptions({ width: chartContainerRef.current.clientWidth }) }
    window.addEventListener('resize', handleResize)
    return () => { window.removeEventListener('resize', handleResize); chart.unsubscribeCrosshairMove(handleCrosshairMove); chart.remove() }
  }, [height, handleCrosshairMove, timeframe])

  // 设置图表系列
  const setupChart = useCallback((chart: IChartApi, data: CandleData[], type: string) => {
    clearSeries()
    
    if (type === 'candle' || type === 'indicators' || type === 'fibonacci' || type === 'profile' || type === 'trend') {
      const candleSeries = chart.addSeries(CandlestickSeries, { upColor: '#00ff88', downColor: '#ff4444', borderUpColor: '#00ff88', borderDownColor: '#ff4444', wickUpColor: '#00ff88', wickDownColor: '#ff4444' })
      seriesRefs.current.set('candle', candleSeries)
      candleSeries.setData(data.map(d => ({ time: d.time, open: d.open, high: d.high, low: d.low, close: d.close })))
      
      const volumeSeries = chart.addSeries(HistogramSeries, { priceFormat: { type: 'volume' }, priceScaleId: 'volume' })
      seriesRefs.current.set('volume', volumeSeries)
      chart.priceScale('volume').applyOptions({ scaleMargins: { top: 0.8, bottom: 0 } })
      volumeSeries.setData(data.map(d => ({ time: d.time, value: d.volume || 0, color: d.close >= d.open ? '#00ff8860' : '#ff444460' })))
      
      // 多周期移动平均线系统 MA5/MA10/MA20/MA60
      const ma5 = chart.addSeries(LineSeries, { color: '#ffaa00', lineWidth: 1, priceLineVisible: false, lastValueVisible: false, title: 'MA5' })
      const ma10 = chart.addSeries(LineSeries, { color: '#00aaff', lineWidth: 1, priceLineVisible: false, lastValueVisible: false, title: 'MA10' })
      const ma20 = chart.addSeries(LineSeries, { color: '#ff00ff', lineWidth: 1, priceLineVisible: false, lastValueVisible: false, title: 'MA20' })
      const ma60 = chart.addSeries(LineSeries, { color: '#00ff88', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false, title: 'MA60' })
      seriesRefs.current.set('ma5', ma5); seriesRefs.current.set('ma10', ma10)
      seriesRefs.current.set('ma20', ma20); seriesRefs.current.set('ma60', ma60)
      ma5.setData(calculateMA(data, 5)); ma10.setData(calculateMA(data, 10))
      ma20.setData(calculateMA(data, 20)); ma60.setData(calculateMA(data, 60))
      
      // EMA12/EMA26 (用于MACD)
      const ema12Line = chart.addSeries(LineSeries, { color: '#888', lineWidth: 1, lineStyle: 3, priceLineVisible: false, lastValueVisible: false, visible: false })
      const ema26Line = chart.addSeries(LineSeries, { color: '#666', lineWidth: 1, lineStyle: 3, priceLineVisible: false, lastValueVisible: false, visible: false })
      seriesRefs.current.set('ema12', ema12Line); seriesRefs.current.set('ema26', ema26Line)
      ema12Line.setData(data.filter(d => d.ema12).map(d => ({ time: d.time, value: d.ema12! })))
      ema26Line.setData(data.filter(d => d.ema26).map(d => ({ time: d.time, value: d.ema26! })))

      if (type === 'candle') {
        // 布林带 (K线模式也显示)
        const bbU = chart.addSeries(LineSeries, { color: '#55555580', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
        const bbL = chart.addSeries(LineSeries, { color: '#55555580', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
        seriesRefs.current.set('bbU', bbU); seriesRefs.current.set('bbL', bbL)
        bbU.setData(data.map(d => ({ time: d.time, value: d.upperBB || d.close })))
        bbL.setData(data.map(d => ({ time: d.time, value: d.lowerBB || d.close })))
      }

      if (type === 'indicators') {
        const bbU = chart.addSeries(LineSeries, { color: '#888', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
        const bbM = chart.addSeries(LineSeries, { color: '#666', lineWidth: 1, priceLineVisible: false, lastValueVisible: false })
        const bbL = chart.addSeries(LineSeries, { color: '#888', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
        seriesRefs.current.set('bbU', bbU); seriesRefs.current.set('bbM', bbM); seriesRefs.current.set('bbL', bbL)
        bbU.setData(data.map(d => ({ time: d.time, value: d.upperBB || d.close })))
        bbM.setData(data.map(d => ({ time: d.time, value: d.middleBB || d.close })))
        bbL.setData(data.map(d => ({ time: d.time, value: d.lowerBB || d.close })))
      }

      if (type === 'fibonacci') {
        const fib = calculateFibonacci(data)
        const colors = ['#ff0000', '#ff8800', '#ffff00', '#00ff00', '#00ffff', '#0088ff', '#ff00ff']
        fib.levels.forEach((level, i) => {
          const line = chart.addSeries(LineSeries, { color: colors[i], lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: true })
          seriesRefs.current.set(`fib${i}`, line)
          line.setData(data.map(d => ({ time: d.time, value: level })))
        })
      }

      if (type === 'trend') {
        const n = data.length; let sumX = 0, sumY = 0, sumXY = 0, sumX2 = 0
        data.forEach((d, i) => { sumX += i; sumY += d.close; sumXY += i * d.close; sumX2 += i * i })
        const slope = (n * sumXY - sumX * sumY) / (n * sumX2 - sumX * sumX), intercept = (sumY - slope * sumX) / n
        const trendLine = chart.addSeries(LineSeries, { color: '#ffaa00', lineWidth: 2, lineStyle: 2, priceLineVisible: false })
        seriesRefs.current.set('trend', trendLine)
        trendLine.setData(data.map((d, i) => ({ time: d.time, value: intercept + slope * i })))
      }
    }
    else if (type === 'area') {
      const isUp = data.length > 1 && data[data.length-1].close >= data[0].open
      const area = chart.addSeries(AreaSeries, { lineColor: isUp ? '#00ff88' : '#ff4444', topColor: isUp ? '#00ff8840' : '#ff444440', bottomColor: isUp ? '#00ff8810' : '#ff444410', lineWidth: 2 })
      seriesRefs.current.set('area', area)
      area.setData(data.map(d => ({ time: d.time, value: d.close })))
      const ma5 = chart.addSeries(LineSeries, { color: '#ffaa00', lineWidth: 1, priceLineVisible: false, lastValueVisible: false })
      const ma10 = chart.addSeries(LineSeries, { color: '#00aaff', lineWidth: 1, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('ma5', ma5); seriesRefs.current.set('ma10', ma10)
      ma5.setData(calculateMA(data, 5)); ma10.setData(calculateMA(data, 10))
    }
    else if (type === 'macd') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const macdLine = chart.addSeries(LineSeries, { color: '#00aaff', lineWidth: 2, priceLineVisible: false })
      const signalLine = chart.addSeries(LineSeries, { color: '#ffaa00', lineWidth: 2, priceLineVisible: false })
      const hist = chart.addSeries(HistogramSeries, { priceFormat: { type: 'price' } })
      const zero = chart.addSeries(LineSeries, { color: '#444', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('macdLine', macdLine); seriesRefs.current.set('signalLine', signalLine); seriesRefs.current.set('hist', hist); seriesRefs.current.set('zero', zero)
      macdLine.setData(data.map(d => ({ time: d.time, value: d.macd || 0 })))
      signalLine.setData(data.map(d => ({ time: d.time, value: d.signal || 0 })))
      hist.setData(data.map(d => ({ time: d.time, value: d.histogram || 0, color: (d.histogram || 0) >= 0 ? '#00ff88' : '#ff4444' })))
      zero.setData(data.map(d => ({ time: d.time, value: 0 })))
    }
    else if (type === 'rsi') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const rsiLine = chart.addSeries(LineSeries, { color: '#00aaff', lineWidth: 2, priceLineVisible: false })
      const ob = chart.addSeries(LineSeries, { color: '#ff4444', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      const os = chart.addSeries(LineSeries, { color: '#00ff88', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      const mid = chart.addSeries(LineSeries, { color: '#444', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('rsi', rsiLine); seriesRefs.current.set('ob', ob); seriesRefs.current.set('os', os); seriesRefs.current.set('mid', mid)
      rsiLine.setData(data.map(d => ({ time: d.time, value: d.rsi || 50 })))
      ob.setData(data.map(d => ({ time: d.time, value: 70 })))
      os.setData(data.map(d => ({ time: d.time, value: 30 })))
      mid.setData(data.map(d => ({ time: d.time, value: 50 })))
    }
    else if (type === 'kdj') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const kLine = chart.addSeries(LineSeries, { color: '#ffaa00', lineWidth: 2, priceLineVisible: false })
      const dLine = chart.addSeries(LineSeries, { color: '#00aaff', lineWidth: 2, priceLineVisible: false })
      const jLine = chart.addSeries(LineSeries, { color: '#ff00ff', lineWidth: 1, priceLineVisible: false })
      const ob = chart.addSeries(LineSeries, { color: '#ff444480', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      const os = chart.addSeries(LineSeries, { color: '#00ff8880', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('kLine', kLine); seriesRefs.current.set('dLine', dLine); seriesRefs.current.set('jLine', jLine)
      kLine.setData(data.map(d => ({ time: d.time, value: d.k || 50 })))
      dLine.setData(data.map(d => ({ time: d.time, value: d.d || 50 })))
      jLine.setData(data.map(d => ({ time: d.time, value: d.j || 50 })))
      ob.setData(data.map(d => ({ time: d.time, value: 80 }))); os.setData(data.map(d => ({ time: d.time, value: 20 })))
    }
    else if (type === 'volume') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const vol = chart.addSeries(HistogramSeries, { priceFormat: { type: 'volume' } })
      seriesRefs.current.set('vol', vol)
      vol.setData(data.map(d => ({ time: d.time, value: d.volume || 0, color: d.close >= d.open ? '#00ff88' : '#ff4444' })))
      const volMA: LineData[] = []; for (let i = 4; i < data.length; i++) { let s = 0; for (let j = 0; j < 5; j++) s += data[i-j].volume || 0; volMA.push({ time: data[i].time, value: s / 5 }) }
      const volMALine = chart.addSeries(LineSeries, { color: '#ffaa00', lineWidth: 1, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('volMA', volMALine); volMALine.setData(volMA)
    }
    else if (type === 'depth') {
      const bid = chart.addSeries(AreaSeries, { lineColor: '#00ff88', topColor: '#00ff8840', bottomColor: '#00ff8810', lineWidth: 1 })
      const ask = chart.addSeries(AreaSeries, { lineColor: '#ff4444', topColor: '#ff444440', bottomColor: '#ff444410', lineWidth: 1 })
      seriesRefs.current.set('bid', bid); seriesRefs.current.set('ask', ask)
      bid.setData(data.map((d, i) => ({ time: d.time, value: (d.volume || 0) * 0.5 * (1 + Math.sin(i * 0.2)) })))
      ask.setData(data.map((d, i) => ({ time: d.time, value: (d.volume || 0) * 0.5 * (1 + Math.cos(i * 0.2)) })))
    }
    else if (type === 'flow') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const inflow = chart.addSeries(HistogramSeries, { priceFormat: { type: 'volume' }, priceScaleId: 'flow' })
      chart.priceScale('flow').applyOptions({ scaleMargins: { top: 0.5, bottom: 0 } })
      const outflow = chart.addSeries(HistogramSeries, { priceFormat: { type: 'volume' }, priceScaleId: 'flow2' })
      chart.priceScale('flow2').applyOptions({ scaleMargins: { top: 0, bottom: 0.5 } })
      const netLine = chart.addSeries(LineSeries, { color: '#ffaa00', lineWidth: 2, priceLineVisible: false })
      seriesRefs.current.set('inflow', inflow); seriesRefs.current.set('outflow', outflow); seriesRefs.current.set('netFlow', netLine)
      inflow.setData(data.map(d => ({ time: d.time, value: d.inflow || 0, color: '#00ff8880' })))
      outflow.setData(data.map(d => ({ time: d.time, value: -(d.outflow || 0), color: '#ff444480' })))
      netLine.setData(data.map(d => ({ time: d.time, value: d.netFlow || 0 })))
    }
    else if (type === 'heatmap') {
      const maxVol = Math.max(...data.map(d => d.volume || 1))
      const heat = chart.addSeries(BaselineSeries, { baseValue: { type: 'price', price: basePrice }, topLineColor: '#00ff88', topFillColor1: '#00ff8880', topFillColor2: '#00ff8820', bottomLineColor: '#ff4444', bottomFillColor1: '#ff444420', bottomFillColor2: '#ff444480', lineWidth: 2 })
      seriesRefs.current.set('heat', heat)
      heat.setData(data.map(d => ({ time: d.time, value: d.close })))
      const volHeat = chart.addSeries(HistogramSeries, { priceFormat: { type: 'volume' }, priceScaleId: 'volHeat' })
      chart.priceScale('volHeat').applyOptions({ scaleMargins: { top: 0.8, bottom: 0 } })
      seriesRefs.current.set('volHeat', volHeat)
      volHeat.setData(data.map(d => { const i = (d.volume || 0) / maxVol; return { time: d.time, value: d.volume || 0, color: `rgba(${Math.floor(255*i)}, ${Math.floor(100*(1-i))}, 50, 0.8)` } }))
    }
    // ATR - 平均真实波幅 (波动率指标)
    else if (type === 'atr') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const atrLine = chart.addSeries(LineSeries, { color: '#ff8800', lineWidth: 2, priceLineVisible: true, lastValueVisible: true, title: 'ATR' })
      const atrArea = chart.addSeries(AreaSeries, { lineColor: '#ff880060', topColor: '#ff880040', bottomColor: '#ff880010', lineWidth: 1, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('atrLine', atrLine); seriesRefs.current.set('atrArea', atrArea)
      atrLine.setData(data.map(d => ({ time: d.time, value: d.atr || 0 })))
      atrArea.setData(data.map(d => ({ time: d.time, value: d.atr || 0 })))
    }
    // VWAP - 成交量加权平均价 (趋势指标)
    else if (type === 'vwap') {
      const candleSeries = chart.addSeries(CandlestickSeries, { upColor: '#00ff88', downColor: '#ff4444', borderUpColor: '#00ff88', borderDownColor: '#ff4444', wickUpColor: '#00ff88', wickDownColor: '#ff4444' })
      seriesRefs.current.set('candle', candleSeries)
      candleSeries.setData(data.map(d => ({ time: d.time, open: d.open, high: d.high, low: d.low, close: d.close })))
      const vwapLine = chart.addSeries(LineSeries, { color: '#00ffff', lineWidth: 2, priceLineVisible: true, lastValueVisible: true, title: 'VWAP' })
      const upperBand = chart.addSeries(LineSeries, { color: '#00ffff60', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      const lowerBand = chart.addSeries(LineSeries, { color: '#00ffff60', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('vwapLine', vwapLine); seriesRefs.current.set('vwapUpper', upperBand); seriesRefs.current.set('vwapLower', lowerBand)
      vwapLine.setData(data.map(d => ({ time: d.time, value: d.vwap || d.close })))
      // VWAP bands: 1 std deviation from VWAP
      const vwapStd = data.length > 20 ? Math.sqrt(data.slice(-20).reduce((s, d) => s + Math.pow((d.close - (d.vwap || d.close)), 2), 0) / 20) : basePrice * 0.01
      upperBand.setData(data.map(d => ({ time: d.time, value: (d.vwap || d.close) + vwapStd })))
      lowerBand.setData(data.map(d => ({ time: d.time, value: (d.vwap || d.close) - vwapStd })))
    }
    // OBV - 能量潮 (成交量指标)
    else if (type === 'obv') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const obvLine = chart.addSeries(LineSeries, { color: '#00aaff', lineWidth: 2, priceLineVisible: true, lastValueVisible: true, title: 'OBV' })
      const obvArea = chart.addSeries(AreaSeries, { lineColor: '#00aaff60', topColor: '#00aaff40', bottomColor: '#00aaff10', lineWidth: 1, priceLineVisible: false, lastValueVisible: false })
      const obvMA = chart.addSeries(LineSeries, { color: '#ffaa00', lineWidth: 1, priceLineVisible: false, lastValueVisible: false, title: 'OBV MA' })
      seriesRefs.current.set('obvLine', obvLine); seriesRefs.current.set('obvArea', obvArea); seriesRefs.current.set('obvMA', obvMA)
      obvLine.setData(data.map(d => ({ time: d.time, value: d.obv || 0 })))
      obvArea.setData(data.map(d => ({ time: d.time, value: d.obv || 0 })))
      // OBV 20 MA
      const maData: LineData[] = []
      for (let i = 19; i < data.length; i++) {
        let sum = 0; for (let j = 0; j < 20; j++) sum += data[i-j].obv || 0
        maData.push({ time: data[i].time, value: sum / 20 })
      }
      obvMA.setData(maData)
    }
    // Williams %R (动量指标)
    else if (type === 'willr') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const willrLine = chart.addSeries(LineSeries, { color: '#ff00ff', lineWidth: 2, priceLineVisible: true, lastValueVisible: true, title: '%R' })
      const ob = chart.addSeries(LineSeries, { color: '#ff4444', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      const os = chart.addSeries(LineSeries, { color: '#00ff88', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      const mid = chart.addSeries(LineSeries, { color: '#444', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('willrLine', willrLine); seriesRefs.current.set('willrOB', ob); seriesRefs.current.set('willrOS', os)
      willrLine.setData(data.map(d => ({ time: d.time, value: d.williamsR || -50 })))
      ob.setData(data.map(d => ({ time: d.time, value: -20 }))); os.setData(data.map(d => ({ time: d.time, value: -80 }))); mid.setData(data.map(d => ({ time: d.time, value: -50 })))
    }
    // CCI - 顺势指标 (动量指标)
    else if (type === 'cci') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const cciLine = chart.addSeries(LineSeries, { color: '#00ff88', lineWidth: 2, priceLineVisible: true, lastValueVisible: true, title: 'CCI' })
      const ob = chart.addSeries(LineSeries, { color: '#ff4444', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      const os = chart.addSeries(LineSeries, { color: '#00aaff', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      const zero = chart.addSeries(LineSeries, { color: '#444', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('cciLine', cciLine); seriesRefs.current.set('cciOB', ob); seriesRefs.current.set('cciOS', os)
      cciLine.setData(data.map(d => ({ time: d.time, value: d.cci || 0 })))
      ob.setData(data.map(d => ({ time: d.time, value: 100 }))); os.setData(data.map(d => ({ time: d.time, value: -100 }))); zero.setData(data.map(d => ({ time: d.time, value: 0 })))
    }
    // Stochastic RSI (动量指标)
    else if (type === 'stochrsi') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const stochRsiLine = chart.addSeries(LineSeries, { color: '#00aaff', lineWidth: 2, priceLineVisible: true, lastValueVisible: true, title: 'StochRSI' })
      const stochRsiK = chart.addSeries(LineSeries, { color: '#ffaa00', lineWidth: 1, priceLineVisible: false, lastValueVisible: false, title: '%K' })
      const ob = chart.addSeries(LineSeries, { color: '#ff4444', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      const os = chart.addSeries(LineSeries, { color: '#00ff88', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('stochRsiLine', stochRsiLine); seriesRefs.current.set('stochRsiK', stochRsiK)
      stochRsiLine.setData(data.map(d => ({ time: d.time, value: (d.stochRsi || 50) })))
      // K line is smoothed version
      const kData: LineData[] = []
      for (let i = 2; i < data.length; i++) {
        const avg = ((data[i].stochRsi || 50) + (data[i-1].stochRsi || 50) + (data[i-2].stochRsi || 50)) / 3
        kData.push({ time: data[i].time, value: avg })
      }
      stochRsiK.setData(kData)
      ob.setData(data.map(d => ({ time: d.time, value: 80 }))); os.setData(data.map(d => ({ time: d.time, value: 20 })))
    }
    // ADX - 平均趋向指数 (趋势强度指标)
    else if (type === 'adx') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const adxLine = chart.addSeries(LineSeries, { color: '#ffaa00', lineWidth: 2, priceLineVisible: true, lastValueVisible: true, title: 'ADX' })
      const strongTrend = chart.addSeries(LineSeries, { color: '#00ff88', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      const weakTrend = chart.addSeries(LineSeries, { color: '#ff4444', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('adxLine', adxLine)
      adxLine.setData(data.map(d => ({ time: d.time, value: d.adx || 25 })))
      strongTrend.setData(data.map(d => ({ time: d.time, value: 25 }))); weakTrend.setData(data.map(d => ({ time: d.time, value: 20 })))
    }
    // MFI - 资金流量指数 (成交量指标)
    else if (type === 'mfi') {
      chart.priceScale('right').applyOptions({ scaleMargins: { top: 0.1, bottom: 0.1 } })
      const mfiLine = chart.addSeries(LineSeries, { color: '#00ff88', lineWidth: 2, priceLineVisible: true, lastValueVisible: true, title: 'MFI' })
      const mfiArea = chart.addSeries(AreaSeries, { lineColor: '#00ff8860', topColor: '#00ff8840', bottomColor: '#00ff8810', lineWidth: 1, priceLineVisible: false, lastValueVisible: false })
      const ob = chart.addSeries(LineSeries, { color: '#ff4444', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      const os = chart.addSeries(LineSeries, { color: '#00aaff', lineWidth: 1, lineStyle: 2, priceLineVisible: false, lastValueVisible: false })
      seriesRefs.current.set('mfiLine', mfiLine); seriesRefs.current.set('mfiArea', mfiArea)
      mfiLine.setData(data.map(d => ({ time: d.time, value: d.mfi || 50 })))
      mfiArea.setData(data.map(d => ({ time: d.time, value: d.mfi || 50 })))
      ob.setData(data.map(d => ({ time: d.time, value: 80 }))); os.setData(data.map(d => ({ time: d.time, value: 20 })))
    }
    
    // 自适应内容确保所有数据可见
    chart.timeScale().fitContent()
  }, [clearSeries, basePrice])

  // 根据图表类型创建系列
  useEffect(() => {
    if (!chartRef.current) return
    if (lastChartTypeRef.current !== chartType) {
      lastChartTypeRef.current = chartType
      setupChart(chartRef.current, dataRef.current, chartType)
    }
  }, [chartType, setupChart])

  // 初始化数据 - 当symbol或timeframe变化时重新生成
  useEffect(() => {
    if (!chartRef.current) return
    const shouldRegenerate = lastSymbolRef.current !== symbol || lastTimeframeRef.current !== timeframe
    if (shouldRegenerate) {
      const initialData = generateInitialData(basePrice, timeframe, symbol)
      dataRef.current = initialData
      tickCountRef.current = initialData.length
      lastSymbolRef.current = symbol
      lastTimeframeRef.current = timeframe
      lastChartTypeRef.current = ''
      setCurrentPrice(initialData[initialData.length - 1].close)
      setPriceChange(initialData[initialData.length - 1].close - initialData[0].open)
      setupChart(chartRef.current, initialData, chartType)
    }
  }, [symbol, basePrice, chartType, timeframe, setupChart])

  // 实时更新 - 根据timeframe添加新K线
  useEffect(() => {
    if (!chartRef.current || dataRef.current.length === 0) return
    const config = TIMEFRAME_CONFIG[timeframe] || TIMEFRAME_CONFIG['1h']
    
    // 根据时间周期调整波动率
    const volatility = timeframe === '1min' ? 0.001 : 
                       timeframe === '1h' ? 0.003 : 
                       timeframe === '24h' ? 0.01 : 
                       timeframe === '1month' ? 0.02 : 0.05
    
    const interval = setInterval(() => {
      const data = dataRef.current; if (data.length === 0) return
      const last = data[data.length - 1]
      const now = Math.floor(Date.now() / 1000)
      const currentBar = floorToInterval(now * 1000, config.intervalMs) / 1000
      
      tickCountRef.current += 1
      const t = tickCountRef.current
      
      // 使用稳定的价格更新算法
      const trend = Math.sin(t * 0.02) * basePrice * volatility
      const noise = (seededRandom(t * 7919) - 0.5) * basePrice * volatility * 0.3
      const newClose = last.close + trend * 0.1 + noise
      
      // 检查是否需要创建新K线
      if (currentBar > (last.time as number)) {
        // 创建新K线 - 使用稳定的数据生成
        const volume = 500000 + basePrice * 10 * (0.5 + seededRandom(t * 4007))
        const isUp = newClose > last.close
        const newBar: CandleData = {
          time: currentBar as Time,
          open: last.close,
          high: Math.max(last.close, newClose) + Math.abs(newClose - last.close) * 0.2,
          low: Math.min(last.close, newClose) - Math.abs(newClose - last.close) * 0.2,
          close: newClose,
          volume: volume,
          ma5: data.slice(-4).reduce((s, d) => s + d.close, newClose) / 5,
          ma10: data.slice(-9).reduce((s, d) => s + d.close, newClose) / 10,
          ma20: data.slice(-19).reduce((s, d) => s + d.close, newClose) / 20,
          rsi: 50 + Math.sin(t * 0.1) * 20,
          macd: Math.sin(t * 0.08) * basePrice * 0.001,
          signal: Math.sin(t * 0.08 - 0.3) * basePrice * 0.0008,
          histogram: Math.sin(t * 0.08) * basePrice * 0.001 - Math.sin(t * 0.08 - 0.3) * basePrice * 0.0008,
          k: 50 + Math.sin(t * 0.12) * 30,
          d: 50 + Math.sin(t * 0.12 - 0.2) * 25,
          j: 50 + Math.sin(t * 0.12 + 0.2) * 35,
          stochRsi: 50 + Math.sin(t * 0.15) * 40,
          upperBB: newClose + basePrice * volatility * 2,
          middleBB: newClose,
          lowerBB: newClose - basePrice * volatility * 2,
          atr: basePrice * volatility,
          obv: (last.obv || 0) + (isUp ? volume : -volume),
          vwap: newClose,
          williamsR: -50 + Math.sin(t * 0.1) * 40,
          cci: Math.sin(t * 0.08) * 100,
          adx: 25 + Math.sin(t * 0.05) * 15,
          mfi: 50 + Math.sin(t * 0.12) * 30,
          inflow: isUp ? volume * 0.6 : volume * 0.35,
          outflow: isUp ? volume * 0.35 : volume * 0.6,
          netFlow: isUp ? volume * 0.25 : -volume * 0.25,
          changePercent: ((newClose - last.close) / last.close) * 100
        }
        data.push(newBar)
        if (data.length > config.points + 10) data.shift() // 保持数据量稳定
        
        // 重新设置图表数据
        setupChart(chartRef.current!, data, chartType)
      } else {
        // 更新当前K线
        const newHigh = Math.max(last.high, newClose), newLow = Math.min(last.low, newClose)
        data[data.length - 1] = { ...last, high: newHigh, low: newLow, close: newClose }

        const candleSeries = seriesRefs.current.get('candle')
        if (candleSeries) candleSeries.update({ time: last.time, open: last.open, high: newHigh, low: newLow, close: newClose })
        const areaSeries = seriesRefs.current.get('area')
        if (areaSeries) areaSeries.update({ time: last.time, value: newClose })
        const heatSeries = seriesRefs.current.get('heat')
        if (heatSeries) heatSeries.update({ time: last.time, value: newClose })
        const volumeSeries = seriesRefs.current.get('volume')
        if (volumeSeries) volumeSeries.update({ time: last.time, value: last.volume || 0, color: newClose >= last.open ? '#00ff8860' : '#ff444460' })
        const ma5 = seriesRefs.current.get('ma5'), ma10 = seriesRefs.current.get('ma10')
        if (ma5 && data.length >= 5) ma5.update({ time: last.time, value: data.slice(-5).reduce((s, d) => s + d.close, 0) / 5 })
        if (ma10 && data.length >= 10) ma10.update({ time: last.time, value: data.slice(-10).reduce((s, d) => s + d.close, 0) / 10 })
      }

      setCurrentPrice(newClose); setPriceChange(newClose - data[0].open)
    }, 100)
    return () => clearInterval(interval)
  }, [basePrice, chartType, timeframe, setupChart])

  const isUp = priceChange >= 0
  
  // 检测数据是否过期
  const isDataStale = Date.now() - dataQuality.lastUpdate > 5000
  const staleDuration = Math.floor((Date.now() - dataQuality.lastUpdate) / 1000)

  // 渲染浮动信息卡片 - 专业量化交易面板，跟随鼠标右下侧
  const renderFloatingTooltip = () => {
    if (!hoverInfo) return null
    const info = hoverInfo
    const candleIsUp = info.close >= info.open
    const config = TIMEFRAME_CONFIG[timeframe] || TIMEFRAME_CONFIG['1h']
    const decimals = config.priceDecimals
    const amplitude = ((info.high - info.low) / info.open * 100).toFixed(2)
    const bodyRatio = (Math.abs(info.close - info.open) / info.open * 100).toFixed(2)
    
    // 计算额外指标
    const shadowRatio = info.high !== info.low ? 
      (((info.high - Math.max(info.open, info.close)) + (Math.min(info.open, info.close) - info.low)) / (info.high - info.low) * 100).toFixed(1) : '0'
    const volumeMA = info.volume
    const volumeRatio = volumeMA > 0 ? (info.volume / volumeMA).toFixed(2) : '1.00'
    
    // 计算悬浮窗位置 - 在鼠标右下侧，避免超出边界
    const tooltipWidth = 260
    const tooltipHeight = 400
    const offsetX = 20
    const offsetY = 15
    let tooltipX = mousePos.x + offsetX
    let tooltipY = mousePos.y + offsetY
    
    // 边界检测
    if (chartContainerRef.current) {
      const containerRect = chartContainerRef.current.getBoundingClientRect()
      if (tooltipX + tooltipWidth > containerRect.width - 10) {
        tooltipX = mousePos.x - tooltipWidth - offsetX
      }
      if (tooltipY + tooltipHeight > containerRect.height - 10) {
        tooltipY = containerRect.height - tooltipHeight - 10
      }
    }

    // K线类型图表的专业信息面板
    if (chartType === 'candle' || chartType === 'area' || chartType === 'indicators' || chartType === 'fibonacci' || chartType === 'profile' || chartType === 'trend' || chartType === 'heatmap' || chartType === 'vwap') {
      return (
        <div 
          className="absolute z-50 pointer-events-none"
          style={{ left: tooltipX, top: tooltipY }}
        >
          <div className="bg-[#0d0d0d]/85 backdrop-blur-sm border-2 border-[#444]/80 rounded-lg shadow-2xl font-mono text-[11px] overflow-hidden w-[260px]" style={{ boxShadow: '0 8px 32px rgba(0,0,0,0.6), 0 0 0 1px rgba(255,255,255,0.05)' }}>
            {/* 标题栏 - 时间与涨跌 */}
            <div className="bg-gradient-to-r from-[#1a1a1a]/90 to-[#222]/90 px-3 py-2 border-b border-[#444]/70 flex items-center justify-between">
              <span className="text-gray-300 text-[10px] font-semibold">{info.time}</span>
              <div className="flex items-center gap-2">
                <span className={`px-2 py-0.5 rounded text-[10px] font-bold ${candleIsUp ? 'bg-green-500/30 text-green-300 border border-green-500/50' : 'bg-red-500/30 text-red-300 border border-red-500/50'}`}>
                  {info.changePercent >= 0 ? '+' : ''}{info.changePercent.toFixed(2)}%
                </span>
              </div>
            </div>
            
            {/* OHLC 核心数据 */}
            <div className="px-2.5 py-2 grid grid-cols-4 gap-1 text-center border-b border-[#333]/80">
              <div className="bg-[#151515]/90 rounded-md px-1.5 py-1 border border-[#333]/70">
                <div className="text-gray-500 text-[8px] font-medium">OPEN</div>
                <div className={`text-[11px] font-bold ${candleIsUp ? 'text-green-400' : 'text-red-400'}`}>{info.open.toFixed(decimals)}</div>
              </div>
              <div className="bg-[#151515]/90 rounded-md px-1.5 py-1 border border-[#333]/70">
                <div className="text-gray-500 text-[8px] font-medium">HIGH</div>
                <div className="text-green-400 text-[11px] font-bold">{info.high.toFixed(decimals)}</div>
              </div>
              <div className="bg-[#151515]/90 rounded-md px-1.5 py-1 border border-[#333]/70">
                <div className="text-gray-500 text-[8px] font-medium">LOW</div>
                <div className="text-red-400 text-[11px] font-bold">{info.low.toFixed(decimals)}</div>
              </div>
              <div className="bg-[#151515]/90 rounded-md px-1.5 py-1 border border-[#333]/70">
                <div className="text-gray-500 text-[8px] font-medium">CLOSE</div>
                <div className={`text-[11px] font-black ${candleIsUp ? 'text-green-400' : 'text-red-400'}`}>{info.close.toFixed(decimals)}</div>
              </div>
            </div>

            {/* 6大核心指标 */}
            <div className="px-2.5 py-2 grid grid-cols-3 gap-1.5 border-b border-[#333]/80">
              <div className="text-center bg-[#111]/90 rounded px-1 py-1">
                <div className="text-gray-500 text-[8px]">成交量</div>
                <div className="text-cyan-400 text-[10px] font-bold">{(info.volume / 1e6).toFixed(2)}M</div>
              </div>
              <div className="text-center bg-[#111]/90 rounded px-1 py-1">
                <div className="text-gray-500 text-[8px]">振幅</div>
                <div className="text-yellow-400 text-[10px] font-bold">{amplitude}%</div>
              </div>
              <div className="text-center bg-[#111]/90 rounded px-1 py-1">
                <div className="text-gray-500 text-[8px]">实体比</div>
                <div className={`text-[10px] font-bold ${candleIsUp ? 'text-green-400' : 'text-red-400'}`}>{bodyRatio}%</div>
              </div>
              <div className="text-center bg-[#111]/90 rounded px-1 py-1">
                <div className="text-gray-500 text-[8px]">影线比</div>
                <div className="text-gray-400 text-[10px] font-bold">{shadowRatio}%</div>
              </div>
              <div className="text-center bg-[#111]/90 rounded px-1 py-1">
                <div className="text-gray-500 text-[8px]">量比</div>
                <div className={`text-[10px] font-bold ${parseFloat(volumeRatio) > 1.5 ? 'text-orange-400' : 'text-gray-400'}`}>{volumeRatio}x</div>
              </div>
              <div className="text-center bg-[#111]/90 rounded px-1 py-1">
                <div className="text-gray-500 text-[8px]">ATR</div>
                <div className="text-orange-400 text-[10px] font-bold">{info.atr?.toFixed(2) || '-'}</div>
              </div>
            </div>

            {/* 均线系统 */}
            <div className="px-2.5 py-1.5 flex items-center justify-between border-b border-[#333]/80 bg-[#0a0a0a]/90">
              <div className="flex items-center gap-2 text-[9px]">
                {info.ma5 && <span><span className="text-[#ffaa00] font-bold">M5</span><span className="text-gray-300 ml-0.5">{info.ma5.toFixed(1)}</span></span>}
                {info.ma10 && <span><span className="text-[#00aaff] font-bold">M10</span><span className="text-gray-300 ml-0.5">{info.ma10.toFixed(1)}</span></span>}
                {info.ma20 && <span><span className="text-[#ff00ff] font-bold">M20</span><span className="text-gray-300 ml-0.5">{info.ma20.toFixed(1)}</span></span>}
              </div>
              {info.ma5 && info.ma10 && (
                <span className={`text-[9px] px-1.5 py-0.5 rounded font-bold ${info.ma5 > info.ma10 ? 'bg-green-900/50 text-green-400 border border-green-700' : 'bg-red-900/50 text-red-400 border border-red-700'}`}>
                  {info.ma5 > info.ma10 ? '▲ 多头' : '▼ 空头'}
                </span>
              )}
            </div>

            {/* 10个专业技术指标矩阵 */}
            <div className="px-2.5 py-2 grid grid-cols-5 gap-1 border-b border-[#333]">
              {/* RSI */}
              <div className="text-center">
                <div className="text-gray-500 text-[7px]">RSI</div>
                <div className={`text-[10px] font-bold ${(info.rsi || 50) > 70 ? 'text-red-400' : (info.rsi || 50) < 30 ? 'text-green-400' : 'text-white'}`}>
                  {info.rsi?.toFixed(0) || '-'}
                </div>
              </div>
              {/* MACD */}
              <div className="text-center">
                <div className="text-gray-500 text-[7px]">MACD</div>
                <div className={`text-[10px] font-bold ${(info.histogram || 0) >= 0 ? 'text-green-400' : 'text-red-400'}`}>
                  {info.macd?.toFixed(1) || '-'}
                </div>
              </div>
              {/* KDJ-K */}
              <div className="text-center">
                <div className="text-gray-500 text-[7px]">K值</div>
                <div className={`text-[10px] font-bold ${(info.k || 50) > 80 ? 'text-red-400' : (info.k || 50) < 20 ? 'text-green-400' : 'text-white'}`}>
                  {info.k?.toFixed(0) || '-'}
                </div>
              </div>
              {/* KDJ-D */}
              <div className="text-center">
                <div className="text-gray-500 text-[7px]">D值</div>
                <div className={`text-[10px] font-bold ${(info.d || 50) > 80 ? 'text-red-400' : (info.d || 50) < 20 ? 'text-green-400' : 'text-white'}`}>
                  {info.d?.toFixed(0) || '-'}
                </div>
              </div>
              {/* KDJ-J */}
              <div className="text-center">
                <div className="text-gray-500 text-[7px]">J值</div>
                <div className={`text-[10px] font-bold ${(info.j || 50) > 100 ? 'text-red-400' : (info.j || 50) < 0 ? 'text-green-400' : 'text-white'}`}>
                  {info.j?.toFixed(0) || '-'}
                </div>
              </div>
              {/* BB% */}
              <div className="text-center">
                <div className="text-gray-500 text-[7px]">BB%</div>
                <div className={`text-[10px] font-bold ${info.upperBB && info.close > info.upperBB ? 'text-red-400' : info.lowerBB && info.close < info.lowerBB ? 'text-green-400' : 'text-white'}`}>
                  {info.upperBB ? ((info.close - (info.lowerBB || 0)) / ((info.upperBB || 1) - (info.lowerBB || 0)) * 100).toFixed(0) : '-'}
                </div>
              </div>
              {/* ADX */}
              <div className="text-center">
                <div className="text-gray-500 text-[7px]">ADX</div>
                <div className={`text-[10px] font-bold ${(info.adx || 0) > 25 ? 'text-yellow-400' : 'text-gray-500'}`}>
                  {info.adx?.toFixed(0) || '-'}
                </div>
              </div>
              {/* MFI */}
              <div className="text-center">
                <div className="text-gray-500 text-[7px]">MFI</div>
                <div className={`text-[10px] font-bold ${(info.mfi || 50) > 80 ? 'text-red-400' : (info.mfi || 50) < 20 ? 'text-green-400' : 'text-white'}`}>
                  {info.mfi?.toFixed(0) || '-'}
                </div>
              </div>
              {/* Williams %R */}
              <div className="text-center">
                <div className="text-gray-500 text-[7px]">%R</div>
                <div className={`text-[10px] font-bold ${(info.williamsR || -50) > -20 ? 'text-red-400' : (info.williamsR || -50) < -80 ? 'text-green-400' : 'text-white'}`}>
                  {info.williamsR?.toFixed(0) || '-'}
                </div>
              </div>
              {/* CCI */}
              <div className="text-center">
                <div className="text-gray-500 text-[7px]">CCI</div>
                <div className={`text-[10px] font-bold ${(info.cci || 0) > 100 ? 'text-red-400' : (info.cci || 0) < -100 ? 'text-green-400' : 'text-white'}`}>
                  {info.cci?.toFixed(0) || '-'}
                </div>
              </div>
            </div>

            {/* 信号综合判断 */}
            <div className="px-2.5 py-2 flex items-center justify-between bg-[#0a0a0a]">
              <div className="flex items-center gap-1">
                {info.rsi !== undefined && (
                  <span className={`px-1.5 py-0.5 rounded text-[9px] font-bold font-mono ${info.rsi > 70 ? 'bg-red-900/60 text-red-300' : info.rsi < 30 ? 'bg-green-900/60 text-green-300' : 'bg-[#333] text-gray-400'}`}>
                    RSI:{info.rsi.toFixed(0)} ({info.rsi > 70 ? 'OB' : info.rsi < 30 ? 'OS' : 'Neutral'})
                  </span>
                )}
                {info.macd !== undefined && info.signal !== undefined && (
                  <span className={`px-1.5 py-0.5 rounded text-[9px] font-bold ${info.macd > info.signal ? 'bg-green-900/60 text-green-300' : 'bg-red-900/60 text-red-300'}`}>
                    {info.macd > info.signal ? 'MACD↑' : 'MACD↓'}
                  </span>
                )}
                {info.adx !== undefined && (
                  <span className={`px-1.5 py-0.5 rounded text-[9px] font-bold font-mono ${info.adx > 50 ? 'bg-green-900/60 text-green-300' : info.adx > 25 ? 'bg-yellow-900/60 text-yellow-300' : info.adx > 15 ? 'bg-blue-900/60 text-blue-300' : 'bg-[#333] text-gray-400'}`}>
                    {info.adx > 50 ? 'Expansion' : info.adx > 25 ? 'Trending' : info.adx > 15 ? 'Range' : 'Breakdown'}
                  </span>
                )}
              </div>
              <span className={`text-[10px] font-black font-mono px-2 py-0.5 rounded ${candleIsUp ? 'bg-green-500/20 text-green-400' : 'bg-red-500/20 text-red-400'}`}>
                {candleIsUp ? '▲' : '▼'} {info.changePercent >= 0 ? '+' : ''}{info.changePercent.toFixed(2)}% (${(info.close - info.open).toFixed(2)})
              </span>
            </div>
          </div>
        </div>
      )
    }

    // 其他指标图表的简化面板
    return (
      <div className="pointer-events-none bg-[#0a0a0a]/95 border border-[#222] rounded-md shadow-xl font-mono text-[10px] p-2 min-w-[140px]">
        <div className="text-gray-400 text-[9px] mb-1.5 pb-1 border-b border-[#222]">{info.time}</div>
        
        {/* MACD */}
        {chartType === 'macd' && (
          <div className="space-y-0.5">
            <div className="flex justify-between"><span className="text-[#00aaff]">MACD:</span><span className="text-white">{info.macd?.toFixed(3)}</span></div>
            <div className="flex justify-between"><span className="text-[#ffaa00]">Signal:</span><span className="text-white">{info.signal?.toFixed(3)}</span></div>
            <div className="flex justify-between"><span className="text-gray-500">Hist:</span><span className={(info.histogram || 0) >= 0 ? 'text-green-400' : 'text-red-400'}>{info.histogram?.toFixed(3)}</span></div>
          </div>
        )}

        {/* RSI */}
        {chartType === 'rsi' && (
          <div className="space-y-1">
            <div className="flex justify-between">
              <span className="text-[#00aaff]">RSI:</span>
              <span className={`${(info.rsi || 50) > 70 ? 'text-red-400' : (info.rsi || 50) < 30 ? 'text-green-400' : 'text-white'}`}>{info.rsi?.toFixed(1)}</span>
            </div>
            <div className={`text-center text-[9px] px-1 py-0.5 rounded ${(info.rsi || 50) > 70 ? 'bg-red-900/50 text-red-300' : (info.rsi || 50) < 30 ? 'bg-green-900/50 text-green-300' : 'bg-[#252525] text-gray-400'}`}>
              {(info.rsi || 50) > 70 ? '超买' : (info.rsi || 50) < 30 ? '超卖' : '中性'}
            </div>
          </div>
        )}

        {/* KDJ */}
        {chartType === 'kdj' && (
          <div className="space-y-0.5">
            <div className="flex justify-between"><span className="text-[#ffaa00]">K:</span><span className="text-white">{info.k?.toFixed(1)}</span></div>
            <div className="flex justify-between"><span className="text-[#00aaff]">D:</span><span className="text-white">{info.d?.toFixed(1)}</span></div>
            <div className="flex justify-between"><span className="text-[#ff00ff]">J:</span><span className="text-white">{info.j?.toFixed(1)}</span></div>
          </div>
        )}

        {/* Volume */}
        {chartType === 'volume' && (
          <div className="space-y-0.5">
            <div className="flex justify-between"><span className="text-gray-500">成交量:</span><span className={candleIsUp ? 'text-green-400' : 'text-red-400'}>{(info.volume / 1e6).toFixed(2)}M</span></div>
          </div>
        )}

        {/* Flow */}
        {chartType === 'flow' && (
          <div className="space-y-0.5">
            <div className="flex justify-between"><span className="text-green-400">流入:</span><span className="text-green-400">{((info.inflow || 0) / 1e6).toFixed(2)}M</span></div>
            <div className="flex justify-between"><span className="text-red-400">流出:</span><span className="text-red-400">{((info.outflow || 0) / 1e6).toFixed(2)}M</span></div>
            <div className="flex justify-between pt-0.5 border-t border-[#222]"><span className="text-gray-500">净流:</span><span className={(info.netFlow || 0) >= 0 ? 'text-green-400' : 'text-red-400'}>{((info.netFlow || 0) / 1e6).toFixed(2)}M</span></div>
          </div>
        )}

        {/* ATR */}
        {chartType === 'atr' && (
          <div className="space-y-1">
            <div className="flex justify-between"><span className="text-[#ff8800]">ATR:</span><span className="text-white">{info.atr?.toFixed(4)}</span></div>
            <div className="text-center text-[9px] px-1 py-0.5 rounded bg-[#ff880020] text-[#ff8800]">波动 {((info.atr || 0) / info.close * 100).toFixed(2)}%</div>
          </div>
        )}

        {/* OBV */}
        {chartType === 'obv' && (
          <div className="space-y-0.5">
            <div className="flex justify-between"><span className="text-[#00aaff]">OBV:</span><span className="text-white">{((info.obv || 0) / 1e6).toFixed(2)}M</span></div>
          </div>
        )}

        {/* Williams %R */}
        {chartType === 'willr' && (
          <div className="space-y-1">
            <div className="flex justify-between"><span className="text-[#ff00ff]">%R:</span><span className={`${(info.williamsR || -50) > -20 ? 'text-red-400' : (info.williamsR || -50) < -80 ? 'text-green-400' : 'text-white'}`}>{info.williamsR?.toFixed(1)}</span></div>
            <div className={`text-center text-[9px] px-1 py-0.5 rounded ${(info.williamsR || -50) > -20 ? 'bg-red-900/50 text-red-300' : (info.williamsR || -50) < -80 ? 'bg-green-900/50 text-green-300' : 'bg-[#252525] text-gray-400'}`}>
              {(info.williamsR || -50) > -20 ? '超买' : (info.williamsR || -50) < -80 ? '超卖' : '中性'}
            </div>
          </div>
        )}

        {/* CCI */}
        {chartType === 'cci' && (
          <div className="space-y-1">
            <div className="flex justify-between"><span className="text-[#00ff88]">CCI:</span><span className={`${(info.cci || 0) > 100 ? 'text-red-400' : (info.cci || 0) < -100 ? 'text-green-400' : 'text-white'}`}>{info.cci?.toFixed(1)}</span></div>
            <div className={`text-center text-[9px] px-1 py-0.5 rounded ${(info.cci || 0) > 100 ? 'bg-red-900/50 text-red-300' : (info.cci || 0) < -100 ? 'bg-green-900/50 text-green-300' : 'bg-[#252525] text-gray-400'}`}>
              {(info.cci || 0) > 100 ? '强势' : (info.cci || 0) < -100 ? '弱势' : '震荡'}
            </div>
          </div>
        )}

        {/* StochRSI */}
        {chartType === 'stochrsi' && (
          <div className="space-y-1">
            <div className="flex justify-between"><span className="text-[#00aaff]">StochRSI:</span><span className={`${(info.stochRsi || 50) > 80 ? 'text-red-400' : (info.stochRsi || 50) < 20 ? 'text-green-400' : 'text-white'}`}>{info.stochRsi?.toFixed(1)}</span></div>
            <div className={`text-center text-[9px] px-1 py-0.5 rounded ${(info.stochRsi || 50) > 80 ? 'bg-red-900/50 text-red-300' : (info.stochRsi || 50) < 20 ? 'bg-green-900/50 text-green-300' : 'bg-[#252525] text-gray-400'}`}>
              {(info.stochRsi || 50) > 80 ? '超买' : (info.stochRsi || 50) < 20 ? '超卖' : '中性'}
            </div>
          </div>
        )}

        {/* ADX */}
        {chartType === 'adx' && (
          <div className="space-y-1">
            <div className="flex justify-between"><span className="text-[#ffaa00]">ADX:</span><span className={`${(info.adx || 25) > 25 ? 'text-green-400' : 'text-red-400'}`}>{info.adx?.toFixed(1)}</span></div>
            <div className={`text-center text-[9px] px-1 py-0.5 rounded ${(info.adx || 25) > 25 ? 'bg-yellow-900/50 text-yellow-300' : 'bg-[#252525] text-gray-400'}`}>
              {(info.adx || 25) > 50 ? '极强趋势' : (info.adx || 25) > 25 ? '趋势形成' : '震荡市场'}
            </div>
          </div>
        )}

        {/* MFI */}
        {chartType === 'mfi' && (
          <div className="space-y-1">
            <div className="flex justify-between"><span className="text-[#00ff88]">MFI:</span><span className={`${(info.mfi || 50) > 80 ? 'text-red-400' : (info.mfi || 50) < 20 ? 'text-green-400' : 'text-white'}`}>{info.mfi?.toFixed(1)}</span></div>
            <div className={`text-center text-[9px] px-1 py-0.5 rounded ${(info.mfi || 50) > 80 ? 'bg-red-900/50 text-red-300' : (info.mfi || 50) < 20 ? 'bg-green-900/50 text-green-300' : 'bg-[#252525] text-gray-400'}`}>
              {(info.mfi || 50) > 80 ? '资金过热' : (info.mfi || 50) < 20 ? '资金枯竭' : '正常'}
            </div>
          </div>
        )}

        {/* Depth */}
        {chartType === 'depth' && (
          <div className="space-y-0.5">
            <div className="flex justify-between"><span className="text-gray-500">价格:</span><span className="text-white">{info.close.toFixed(2)}</span></div>
            <div className="flex justify-between"><span className="text-green-400">买盘:</span><span className="text-green-400">{(info.volume * 0.6 / 1e6).toFixed(2)}M</span></div>
            <div className="flex justify-between"><span className="text-red-400">卖盘:</span><span className="text-red-400">{(info.volume * 0.4 / 1e6).toFixed(2)}M</span></div>
          </div>
        )}
      </div>
    )
  }

  // 渲染顶部信息栏
  const renderTopBar = () => {
    if (!hoverInfo) return null
    const info = hoverInfo, candleIsUp = info.close >= info.open
    switch (chartType) {
      case 'macd': return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className="text-[#00aaff]">MACD: {info.macd?.toFixed(2)}</span><span className="text-[#ffaa00]">Signal: {info.signal?.toFixed(2)}</span><span className={`${(info.histogram || 0) >= 0 ? 'text-green-400' : 'text-red-400'}`}>Hist: {info.histogram?.toFixed(2)}</span></div>
      case 'rsi': const r = info.rsi || 50; return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className={`${r > 70 ? 'text-red-400' : r < 30 ? 'text-green-400' : 'text-[#00aaff]'}`}>RSI(14): {r.toFixed(1)}</span><span className="text-gray-500">{r > 70 ? '超买' : r < 30 ? '超卖' : '中性'}</span></div>
      case 'kdj': return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className="text-[#ffaa00]">K: {info.k?.toFixed(1)}</span><span className="text-[#00aaff]">D: {info.d?.toFixed(1)}</span><span className="text-[#ff00ff]">J: {info.j?.toFixed(1)}</span></div>
      case 'volume': return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className={candleIsUp ? 'text-green-400' : 'text-red-400'}>Vol: {(info.volume / 1e6).toFixed(2)}M</span></div>
      case 'flow': return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className="text-green-400">流入: {((info.inflow || 0) / 1e6).toFixed(2)}M</span><span className="text-red-400">流出: {((info.outflow || 0) / 1e6).toFixed(2)}M</span><span className={(info.netFlow || 0) >= 0 ? 'text-green-400' : 'text-red-400'}>净流: {((info.netFlow || 0) / 1e6).toFixed(2)}M</span></div>
      case 'indicators': return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className={candleIsUp ? 'text-green-400' : 'text-red-400'}>C: {info.close.toFixed(2)}</span><span className="text-gray-400">BB: {info.lowerBB?.toFixed(0)}-{info.upperBB?.toFixed(0)}</span><span className="text-[#ffaa00]">MA5: {info.ma5?.toFixed(1)}</span><span className="text-[#00aaff]">MA10: {info.ma10?.toFixed(1)}</span></div>
      // 新增专业量化指标
      case 'atr': return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className="text-[#ff8800]">ATR(14): {info.atr?.toFixed(4)}</span><span className="text-gray-500">波动率: {((info.atr || 0) / info.close * 100).toFixed(2)}%</span></div>
      case 'vwap': return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className="text-[#00ffff]">VWAP: {info.vwap?.toFixed(2)}</span><span className={info.close >= (info.vwap || 0) ? 'text-green-400' : 'text-red-400'}>价格: {info.close.toFixed(2)}</span></div>
      case 'obv': return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className="text-[#00aaff]">OBV: {((info.obv || 0) / 1e6).toFixed(2)}M</span><span className="text-gray-500">成交量: {((info.volume || 0) / 1e6).toFixed(2)}M</span></div>
      case 'willr': const wr = info.williamsR || -50; return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className={`${wr > -20 ? 'text-red-400' : wr < -80 ? 'text-green-400' : 'text-[#ff00ff]'}`}>%R(14): {wr.toFixed(1)}</span><span className="text-gray-500">{wr > -20 ? '超买' : wr < -80 ? '超卖' : '中性'}</span></div>
      case 'cci': const cci = info.cci || 0; return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className={`${cci > 100 ? 'text-red-400' : cci < -100 ? 'text-green-400' : 'text-[#00ff88]'}`}>CCI(20): {cci.toFixed(1)}</span><span className="text-gray-500">{cci > 100 ? '强势' : cci < -100 ? '弱势' : '震荡'}</span></div>
      case 'stochrsi': const sr = info.stochRsi || 50; return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className={`${sr > 80 ? 'text-red-400' : sr < 20 ? 'text-green-400' : 'text-[#00aaff]'}`}>StochRSI: {sr.toFixed(1)}</span><span className="text-gray-500">{sr > 80 ? '超买' : sr < 20 ? '超卖' : '中性'}</span></div>
      case 'adx': const adx = info.adx || 25; return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className={`${adx > 25 ? 'text-green-400' : 'text-red-400'}`}>ADX(14): {adx.toFixed(1)}</span><span className="text-gray-500">{adx > 50 ? '极强趋势' : adx > 25 ? '趋势形成' : '震荡'}</span></div>
      case 'mfi': const mfi = info.mfi || 50; return <div className="flex flex-wrap items-center gap-2 text-[9px] font-mono"><span className="text-gray-500">{info.time}</span><span className={`${mfi > 80 ? 'text-red-400' : mfi < 20 ? 'text-green-400' : 'text-[#00ff88]'}`}>MFI(14): {mfi.toFixed(1)}</span><span className="text-gray-500">{mfi > 80 ? '资金过热' : mfi < 20 ? '资金枯竭' : '正常'}</span></div>
      // K线模式 - 专业顶栏
      case 'candle':
      case 'area':
      case 'fibonacci':
      case 'profile':
      case 'trend':
      case 'heatmap':
      default: 
        return (
          <div className="flex flex-wrap items-center gap-1.5 text-[9px] font-mono">
            <span className="text-gray-500">{info.time}</span>
            <span className="text-gray-400">O</span><span className={candleIsUp ? 'text-green-400' : 'text-red-400'}>{info.open.toFixed(2)}</span>
            <span className="text-gray-400">H</span><span className="text-green-400">{info.high.toFixed(2)}</span>
            <span className="text-gray-400">L</span><span className="text-red-400">{info.low.toFixed(2)}</span>
            <span className="text-gray-400">C</span><span className={candleIsUp ? 'text-green-400' : 'text-red-400'}>{info.close.toFixed(2)}</span>
            <span className={candleIsUp ? 'text-green-400' : 'text-red-400'}>{info.changePercent >= 0 ? '+' : ''}{info.changePercent.toFixed(2)}%</span>
            <span className="text-gray-600">|</span>
            <span className="text-[#ffaa00]">M5:{info.ma5?.toFixed(1)}</span>
            <span className="text-[#00aaff]">M10:{info.ma10?.toFixed(1)}</span>
            <span className="text-[#ff00ff]">M20:{info.ma20?.toFixed(1)}</span>
            {info.rsi !== undefined && <><span className="text-gray-600">|</span><span className={info.rsi > 70 ? 'text-red-400' : info.rsi < 30 ? 'text-green-400' : 'text-gray-400'}>RSI:{info.rsi.toFixed(0)}</span></>}
            <span className="text-gray-500">Vol:{(info.volume / 1e6).toFixed(1)}M</span>
          </div>
        )
    }
  }

  return (
    <div 
      className="w-full h-full relative bg-[#0a0a0a] rounded-lg overflow-hidden"
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
      onMouseLeave={handleMouseUp}
    >
      {/* STALE 数据过期水印 */}
      {isDataStale && (
        <div className="absolute inset-0 z-40 pointer-events-none flex items-center justify-center">
          <div className="text-center">
            <div className="text-6xl font-black text-red-500/20 tracking-widest">STALE</div>
            <div className="text-sm text-red-400/60 mt-2">
              数据过期 {staleDuration}s | 最后更新: {new Date(dataQuality.lastUpdate).toLocaleTimeString()}
            </div>
          </div>
        </div>
      )}

      {/* 顶部信息栏 */}
      <div className="absolute top-1 left-1 z-10 pointer-events-none bg-[#0a0a0a]/90 px-2 py-0.5 rounded">
        <div className="flex items-center gap-2 text-xs font-mono">
          <span className="text-gray-400">{symbol}</span>
          <span className={isUp ? 'text-green-400' : 'text-red-400'}>${currentPrice.toFixed(2)}</span>
          <span className={`text-[10px] ${isUp ? 'text-green-400' : 'text-red-400'}`}>{isUp ? '+' : ''}{priceChange.toFixed(2)} ({((priceChange / (currentPrice - priceChange || 1)) * 100).toFixed(2)}%)</span>
        </div>
        {hoverInfo ? renderTopBar() : (
          <div className="flex items-center gap-2 text-[9px] font-mono mt-0.5">
            {(chartType === 'candle' || chartType === 'area' || chartType === 'indicators' || chartType === 'fibonacci' || chartType === 'profile' || chartType === 'trend') && (
              <>
                <span className="text-[#ffaa00]">MA5</span>
                <span className="text-[#00aaff]">MA10</span>
                <span className="text-[#ff00ff]">MA20</span>
                <span className="text-[#00ff88]">MA60</span>
                <span className="text-gray-600">|</span>
                <span className="text-gray-500">布林带</span>
              </>
            )}
            {chartType === 'macd' && <span className="text-gray-500">MACD(12,26,9)</span>}
            {chartType === 'rsi' && <span className="text-gray-500">RSI(14) | 超买70 超卖30</span>}
            {chartType === 'kdj' && <span className="text-gray-500">KDJ(9,3,3)</span>}
            {chartType === 'depth' && <><span className="text-green-400">买盘深度</span><span className="text-red-400">卖盘深度</span></>}
            {chartType === 'flow' && <span className="text-gray-500">资金流向分析</span>}
            {chartType === 'heatmap' && <span className="text-gray-500">成交热度分布</span>}
            {chartType === 'atr' && <span className="text-[#ff8800]">ATR(14) 平均真实波幅</span>}
            {chartType === 'vwap' && <span className="text-[#00ffff]">VWAP 成交量加权均价 ±1σ</span>}
            {chartType === 'obv' && <span className="text-[#00aaff]">OBV 能量潮 + MA20</span>}
            {chartType === 'willr' && <span className="text-[#ff00ff]">Williams %R(14) | 超买-20 超卖-80</span>}
            {chartType === 'cci' && <span className="text-[#00ff88]">CCI(20) | 强势+100 弱势-100</span>}
            {chartType === 'stochrsi' && <span className="text-[#00aaff]">StochRSI(14) | 超买80 超卖20</span>}
            {chartType === 'adx' && <span className="text-[#ffaa00]">ADX(14) | 趋势&gt;25 强趋势&gt;50</span>}
            {chartType === 'mfi' && <span className="text-[#00ff88]">MFI(14) | 过热80 枯竭20</span>}
          </div>
        )}
      </div>

      {/* 可拖动风险指标面板 */}
      <div 
        className="fixed z-50 font-mono text-[9px]"
        style={{ left: riskPanelPos.x, top: riskPanelPos.y }}
      >
        <div className="bg-[#0a0a0a]/98 border border-[#444] rounded-md min-w-[180px] shadow-lg" style={{ boxShadow: '0 4px 20px rgba(0,0,0,0.6)' }}>
          {/* 可拖动折叠标题栏 */}
          <div 
            className="flex items-center justify-between px-2 py-1.5 cursor-move hover:bg-[#1a1a1a] transition-colors border-b border-[#333] select-none"
            onMouseDown={(e) => {
              e.preventDefault()
              setIsDraggingRisk(true)
              setDragOffset({ x: e.clientX - riskPanelPos.x, y: e.clientY - riskPanelPos.y })
            }}
          >
            <div className="flex items-center gap-1.5">
              <span className="text-[10px]">📊</span>
              <span className="text-gray-300 font-semibold">RISK METRICS</span>
            </div>
            <button 
              onClick={(e) => { e.stopPropagation(); setShowRiskPanel(!showRiskPanel) }}
              className="text-gray-500 hover:text-white text-[10px] px-1"
            >
              {showRiskPanel ? '▲' : '▼'}
            </button>
          </div>
          
          {/* 可折叠内容区 */}
          {showRiskPanel && (
            <div className="px-2 py-1.5">
              <div className="flex items-center justify-between mb-1 pb-1 border-b border-[#222]">
            <span className="text-gray-500">RISK METRICS</span>
            <span className={`text-[8px] px-1 rounded ${riskMetrics.riskPercent > 50 ? 'bg-red-900/50 text-red-400' : riskMetrics.riskPercent > 25 ? 'bg-yellow-900/50 text-yellow-400' : 'bg-green-900/50 text-green-400'}`}>
              {riskMetrics.riskPercent > 50 ? 'HIGH' : riskMetrics.riskPercent > 25 ? 'MED' : 'LOW'}
            </span>
          </div>
          {/* Day P&L */}
          <div className="flex items-center justify-between">
            <span className="text-gray-500">DAY:</span>
            <div className="flex items-center gap-1">
              <span className={riskMetrics.dayPnL >= 0 ? 'text-green-400' : 'text-red-400'}>
                {riskMetrics.dayPnL >= 0 ? '+' : ''}${riskMetrics.dayPnL.toFixed(0)}
              </span>
              <span className="text-gray-600">|</span>
              <span className="text-gray-500">DD:</span>
              <span className="text-red-400">${riskMetrics.dayDD.toFixed(0)}</span>
            </div>
          </div>
          {/* Week P&L */}
          <div className="flex items-center justify-between mt-0.5">
            <span className="text-gray-500">WEEK:</span>
            <div className="flex items-center gap-1">
              <span className={riskMetrics.weekPnL >= 0 ? 'text-green-400' : 'text-red-400'}>
                {riskMetrics.weekPnL >= 0 ? '+' : ''}${(riskMetrics.weekPnL / 1000).toFixed(1)}K
              </span>
              <span className="text-gray-600">|</span>
              <span className="text-gray-500">MaxDD:</span>
              <span className="text-red-400">${(riskMetrics.weekMaxDD / 1000).toFixed(1)}K</span>
            </div>
          </div>
          {/* VAR & Exposure */}
          <div className="flex items-center justify-between mt-1 pt-1 border-t border-[#222]">
            <div className="flex items-center gap-1">
              <span className="text-gray-500">VAR:</span>
              <span className="text-orange-400">{riskMetrics.varPercent.toFixed(1)}%</span>
            </div>
            <div className="flex items-center gap-1">
              <span className="text-gray-500">Exp:</span>
              <span className={riskMetrics.openExposure > 80 ? 'text-red-400' : riskMetrics.openExposure > 50 ? 'text-yellow-400' : 'text-green-400'}>
                {riskMetrics.openExposure.toFixed(0)}%
              </span>
            </div>
          </div>
          {/* Sharpe & Win Rate */}
          <div className="flex items-center justify-between mt-0.5">
            <div className="flex items-center gap-1">
              <span className="text-gray-500">Sharpe:</span>
              <span className={riskMetrics.sharpeRatio > 1 ? 'text-green-400' : 'text-gray-400'}>{riskMetrics.sharpeRatio.toFixed(2)}</span>
            </div>
            <div className="flex items-center gap-1">
              <span className="text-gray-500">Win:</span>
              <span className={riskMetrics.winRate > 50 ? 'text-green-400' : 'text-red-400'}>{riskMetrics.winRate.toFixed(1)}%</span>
            </div>
          </div>
            </div>
          )}
        </div>
      </div>

      {/* 可拖动数据质量面板 */}
      <div 
        className="fixed z-50 font-mono text-[9px]"
        style={{ left: dataPanelPos.x, top: dataPanelPos.y }}
      >
        <div className="bg-[#0a0a0a]/98 border border-[#444] rounded-md overflow-hidden min-w-[180px] shadow-lg" style={{ boxShadow: '0 4px 20px rgba(0,0,0,0.6)' }}>
          {/* 可拖动折叠标题栏 */}
          <div 
            className="flex items-center justify-between px-2 py-1.5 cursor-move hover:bg-[#1a1a1a] transition-colors select-none"
            onMouseDown={(e) => {
              e.preventDefault()
              setIsDraggingData(true)
              setDragOffset({ x: e.clientX - dataPanelPos.x, y: e.clientY - dataPanelPos.y })
            }}
          >
            <div className="flex items-center gap-1.5">
              <span className={`w-1.5 h-1.5 rounded-full ${
                dataQuality.feedStatus === 'connected' ? 'bg-green-400 animate-pulse' :
                dataQuality.feedStatus === 'delayed' ? 'bg-yellow-400' :
                dataQuality.feedStatus === 'stale' ? 'bg-orange-400' : 'bg-red-400'
              }`}></span>
              <span className="text-gray-300 font-semibold">DATA FEED</span>
              <span className={`text-[8px] ${dataQuality.latencyMs < 30 ? 'text-green-400' : dataQuality.latencyMs < 100 ? 'text-yellow-400' : 'text-red-400'}`}>
                {dataQuality.latencyMs.toFixed(0)}ms
              </span>
            </div>
            <button 
              onClick={(e) => { e.stopPropagation(); setShowDataPanel(!showDataPanel) }}
              className="text-gray-500 hover:text-white text-[10px] px-1"
            >
              {showDataPanel ? '▲' : '▼'}
            </button>
          </div>
          
          {/* 可折叠内容区 */}
          {showDataPanel && (
            <div className="px-2 py-1.5 border-t border-[#333] space-y-1">
              <div className="flex items-center justify-between">
                <span className="text-gray-500">Feed Status:</span>
                <span className={`px-1 rounded text-[8px] ${
                  dataQuality.feedStatus === 'connected' ? 'bg-green-900/50 text-green-400' :
                  dataQuality.feedStatus === 'delayed' ? 'bg-yellow-900/50 text-yellow-400' :
                  'bg-red-900/50 text-red-400'
                }`}>
                  {dataQuality.feedStatus.toUpperCase()}
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-gray-500">Packet Loss:</span>
                <span className={dataQuality.packetLoss < 1 ? 'text-green-400' : dataQuality.packetLoss < 3 ? 'text-yellow-400' : 'text-red-400'}>
                  {dataQuality.packetLoss.toFixed(2)}%
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-gray-500">Outage Count:</span>
                <span className={dataQuality.outageCount === 0 ? 'text-green-400' : 'text-red-400'}>
                  {dataQuality.outageCount}
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-gray-500">Last Update:</span>
                <span className={isDataStale ? 'text-red-400' : 'text-gray-400'}>
                  {new Date(dataQuality.lastUpdate).toLocaleTimeString()}
                </span>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* 浮动信息卡片 - 跟随鼠标右下侧 */}
      {renderFloatingTooltip()}

      {/* 事件标记叠加层 - 缩小尺寸 + 虚化效果 + 8+2判断依据 */}
      <div className="absolute bottom-14 left-1/2 transform -translate-x-1/2 z-20 flex items-center gap-2">
        {eventMarkers.slice(-3).map((event, idx) => (
          <div 
            key={idx}
            className={`relative px-2.5 py-1 rounded-md text-[10px] font-mono flex items-center gap-1.5 shadow-lg cursor-pointer transition-all duration-200 hover:scale-102 backdrop-blur-sm ${
              event.type === 'news' ? 'bg-blue-900/60 text-blue-200/90 border border-blue-500/50 hover:bg-blue-900/80' :
              event.type === 'liq' ? 'bg-orange-900/60 text-orange-200/90 border border-orange-500/50 hover:bg-orange-900/80' :
              event.type === 'spike' ? 'bg-red-900/60 text-red-200/90 border border-red-500/50 hover:bg-red-900/80' :
              'bg-yellow-900/60 text-yellow-200/90 border border-yellow-500/50 hover:bg-yellow-900/80'
            }`}
            onMouseEnter={() => setHoveredEvent(event)}
            onMouseLeave={() => setHoveredEvent(null)}
          >
            <span className="text-[11px] opacity-80">{event.type === 'news' ? '📰' : event.type === 'liq' ? '💧' : event.type === 'spike' ? '⚡' : '⏸️'}</span>
            <span className="font-medium text-[10px] opacity-90">{event.label}</span>
            {/* 可信度小标签 */}
            <span className={`text-[8px] px-1 py-0.5 rounded opacity-80 ${
              event.confidence >= 75 && event.verified ? 'bg-green-500/40 text-green-200' :
              event.confidence >= 60 ? 'bg-yellow-500/40 text-yellow-200' :
              'bg-red-500/40 text-red-200'
            }`}>
              {event.confidence}%
            </span>
            {/* 验证状态 */}
            {event.verified && <span className="text-[8px] text-green-400 opacity-70">✓</span>}
            
            {/* 悬浮详情框 - 8+2判断依据 */}
            {hoveredEvent === event && (
              <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 w-96 bg-[#0a0a0a]/98 border border-[#333] rounded-lg shadow-2xl p-3 z-50 backdrop-blur-xl">
                {/* 箭头 */}
                <div className="absolute bottom-0 left-1/2 -translate-x-1/2 translate-y-full border-6 border-transparent border-t-[#333]"></div>
                
                {/* 头部 */}
                <div className="flex items-center justify-between mb-2 pb-2 border-b border-[#222]">
                  <div className="flex items-center gap-2">
                    <span className="text-[14px]">{event.type === 'news' ? '📰' : event.type === 'liq' ? '💧' : event.type === 'spike' ? '⚡' : '⏸️'}</span>
                    <span className="text-[13px] font-bold text-white">{event.label}</span>
                    {event.verified ? (
                      <span className="text-[9px] px-1.5 py-0.5 bg-green-500/20 text-green-400 rounded border border-green-500/30">✓ 已验证</span>
                    ) : (
                      <span className="text-[9px] px-1.5 py-0.5 bg-yellow-500/20 text-yellow-400 rounded border border-yellow-500/30">⚠ 待验证</span>
                    )}
                  </div>
                  <div className="flex items-center gap-1">
                    <span className={`text-[10px] px-1.5 py-0.5 rounded font-bold ${
                      event.impact === 'HIGH' ? 'bg-red-500/30 text-red-300' :
                      event.impact === 'MEDIUM' ? 'bg-yellow-500/30 text-yellow-300' :
                      'bg-green-500/30 text-green-300'
                    }`}>
                      {event.impact}
                    </span>
                  </div>
                </div>
                
                {/* 可信度 + 来源 */}
                <div className="flex items-center justify-between mb-2 text-[10px]">
                  <div className="flex items-center gap-2">
                    <span className="text-[#666]">来源:</span>
                    <span className="text-cyan-400 font-mono">{event.source}</span>
                  </div>
                  <div className="flex items-center gap-1">
                    <span className="text-[#666]">可信度:</span>
                    <span className={`font-bold ${
                      event.confidence >= 75 ? 'text-green-400' :
                      event.confidence >= 60 ? 'text-yellow-400' :
                      'text-red-400'
                    }`}>{event.confidence}%</span>
                  </div>
                </div>
                
                {/* 8项基本判断依据 */}
                <div className="mb-2">
                  <div className="flex items-center gap-2 mb-1.5">
                    <span className="text-[9px] text-[#666]">📋 基本判断依据</span>
                    <span className="text-[8px] text-[#444]">({event.basicReasons.filter(r => r.passed).length}/8 通过)</span>
                  </div>
                  <div className="grid grid-cols-2 gap-1">
                    {event.basicReasons.map((reason, i) => (
                      <div key={i} className={`flex items-center justify-between px-1.5 py-0.5 rounded text-[8px] ${
                        reason.passed ? 'bg-green-500/10 border border-green-500/20' : 'bg-red-500/10 border border-red-500/20'
                      }`}>
                        <span className={reason.passed ? 'text-[#888]' : 'text-[#666]'}>{reason.name}</span>
                        <div className="flex items-center gap-1">
                          <span className={reason.passed ? 'text-green-400' : 'text-red-400'}>{reason.value}</span>
                          <span className={reason.passed ? 'text-green-500' : 'text-red-500'}>{reason.passed ? '✓' : '✗'}</span>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
                
                {/* 2项专业判断 */}
                <div className="mb-2">
                  <span className="text-[9px] text-[#666] block mb-1.5">🎯 专业判断</span>
                  <div className="space-y-1.5">
                    {event.expertReasons.map((reason, i) => (
                      <div key={i} className="bg-[#111] border border-[#222] rounded p-1.5">
                        <div className="flex items-center justify-between mb-0.5">
                          <span className="text-[9px] text-cyan-400 font-medium">{reason.name}</span>
                          <span className={`text-[8px] px-1 rounded ${
                            reason.confidence >= 80 ? 'bg-green-500/20 text-green-400' :
                            reason.confidence >= 60 ? 'bg-yellow-500/20 text-yellow-400' :
                            'bg-red-500/20 text-red-400'
                          }`}>{reason.confidence}%</span>
                        </div>
                        <span className="text-[9px] text-[#aaa] leading-tight block">{reason.analysis}</span>
                      </div>
                    ))}
                  </div>
                </div>
                
                {/* 关联资产 + 时间戳 */}
                <div className="flex items-center justify-between pt-1.5 border-t border-[#222] text-[8px]">
                  <div className="flex items-center gap-1">
                    <span className="text-[#555]">关联:</span>
                    {event.relatedAssets?.map((asset, i) => (
                      <span key={i} className="px-1 py-0.5 bg-[#1a1a1a] text-[#777] rounded border border-[#333]">{asset}</span>
                    ))}
                  </div>
                  <span className="text-[#444]">🕐 {event.timestamp}</span>
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
      
      {/* 图表容器 */}
      <div className="relative w-full" style={{ height: height }}>
        {/* Persistent OHLCV Display - 增大字体 */}
        <div className="absolute top-0 left-[50%] transform -translate-x-1/2 z-20 bg-[#0a0a0a]/95 px-4 py-1 rounded-b border-x border-b border-[#222] font-mono text-[11px] flex items-center gap-3 whitespace-nowrap">
          <span className="text-[#555]">O</span>
          <span className="text-[#aaa] font-semibold">{hoverInfo ? hoverInfo.open.toFixed(2) : dataRef.current[dataRef.current.length - 1]?.open?.toFixed(2) || '--'}</span>
          <span className="text-[#555]">H</span>
          <span className="text-[#00cc77] font-semibold">{hoverInfo ? hoverInfo.high.toFixed(2) : dataRef.current[dataRef.current.length - 1]?.high?.toFixed(2) || '--'}</span>
          <span className="text-[#555]">L</span>
          <span className="text-[#dd4444] font-semibold">{hoverInfo ? hoverInfo.low.toFixed(2) : dataRef.current[dataRef.current.length - 1]?.low?.toFixed(2) || '--'}</span>
          <span className="text-[#555]">C</span>
          <span className={`font-bold ${(hoverInfo?.changePercent ?? 0) >= 0 ? 'text-[#00cc77]' : 'text-[#dd4444]'}`}>
            {hoverInfo ? hoverInfo.close.toFixed(2) : dataRef.current[dataRef.current.length - 1]?.close?.toFixed(2) || '--'}
          </span>
          <span className="text-[#333]">|</span>
          <span className="text-[#555]">V</span>
          <span className="text-[#888] font-semibold">{hoverInfo ? ((hoverInfo.volume || 0) / 1e6).toFixed(1) + 'M' : ((dataRef.current[dataRef.current.length - 1]?.volume || 0) / 1e6).toFixed(1) + 'M'}</span>
        </div>
        <div ref={chartContainerRef} className="w-full h-full" />
      </div>
    </div>
  )
}

export default LightweightChart
