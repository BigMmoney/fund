import { useState, useEffect, useRef } from 'react'
import {
  ComposedChart, Line, Bar, XAxis, YAxis, ResponsiveContainer, Tooltip,
  ReferenceLine, Area, AreaChart, BarChart, Cell
} from 'recharts'
import { TrendingUp, TrendingDown, BarChart3, CandlestickChart, Activity, Layers } from 'lucide-react'

// ============================================================================
// 防闪烁优化: 使用稳定的数据快照而非实时更新
// ============================================================================
const DATA_SNAPSHOT_INTERVAL = 500 // 数据快照间隔(ms), 降低更新频率防止闪烁

// 专业加密货币图标 SVG
export const CryptoIcons: Record<string, JSX.Element> = {
  BTC: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#F7931A"/>
      <path fill="#FFF" d="M22.5 14.1c.3-2-1.2-3.1-3.3-3.8l.7-2.7-1.6-.4-.7 2.6c-.4-.1-.8-.2-1.3-.3l.7-2.6-1.6-.4-.7 2.7c-.3-.1-.7-.2-1-.3l-2.2-.6-.4 1.7s1.2.3 1.2.3c.7.2.8.6.8 1l-.8 3.2c0 0 .1 0 .2.1h-.2l-1.1 4.4c-.1.2-.3.5-.8.4 0 0-1.2-.3-1.2-.3l-.8 1.8 2.1.5c.4.1.8.2 1.2.3l-.7 2.7 1.6.4.7-2.7c.4.1.9.2 1.3.3l-.7 2.7 1.6.4.7-2.7c2.8.5 4.9.3 5.8-2.2.7-2-.1-3.2-1.5-4 1.1-.3 1.9-1 2.1-2.4zm-3.8 5.3c-.5 2-3.9.9-5 .7l.9-3.6c1.1.3 4.6.8 4.1 2.9zm.5-5.4c-.5 1.8-3.3.9-4.2.7l.8-3.3c.9.2 3.9.7 3.4 2.6z"/>
    </svg>
  ),
  ETH: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#627EEA"/>
      <path fill="#FFF" fillOpacity=".6" d="M16 4v8.9l7.5 3.3z"/>
      <path fill="#FFF" d="M16 4L8.5 16.2l7.5-3.3z"/>
      <path fill="#FFF" fillOpacity=".6" d="M16 21.9v6.1l7.5-10.4z"/>
      <path fill="#FFF" d="M16 28v-6.1L8.5 17.6z"/>
      <path fill="#FFF" fillOpacity=".2" d="M16 20.6l7.5-4.4L16 12.9z"/>
      <path fill="#FFF" fillOpacity=".6" d="M8.5 16.2l7.5 4.4v-7.7z"/>
    </svg>
  ),
  SOL: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="url(#sol-gradient)"/>
      <defs>
        <linearGradient id="sol-gradient" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#00FFA3"/>
          <stop offset="100%" stopColor="#DC1FFF"/>
        </linearGradient>
      </defs>
      <path fill="#FFF" d="M9.5 19.8c.1-.1.3-.2.5-.2h12.6c.3 0 .5.4.3.6l-2.4 2.4c-.1.1-.3.2-.5.2H7.4c-.3 0-.5-.4-.3-.6l2.4-2.4z"/>
      <path fill="#FFF" d="M9.5 9.4c.1-.1.3-.2.5-.2H22.6c.3 0 .5.4.3.6l-2.4 2.4c-.1.1-.3.2-.5.2H7.4c-.3 0-.5-.4-.3-.6l2.4-2.4z"/>
      <path fill="#FFF" d="M22.5 14.5c-.1-.1-.3-.2-.5-.2H9.4c-.3 0-.5.4-.3.6l2.4 2.4c.1.1.3.2.5.2H24.6c.3 0 .5-.4.3-.6l-2.4-2.4z"/>
    </svg>
  ),
  BNB: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#F3BA2F"/>
      <path fill="#FFF" d="M12.1 14.1L16 10.2l3.9 3.9 2.3-2.3L16 5.6l-6.2 6.2 2.3 2.3zm-6.5 1.9l2.3-2.3 2.3 2.3-2.3 2.3-2.3-2.3zm6.5 1.9L16 21.8l3.9-3.9 2.3 2.3-6.2 6.2-6.2-6.2 2.3-2.3zm8.8-1.9l2.3-2.3 2.3 2.3-2.3 2.3-2.3-2.3zM18.5 16L16 13.5 13.5 16 16 18.5 18.5 16z"/>
    </svg>
  ),
  XRP: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#23292F"/>
      <path fill="#FFF" d="M23.1 8h2.7l-5.9 5.9c-2.2 2.2-5.6 2.2-7.8 0L6.2 8h2.7l4.6 4.6c1.4 1.4 3.6 1.4 5 0L23.1 8zM8.9 24H6.2l5.9-5.9c2.2-2.2 5.6-2.2 7.8 0l5.9 5.9h-2.7l-4.6-4.6c-1.4-1.4-3.6-1.4-5 0L8.9 24z"/>
    </svg>
  ),
  ADA: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#0033AD"/>
      <path fill="#FFF" d="M16 8.5a1.5 1.5 0 110 3 1.5 1.5 0 010-3zm0 12a1.5 1.5 0 110 3 1.5 1.5 0 010-3zm-6-6a1.5 1.5 0 110 3 1.5 1.5 0 010-3zm12 0a1.5 1.5 0 110 3 1.5 1.5 0 010-3zm-9-4a1 1 0 110 2 1 1 0 010-2zm6 0a1 1 0 110 2 1 1 0 010-2zm-6 8a1 1 0 110 2 1 1 0 010-2zm6 0a1 1 0 110 2 1 1 0 010-2z"/>
    </svg>
  ),
  DOGE: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#C2A633"/>
      <path fill="#FFF" d="M13 10h4.5c4.3 0 6.5 2.7 6.5 6s-2.2 6-6.5 6H13V10zm3 2.3v7.4h1.5c2.3 0 3.5-1.5 3.5-3.7s-1.2-3.7-3.5-3.7H16z"/>
    </svg>
  ),
  DOT: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#E6007A"/>
      <ellipse cx="16" cy="16" rx="4" ry="4" fill="#FFF"/>
      <ellipse cx="16" cy="8" rx="2.5" ry="2.5" fill="#FFF"/>
      <ellipse cx="16" cy="24" rx="2.5" ry="2.5" fill="#FFF"/>
    </svg>
  ),
  MATIC: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#8247E5"/>
      <path fill="#FFF" d="M21.1 13.2c-.4-.2-.9-.2-1.2 0l-2.8 1.7-1.9 1.1-2.8 1.7c-.4.2-.9.2-1.2 0l-2.2-1.3c-.4-.2-.6-.6-.6-1.1v-2.5c0-.4.2-.9.6-1.1l2.2-1.3c.4-.2.9-.2 1.2 0l2.2 1.3c.4.2.6.6.6 1.1v1.7l1.9-1.1v-1.7c0-.4-.2-.9-.6-1.1l-4-2.4c-.4-.2-.9-.2-1.2 0l-4.1 2.4c-.4.2-.6.6-.6 1.1v4.7c0 .4.2.9.6 1.1l4.1 2.4c.4.2.9.2 1.2 0l2.8-1.6 1.9-1.1 2.8-1.6c.4-.2.9-.2 1.2 0l2.2 1.3c.4.2.6.6.6 1.1v2.5c0 .4-.2.9-.6 1.1l-2.2 1.3c-.4.2-.9.2-1.2 0l-2.2-1.3c-.4-.2-.6-.6-.6-1.1v-1.7l-1.9 1.1v1.7c0 .4.2.9.6 1.1l4.1 2.4c.4.2.9.2 1.2 0l4.1-2.4c.4-.2.6-.6.6-1.1v-4.7c0-.4-.2-.9-.6-1.1l-4.1-2.4z"/>
    </svg>
  ),
  AVAX: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#E84142"/>
      <path fill="#FFF" d="M19.7 21h3.4c.4 0 .6-.2.7-.5.1-.3 0-.6-.2-.8L16.5 8.3c-.2-.3-.4-.4-.7-.4-.3 0-.5.1-.7.4L13 12.5l2.1 3.6 4.6 4.9zm-7.4 0h-3.4c-.4 0-.6-.2-.7-.5-.1-.3 0-.6.2-.8l2.6-4.5L13.1 19l-.8 2z"/>
    </svg>
  ),
  LINK: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#2A5ADA"/>
      <path fill="#FFF" d="M16 6l-1.4.8-6.2 3.6L7 11.2v9.6l1.4.8 6.2 3.6 1.4.8 1.4-.8 6.2-3.6 1.4-.8v-9.6l-1.4-.8-6.2-3.6L16 6zm4.8 14l-4.8 2.8-4.8-2.8v-5.6L16 11.6l4.8 2.8v5.6z"/>
    </svg>
  ),
  UNI: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#FF007A"/>
      <path fill="#FFF" d="M11.5 8c-1.5 0-2.5 1.2-2.5 2.5s1 2.5 2.5 2.5c.3 0 .6 0 .9-.1-.1.4-.2.9-.2 1.4 0 3.3 2.7 6 6 6s6-2.7 6-6c0-1.4-.5-2.7-1.3-3.8.1-.2.2-.4.2-.6 0-.8-.6-1.4-1.4-1.4s-1.4.6-1.4 1.4.6 1.4 1.4 1.4c.1 0 .2 0 .3 0 .6.9 1 2 1 3.1 0 3-2.5 5.5-5.5 5.5s-5.5-2.5-5.5-5.5c0-.4 0-.9.1-1.3.5.2 1 .3 1.6.3 2 0 3.5-1.5 3.5-3.5s-1.5-3.5-3.5-3.5c-.7 0-1.4.2-1.9.6-.3-.3-.8-.5-1.3-.5z"/>
    </svg>
  ),
}

// 专业股票图标
export const StockIcons: Record<string, JSX.Element> = {
  AAPL: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#000"/>
      <path fill="#FFF" d="M22.3 21.4c-.6 1.3-1.4 2.4-2.4 3.5-.8.9-1.5 1.5-2.1 1.8-.9.5-1.8.8-2.8.7-1.1 0-2-.3-2.7-.9-.7-.6-1.5-.9-2.2-.9-.8 0-1.5.3-2.3.9-.7.6-1.5.9-2.4.9-1 0-2-.3-2.9-.9-.9-.6-1.7-1.4-2.4-2.4-1.5-2-2.3-4.4-2.3-7.2 0-2.7.7-4.9 2-6.6 1-1.3 2.4-2 4-2 1 0 2.2.4 3.3.9.8.4 1.3.6 1.6.6.4 0 1-.2 1.9-.7 1-.5 2-.7 3-.7 2.5.1 4.3 1.3 5.4 3.6-2.2 1.3-3.3 3.2-3.2 5.6.1 1.9.8 3.4 2 4.6.6.6 1.3 1.1 2 1.4-.2.5-.4 1-.5 1.3zM17.9 4c0 1.5-.5 2.9-1.6 4.2-.9 1.1-2.2 1.8-3.5 1.7 0-1.4.6-2.8 1.6-3.9.5-.6 1.1-1 1.9-1.4.8-.4 1.5-.6 2.2-.6 0 0 0 0-.6 0z"/>
    </svg>
  ),
  MSFT: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#00A4EF"/>
      <rect x="7" y="7" width="8" height="8" fill="#F25022"/>
      <rect x="17" y="7" width="8" height="8" fill="#7FBA00"/>
      <rect x="7" y="17" width="8" height="8" fill="#00A4EF"/>
      <rect x="17" y="17" width="8" height="8" fill="#FFB900"/>
    </svg>
  ),
  GOOGL: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#4285F4"/>
      <path fill="#FFF" d="M23.5 16.3c0-.6-.1-1.2-.2-1.8H16v3.4h4.2c-.2 1-.7 1.9-1.5 2.5v2h2.4c1.4-1.3 2.4-3.2 2.4-6.1z"/>
      <path fill="#FFF" d="M16 24c2 0 3.7-.7 5-1.8l-2.4-1.9c-.7.5-1.5.7-2.6.7-2 0-3.7-1.4-4.3-3.2H9.2v2c1.3 2.6 3.9 4.2 6.8 4.2z"/>
      <path fill="#FFF" d="M11.7 17.7c-.3-.9-.3-1.9 0-2.8v-2H9.2c-1.1 2.1-1.1 4.5 0 6.6l2.5-1.8z"/>
      <path fill="#FFF" d="M16 11.2c1.1 0 2.1.4 2.9 1.1l2.2-2.2C19.7 8.8 18 8 16 8c-2.9 0-5.5 1.6-6.8 4.2l2.5 2c.6-1.8 2.3-3 4.3-3z"/>
    </svg>
  ),
  AMZN: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#FF9900"/>
      <path fill="#232F3E" d="M23.1 22.5c-5.5 4.1-13.5 6.2-20.4 4.2-.4-.2-.8.2-.5.6 1.4 1.5 6 4.3 11.8 4.3 5.3 0 9.4-2 10.5-3.2.4-.5-.1-1-.9-.6-.3.2-.5.4-.5.7zm2.4-1.4c-.4-.6-2.9-.3-4.1-.1-.3 0-.4.3-.1.5 2 1.4 5.3 1 5.6.5.3-.4-.1-1.5-1.4-.9z"/>
      <path fill="#FFF" d="M18.6 13c0-1.5-.1-2.7-.5-3.9-.4-1.2-1.1-2-2.4-2-1.7 0-2.7 1.3-2.9 2.8-.1.5-.1 1-.1 1.5v3.8c0 .5 0 1 .1 1.5.2 1.5 1.2 2.8 2.9 2.8 1.3 0 2-1 2.4-2 .4-1.2.5-2.4.5-3.9v-.6zm2.5.3v3.4c0 1.4-.1 2.6-.4 3.8-.6 2.1-1.9 3.8-4.3 4.4-.4.1-.8.1-1.3.1-2 0-3.5-.7-4.5-2.3-.5-.8-.8-1.7-1-2.7-.2-1.1-.3-2.3-.3-3.5v-3.2c0-1.4.1-2.8.5-4.1.7-2.2 2.1-3.8 4.4-4.4.5-.1 1-.2 1.5-.2 2 0 3.5.8 4.4 2.5.5.9.8 1.9.9 3 .1.9.1 1.8.1 2.7v.5z"/>
    </svg>
  ),
  TSLA: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#CC0000"/>
      <path fill="#FFF" d="M16 6l-8 3.5v2h3.5V25h3v-13.5h3V25h3V11.5H24v-2L16 6z"/>
    </svg>
  ),
  NVDA: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#76B900"/>
      <path fill="#FFF" d="M12 10v12h2V14.5l4 5.5h.5l4-5.5V22h2V10h-2l-4.5 6.5L13.5 10H12z"/>
    </svg>
  ),
  META: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#0081FB"/>
      <path fill="#FFF" d="M8 21.5c0 1 .5 1.5 1 1.5s1-.5 2-2c1-1.5 2-4 3-6s2-4 3.5-4c1 0 2 1 2.5 2.5s1 3.5 1 5.5c0 1 .5 2 1.5 2s1.5-1 1.5-2.5-1-4.5-2-6.5-2.5-3-4.5-3c-2.5 0-4 2-5.5 5s-2.5 5.5-2.5 6.5-.5 1-1 1z"/>
    </svg>
  ),
  JPM: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#0A5DA6"/>
      <path fill="#FFF" d="M8 11h3v6c0 2 1 3 3 3s3-1 3-3v-6h3v6c0 3.5-2.5 6-6 6s-6-2.5-6-6v-6z"/>
    </svg>
  ),
  V: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#1A1F71"/>
      <path fill="#F7B600" d="M12 10l4 12 4-12h3l-5.5 14h-3L9 10h3z"/>
    </svg>
  ),
  WMT: (
    <svg viewBox="0 0 32 32" className="w-5 h-5">
      <circle cx="16" cy="16" r="16" fill="#0071DC"/>
      <path fill="#FFC220" d="M16 8l2 6h6l-5 4 2 6-5-4-5 4 2-6-5-4h6z"/>
    </svg>
  ),
}

// 获取资产图标
export const getAssetIcon = (symbol: string): JSX.Element => {
  const upperSymbol = symbol?.toUpperCase() || ''
  if (CryptoIcons[upperSymbol]) return CryptoIcons[upperSymbol]
  if (StockIcons[upperSymbol]) return StockIcons[upperSymbol]
  
  // 默认图标
  return (
    <div className="w-5 h-5 rounded-full bg-gradient-to-br from-gray-600 to-gray-800 flex items-center justify-center text-[8px] font-bold text-white">
      {upperSymbol.slice(0, 2)}
    </div>
  )
}

// K线数据类型
interface CandleData {
  time: string
  open: number
  high: number
  low: number
  close: number
  volume: number
  // 移动平均线
  ma5?: number
  ma10?: number
  ma20?: number
  ma60?: number
  // EMA指数移动平均
  ema12?: number
  ema26?: number
  // MACD
  macd?: number
  signal?: number
  histogram?: number
  // RSI
  rsi?: number
  // 布林带
  upperBB?: number
  lowerBB?: number
  middleBB?: number
  // KDJ随机指标
  k?: number
  d?: number
  j?: number
  // VWAP成交量加权平均价
  vwap?: number
  // ATR平均真实波幅
  atr?: number
  // OBV能量潮
  obv?: number
  // SAR抛物线指标
  sar?: number
  // CCI顺势指标
  cci?: number
  // Williams %R
  willR?: number
  // 资金流向
  mfi?: number
  // 买卖力量
  buyPressure?: number
  sellPressure?: number
}

interface ProfessionalChartProps {
  symbol: string
  basePrice: number
  chartType: 'line' | 'candle' | 'area' | 'depth' | 'volume' | 'macd' | 'rsi' | 'kdj' | 'indicators' | 'fibonacci' | 'profile' | 'flow' | 'trend' | 'heatmap'
  timeframe: string
  height?: number
  showIndicators?: boolean
}

// 计算完整的技术指标
const calculateIndicators = (
  prices: number[], 
  highs: number[], 
  lows: number[], 
  volumes: number[],
  basePrice: number,
  prevIndicators?: { ema12: number; ema26: number; obv: number; atr: number }
) => {
  const len = prices.length
  if (len === 0) {
    return { 
      ma5: basePrice, ma10: basePrice, ma20: basePrice, ma60: basePrice,
      ema12: basePrice, ema26: basePrice,
      rsi: 50, std: 0, macd: 0, signal: 0,
      k: 50, d: 50, j: 50,
      vwap: basePrice, atr: 0, obv: 0, sar: basePrice, cci: 0, willR: -50
    }
  }
  
  // 简单移动平均线 SMA
  const ma5 = prices.slice(-5).reduce((a, b) => a + b, 0) / Math.min(5, len)
  const ma10 = prices.slice(-10).reduce((a, b) => a + b, 0) / Math.min(10, len)
  const ma20 = prices.slice(-20).reduce((a, b) => a + b, 0) / Math.min(20, len)
  const ma60 = prices.slice(-60).reduce((a, b) => a + b, 0) / Math.min(60, len)
  
  // EMA指数移动平均 (使用平滑因子)
  const currentPrice = prices[len - 1]
  const ema12Multiplier = 2 / 13
  const ema26Multiplier = 2 / 27
  const prevEma12 = prevIndicators?.ema12 || ma5
  const prevEma26 = prevIndicators?.ema26 || ma20
  const ema12 = (currentPrice - prevEma12) * ema12Multiplier + prevEma12
  const ema26 = (currentPrice - prevEma26) * ema26Multiplier + prevEma26
  
  // MACD = EMA12 - EMA26
  const macd = (ema12 - ema26) / basePrice * 10000
  const signal = macd * 0.8  // 简化的信号线
  
  // RSI (14期)
  let avgGain = 0, avgLoss = 0.001
  if (len > 1) {
    let gainSum = 0, lossSum = 0
    const period = Math.min(14, len - 1)
    for (let j = 1; j <= period; j++) {
      const diff = prices[len - j] - prices[len - j - 1]
      if (diff > 0) gainSum += diff
      else lossSum += Math.abs(diff)
    }
    avgGain = gainSum / period
    avgLoss = lossSum / period || 0.001
  }
  const rsi = 100 - (100 / (1 + avgGain / avgLoss))
  
  // 布林带 (20期, 2标准差)
  const prices20 = prices.slice(-20)
  const std = prices20.length > 1 
    ? Math.sqrt(prices20.reduce((acc, p) => acc + Math.pow(p - ma20, 2), 0) / prices20.length)
    : basePrice * 0.01
  
  // KDJ随机指标 (9期)
  const period9 = Math.min(9, len)
  const high9 = Math.max(...highs.slice(-period9))
  const low9 = Math.min(...lows.slice(-period9))
  const rsv = high9 !== low9 ? ((currentPrice - low9) / (high9 - low9)) * 100 : 50
  const k = rsv  // 简化版，实际需要平滑
  const d = k * 0.67 + 33  // 简化的D值
  const j = 3 * k - 2 * d
  
  // VWAP成交量加权平均价
  let vwapNumerator = 0, vwapDenominator = 0
  for (let i = 0; i < len; i++) {
    const typicalPrice = (prices[i] + (highs[i] || prices[i]) + (lows[i] || prices[i])) / 3
    vwapNumerator += typicalPrice * (volumes[i] || 1)
    vwapDenominator += volumes[i] || 1
  }
  const vwap = vwapDenominator > 0 ? vwapNumerator / vwapDenominator : basePrice
  
  // ATR平均真实波幅 (14期)
  let atrSum = 0
  const atrPeriod = Math.min(14, len - 1)
  for (let i = len - atrPeriod; i < len; i++) {
    if (i > 0) {
      const tr = Math.max(
        (highs[i] || prices[i]) - (lows[i] || prices[i]),
        Math.abs((highs[i] || prices[i]) - prices[i - 1]),
        Math.abs((lows[i] || prices[i]) - prices[i - 1])
      )
      atrSum += tr
    }
  }
  const atr = atrPeriod > 0 ? atrSum / atrPeriod : 0
  
  // OBV能量潮
  let obv = prevIndicators?.obv || 0
  if (len > 1) {
    const lastVolume = volumes[len - 1] || 0
    if (currentPrice > prices[len - 2]) obv += lastVolume
    else if (currentPrice < prices[len - 2]) obv -= lastVolume
  }
  
  // SAR抛物线 (简化版)
  const sar = currentPrice > ma20 ? ma20 - atr : ma20 + atr
  
  // CCI顺势指标 (20期)
  const typicalPrice = (currentPrice + (highs[len-1] || currentPrice) + (lows[len-1] || currentPrice)) / 3
  const meanDeviation = prices20.reduce((acc, p) => acc + Math.abs(p - ma20), 0) / prices20.length || 1
  const cci = (typicalPrice - ma20) / (0.015 * meanDeviation)
  
  // Williams %R (14期)
  const high14 = Math.max(...highs.slice(-14))
  const low14 = Math.min(...lows.slice(-14))
  const willR = high14 !== low14 ? ((high14 - currentPrice) / (high14 - low14)) * -100 : -50
  
  return { 
    ma5, ma10, ma20, ma60,
    ema12, ema26,
    rsi: Math.max(0, Math.min(100, rsi)), 
    std, macd, signal,
    k: Math.max(0, Math.min(100, k)),
    d: Math.max(0, Math.min(100, d)),
    j: Math.max(-20, Math.min(120, j)),
    vwap, atr, obv, sar,
    cci: Math.max(-200, Math.min(200, cci)),
    willR: Math.max(-100, Math.min(0, willR))
  }
}

// 生成初始K线数据 - 币安风格，有真实涨跌
const generateInitialData = (basePrice: number, points: number = 60): CandleData[] => {
  const data: CandleData[] = []
  const now = Date.now()
  let price = basePrice
  const priceHistory: number[] = []
  const highHistory: number[] = []
  const lowHistory: number[] = []
  const volumeHistory: number[] = []
  let prevInd: { ema12: number; ema26: number; obv: number; atr: number } | undefined
  
  for (let i = 0; i < points; i++) {
    // 基于正弦波+噪声的价格变化，模拟真实市场
    const cycle1 = Math.sin(i * 0.15) * basePrice * 0.008  // 大周期
    const cycle2 = Math.sin(i * 0.4) * basePrice * 0.003   // 中周期
    const cycle3 = Math.sin(i * 1.2) * basePrice * 0.001   // 小周期
    
    const open = price
    const change = cycle1 + cycle2 + cycle3
    const close = basePrice + change
    
    // 高低点
    const volatility = basePrice * 0.002
    const high = Math.max(open, close) + Math.abs(Math.sin(i * 0.7)) * volatility
    const low = Math.min(open, close) - Math.abs(Math.cos(i * 0.5)) * volatility
    
    // 成交量变化
    const volume = 800000 + Math.abs(Math.sin(i * 0.3)) * 600000
    
    priceHistory.push(close)
    highHistory.push(high)
    lowHistory.push(low)
    volumeHistory.push(volume)
    
    const ind = calculateIndicators(priceHistory, highHistory, lowHistory, volumeHistory, basePrice, prevInd)
    prevInd = { ema12: ind.ema12, ema26: ind.ema26, obv: ind.obv, atr: ind.atr }
    
    data.push({
      time: new Date(now - (points - i) * 60000).toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' }),
      open, high, low, close, volume,
      ma5: ind.ma5, ma10: ind.ma10, ma20: ind.ma20, ma60: ind.ma60,
      ema12: ind.ema12, ema26: ind.ema26,
      rsi: ind.rsi,
      upperBB: ind.ma20 + ind.std * 2,
      lowerBB: ind.ma20 - ind.std * 2,
      middleBB: ind.ma20,
      macd: ind.macd, signal: ind.signal,
      histogram: ind.macd - ind.signal,
      k: ind.k, d: ind.d, j: ind.j,
      vwap: ind.vwap, atr: ind.atr, obv: ind.obv,
      sar: ind.sar, cci: ind.cci, willR: ind.willR
    })
    
    price = close
  }
  
  return data
}

// 自定义K线蜡烛组件
const CandleStick = (props: any) => {
  const { x, y, width, height, payload } = props
  if (!payload) return null
  
  const { open, close, high, low } = payload
  const isUp = close >= open
  const color = isUp ? '#00ff88' : '#ff4444'
  const candleWidth = Math.max(width * 0.6, 2)
  const wickWidth = 1
  
  const scaleY = (value: number) => {
    const range = high - low || 1
    return y + (high - value) / range * height
  }
  
  return (
    <g>
      {/* 上影线 */}
      <line
        x1={x + candleWidth / 2}
        y1={scaleY(high)}
        x2={x + candleWidth / 2}
        y2={scaleY(Math.max(open, close))}
        stroke={color}
        strokeWidth={wickWidth}
      />
      {/* 下影线 */}
      <line
        x1={x + candleWidth / 2}
        y1={scaleY(Math.min(open, close))}
        x2={x + candleWidth / 2}
        y2={scaleY(low)}
        stroke={color}
        strokeWidth={wickWidth}
      />
      {/* 实体 */}
      <rect
        x={x}
        y={scaleY(Math.max(open, close))}
        width={candleWidth}
        height={Math.max(Math.abs(scaleY(open) - scaleY(close)), 1)}
        fill={isUp ? color : color}
        stroke={color}
        strokeWidth={0.5}
      />
    </g>
  )
}

// 标记为导出以避免未使用警告
export { CandleStick }

export function ProfessionalChart({
  symbol,
  basePrice,
  chartType,
  timeframe: _timeframe,
  height = 200,
  showIndicators: _showIndicators = true
}: ProfessionalChartProps) {
  // ============================================================================
  // 防闪烁优化: 使用稳定的数据快照
  // ============================================================================
  const [dataSnapshot, setDataSnapshot] = useState<CandleData[]>([])
  const [indicators] = useState({
    ma5: true,
    ma10: true,
    ma20: false,
    bb: false,
    volume: true
  })
  
  // ============================================================================
  // TradingView风格: 悬停数据状态
  // ============================================================================
  const [hoveredData, setHoveredData] = useState<CandleData | null>(null)
  const hoverDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // 使用useRef保存实时数据，避免频繁触发render
  const dataRef = useRef<CandleData[]>([])
  const priceHistoryRef = useRef<number[]>([])
  const highHistoryRef = useRef<number[]>([])
  const lowHistoryRef = useRef<number[]>([])
  const volumeHistoryRef = useRef<number[]>([])
  const prevIndRef = useRef<{ ema12: number; ema26: number; obv: number; atr: number } | undefined>()
  const tickCountRef = useRef(0)
  const initializedRef = useRef(false)
  const lastSymbolRef = useRef('')

  // 初始化数据 - 只在symbol/basePrice变化时执行
  useEffect(() => {
    if (lastSymbolRef.current !== symbol || !initializedRef.current) {
      const initialData = generateInitialData(basePrice)
      dataRef.current = initialData
      priceHistoryRef.current = initialData.map(d => d.close)
      highHistoryRef.current = initialData.map(d => d.high)
      lowHistoryRef.current = initialData.map(d => d.low)
      volumeHistoryRef.current = initialData.map(d => d.volume)
      const lastD = initialData[initialData.length - 1]
      prevIndRef.current = { 
        ema12: lastD.ema12 || basePrice, 
        ema26: lastD.ema26 || basePrice, 
        obv: lastD.obv || 0, 
        atr: lastD.atr || 0 
      }
      tickCountRef.current = initialData.length
      initializedRef.current = true
      lastSymbolRef.current = symbol
      // 初始化时立即设置快照
      setDataSnapshot([...initialData])
    }
  }, [symbol, basePrice])

  // 实时更新 - 币安风格，价格围绕基准价波动
  // 防闪烁优化: 数据实时更新到ref, 但只定期触发React重渲染
  useEffect(() => {
    if (!initializedRef.current) return

    // 数据更新定时器 - 高频更新ref但不触发render
    const interval = setInterval(() => {
      const data = dataRef.current
      if (data.length === 0) return
      
      const last = data[data.length - 1]
      tickCountRef.current += 0.05  // 慢速递增
      
      const t = tickCountRef.current
      
      // 多周期叠加 - 模拟真实市场波动
      const cycle1 = Math.sin(t * 0.15) * basePrice * 0.008  // 大周期趋势
      const cycle2 = Math.sin(t * 0.4) * basePrice * 0.003   // 中周期波动
      const cycle3 = Math.sin(t * 1.2) * basePrice * 0.001   // 小周期噪声
      
      // 价格围绕基准价波动
      const newClose = basePrice + cycle1 + cycle2 + cycle3
      
      // 更新最后一根K线
      const newHigh = Math.max(last.high, newClose)
      const newLow = Math.min(last.low, newClose)
      
      // 更新历史数据
      const len = priceHistoryRef.current.length
      priceHistoryRef.current[len - 1] = newClose
      highHistoryRef.current[len - 1] = newHigh
      lowHistoryRef.current[len - 1] = newLow
      
      // 重新计算所有指标
      const ind = calculateIndicators(
        priceHistoryRef.current, 
        highHistoryRef.current, 
        lowHistoryRef.current, 
        volumeHistoryRef.current, 
        basePrice, 
        prevIndRef.current
      )
      prevIndRef.current = { ema12: ind.ema12, ema26: ind.ema26, obv: ind.obv, atr: ind.atr }
      
      // 直接修改最后一条数据 (ref中, 不触发render)
      data[data.length - 1] = {
        ...last,
        high: newHigh,
        low: newLow,
        close: newClose,
        ma5: ind.ma5,
        ma10: ind.ma10,
        ma20: ind.ma20,
        ma60: ind.ma60,
        ema12: ind.ema12,
        ema26: ind.ema26,
        rsi: ind.rsi,
        upperBB: ind.ma20 + ind.std * 2,
        lowerBB: ind.ma20 - ind.std * 2,
        middleBB: ind.ma20,
        macd: ind.macd,
        signal: ind.signal,
        histogram: ind.macd - ind.signal,
        k: ind.k,
        d: ind.d,
        j: ind.j,
        vwap: ind.vwap,
        atr: ind.atr,
        obv: ind.obv,
        sar: ind.sar,
        cci: ind.cci,
        willR: ind.willR
      }
      
      // 每60秒新建一根K线
      if (Math.floor(t) % 60 === 0 && Math.floor(t) !== Math.floor(t - 0.05)) {
        data.shift()
        priceHistoryRef.current.shift()
        highHistoryRef.current.shift()
        lowHistoryRef.current.shift()
        volumeHistoryRef.current.shift()
        
        const newVolume = 800000 + Math.abs(Math.sin(t * 0.3)) * 600000
        const newCandle: CandleData = {
          time: new Date().toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' }),
          open: newClose,
          high: newClose,
          low: newClose,
          close: newClose,
          volume: newVolume,
          ma5: ind.ma5, ma10: ind.ma10, ma20: ind.ma20, ma60: ind.ma60,
          ema12: ind.ema12, ema26: ind.ema26,
          rsi: ind.rsi,
          upperBB: ind.ma20 + ind.std * 2,
          lowerBB: ind.ma20 - ind.std * 2,
          middleBB: ind.ma20,
          macd: ind.macd, signal: ind.signal,
          histogram: ind.macd - ind.signal,
          k: ind.k, d: ind.d, j: ind.j,
          vwap: ind.vwap, atr: ind.atr, obv: ind.obv,
          sar: ind.sar, cci: ind.cci, willR: ind.willR
        }
        data.push(newCandle)
        priceHistoryRef.current.push(newClose)
        highHistoryRef.current.push(newClose)
        lowHistoryRef.current.push(newClose)
        volumeHistoryRef.current.push(newVolume)
      }
      
      // 防闪烁: 不在这里触发render, 而是在snapshot定时器中
    }, 100)
    
    // 快照定时器 - 低频触发React重渲染
    const snapshotInterval = setInterval(() => {
      if (dataRef.current.length > 0) {
        // 创建数据快照触发render
        setDataSnapshot([...dataRef.current])
      }
    }, DATA_SNAPSHOT_INTERVAL)
    
    return () => {
      clearInterval(interval)
      clearInterval(snapshotInterval)
    }
  }, [basePrice])

  // 使用快照数据渲染图表 (而非实时ref)
  const data = dataSnapshot.length > 0 ? dataSnapshot : dataRef.current
  const currentPrice = data[data.length - 1]?.close || basePrice
  const priceChange = currentPrice - (data[0]?.open || basePrice)
  const isUp = priceChange >= 0

  // 专业K线图Tooltip - TradingView风格改为固定位置显示,此函数保留供其他图表类型使用
  const CandleTooltip = ({ active, label }: any) => {
    if (!active || !label) return null
    
    // 直接从数据数组中根据时间查找对应的K线，不依赖payload
    const d = data.find(item => item.time === label)
    if (!d || d.open === undefined || d.close === undefined) return null
    
    // 使用数据自身的值，完全不受鼠标位置影响
    const candleIsUp = d.close >= d.open
    const change = d.close - d.open
    const changePercent = d.open !== 0 ? (change / d.open * 100) : 0
    const amplitude = d.low !== 0 ? ((d.high - d.low) / d.low * 100) : 0
    
    return (
      <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[10px] shadow-xl min-w-[180px]">
        <div className="text-gray-400 border-b border-[#333] pb-1 mb-1">{label}</div>
        
        {/* OHLC */}
        <div className="grid grid-cols-2 gap-x-3 gap-y-0.5 mb-2">
          <div className="flex justify-between">
            <span className="text-gray-500">开盘:</span>
            <span className="text-white font-mono">{d.open?.toFixed(2)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-gray-500">收盘:</span>
            <span className={`font-mono ${candleIsUp ? 'text-green-400' : 'text-red-400'}`}>{d.close?.toFixed(2)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-gray-500">最高:</span>
            <span className="text-green-300 font-mono">{d.high?.toFixed(2)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-gray-500">最低:</span>
            <span className="text-red-300 font-mono">{d.low?.toFixed(2)}</span>
          </div>
        </div>
        
        {/* 涨跌幅 */}
        <div className="flex justify-between border-b border-[#222] pb-1 mb-1">
          <span className="text-gray-500">涨跌:</span>
          <span className={`font-mono ${candleIsUp ? 'text-green-400' : 'text-red-400'}`}>
            {candleIsUp ? '+' : ''}{change.toFixed(2)} ({candleIsUp ? '+' : ''}{changePercent.toFixed(2)}%)
          </span>
        </div>
        <div className="flex justify-between">
          <span className="text-gray-500">振幅:</span>
          <span className="text-yellow-400 font-mono">{amplitude.toFixed(2)}%</span>
        </div>
        
        {/* 成交量 */}
        <div className="flex justify-between border-t border-[#222] pt-1 mt-1">
          <span className="text-gray-500">成交量:</span>
          <span className="text-cyan-400 font-mono">{(d.volume / 1e6).toFixed(2)}M</span>
        </div>
        
        {/* 均线指标 */}
        <div className="border-t border-[#222] pt-1 mt-1 grid grid-cols-2 gap-x-3 gap-y-0.5">
          <div className="flex justify-between">
            <span className="text-[#ffaa00]">MA5:</span>
            <span className="text-gray-300 font-mono">{d.ma5?.toFixed(2)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-[#00aaff]">MA10:</span>
            <span className="text-gray-300 font-mono">{d.ma10?.toFixed(2)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-[#ff00ff]">MA20:</span>
            <span className="text-gray-300 font-mono">{d.ma20?.toFixed(2)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-gray-500">VWAP:</span>
            <span className="text-gray-300 font-mono">{d.vwap?.toFixed(2)}</span>
          </div>
        </div>
        
        {/* 技术指标 */}
        <div className="border-t border-[#222] pt-1 mt-1 grid grid-cols-2 gap-x-3 gap-y-0.5">
          <div className="flex justify-between">
            <span className="text-gray-500">RSI:</span>
            <span className={`font-mono ${(d.rsi || 50) > 70 ? 'text-red-400' : (d.rsi || 50) < 30 ? 'text-green-400' : 'text-gray-300'}`}>
              {d.rsi?.toFixed(1)}
            </span>
          </div>
          <div className="flex justify-between">
            <span className="text-gray-500">MACD:</span>
            <span className={`font-mono ${(d.macd || 0) >= 0 ? 'text-green-400' : 'text-red-400'}`}>
              {d.macd?.toFixed(2)}
            </span>
          </div>
          <div className="flex justify-between">
            <span className="text-gray-500">KDJ:</span>
            <span className="text-gray-300 font-mono">{d.k?.toFixed(0)}/{d.d?.toFixed(0)}/{d.j?.toFixed(0)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-gray-500">ATR:</span>
            <span className="text-gray-300 font-mono">{d.atr?.toFixed(2)}</span>
          </div>
        </div>
        
        {/* 布林带 */}
        <div className="border-t border-[#222] pt-1 mt-1">
          <div className="text-gray-500 text-[9px] mb-0.5">布林带 (20,2):</div>
          <div className="flex justify-between text-[9px]">
            <span className="text-red-300">上轨: {d.upperBB?.toFixed(2)}</span>
            <span className="text-gray-400">中轨: {d.middleBB?.toFixed(2)}</span>
            <span className="text-green-300">下轨: {d.lowerBB?.toFixed(2)}</span>
          </div>
        </div>
      </div>
    )
  }
  
  // 抑制未使用警告 - CandleTooltip保留供未来使用
  void CandleTooltip

  // K线图 - TradingView风格
  if (chartType === 'candle') {
    // 计算K线图的Y轴范围
    const allPrices = data.flatMap(d => [d.high, d.low, d.upperBB || d.high, d.lowerBB || d.low])
    const minY = Math.min(...allPrices) * 0.999
    const maxY = Math.max(...allPrices) * 1.001
    
    // TradingView风格: 显示悬停数据或最新数据
    const displayData = hoveredData || data[data.length - 1]
    const displayIsUp = displayData ? (displayData.close ?? 0) >= (displayData.open ?? 0) : isUp
    
    // 鼠标移动处理 - 防抖更新悬停数据
    const handleChartMouseMove = (e: any) => {
      if (e?.activePayload?.[0]?.payload) {
        const payload = e.activePayload[0].payload as CandleData
        if (hoverDebounceRef.current) clearTimeout(hoverDebounceRef.current)
        hoverDebounceRef.current = setTimeout(() => setHoveredData(payload), 16)
      }
    }
    
    const handleChartMouseLeave = () => {
      if (hoverDebounceRef.current) clearTimeout(hoverDebounceRef.current)
      setHoveredData(null)
    }
    
    return (
      <div className="h-full w-full relative">
        {/* TradingView风格: 固定位置信息面板 */}
        {displayData && (
          <div className="absolute top-1 left-1 z-20 pointer-events-none">
            <div className="flex items-center gap-2 text-[10px] font-mono bg-[#0a0a0a]/80 px-1 rounded">
              <span className="text-gray-500">{displayData.time}</span>
              <span className="text-gray-400">O</span>
              <span className={displayIsUp ? 'text-green-400' : 'text-red-400'}>{displayData.open?.toFixed(2)}</span>
              <span className="text-gray-400">H</span>
              <span className={displayIsUp ? 'text-green-400' : 'text-red-400'}>{displayData.high?.toFixed(2)}</span>
              <span className="text-gray-400">L</span>
              <span className={displayIsUp ? 'text-green-400' : 'text-red-400'}>{displayData.low?.toFixed(2)}</span>
              <span className="text-gray-400">C</span>
              <span className={displayIsUp ? 'text-green-400' : 'text-red-400'}>{displayData.close?.toFixed(2)}</span>
            </div>
            <div className="flex items-center gap-2 text-[9px] font-mono mt-0.5">
              {indicators.ma5 && <span className="text-[#ffaa00]">MA5:{displayData.ma5?.toFixed(1)}</span>}
              {indicators.ma10 && <span className="text-[#00aaff]">MA10:{displayData.ma10?.toFixed(1)}</span>}
            </div>
          </div>
        )}
        
        <ResponsiveContainer width="100%" height="70%">
          <ComposedChart 
            data={data} 
            margin={{ top: 18, right: 5, bottom: 5, left: 5 }}
            onMouseMove={handleChartMouseMove}
            onMouseLeave={handleChartMouseLeave}
          >
            <XAxis dataKey="time" tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} interval="preserveEnd" />
            <YAxis domain={[minY, maxY]} tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} orientation="right" tickFormatter={(v) => v?.toFixed(0)} />
            {/* TradingView风格: 只显示十字线,不显示浮动Tooltip */}
            <Tooltip 
              content={() => null}
              cursor={{ stroke: '#555', strokeWidth: 1, strokeDasharray: '3 3' }}
              isAnimationActive={false}
            />
            
            {/* 布林带填充区域 */}
            {indicators.bb && (
              <Area type="monotone" dataKey="upperBB" stroke="transparent" fill="#88888810" isAnimationActive={false} />
            )}
            
            {/* 触发区域 */}
            <Area type="monotone" dataKey="close" stroke="transparent" fill="transparent" isAnimationActive={false} />
            
            {/* K线实体 */}
            <Bar dataKey="close" fill="transparent" isAnimationActive={false} barSize={6}>
              {data.map((entry, index) => {
                const candleUp = (entry.close ?? 0) >= (entry.open ?? 0)
                return (
                  <Cell
                    key={`candle-${index}`}
                    fill={candleUp ? '#00ff88' : '#ff4444'}
                    stroke={candleUp ? '#00ff88' : '#ff4444'}
                  />
                )
              })}
            </Bar>
            
            {/* 移动平均线 */}
            {indicators.ma5 && <Line type="monotone" dataKey="ma5" stroke="#ffaa00" dot={false} strokeWidth={1} name="MA5" isAnimationActive={false} />}
            {indicators.ma10 && <Line type="monotone" dataKey="ma10" stroke="#00aaff" dot={false} strokeWidth={1} name="MA10" isAnimationActive={false} />}
            {indicators.ma20 && <Line type="monotone" dataKey="ma20" stroke="#ff00ff" dot={false} strokeWidth={1} name="MA20" isAnimationActive={false} />}
            
            {/* 布林带 */}
            {indicators.bb && (
              <>
                <Line type="monotone" dataKey="upperBB" stroke="#888" dot={false} strokeWidth={0.5} strokeDasharray="3 3" name="BB上轨" isAnimationActive={false} />
                <Line type="monotone" dataKey="middleBB" stroke="#666" dot={false} strokeWidth={0.5} name="BB中轨" isAnimationActive={false} />
                <Line type="monotone" dataKey="lowerBB" stroke="#888" dot={false} strokeWidth={0.5} strokeDasharray="3 3" name="BB下轨" isAnimationActive={false} />
              </>
            )}
          </ComposedChart>
        </ResponsiveContainer>
        
        {/* 成交量 */}
        {indicators.volume && (
          <ResponsiveContainer width="100%" height="30%">
            <BarChart data={data} margin={{ top: 0, right: 5, bottom: 5, left: 5 }}>
              <XAxis dataKey="time" tick={false} axisLine={false} tickLine={false} />
              <YAxis tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} orientation="right" tickFormatter={(v) => `${(v/1e6).toFixed(0)}M`} />
              <Tooltip content={() => null} cursor={{ stroke: '#555', strokeDasharray: '3 3' }} isAnimationActive={false} />
              <Bar dataKey="volume" isAnimationActive={false}>
                {data.map((entry, index) => (
                  <Cell key={`vol-${index}`} fill={(entry.close ?? 0) >= (entry.open ?? 0) ? '#00ff8860' : '#ff444460'} />
                ))}
              </Bar>
            </BarChart>
          </ResponsiveContainer>
        )}
      </div>
    )
  }

  // MACD 图
  if (chartType === 'macd') {
    return (
      <ResponsiveContainer width="100%" height={height}>
        <ComposedChart data={data} margin={{ top: 5, right: 5, bottom: 5, left: 5 }}>
          <XAxis dataKey="time" tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} />
          <YAxis domain={['auto', 'auto']} tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} orientation="right" />
          <Tooltip 
            content={({ active, payload, label }) => {
              if (!active || !payload || !payload.length) return null
              const d = payload[0]?.payload as CandleData
              if (!d) return null
              
              return (
                <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[10px] shadow-xl min-w-[150px]">
                  <div className="text-gray-400 border-b border-[#333] pb-1 mb-1">{label}</div>
                  <div className="space-y-0.5">
                    <div className="flex justify-between">
                      <span className="text-[#00aaff]">MACD (12,26):</span>
                      <span className={`font-mono ${(d.macd || 0) >= 0 ? 'text-green-400' : 'text-red-400'}`}>
                        {d.macd?.toFixed(4)}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-[#ffaa00]">Signal (9):</span>
                      <span className="text-gray-300 font-mono">{d.signal?.toFixed(4)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">Histogram:</span>
                      <span className={`font-mono ${(d.histogram || 0) >= 0 ? 'text-green-400' : 'text-red-400'}`}>
                        {d.histogram?.toFixed(4)}
                      </span>
                    </div>
                    <div className="border-t border-[#222] pt-1 mt-1 flex justify-between">
                      <span className="text-gray-500">价格:</span>
                      <span className="text-white font-mono">{d.close?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">EMA12:</span>
                      <span className="text-gray-300 font-mono">{d.ema12?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">EMA26:</span>
                      <span className="text-gray-300 font-mono">{d.ema26?.toFixed(2)}</span>
                    </div>
                  </div>
                </div>
              )
            }}
          />
          <ReferenceLine y={0} stroke="#333" />
          <Bar dataKey="histogram" isAnimationActive={false} name="柱状图">
            {data.map((entry, index) => (
              <Cell key={`hist-${index}`} fill={(entry.histogram ?? 0) >= 0 ? '#00ff88' : '#ff4444'} />
            ))}
          </Bar>
          <Line type="monotone" dataKey="macd" stroke="#00aaff" dot={false} strokeWidth={1.5} name="MACD" isAnimationActive={false} />
          <Line type="monotone" dataKey="signal" stroke="#ffaa00" dot={false} strokeWidth={1.5} name="Signal" isAnimationActive={false} />
        </ComposedChart>
      </ResponsiveContainer>
    )
  }

  // RSI 图
  if (chartType === 'rsi') {
    return (
      <ResponsiveContainer width="100%" height={height}>
        <AreaChart data={data} margin={{ top: 5, right: 5, bottom: 5, left: 5 }}>
          <defs>
            <linearGradient id="rsiGradient" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor="#00aaff" stopOpacity={0.3}/>
              <stop offset="95%" stopColor="#00aaff" stopOpacity={0}/>
            </linearGradient>
          </defs>
          <XAxis dataKey="time" tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} />
          <YAxis domain={[0, 100]} tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} orientation="right" />
          <Tooltip 
            content={({ active, payload, label }) => {
              if (!active || !payload || !payload.length) return null
              const d = payload[0]?.payload as CandleData
              if (!d) return null
              
              const rsiStatus = (d.rsi || 50) > 70 ? '超买' : (d.rsi || 50) < 30 ? '超卖' : '中性'
              const rsiColor = (d.rsi || 50) > 70 ? 'text-red-400' : (d.rsi || 50) < 30 ? 'text-green-400' : 'text-gray-300'
              
              return (
                <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[10px] shadow-xl min-w-[140px]">
                  <div className="text-gray-400 border-b border-[#333] pb-1 mb-1">{label}</div>
                  <div className="space-y-0.5">
                    <div className="flex justify-between">
                      <span className="text-[#00aaff]">RSI (14):</span>
                      <span className={`font-mono font-bold ${rsiColor}`}>{d.rsi?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">状态:</span>
                      <span className={`${rsiColor}`}>{rsiStatus}</span>
                    </div>
                    <div className="border-t border-[#222] pt-1 mt-1 flex justify-between">
                      <span className="text-gray-500">价格:</span>
                      <span className="text-white font-mono">{d.close?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">Williams %R:</span>
                      <span className="text-gray-300 font-mono">{d.willR?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">CCI:</span>
                      <span className="text-gray-300 font-mono">{d.cci?.toFixed(2)}</span>
                    </div>
                  </div>
                </div>
              )
            }}
          />
          <ReferenceLine y={70} stroke="#ff4444" strokeDasharray="3 3" label={{ value: '超买 70', fill: '#ff4444', fontSize: 9, position: 'right' }} />
          <ReferenceLine y={30} stroke="#00ff88" strokeDasharray="3 3" label={{ value: '超卖 30', fill: '#00ff88', fontSize: 9, position: 'right' }} />
          <ReferenceLine y={50} stroke="#555" />
          <Area type="monotone" dataKey="rsi" stroke="#00aaff" fill="url(#rsiGradient)" strokeWidth={1.5} name="RSI" />
        </AreaChart>
      </ResponsiveContainer>
    )
  }

  // KDJ随机指标图
  if (chartType === 'kdj') {
    return (
      <ResponsiveContainer width="100%" height={height}>
        <ComposedChart data={data} margin={{ top: 5, right: 5, bottom: 5, left: 5 }}>
          <XAxis dataKey="time" tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} />
          <YAxis domain={[0, 100]} tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} orientation="right" />
          <Tooltip 
            content={({ active, payload, label }) => {
              if (!active || !payload || !payload.length) return null
              const d = payload[0]?.payload as CandleData
              if (!d) return null
              
              const kdjSignal = (d.k || 50) > (d.d || 50) ? '金叉看涨' : '死叉看跌'
              const kdjColor = (d.k || 50) > (d.d || 50) ? 'text-green-400' : 'text-red-400'
              
              return (
                <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[10px] shadow-xl min-w-[150px]">
                  <div className="text-gray-400 border-b border-[#333] pb-1 mb-1">{label}</div>
                  <div className="space-y-0.5">
                    <div className="flex justify-between">
                      <span className="text-[#ffaa00]">K值 (9,3,3):</span>
                      <span className="text-[#ffaa00] font-mono font-bold">{d.k?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-[#00aaff]">D值:</span>
                      <span className="text-[#00aaff] font-mono">{d.d?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-[#ff00ff]">J值:</span>
                      <span className="text-[#ff00ff] font-mono">{d.j?.toFixed(2)}</span>
                    </div>
                    <div className="border-t border-[#222] pt-1 mt-1 flex justify-between">
                      <span className="text-gray-500">信号:</span>
                      <span className={kdjColor}>{kdjSignal}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">价格:</span>
                      <span className="text-white font-mono">{d.close?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">RSI:</span>
                      <span className="text-gray-300 font-mono">{d.rsi?.toFixed(1)}</span>
                    </div>
                  </div>
                </div>
              )
            }}
          />
          <ReferenceLine y={80} stroke="#ff4444" strokeDasharray="3 3" label={{ value: '超买', fill: '#ff4444', fontSize: 9, position: 'right' }} />
          <ReferenceLine y={20} stroke="#00ff88" strokeDasharray="3 3" label={{ value: '超卖', fill: '#00ff88', fontSize: 9, position: 'right' }} />
          <ReferenceLine y={50} stroke="#333" />
          <Line type="monotone" dataKey="k" stroke="#ffaa00" dot={false} strokeWidth={1.5} name="K" isAnimationActive={false} />
          <Line type="monotone" dataKey="d" stroke="#00aaff" dot={false} strokeWidth={1.5} name="D" isAnimationActive={false} />
          <Line type="monotone" dataKey="j" stroke="#ff00ff" dot={false} strokeWidth={1} name="J" isAnimationActive={false} />
        </ComposedChart>
      </ResponsiveContainer>
    )
  }

  // 综合指标图 - 显示多个指标
  if (chartType === 'indicators') {
    return (
      <div className="h-full w-full flex flex-col gap-1">
        {/* 价格 + MA + BB */}
        <ResponsiveContainer width="100%" height="40%">
          <ComposedChart data={data} margin={{ top: 5, right: 5, bottom: 0, left: 5 }}>
            <XAxis dataKey="time" tick={false} axisLine={false} tickLine={false} />
            <YAxis domain={['auto', 'auto']} tick={{ fill: '#555', fontSize: 7 }} axisLine={false} tickLine={false} orientation="right" />
            <Tooltip 
              content={({ active, payload, label }) => {
                if (!active || !payload || !payload.length) return null
                const d = payload[0]?.payload as CandleData
                if (!d) return null
                
                return (
                  <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[9px] shadow-xl min-w-[160px]">
                    <div className="text-gray-400 border-b border-[#333] pb-1 mb-1">{label}</div>
                    <div className="grid grid-cols-2 gap-x-2 gap-y-0.5">
                      <div className="flex justify-between col-span-2">
                        <span className="text-gray-500">价格:</span>
                        <span className={`font-mono ${d.close >= d.open ? 'text-green-400' : 'text-red-400'}`}>{d.close?.toFixed(2)}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-[#ffaa00]">MA5:</span>
                        <span className="text-gray-300 font-mono">{d.ma5?.toFixed(2)}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-[#00aaff]">MA20:</span>
                        <span className="text-gray-300 font-mono">{d.ma20?.toFixed(2)}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-[#ff00ff]">VWAP:</span>
                        <span className="text-gray-300 font-mono">{d.vwap?.toFixed(2)}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-gray-500">ATR:</span>
                        <span className="text-gray-300 font-mono">{d.atr?.toFixed(2)}</span>
                      </div>
                      <div className="flex justify-between col-span-2 border-t border-[#222] pt-1 mt-1">
                        <span className="text-gray-500">布林带:</span>
                        <span className="text-gray-300 font-mono text-[8px]">{d.lowerBB?.toFixed(0)} - {d.middleBB?.toFixed(0)} - {d.upperBB?.toFixed(0)}</span>
                      </div>
                    </div>
                  </div>
                )
              }}
            />
            <Area type="monotone" dataKey="close" stroke={isUp ? '#00ff88' : '#ff4444'} fill={isUp ? '#00ff8815' : '#ff444415'} strokeWidth={1} />
            <Line type="monotone" dataKey="ma5" stroke="#ffaa00" dot={false} strokeWidth={0.8} name="MA5" isAnimationActive={false} />
            <Line type="monotone" dataKey="ma20" stroke="#00aaff" dot={false} strokeWidth={0.8} name="MA20" isAnimationActive={false} />
            <Line type="monotone" dataKey="upperBB" stroke="#666" dot={false} strokeWidth={0.5} strokeDasharray="2 2" />
            <Line type="monotone" dataKey="lowerBB" stroke="#666" dot={false} strokeWidth={0.5} strokeDasharray="2 2" />
            <Line type="monotone" dataKey="vwap" stroke="#ff00ff" dot={false} strokeWidth={0.8} name="VWAP" isAnimationActive={false} />
          </ComposedChart>
        </ResponsiveContainer>
        
        {/* RSI + Williams %R */}
        <ResponsiveContainer width="100%" height="20%">
          <ComposedChart data={data} margin={{ top: 0, right: 5, bottom: 0, left: 5 }}>
            <XAxis dataKey="time" tick={false} axisLine={false} tickLine={false} />
            <YAxis domain={[0, 100]} tick={{ fill: '#555', fontSize: 7 }} axisLine={false} tickLine={false} orientation="right" />
            <Tooltip 
              content={({ active, payload, label }) => {
                if (!active || !payload || !payload.length) return null
                const d = payload[0]?.payload as CandleData
                if (!d) return null
                
                return (
                  <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[9px] shadow-xl">
                    <div className="text-gray-400">{label}</div>
                    <div className="flex justify-between gap-4">
                      <span className={`font-mono ${(d.rsi || 50) > 70 ? 'text-red-400' : (d.rsi || 50) < 30 ? 'text-green-400' : 'text-[#00aaff]'}`}>
                        RSI: {d.rsi?.toFixed(1)}
                      </span>
                      <span className="text-gray-400 font-mono">W%R: {d.willR?.toFixed(1)}</span>
                    </div>
                  </div>
                )
              }}
            />
            <ReferenceLine y={70} stroke="#ff444466" strokeDasharray="2 2" />
            <ReferenceLine y={30} stroke="#00ff8866" strokeDasharray="2 2" />
            <Line type="monotone" dataKey="rsi" stroke="#00aaff" dot={false} strokeWidth={1} name="RSI" isAnimationActive={false} />
          </ComposedChart>
        </ResponsiveContainer>
        
        {/* MACD */}
        <ResponsiveContainer width="100%" height="20%">
          <ComposedChart data={data} margin={{ top: 0, right: 5, bottom: 0, left: 5 }}>
            <XAxis dataKey="time" tick={false} axisLine={false} tickLine={false} />
            <YAxis tick={{ fill: '#555', fontSize: 7 }} axisLine={false} tickLine={false} orientation="right" />
            <Tooltip 
              content={({ active, payload, label }) => {
                if (!active || !payload || !payload.length) return null
                const d = payload[0]?.payload as CandleData
                if (!d) return null
                
                return (
                  <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[9px] shadow-xl">
                    <div className="text-gray-400">{label}</div>
                    <div className="space-y-0.5">
                      <div className="flex justify-between gap-2">
                        <span className="text-[#00aaff]">MACD:</span>
                        <span className={`font-mono ${(d.macd || 0) >= 0 ? 'text-green-400' : 'text-red-400'}`}>{d.macd?.toFixed(3)}</span>
                      </div>
                      <div className="flex justify-between gap-2">
                        <span className="text-[#ffaa00]">Signal:</span>
                        <span className="text-gray-300 font-mono">{d.signal?.toFixed(3)}</span>
                      </div>
                    </div>
                  </div>
                )
              }}
            />
            <ReferenceLine y={0} stroke="#333" />
            <Bar dataKey="histogram" isAnimationActive={false}>
              {data.map((entry, index) => (
                <Cell key={`hist-${index}`} fill={(entry.histogram ?? 0) >= 0 ? '#00ff8866' : '#ff444466'} />
              ))}
            </Bar>
            <Line type="monotone" dataKey="macd" stroke="#00aaff" dot={false} strokeWidth={0.8} />
            <Line type="monotone" dataKey="signal" stroke="#ffaa00" dot={false} strokeWidth={0.8} />
          </ComposedChart>
        </ResponsiveContainer>
        
        {/* 成交量 */}
        <ResponsiveContainer width="100%" height="20%">
          <BarChart data={data} margin={{ top: 0, right: 5, bottom: 5, left: 5 }}>
            <XAxis dataKey="time" tick={{ fill: '#555', fontSize: 7 }} axisLine={false} tickLine={false} interval="preserveEnd" />
            <YAxis tick={{ fill: '#555', fontSize: 7 }} axisLine={false} tickLine={false} orientation="right" tickFormatter={(v) => `${(v / 1000000).toFixed(0)}M`} />
            <Tooltip 
              content={({ active, payload, label }) => {
                if (!active || !payload || !payload.length) return null
                const d = payload[0]?.payload as CandleData
                if (!d) return null
                
                return (
                  <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[9px] shadow-xl">
                    <div className="text-gray-400">{label}</div>
                    <div className="flex justify-between gap-2">
                      <span className="text-cyan-400">Vol:</span>
                      <span className="text-white font-mono">{(d.volume / 1e6).toFixed(2)}M</span>
                    </div>
                    <div className="flex justify-between gap-2">
                      <span className="text-gray-500">OBV:</span>
                      <span className="text-gray-300 font-mono">{((d.obv || 0) / 1e6).toFixed(2)}M</span>
                    </div>
                  </div>
                )
              }}
            />
            <Bar dataKey="volume" isAnimationActive={false}>
              {data.map((entry, index) => (
                <Cell key={`vol-${index}`} fill={(entry.close ?? 0) >= (entry.open ?? 0) ? '#00ff8840' : '#ff444440'} />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </div>
    )
  }

  // 生成深度图数据（移到条件外部避免hooks规则问题）
  const generateDepthData = () => {
    const bids: { price: number; cumulative: number; type: string }[] = []
    const asks: { price: number; cumulative: number; type: string }[] = []
    let bidTotal = 0
    let askTotal = 0
    
    for (let i = 19; i >= 0; i--) {
      bidTotal += 30 + Math.sin(i * 0.5) * 20 + 10
      bids.push({ 
        price: basePrice * (1 - (i + 1) * 0.002), 
        cumulative: bidTotal,
        type: 'bid'
      })
    }
    
    for (let i = 0; i < 20; i++) {
      askTotal += 30 + Math.sin(i * 0.5) * 20 + 10
      asks.push({ 
        price: basePrice * (1 + (i + 1) * 0.002), 
        cumulative: askTotal,
        type: 'ask'
      })
    }
    
    return [...bids, ...asks]
  }

  // 深度图 (修复版)
  if (chartType === 'depth') {
    const depthData = generateDepthData()
    
    return (
      <ResponsiveContainer width="100%" height={height}>
        <AreaChart data={depthData} margin={{ top: 5, right: 5, bottom: 5, left: 5 }}>
          <defs>
            <linearGradient id="depthBidGradient" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor="#00ff88" stopOpacity={0.6}/>
              <stop offset="95%" stopColor="#00ff88" stopOpacity={0.1}/>
            </linearGradient>
            <linearGradient id="depthAskGradient" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor="#ff4444" stopOpacity={0.6}/>
              <stop offset="95%" stopColor="#ff4444" stopOpacity={0.1}/>
            </linearGradient>
          </defs>
          <XAxis 
            dataKey="price" 
            tick={{ fill: '#555', fontSize: 8 }} 
            axisLine={false} 
            tickLine={false} 
            tickFormatter={(v) => v?.toFixed(0)}
            domain={['dataMin', 'dataMax']}
          />
          <YAxis tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} orientation="right" />
          <Tooltip 
            content={({ active, payload }) => {
              if (!active || !payload || !payload.length) return null
              const d = payload[0]?.payload as { price: number; cumulative: number; type: string }
              if (!d) return null
              
              const isBid = d.price < basePrice
              
              return (
                <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[10px] shadow-xl min-w-[130px]">
                  <div className={`font-bold ${isBid ? 'text-green-400' : 'text-red-400'}`}>
                    {isBid ? '买盘深度' : '卖盘深度'}
                  </div>
                  <div className="space-y-0.5 mt-1">
                    <div className="flex justify-between">
                      <span className="text-gray-500">价格:</span>
                      <span className="text-white font-mono">{d.price?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">累计量:</span>
                      <span className={`font-mono ${isBid ? 'text-green-400' : 'text-red-400'}`}>
                        {d.cumulative?.toFixed(2)}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">距中价:</span>
                      <span className="text-yellow-400 font-mono">
                        {((d.price - basePrice) / basePrice * 100).toFixed(2)}%
                      </span>
                    </div>
                  </div>
                </div>
              )
            }}
          />
          <ReferenceLine x={basePrice} stroke="#ffffff" strokeWidth={2} strokeDasharray="3 3" label={{ value: '当前价', fill: '#fff', fontSize: 9 }} />
          <Area 
            type="stepAfter" 
            dataKey="cumulative" 
            stroke="#00ff88"
            fill="url(#depthBidGradient)"
            strokeWidth={1}
          />
        </AreaChart>
      </ResponsiveContainer>
    )
  }

  // 1. 斐波那契回撤线
  if (chartType === 'fibonacci') {
    const prices = data.map(d => d.close)
    const highPrice = Math.max(...prices)
    const lowPrice = Math.min(...prices)
    const diff = highPrice - lowPrice
    
    // 斐波那契水平
    const fibLevels = [
      { level: 0, price: highPrice, label: '0%', color: '#ff4444' },
      { level: 0.236, price: highPrice - diff * 0.236, label: '23.6%', color: '#ff8800' },
      { level: 0.382, price: highPrice - diff * 0.382, label: '38.2%', color: '#ffaa00' },
      { level: 0.5, price: highPrice - diff * 0.5, label: '50%', color: '#ffff00' },
      { level: 0.618, price: highPrice - diff * 0.618, label: '61.8%', color: '#00ff88' },
      { level: 0.786, price: highPrice - diff * 0.786, label: '78.6%', color: '#00aaff' },
      { level: 1, price: lowPrice, label: '100%', color: '#0066ff' },
    ]
    
    return (
      <ResponsiveContainer width="100%" height={height}>
        <ComposedChart data={data} margin={{ top: 5, right: 60, bottom: 5, left: 5 }}>
          <defs>
            <linearGradient id="fibGradient" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#ff4444" stopOpacity={0.1}/>
              <stop offset="50%" stopColor="#ffff00" stopOpacity={0.05}/>
              <stop offset="100%" stopColor="#0066ff" stopOpacity={0.1}/>
            </linearGradient>
          </defs>
          <XAxis dataKey="time" tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} />
          <YAxis domain={[lowPrice * 0.998, highPrice * 1.002]} tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} orientation="right" />
          <Tooltip 
            content={({ active, payload, label }) => {
              if (!active || !payload || !payload.length) return null
              const d = payload[0]?.payload as CandleData
              if (!d) return null
              
              // 找到最近的斐波那契水平
              const currentPrice = d.close
              let nearestFib = fibLevels[0]
              let minDist = Math.abs(currentPrice - fibLevels[0].price)
              fibLevels.forEach(fib => {
                const dist = Math.abs(currentPrice - fib.price)
                if (dist < minDist) {
                  minDist = dist
                  nearestFib = fib
                }
              })
              
              // 计算回撤比例
              const retracement = highPrice !== lowPrice ? ((highPrice - currentPrice) / diff * 100) : 0
              
              return (
                <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[10px] shadow-xl min-w-[160px]">
                  <div className="text-gray-400 border-b border-[#333] pb-1 mb-1">{label}</div>
                  <div className="space-y-0.5">
                    <div className="flex justify-between">
                      <span className="text-gray-500">价格:</span>
                      <span className={`font-mono ${d.close >= d.open ? 'text-green-400' : 'text-red-400'}`}>{d.close?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">回撤位置:</span>
                      <span className="text-yellow-400 font-mono">{retracement.toFixed(1)}%</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">最近支撑/阻力:</span>
                      <span style={{ color: nearestFib.color }} className="font-mono">{nearestFib.label}</span>
                    </div>
                    <div className="border-t border-[#222] pt-1 mt-1">
                      <div className="text-gray-500 text-[9px]">斐波那契区间:</div>
                      <div className="flex justify-between text-[9px]">
                        <span className="text-red-400">高: {highPrice.toFixed(2)}</span>
                        <span className="text-blue-400">低: {lowPrice.toFixed(2)}</span>
                      </div>
                    </div>
                  </div>
                </div>
              )
            }}
          />
          
          {/* 斐波那契区域填充 */}
          <Area type="monotone" dataKey="close" stroke="transparent" fill="url(#fibGradient)" />
          
          {/* 斐波那契水平线 */}
          {fibLevels.map((fib, i) => (
            <ReferenceLine 
              key={i} 
              y={fib.price} 
              stroke={fib.color} 
              strokeWidth={1} 
              strokeDasharray="5 3"
              label={{ value: `${fib.label} (${fib.price.toFixed(2)})`, fill: fib.color, fontSize: 9, position: 'right' }}
            />
          ))}
          
          {/* 价格线 */}
          <Line type="monotone" dataKey="close" stroke={isUp ? '#00ff88' : '#ff4444'} dot={false} strokeWidth={1.5} />
        </ComposedChart>
      </ResponsiveContainer>
    )
  }

  // 2. 成交量分布图 (Volume Profile)
  if (chartType === 'profile') {
    // 计算各价格区间的成交量
    const prices = data.map(d => d.close)
    const volumes = data.map(d => d.volume)
    const minPrice = Math.min(...prices)
    const maxPrice = Math.max(...prices)
    const priceRange = maxPrice - minPrice
    const bucketCount = 20
    const bucketSize = priceRange / bucketCount
    
    const volumeProfile: { price: number; buyVolume: number; sellVolume: number; total: number }[] = []
    
    for (let i = 0; i < bucketCount; i++) {
      const bucketLow = minPrice + i * bucketSize
      const bucketHigh = bucketLow + bucketSize
      const bucketMidPrice = (bucketLow + bucketHigh) / 2
      
      let buyVol = 0, sellVol = 0
      data.forEach((d, idx) => {
        if (d.close >= bucketLow && d.close < bucketHigh) {
          if (d.close >= d.open) {
            buyVol += volumes[idx] || 0
          } else {
            sellVol += volumes[idx] || 0
          }
        }
      })
      
      volumeProfile.push({
        price: bucketMidPrice,
        buyVolume: buyVol / 1000000,
        sellVolume: -sellVol / 1000000,
        total: (buyVol + sellVol) / 1000000
      })
    }
    
    // 找到POC (Point of Control - 最大成交量价格)
    const poc = volumeProfile.reduce((max, p) => p.total > max.total ? p : max, volumeProfile[0])
    
    return (
      <div className="h-full w-full flex">
        {/* 主价格图 */}
        <ResponsiveContainer width="70%" height="100%">
          <ComposedChart data={data} margin={{ top: 5, right: 0, bottom: 5, left: 5 }}>
            <XAxis dataKey="time" tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} />
            <YAxis domain={['auto', 'auto']} tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} orientation="right" />
            <Tooltip 
              content={({ active, payload, label }) => {
                if (!active || !payload || !payload.length) return null
                const d = payload[0]?.payload as CandleData
                if (!d) return null
                
                // 找到当前价格所在的成交量区间
                const bucket = volumeProfile.find(p => Math.abs(p.price - d.close) < bucketSize)
                
                return (
                  <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[10px] shadow-xl min-w-[150px]">
                    <div className="text-gray-400 border-b border-[#333] pb-1 mb-1">{label}</div>
                    <div className="space-y-0.5">
                      <div className="flex justify-between">
                        <span className="text-gray-500">价格:</span>
                        <span className={`font-mono ${d.close >= d.open ? 'text-green-400' : 'text-red-400'}`}>{d.close?.toFixed(2)}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-gray-500">成交量:</span>
                        <span className="text-cyan-400 font-mono">{(d.volume / 1e6).toFixed(2)}M</span>
                      </div>
                      {bucket && (
                        <>
                          <div className="border-t border-[#222] pt-1 mt-1">
                            <div className="text-gray-500 text-[9px]">该价位区间成交:</div>
                          </div>
                          <div className="flex justify-between">
                            <span className="text-green-400">买入:</span>
                            <span className="text-green-400 font-mono">{bucket.buyVolume.toFixed(2)}M</span>
                          </div>
                          <div className="flex justify-between">
                            <span className="text-red-400">卖出:</span>
                            <span className="text-red-400 font-mono">{Math.abs(bucket.sellVolume).toFixed(2)}M</span>
                          </div>
                        </>
                      )}
                      <div className="flex justify-between border-t border-[#222] pt-1">
                        <span className="text-[#ffaa00]">POC价格:</span>
                        <span className="text-[#ffaa00] font-mono">{poc.price.toFixed(2)}</span>
                      </div>
                    </div>
                  </div>
                )
              }}
            />
            <ReferenceLine y={poc.price} stroke="#ffaa00" strokeWidth={2} strokeDasharray="5 5" label={{ value: 'POC', fill: '#ffaa00', fontSize: 10 }} />
            <Area type="monotone" dataKey="close" stroke={isUp ? '#00ff88' : '#ff4444'} fill={isUp ? '#00ff8820' : '#ff444420'} strokeWidth={1.5} />
          </ComposedChart>
        </ResponsiveContainer>
        
        {/* 成交量分布 (水平柱状图) */}
        <ResponsiveContainer width="30%" height="100%">
          <BarChart data={volumeProfile} layout="vertical" margin={{ top: 5, right: 5, bottom: 5, left: 0 }}>
            <XAxis type="number" tick={{ fill: '#555', fontSize: 7 }} axisLine={false} tickLine={false} />
            <YAxis type="category" dataKey="price" tick={{ fill: '#555', fontSize: 7 }} axisLine={false} tickLine={false} tickFormatter={(v) => v?.toFixed(0)} />
            <Tooltip 
              content={({ active, payload }) => {
                if (!active || !payload || !payload.length) return null
                const d = payload[0]?.payload as { price: number; buyVolume: number; sellVolume: number; total: number }
                if (!d) return null
                
                return (
                  <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[9px] shadow-xl">
                    <div className="text-gray-400">价格区间: {d.price.toFixed(2)}</div>
                    <div className="flex justify-between gap-3">
                      <span className="text-green-400">买: {d.buyVolume.toFixed(2)}M</span>
                      <span className="text-red-400">卖: {Math.abs(d.sellVolume).toFixed(2)}M</span>
                    </div>
                  </div>
                )
              }}
            />
            <Bar dataKey="buyVolume" fill="#00ff88" stackId="stack" />
            <Bar dataKey="sellVolume" fill="#ff4444" stackId="stack" />
          </BarChart>
        </ResponsiveContainer>
      </div>
    )
  }

  // 3. 资金流向指标
  if (chartType === 'flow') {
    // 计算资金流向
    const flowData = data.map((d, i) => {
      const typicalPrice = (d.high + d.low + d.close) / 3
      const rawMF = typicalPrice * d.volume
      const isPositive = i > 0 ? d.close > data[i-1].close : true
      
      return {
        ...d,
        moneyFlow: isPositive ? rawMF / 1e9 : -rawMF / 1e9,
        buyPressure: d.close > d.open ? (d.close - d.open) / d.open * 100 : 0,
        sellPressure: d.close < d.open ? (d.open - d.close) / d.open * 100 : 0,
      }
    })
    
    // 计算累积资金流
    let cumFlow = 0
    const cumFlowData = flowData.map(d => {
      cumFlow += d.moneyFlow
      return { ...d, cumulativeFlow: cumFlow }
    })
    
    // 计算MFI (资金流量指数)
    const totalPositive = flowData.filter(d => d.moneyFlow > 0).reduce((sum, d) => sum + d.moneyFlow, 0)
    const totalNegative = Math.abs(flowData.filter(d => d.moneyFlow < 0).reduce((sum, d) => sum + d.moneyFlow, 0))
    const mfiValue = totalNegative > 0 ? 100 - (100 / (1 + totalPositive / totalNegative)) : 100
    
    return (
      <div className="h-full w-full flex flex-col">
        {/* 价格图 */}
        <ResponsiveContainer width="100%" height="40%">
          <AreaChart data={data} margin={{ top: 5, right: 5, bottom: 0, left: 5 }}>
            <XAxis dataKey="time" tick={false} axisLine={false} tickLine={false} />
            <YAxis domain={['auto', 'auto']} tick={{ fill: '#555', fontSize: 7 }} axisLine={false} tickLine={false} orientation="right" />
            <Tooltip 
              content={({ active, payload, label }) => {
                if (!active || !payload || !payload.length) return null
                const d = payload[0]?.payload as CandleData
                if (!d) return null
                
                return (
                  <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[9px] shadow-xl">
                    <div className="text-gray-400">{label}</div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">价格:</span>
                      <span className={`font-mono ${d.close >= d.open ? 'text-green-400' : 'text-red-400'}`}>{d.close?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">MFI:</span>
                      <span className={`font-mono ${mfiValue > 80 ? 'text-red-400' : mfiValue < 20 ? 'text-green-400' : 'text-gray-300'}`}>{mfiValue.toFixed(1)}</span>
                    </div>
                  </div>
                )
              }}
            />
            <Area type="monotone" dataKey="close" stroke={isUp ? '#00ff88' : '#ff4444'} fill={isUp ? '#00ff8815' : '#ff444415'} strokeWidth={1} />
          </AreaChart>
        </ResponsiveContainer>
        
        {/* 资金流柱状图 */}
        <ResponsiveContainer width="100%" height="30%">
          <BarChart data={cumFlowData} margin={{ top: 0, right: 5, bottom: 0, left: 5 }}>
            <XAxis dataKey="time" tick={false} axisLine={false} tickLine={false} />
            <YAxis tick={{ fill: '#555', fontSize: 7 }} axisLine={false} tickLine={false} orientation="right" />
            <Tooltip 
              content={({ active, payload, label }) => {
                if (!active || !payload || !payload.length) return null
                const d = payload[0]?.payload as typeof cumFlowData[0]
                if (!d) return null
                
                return (
                  <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[9px] shadow-xl">
                    <div className="text-gray-400">{label}</div>
                    <div className="flex justify-between gap-2">
                      <span className="text-gray-500">单周期资金流:</span>
                      <span className={`font-mono ${d.moneyFlow >= 0 ? 'text-green-400' : 'text-red-400'}`}>
                        {d.moneyFlow >= 0 ? '+' : ''}{d.moneyFlow.toFixed(2)}B
                      </span>
                    </div>
                    <div className="flex justify-between gap-2">
                      <span className="text-gray-500">买压:</span>
                      <span className="text-green-400 font-mono">{d.buyPressure.toFixed(3)}%</span>
                    </div>
                    <div className="flex justify-between gap-2">
                      <span className="text-gray-500">卖压:</span>
                      <span className="text-red-400 font-mono">{d.sellPressure.toFixed(3)}%</span>
                    </div>
                  </div>
                )
              }}
            />
            <ReferenceLine y={0} stroke="#333" />
            <Bar dataKey="moneyFlow" isAnimationActive={false}>
              {cumFlowData.map((entry, index) => (
                <Cell key={`flow-${index}`} fill={entry.moneyFlow >= 0 ? '#00ff88' : '#ff4444'} />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
        
        {/* 累积资金流线 */}
        <ResponsiveContainer width="100%" height="30%">
          <AreaChart data={cumFlowData} margin={{ top: 0, right: 5, bottom: 5, left: 5 }}>
            <defs>
              <linearGradient id="cumFlowGradient" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#00aaff" stopOpacity={0.3}/>
                <stop offset="95%" stopColor="#00aaff" stopOpacity={0}/>
              </linearGradient>
            </defs>
            <XAxis dataKey="time" tick={{ fill: '#555', fontSize: 7 }} axisLine={false} tickLine={false} interval="preserveEnd" />
            <YAxis tick={{ fill: '#555', fontSize: 7 }} axisLine={false} tickLine={false} orientation="right" />
            <Tooltip 
              content={({ active, payload, label }) => {
                if (!active || !payload || !payload.length) return null
                const d = payload[0]?.payload as typeof cumFlowData[0]
                if (!d) return null
                
                const flowTrend = d.cumulativeFlow > 0 ? '资金净流入' : '资金净流出'
                
                return (
                  <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[9px] shadow-xl">
                    <div className="text-gray-400">{label}</div>
                    <div className="flex justify-between gap-2">
                      <span className="text-[#00aaff]">累积资金流:</span>
                      <span className={`font-mono ${d.cumulativeFlow >= 0 ? 'text-green-400' : 'text-red-400'}`}>
                        {d.cumulativeFlow >= 0 ? '+' : ''}{d.cumulativeFlow.toFixed(2)}B
                      </span>
                    </div>
                    <div className="flex justify-between gap-2">
                      <span className="text-gray-500">趋势:</span>
                      <span className={d.cumulativeFlow >= 0 ? 'text-green-400' : 'text-red-400'}>{flowTrend}</span>
                    </div>
                  </div>
                )
              }}
            />
            <ReferenceLine y={0} stroke="#333" />
            <Area type="monotone" dataKey="cumulativeFlow" stroke="#00aaff" fill="url(#cumFlowGradient)" strokeWidth={1.5} />
          </AreaChart>
        </ResponsiveContainer>
      </div>
    )
  }

  // 4. 趋势线自动绘制
  if (chartType === 'trend') {
    const prices = data.map(d => d.close)
    const len = prices.length
    
    // 计算线性回归趋势线
    let sumX = 0, sumY = 0, sumXY = 0, sumX2 = 0
    for (let i = 0; i < len; i++) {
      sumX += i
      sumY += prices[i]
      sumXY += i * prices[i]
      sumX2 += i * i
    }
    const slope = (len * sumXY - sumX * sumY) / (len * sumX2 - sumX * sumX)
    const intercept = (sumY - slope * sumX) / len
    
    // 计算R² (拟合优度)
    const yMean = sumY / len
    let ssTot = 0, ssRes = 0
    for (let i = 0; i < len; i++) {
      const yPred = intercept + slope * i
      ssTot += Math.pow(prices[i] - yMean, 2)
      ssRes += Math.pow(prices[i] - yPred, 2)
    }
    const rSquared = ssTot > 0 ? 1 - ssRes / ssTot : 0
    
    // 添加趋势线数据
    const trendData = data.map((d, i) => ({
      ...d,
      trend: intercept + slope * i,
      upperChannel: intercept + slope * i + basePrice * 0.005,
      lowerChannel: intercept + slope * i - basePrice * 0.005,
    }))
    
    // 找局部高低点
    const pivots: { index: number; price: number; type: 'high' | 'low' }[] = []
    for (let i = 2; i < len - 2; i++) {
      if (prices[i] > prices[i-1] && prices[i] > prices[i-2] && prices[i] > prices[i+1] && prices[i] > prices[i+2]) {
        pivots.push({ index: i, price: prices[i], type: 'high' })
      }
      if (prices[i] < prices[i-1] && prices[i] < prices[i-2] && prices[i] < prices[i+1] && prices[i] < prices[i+2]) {
        pivots.push({ index: i, price: prices[i], type: 'low' })
      }
    }
    
    const trendDirection = slope > 0 ? '↑ 上升趋势' : slope < 0 ? '↓ 下降趋势' : '→ 横盘'
    const trendStrength = Math.abs(slope / basePrice * 10000).toFixed(2)
    const slopeAngle = Math.atan(slope / basePrice * 1000) * 180 / Math.PI
    
    return (
      <div className="h-full w-full relative">
        <div className="absolute top-1 left-2 z-10 text-xs flex gap-3">
          <span className={slope > 0 ? 'text-green-400' : 'text-red-400'}>
            {trendDirection}
          </span>
          <span className="text-gray-400">强度: {trendStrength}</span>
          <span className="text-gray-400">R²: {(rSquared * 100).toFixed(1)}%</span>
        </div>
        <ResponsiveContainer width="100%" height="100%">
          <ComposedChart data={trendData} margin={{ top: 20, right: 5, bottom: 5, left: 5 }}>
            <defs>
              <linearGradient id="channelGradient" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor="#00aaff" stopOpacity={0.1}/>
                <stop offset="100%" stopColor="#00aaff" stopOpacity={0.05}/>
              </linearGradient>
            </defs>
            <XAxis dataKey="time" tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} />
            <YAxis domain={['auto', 'auto']} tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} orientation="right" />
            <Tooltip 
              content={({ active, payload, label }) => {
                if (!active || !payload || !payload.length) return null
                const d = payload[0]?.payload as typeof trendData[0]
                if (!d) return null
                
                const deviation = ((d.close - d.trend) / d.trend * 100)
                const channelPosition = d.close > d.trend 
                  ? (d.close >= d.upperChannel ? '上轨附近' : '通道上半')
                  : (d.close <= d.lowerChannel ? '下轨附近' : '通道下半')
                
                return (
                  <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[10px] shadow-xl min-w-[160px]">
                    <div className="text-gray-400 border-b border-[#333] pb-1 mb-1">{label}</div>
                    <div className="space-y-0.5">
                      <div className="flex justify-between">
                        <span className="text-gray-500">价格:</span>
                        <span className={`font-mono ${d.close >= d.open ? 'text-green-400' : 'text-red-400'}`}>{d.close?.toFixed(2)}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-[#ffaa00]">趋势线:</span>
                        <span className="text-[#ffaa00] font-mono">{d.trend?.toFixed(2)}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-gray-500">偏离度:</span>
                        <span className={`font-mono ${deviation >= 0 ? 'text-green-400' : 'text-red-400'}`}>
                          {deviation >= 0 ? '+' : ''}{deviation.toFixed(2)}%
                        </span>
                      </div>
                      <div className="border-t border-[#222] pt-1 mt-1">
                        <div className="flex justify-between">
                          <span className="text-gray-500">通道位置:</span>
                          <span className="text-cyan-400">{channelPosition}</span>
                        </div>
                        <div className="flex justify-between text-[9px]">
                          <span className="text-gray-500">上轨:</span>
                          <span className="text-gray-400">{d.upperChannel?.toFixed(2)}</span>
                        </div>
                        <div className="flex justify-between text-[9px]">
                          <span className="text-gray-500">下轨:</span>
                          <span className="text-gray-400">{d.lowerChannel?.toFixed(2)}</span>
                        </div>
                      </div>
                      <div className="border-t border-[#222] pt-1 mt-1 text-[9px]">
                        <div className="flex justify-between">
                          <span className="text-gray-500">趋势角度:</span>
                          <span className="text-gray-400">{slopeAngle.toFixed(1)}°</span>
                        </div>
                      </div>
                    </div>
                  </div>
                )
              }}
            />
            
            {/* 通道区域 */}
            <Area type="monotone" dataKey="upperChannel" stroke="transparent" fill="url(#channelGradient)" />
            <Area type="monotone" dataKey="lowerChannel" stroke="transparent" fill="#0a0a0a" />
            
            {/* 通道边界线 */}
            <Line type="monotone" dataKey="upperChannel" stroke="#00aaff" dot={false} strokeWidth={1} strokeDasharray="3 3" name="上轨" isAnimationActive={false} />
            <Line type="monotone" dataKey="lowerChannel" stroke="#00aaff" dot={false} strokeWidth={1} strokeDasharray="3 3" name="下轨" isAnimationActive={false} />
            
            {/* 趋势线 */}
            <Line type="monotone" dataKey="trend" stroke="#ffaa00" dot={false} strokeWidth={2} name="趋势线" isAnimationActive={false} />
            
            {/* 价格线 */}
            <Line type="monotone" dataKey="close" stroke={isUp ? '#00ff88' : '#ff4444'} dot={false} strokeWidth={1.5} name="价格" isAnimationActive={false} />
            
            {/* 标记局部高低点 */}
            {pivots.slice(-10).map((pivot, i) => (
              <ReferenceLine 
                key={i}
                y={pivot.price} 
                stroke={pivot.type === 'high' ? '#ff4444' : '#00ff88'} 
                strokeWidth={0.5}
                strokeDasharray="2 4"
              />
            ))}
          </ComposedChart>
        </ResponsiveContainer>
      </div>
    )
  }

  // 5. 热力图 (价格密度)
  if (chartType === 'heatmap') {
    const prices = data.map(d => d.close)
    const minPrice = Math.min(...prices)
    const maxPrice = Math.max(...prices)
    const range = maxPrice - minPrice
    const buckets = 15
    const bucketSize = range / buckets
    
    // 计算每个时间-价格格子的热度
    const heatmapData: { time: string; [key: string]: number | string }[] = data.map((d) => {
      const row: { time: string; [key: string]: number | string } = { time: d.time }
      
      for (let b = 0; b < buckets; b++) {
        const bucketLow = minPrice + b * bucketSize
        const bucketHigh = bucketLow + bucketSize
        
        // 热度基于价格接近度和成交量
        let heat = 0
        if (d.close >= bucketLow && d.close < bucketHigh) {
          heat = 100
        } else if (d.high >= bucketLow && d.low <= bucketHigh) {
          heat = 50
        }
        
        // 加入成交量因素
        heat *= (d.volume / 1000000)
        
        row[`p${b}`] = Math.min(heat, 100)
      }
      return row
    })
    
    // 热力图颜色
    const getHeatColor = (value: number) => {
      if (value > 80) return '#ff0000'
      if (value > 60) return '#ff4400'
      if (value > 40) return '#ff8800'
      if (value > 20) return '#ffaa00'
      if (value > 10) return '#444400'
      return '#111111'
    }
    
    return (
      <div className="h-full w-full flex flex-col">
        <div className="text-xs text-gray-500 px-2 py-1">价格热力图 - 显示价格在各区间的活跃度</div>
        <div className="flex-1 grid" style={{ 
          gridTemplateColumns: `repeat(${Math.min(data.length, 60)}, 1fr)`,
          gridTemplateRows: `repeat(${buckets}, 1fr)`,
          gap: '1px',
          padding: '4px'
        }}>
          {Array.from({ length: buckets }).map((_, rowIdx) => (
            data.slice(-60).map((d, colIdx) => {
              const value = (heatmapData[heatmapData.length - 60 + colIdx] || {})[`p${buckets - 1 - rowIdx}`] as number || 0
              return (
                <div
                  key={`${rowIdx}-${colIdx}`}
                  className="rounded-sm"
                  style={{ backgroundColor: getHeatColor(value) }}
                  title={`${d.time}: ${(minPrice + (buckets - 1 - rowIdx) * bucketSize).toFixed(2)}`}
                />
              )
            })
          ))}
        </div>
        <div className="flex justify-between px-2 py-1 text-xs text-gray-500">
          <span>{minPrice.toFixed(2)}</span>
          <span>价格区间</span>
          <span>{maxPrice.toFixed(2)}</span>
        </div>
      </div>
    )
  }

  // 成交量图
  if (chartType === 'volume') {
    // 计算成交量统计
    const avgVolume = data.reduce((sum, d) => sum + d.volume, 0) / data.length
    
    return (
      <ResponsiveContainer width="100%" height={height}>
        <BarChart data={data} margin={{ top: 5, right: 5, bottom: 5, left: 5 }}>
          <XAxis dataKey="time" tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} />
          <YAxis tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} orientation="right" tickFormatter={(v) => `${(v / 1000000).toFixed(1)}M`} />
          <Tooltip 
            content={({ active, payload, label }) => {
              if (!active || !payload || !payload.length) return null
              const d = payload[0]?.payload as CandleData
              if (!d) return null
              
              const volRatio = d.volume / avgVolume
              const isHighVol = volRatio > 1.5
              const isLowVol = volRatio < 0.5
              
              return (
                <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[10px] shadow-xl min-w-[150px]">
                  <div className="text-gray-400 border-b border-[#333] pb-1 mb-1">{label}</div>
                  <div className="space-y-0.5">
                    <div className="flex justify-between">
                      <span className="text-gray-500">成交量:</span>
                      <span className={`font-mono ${d.close >= d.open ? 'text-green-400' : 'text-red-400'}`}>
                        {(d.volume / 1e6).toFixed(2)}M
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">相对均量:</span>
                      <span className={`font-mono ${isHighVol ? 'text-yellow-400' : isLowVol ? 'text-gray-500' : 'text-gray-300'}`}>
                        {volRatio.toFixed(2)}x {isHighVol ? '放量' : isLowVol ? '缩量' : '正常'}
                      </span>
                    </div>
                    <div className="border-t border-[#222] pt-1 mt-1">
                      <div className="flex justify-between">
                        <span className="text-gray-500">价格:</span>
                        <span className={`font-mono ${d.close >= d.open ? 'text-green-400' : 'text-red-400'}`}>
                          {d.close?.toFixed(2)}
                        </span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-gray-500">涨跌:</span>
                        <span className={`font-mono ${d.close >= d.open ? 'text-green-400' : 'text-red-400'}`}>
                          {((d.close - d.open) / d.open * 100).toFixed(2)}%
                        </span>
                      </div>
                    </div>
                    <div className="border-t border-[#222] pt-1 mt-1 text-[9px]">
                      <div className="flex justify-between">
                        <span className="text-gray-500">均量:</span>
                        <span className="text-gray-400">{(avgVolume / 1e6).toFixed(2)}M</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-gray-500">OBV:</span>
                        <span className="text-gray-400">{((d.obv || 0) / 1e6).toFixed(2)}M</span>
                      </div>
                    </div>
                  </div>
                </div>
              )
            }}
          />
          <ReferenceLine y={avgVolume} stroke="#ffaa00" strokeDasharray="3 3" strokeWidth={1} />
          <Bar dataKey="volume" isAnimationActive={false}>
            {data.map((entry, index) => (
              <Cell key={`vol-${index}`} fill={(entry.close ?? 0) >= (entry.open ?? 0) ? '#00ff88' : '#ff4444'} />
            ))}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
    )
  }

  // 默认面积图
  return (
    <ResponsiveContainer width="100%" height={height}>
      <AreaChart data={data} margin={{ top: 5, right: 5, bottom: 5, left: 5 }}>
        <defs>
          <linearGradient id="areaGradient" x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor={isUp ? '#00ff88' : '#ff4444'} stopOpacity={0.3}/>
            <stop offset="95%" stopColor={isUp ? '#00ff88' : '#ff4444'} stopOpacity={0}/>
          </linearGradient>
        </defs>
        <XAxis dataKey="time" tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} />
        <YAxis domain={['auto', 'auto']} tick={{ fill: '#555', fontSize: 8 }} axisLine={false} tickLine={false} orientation="right" />
        <Tooltip 
          content={({ active, payload, label }) => {
            if (!active || !payload || !payload.length) return null
            const d = payload[0]?.payload as CandleData
            if (!d) return null
            
            const priceChange = d.close - d.open
            const changePercent = d.open ? (priceChange / d.open * 100) : 0
            
            return (
              <div className="bg-[#0a0a0a] border border-[#333] rounded-lg p-2 text-[10px] shadow-xl min-w-[160px]">
                <div className="text-gray-400 border-b border-[#333] pb-1 mb-1">{label}</div>
                <div className="space-y-0.5">
                  <div className="flex justify-between">
                    <span className="text-gray-500">价格:</span>
                    <span className={`font-mono font-bold ${d.close >= d.open ? 'text-green-400' : 'text-red-400'}`}>
                      {d.close?.toFixed(2)}
                    </span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-gray-500">涨跌:</span>
                    <span className={`font-mono ${changePercent >= 0 ? 'text-green-400' : 'text-red-400'}`}>
                      {changePercent >= 0 ? '+' : ''}{changePercent.toFixed(2)}%
                    </span>
                  </div>
                  
                  {/* 均线 */}
                  <div className="border-t border-[#222] pt-1 mt-1 grid grid-cols-2 gap-x-2 gap-y-0.5">
                    <div className="flex justify-between">
                      <span className="text-[#ffaa00]">MA5:</span>
                      <span className="text-gray-300 font-mono">{d.ma5?.toFixed(2)}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-[#00aaff]">MA10:</span>
                      <span className="text-gray-300 font-mono">{d.ma10?.toFixed(2)}</span>
                    </div>
                  </div>
                  
                  {/* 技术指标摘要 */}
                  <div className="border-t border-[#222] pt-1 mt-1 text-[9px]">
                    <div className="flex justify-between">
                      <span className="text-gray-500">RSI:</span>
                      <span className={`font-mono ${(d.rsi || 50) > 70 ? 'text-red-400' : (d.rsi || 50) < 30 ? 'text-green-400' : 'text-gray-300'}`}>
                        {d.rsi?.toFixed(1)}
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-500">成交量:</span>
                      <span className="text-cyan-400 font-mono">{(d.volume / 1e6).toFixed(2)}M</span>
                    </div>
                  </div>
                </div>
              </div>
            )
          }}
        />
        <ReferenceLine y={currentPrice} stroke={isUp ? '#00ff88' : '#ff4444'} strokeDasharray="3 3" />
        <Area type="monotone" dataKey="close" stroke={isUp ? '#00ff88' : '#ff4444'} fill="url(#areaGradient)" strokeWidth={1.5} name="价格" />
        {indicators.ma5 && <Line type="monotone" dataKey="ma5" stroke="#ffaa00" dot={false} strokeWidth={1} name="MA5" />}
        {indicators.ma10 && <Line type="monotone" dataKey="ma10" stroke="#00aaff" dot={false} strokeWidth={1} name="MA10" />}
      </AreaChart>
    </ResponsiveContainer>
  )
}

// 图表类型选择器 - 9种专业图表类型
export function ChartTypeSelector({ 
  value, 
  onChange 
}: { 
  value: string
  onChange: (type: string) => void 
}) {
  // 9种专业图表类型
  const mainTypes = [
    { id: 'candle', label: 'K线' },
    { id: 'area', label: 'Area' },
    { id: 'indicators', label: '综合' },
    { id: 'macd', label: 'MACD' },
    { id: 'rsi', label: 'RSI' },
    { id: 'kdj', label: 'KDJ' },
    { id: 'volume', label: '成交量' },
    { id: 'vwap', label: 'VWAP' },
    { id: 'depth', label: '深度' },
  ]

  return (
    <div className="flex items-center gap-0.5 bg-[#0a0a0a] border border-[#222] rounded px-1 py-0.5">
      {mainTypes.map(type => (
        <button
          key={type.id}
          onClick={() => onChange(type.id)}
          className={`px-2 py-0.5 rounded text-[9px] transition-all duration-100 ${
            value === type.id 
              ? 'bg-[#00aa6650] text-[#00ff88] font-bold' 
              : 'text-[#555] hover:text-[#aaa] hover:bg-[#151515]'
          }`}
        >
          {type.label}
        </button>
      ))}
    </div>
  )
}