import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Bar, BarChart, CartesianGrid, Cell, ComposedChart, Line, LineChart, Pie, PieChart, PolarAngleAxis, PolarGrid, Radar, RadarChart, ReferenceLine, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { AppShell } from '@/components/AppShell'
import { EmptyStatePanel } from '@/components/EmptyStatePanel'
import { StatusBanner } from '@/components/StatusBanner'
import { useAuth } from '@/contexts/AuthContext'
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts'
import {
  exchangeAPI,
  type BalanceRecord,
  type EarnPositionRecord,
  type Fill,
  type MarketSummary,
  type OpenOrderRecord,
  type OrderBookSnapshot,
  type OtcQuoteRecord,
  type TradeRecord,
} from '@/services/exchangeAPI'

type KindFilter = 'all' | MarketSummary['kind']
type OtcStatusFilter = 'all' | 'open' | 'accepted'
type OtcSideFilter = 'all' | 'buy' | 'sell'
type OrderSide = 'buy' | 'sell'
type LogTone = 'neutral' | 'success' | 'danger'

interface ActionLogItem {
  id: string
  tone: LogTone
  title: string
  message: string
  time: string
}

interface TerminalCommand {
  id: string
  label: string
  detail: string
  shortcut?: string
  disabled?: boolean
  run: () => void | Promise<void>
}

interface HoverLensState {
  source: 'ask' | 'bid' | 'heat' | 'trade' | 'order'
  side: OrderSide
  price: number
  amount: number
  cumulative?: number
  levelIndex?: number
  hint: string
}

interface MemoryEvent {
  id: string
  label: string
  detail: string
  time: string
}

interface ParsedPair {
  base: string
  quote: string
}

interface CustomTooltipProps {
  active?: boolean
  label?: string | number
  payload?: Array<{ name?: string; value?: number | string; color?: string }>
}

interface ParsedResponseError {
  code?: string
  message?: string
  error?: string
  details?: string
}

const kindOptions: Array<{ value: KindFilter; label: string }> = [
  { value: 'all', label: '全部品类' },
  { value: 'spot', label: '现货' },
  { value: 'margin', label: '杠杆' },
  { value: 'perp', label: '永续' },
  { value: 'future', label: '交割' },
  { value: 'option', label: '期权' },
  { value: 'otc', label: 'OTC' },
  { value: 'earn', label: '理财' },
]

const otcStatusOptions: Array<{ value: OtcStatusFilter; label: string }> = [
  { value: 'all', label: '全部报价' },
  { value: 'open', label: '待接受' },
  { value: 'accepted', label: '已成交' },
]

const otcSideOptions: Array<{ value: OtcSideFilter; label: string }> = [
  { value: 'all', label: '全部方向' },
  { value: 'buy', label: '买方需求' },
  { value: 'sell', label: '卖方需求' },
]

const kindLabel: Record<MarketSummary['kind'], string> = {
  spot: '现货',
  margin: '杠杆',
  perp: '永续',
  future: '交割',
  option: '期权',
  otc: 'OTC',
  earn: '理财',
}

const palette = ['#111111', '#404040', '#737373', '#a3a3a3', '#d4d4d4']

function formatNumber(value: number, digits = 4) {
  return Number.isFinite(value) ? value.toLocaleString('zh-CN', { maximumFractionDigits: digits }) : '-'
}

function formatSignedNumber(value: number, digits = 4) {
  if (!Number.isFinite(value)) return '-'
  const prefix = value > 0 ? '+' : ''
  return `${prefix}${value.toLocaleString('zh-CN', { maximumFractionDigits: digits })}`
}

function formatPercent(value: number, digits = 2) {
  return Number.isFinite(value) ? `${value.toLocaleString('zh-CN', { maximumFractionDigits: digits })}%` : '-'
}

function marketPriceDigits(kind?: MarketSummary['kind']) {
  switch (kind) {
    case 'option':
    case 'earn':
      return 4
    case 'otc':
      return 2
    default:
      return 2
  }
}

function marketAmountDigits(kind?: MarketSummary['kind']) {
  switch (kind) {
    case 'earn':
      return 6
    case 'otc':
      return 4
    default:
      return 6
  }
}

function formatTime(value?: string) {
  return value ? new Date(value).toLocaleString() : '-'
}

function classifyTradePerspective(trade: TradeRecord, userId: string) {
  const normalizedUser = userId.toLowerCase()
  if ((trade.buyer ?? '').toLowerCase() === normalizedUser) {
    return { label: '我买入', tone: 'buy' as const, counterparty: trade.seller ?? 'market' }
  }
  if ((trade.seller ?? '').toLowerCase() === normalizedUser) {
    return { label: '我卖出', tone: 'sell' as const, counterparty: trade.buyer ?? 'market' }
  }
  return { label: '市场成交', tone: 'neutral' as const, counterparty: `${trade.buyer ?? 'buyer'} / ${trade.seller ?? 'seller'}` }
}

function pretty(value: unknown) {
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

async function copyToClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    try {
      const textarea = document.createElement('textarea')
      textarea.value = text
      textarea.style.position = 'fixed'
      textarea.style.left = '-9999px'
      textarea.style.top = '0'
      document.body.appendChild(textarea)
      textarea.focus()
      textarea.select()
      const ok = document.execCommand('copy')
      textarea.remove()
      return ok
    } catch {
      return false
    }
  }
}

function parseResponseError(value: unknown): ParsedResponseError | null {
  if (!value || typeof value !== 'object') {
    return null
  }

  const record = value as Record<string, unknown>
  const message = typeof record.message === 'string' ? record.message : undefined
  const error = typeof record.error === 'string' ? record.error : undefined
  const code = typeof record.code === 'string' ? record.code : undefined
  let details: string | undefined

  if (typeof record.details === 'string') {
    details = record.details
  } else if (record.details && typeof record.details === 'object') {
    details = pretty(record.details)
  }

  if (!message && !error && !code && !details) {
    return null
  }

  return { code, message, error, details }
}

function parsePair(marketId?: string): ParsedPair {
  if (!marketId) {
    return { base: 'BTC', quote: 'USDT' }
  }

  if (marketId.startsWith('earn:')) {
    const [, asset] = marketId.split(':')
    return { base: (asset ?? 'USDC').toUpperCase(), quote: 'EARN' }
  }

  const parts = marketId.split(':')
  const rawPair = parts.length > 1 ? parts[1] : parts[0]
  const [base, quote] = rawPair.split('-')
  return {
    base: (base ?? 'BTC').toUpperCase(),
    quote: (quote ?? 'USDT').toUpperCase(),
  }
}

function matchBalance(balances: BalanceRecord[], assetHint: string) {
  const normalizedHint = assetHint.toUpperCase()
  return (
    balances.find((entry) => entry.asset.toUpperCase() === normalizedHint) ??
    balances.find((entry) => entry.asset.toUpperCase().startsWith(`${normalizedHint}-`)) ??
    balances.find((entry) => entry.asset.toUpperCase().includes(normalizedHint)) ??
    null
  )
}

function estimateEarn(position: EarnPositionRecord) {
  const createdAt = new Date(position.createdAt).getTime()
  const elapsedMs = Number.isFinite(createdAt) ? Math.max(Date.now() - createdAt, 0) : 0
  const elapsedDays = elapsedMs / 86_400_000
  const apr = position.aprBps / 10_000
  const yieldAmount = position.principalAmount * apr * (elapsedDays / 365)
  return {
    yieldAmount,
    redeemable: position.principalAmount + yieldAmount,
  }
}

function toneClass(isPositive: boolean, isNegative = false) {
  if (isNegative) return 'signal-negative'
  if (isPositive) return 'signal-positive'
  return 'signal-neutral'
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value))
}

function isNearPrice(left: number, right: number | null, tickSize: number, ticks = 2) {
  if (!right || !Number.isFinite(left) || !Number.isFinite(right)) return false
  return Math.abs(left - right) <= tickSize * ticks
}

function PremiumTooltip({ active, label, payload }: CustomTooltipProps) {
  if (!active || !payload || payload.length === 0) {
    return null
  }

  return (
    <div className="premium-tooltip">
      <div className="premium-tooltip-label">{label ?? '数据点'}</div>
      {payload.map((entry, index) => (
        <div key={`${entry.name ?? 'line'}-${index}`} className="premium-tooltip-row">
          <span className="text-neutral-500">{entry.name ?? '数值'}</span>
          <span className="data-mono font-medium text-black">{typeof entry.value === 'number' ? formatNumber(entry.value, 6) : String(entry.value ?? '-')}</span>
        </div>
      ))}
    </div>
  )
}

export function TradingTerminal() {
  const { session } = useAuth()
  const userId = session?.username ?? 'trader'
  const [markets, setMarkets] = useState<MarketSummary[]>([])
  const [kindFilter, setKindFilter] = useState<KindFilter>('all')
  const [selectedMarketId, setSelectedMarketId] = useState('')
  const [marketSearch, setMarketSearch] = useState('')
  const [onlyFavorites, setOnlyFavorites] = useState(false)
  const [showBrief, setShowBrief] = useState(() => {
    try {
      return window.localStorage.getItem('terminal:showBrief') === 'true'
    } catch {
      return false
    }
  })
  const [showInsights, setShowInsights] = useState(() => {
    try {
      return window.localStorage.getItem('terminal:showInsights') === 'true'
    } catch {
      return false
    }
  })
  const [focusLock, setFocusLock] = useState(() => {
    try {
      return window.localStorage.getItem('terminal:focusLock') === 'true'
    } catch {
      return false
    }
  })
  const [favoriteMarketIds, setFavoriteMarketIds] = useState<string[]>(() => {
    try {
      const raw = window.localStorage.getItem('terminal:favorites')
      const parsed = raw ? (JSON.parse(raw) as unknown) : []
      if (Array.isArray(parsed)) {
        return parsed.filter((item) => typeof item === 'string').slice(0, 200) as string[]
      }
    } catch {
      // ignore
    }
    return []
  })
  const [otcStatusFilter, setOtcStatusFilter] = useState<OtcStatusFilter>('all')
  const [otcSideFilter, setOtcSideFilter] = useState<OtcSideFilter>('all')
  const [balances, setBalances] = useState<BalanceRecord[]>([])
  const [orderBook, setOrderBook] = useState<OrderBookSnapshot | null>(null)
  const [trades, setTrades] = useState<TradeRecord[]>([])
  const [orders, setOrders] = useState<OpenOrderRecord[]>([])
  const [fills, setFills] = useState<Fill[]>([])
  const [otcQuotes, setOtcQuotes] = useState<OtcQuoteRecord[]>([])
  const [earnPositions, setEarnPositions] = useState<EarnPositionRecord[]>([])
  const [fundingRates, setFundingRates] = useState<Record<string, unknown>[]>([])
  const [riskEvents, setRiskEvents] = useState<Record<string, unknown>[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [notice, setNotice] = useState('交易终端已接入真实接口；若后端不可用，会明确展示真实错误体或 mock 回退状态。')
  const [lastResponse, setLastResponse] = useState<unknown>(null)
  const [actionLog, setActionLog] = useState<ActionLogItem[]>([])
  const [memoryEvents, setMemoryEvents] = useState<MemoryEvent[]>([])
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [paletteQuery, setPaletteQuery] = useState('')
  const [paletteIndex, setPaletteIndex] = useState(0)
  const [densityMode, setDensityMode] = useState<'compact' | 'comfortable'>(() => {
    try {
      const stored = window.localStorage.getItem('terminal:density')
      return stored === 'comfortable' ? 'comfortable' : 'compact'
    } catch {
      return 'compact'
    }
  })
  const [form, setForm] = useState({ side: 'buy' as OrderSide, price: '65000', amount: '1', outcome: '0' })
  const [highlightedBalances, setHighlightedBalances] = useState<string[]>([])
  const [highlightedOrders, setHighlightedOrders] = useState<string[]>([])
  const [highlightedFills, setHighlightedFills] = useState<string[]>([])
  const [selectedTradeId, setSelectedTradeId] = useState<string | null>(null)
  const [selectedOrderId, setSelectedOrderId] = useState<string | null>(null)
  const [hoveredTradeId, setHoveredTradeId] = useState<string | null>(null)
  const [hoveredOrderId, setHoveredOrderId] = useState<string | null>(null)
  const [balanceChangeSummary, setBalanceChangeSummary] = useState<string | null>(null)
  const [orderChangeSummary, setOrderChangeSummary] = useState<string | null>(null)
  const [fillChangeSummary, setFillChangeSummary] = useState<string | null>(null)
  const balanceSnapshotRef = useRef<Map<string, { available: number; hold: number }> | null>(null)
  const orderSnapshotRef = useRef<Map<string, Pick<OpenOrderRecord, 'remaining' | 'filled' | 'status'>> | null>(null)
  const fillSnapshotRef = useRef<Set<string> | null>(null)
  const balanceHighlightTimerRef = useRef<number | null>(null)
  const orderHighlightTimerRef = useRef<number | null>(null)
  const fillHighlightTimerRef = useRef<number | null>(null)
  const paletteInputRef = useRef<HTMLInputElement | null>(null)
  const [latencySamples, setLatencySamples] = useState<Record<string, number[]>>({})
  const [hoverLens, setHoverLens] = useState<HoverLensState | null>(null)
  const submitArmTimerRef = useRef<number | null>(null)
  const [submitArmed, setSubmitArmed] = useState(false)
  const didApplyShareLinkRef = useRef(false)
  const [centerTab, setCenterTab] = useState<'chart' | 'depth' | 'pulse'>('chart')
  const [bottomTab, setBottomTab] = useState<'orders' | 'trades' | 'fills' | 'risk' | 'log'>('orders')

  const appendLog = useCallback((title: string, message: string, tone: LogTone = 'neutral') => {
    setActionLog((current) => [
      { id: `${Date.now()}-${Math.random()}`, title, message, tone, time: new Date().toLocaleTimeString() },
      ...current,
    ].slice(0, 12))
  }, [])

  const appendMemory = useCallback((label: string, detail: string) => {
    setMemoryEvents((current) => [
      { id: `${Date.now()}-${Math.random()}`, label, detail, time: new Date().toLocaleTimeString() },
      ...current,
    ].slice(0, 8))
  }, [])

  useEffect(() => {
    try {
      window.localStorage.setItem('terminal:density', densityMode)
    } catch {
      // ignore
    }
  }, [densityMode])

  useEffect(() => {
    try {
      window.localStorage.setItem('terminal:showBrief', showBrief ? 'true' : 'false')
    } catch {
      // ignore
    }
  }, [showBrief])

  useEffect(() => {
    try {
      window.localStorage.setItem('terminal:showInsights', showInsights ? 'true' : 'false')
    } catch {
      // ignore
    }
  }, [showInsights])

  const favoriteMarketIdSet = useMemo(() => new Set(favoriteMarketIds), [favoriteMarketIds])
  const favoriteIndexMap = useMemo(() => new Map(favoriteMarketIds.map((id, index) => [id, index])), [favoriteMarketIds])

  useEffect(() => {
    try {
      window.localStorage.setItem('terminal:favorites', JSON.stringify(favoriteMarketIds))
    } catch {
      // ignore
    }
  }, [favoriteMarketIds])

  useEffect(() => {
    try {
      window.localStorage.setItem('terminal:focusLock', focusLock ? 'true' : 'false')
    } catch {
      // ignore
    }
  }, [focusLock])

  const marketsByKind = useMemo(
    () => (kindFilter === 'all' ? markets : markets.filter((item) => item.kind === kindFilter)),
    [kindFilter, markets],
  )
  const filteredMarkets = useMemo(() => {
    const query = marketSearch.trim().toLowerCase()
    let base = marketsByKind
    if (query) {
      base = base.filter((item) => `${item.name} ${item.id}`.toLowerCase().includes(query))
    }
    if (onlyFavorites) {
      base = base.filter((item) => favoriteMarketIdSet.has(item.id))
    }
    const sorted = [...base].sort((left, right) => {
      const leftFavIndex = favoriteIndexMap.has(left.id) ? (favoriteIndexMap.get(left.id) ?? 10_000) : 10_000
      const rightFavIndex = favoriteIndexMap.has(right.id) ? (favoriteIndexMap.get(right.id) ?? 10_000) : 10_000
      if (leftFavIndex !== rightFavIndex) return leftFavIndex - rightFavIndex
      if (favoriteMarketIdSet.has(left.id) !== favoriteMarketIdSet.has(right.id)) {
        return favoriteMarketIdSet.has(left.id) ? -1 : 1
      }
      return left.name.localeCompare(right.name)
    })
    return sorted
  }, [favoriteIndexMap, favoriteMarketIdSet, marketSearch, marketsByKind, onlyFavorites])

  const tradableCount = useMemo(() => filteredMarkets.filter((item) => item.tradingEnabled).length, [filteredMarkets])
  const liveBackendCount = useMemo(() => filteredMarkets.filter((item) => item.backendAvailable).length, [filteredMarkets])
  const selectedMarket = useMemo(
    () => markets.find((item) => item.id === selectedMarketId) ?? (selectedMarketId ? null : filteredMarkets[0] ?? null),
    [filteredMarkets, markets, selectedMarketId],
  )
  const selectedOtcQuotes = useMemo(
    () => otcQuotes.filter((quote) => quote.marketId === selectedMarket?.id),
    [otcQuotes, selectedMarket],
  )
  const visibleOtcQuotes = useMemo(
    () =>
      selectedOtcQuotes.filter(
        (quote) =>
          (otcStatusFilter === 'all' || quote.status === otcStatusFilter) &&
          (otcSideFilter === 'all' || quote.side === otcSideFilter),
      ),
    [otcSideFilter, otcStatusFilter, selectedOtcQuotes],
  )
  const orderSource = orderBook?.source ?? (selectedMarket?.backendAvailable ? 'api' : 'mock')
  const totalAvailable = useMemo(() => balances.reduce((sum, item) => sum + item.available, 0), [balances])
  const totalHold = useMemo(() => balances.reduce((sum, item) => sum + item.hold, 0), [balances])
  const bestBid = orderBook?.bids[0]?.price ?? 0
  const bestAsk = orderBook?.asks[0]?.price ?? 0
  const midPrice = bestBid && bestAsk ? (bestBid + bestAsk) / 2 : bestBid || bestAsk || 0
  const spread = bestBid && bestAsk ? bestAsk - bestBid : 0
  const spreadBps = midPrice > 0 ? (spread / midPrice) * 10_000 : 0
  const ticketPrice = Number(form.price) || 0
  const ticketAmount = Number(form.amount) || 0
  const ticketNotional = ticketPrice * ticketAmount
  const pair = useMemo(() => parsePair(selectedMarket?.id), [selectedMarket])
  const priceDigits = useMemo(() => marketPriceDigits(selectedMarket?.kind), [selectedMarket?.kind])
  const amountDigits = useMemo(() => marketAmountDigits(selectedMarket?.kind), [selectedMarket?.kind])
  const quoteDigits = useMemo(() => (pair.quote === 'USDT' || pair.quote === 'USD' ? 2 : 4), [pair.quote])
  const baseBalance = useMemo(() => matchBalance(balances, pair.base), [balances, pair.base])
  const quoteBalance = useMemo(() => matchBalance(balances, pair.quote), [balances, pair.quote])
  const maxBidAmount = useMemo(() => Math.max(...(orderBook?.bids.map((level) => level.amount) ?? [1])), [orderBook])
  const maxAskAmount = useMemo(() => Math.max(...(orderBook?.asks.map((level) => level.amount) ?? [1])), [orderBook])
  const balanceChartData = useMemo(() => balances.map((item) => ({ name: item.asset, value: item.available })), [balances])
  const priceChartData = useMemo(
    () =>
      trades
        .slice()
        .reverse()
        .map((trade, index) => ({ name: `${index + 1}`, 价格: trade.price, 数量: trade.amount })),
    [trades],
  )
  const priceChartSummary = useMemo(() => {
    if (trades.length === 0) {
      return {
        latest: null as number | null,
        first: null as number | null,
        high: null as number | null,
        low: null as number | null,
        volume: 0,
        changePct: null as number | null,
      }
    }

    const prices = trades.map((trade) => trade.price)
    const first = trades[0]?.price ?? null
    const latest = trades[trades.length - 1]?.price ?? null
    const high = Math.max(...prices)
    const low = Math.min(...prices)
    const volume = trades.reduce((sum, trade) => sum + trade.amount, 0)
    const changePct = first && latest ? ((latest - first) / first) * 100 : null

    return { latest, first, high, low, volume, changePct }
  }, [trades])
  const priceChartMetricItems = useMemo(
    () => [
      { label: '最新', value: priceChartSummary.latest === null ? '-' : formatNumber(priceChartSummary.latest, priceDigits) },
      { label: '区间高', value: priceChartSummary.high === null ? '-' : formatNumber(priceChartSummary.high, priceDigits) },
      { label: '区间低', value: priceChartSummary.low === null ? '-' : formatNumber(priceChartSummary.low, priceDigits) },
      { label: '累计量', value: formatNumber(priceChartSummary.volume, amountDigits) },
      {
        label: '区间变化',
        value: priceChartSummary.changePct === null ? '-' : `${formatSignedNumber(priceChartSummary.changePct, 2)}%`,
        tone: priceChartSummary.changePct === null ? 'neutral' : priceChartSummary.changePct >= 0 ? 'positive' : 'negative',
      },
    ],
    [amountDigits, priceChartSummary.changePct, priceChartSummary.high, priceChartSummary.latest, priceChartSummary.low, priceChartSummary.volume, priceDigits],
  )
  const otcChartData = useMemo(
    () => visibleOtcQuotes.map((quote, index) => ({ name: `${index + 1}`, 报价规模: quote.amount, 报价价格: quote.price })),
    [visibleOtcQuotes],
  )
  const earnPositionStats = useMemo(
    () =>
      earnPositions.map((position, index) => {
        const estimate = estimateEarn(position)
        return {
          name: position.asset || `P${index + 1}`,
          本金: position.principalAmount,
          估算收益: estimate.yieldAmount,
          可赎回: estimate.redeemable,
          record: position,
        }
      }),
    [earnPositions],
  )
  const earnTotalPrincipal = useMemo(() => earnPositionStats.reduce((sum, item) => sum + item.本金, 0), [earnPositionStats])
  const earnTotalYield = useMemo(() => earnPositionStats.reduce((sum, item) => sum + item.估算收益, 0), [earnPositionStats])
  const earnTotalRedeemable = useMemo(() => earnPositionStats.reduce((sum, item) => sum + item.可赎回, 0), [earnPositionStats])
  const estimatedDebit = useMemo(() => {
    if (!selectedMarket) return 0
    if (selectedMarket.kind === 'earn') return ticketAmount
    if (form.side === 'buy') return ticketNotional
    return ticketAmount
  }, [form.side, selectedMarket, ticketAmount, ticketNotional])
  const impactAsset = useMemo(() => {
    if (!selectedMarket) return pair.quote
    if (selectedMarket.kind === 'earn') return pair.base
    return form.side === 'buy' ? pair.quote : pair.base
  }, [form.side, pair.base, pair.quote, selectedMarket])
  const receiveAsset = useMemo(() => {
    if (!selectedMarket) return pair.base
    if (selectedMarket.kind === 'earn') return '可赎回份额'
    return form.side === 'buy' ? pair.base : pair.quote
  }, [form.side, pair.base, pair.quote, selectedMarket])
  const receiveAmount = useMemo(() => {
    if (!selectedMarket) return 0
    if (selectedMarket.kind === 'earn') return ticketAmount
    return form.side === 'buy' ? ticketAmount : ticketNotional
  }, [form.side, selectedMarket, ticketAmount, ticketNotional])
  const projectedAvailable = useMemo(() => {
    const balance = impactAsset === pair.base ? baseBalance : quoteBalance
    if (!balance) return null
    return balance.available - estimatedDebit
  }, [baseBalance, estimatedDebit, impactAsset, pair.base, quoteBalance])
  const currentImpactAvailable = useMemo(() => {
    const balance = impactAsset === pair.base ? baseBalance : quoteBalance
    return balance ? balance.available : null
  }, [baseBalance, impactAsset, pair.base, quoteBalance])
  const bookBidDepth = useMemo(() => orderBook?.bids.reduce((sum, level) => sum + level.amount, 0) ?? 0, [orderBook])
  const bookAskDepth = useMemo(() => orderBook?.asks.reduce((sum, level) => sum + level.amount, 0) ?? 0, [orderBook])
  const bookImbalance = useMemo(() => {
    const totalDepth = bookBidDepth + bookAskDepth
    return totalDepth > 0 ? ((bookBidDepth - bookAskDepth) / totalDepth) * 100 : 0
  }, [bookAskDepth, bookBidDepth])
  const parsedLastResponseError = useMemo(() => parseResponseError(lastResponse), [lastResponse])
  const executionStateLabel = parsedLastResponseError ? '错误' : orderSource === 'api' ? '真实' : '回退'
  const selectedMarketStateLabel = selectedMarket?.tradingEnabled ? '交易中' : '已暂停'
  const needsSubmitConfirm = useMemo(() => {
    if (!selectedMarket) return false
    if (!selectedMarket.tradingEnabled) return true
    if (orderSource !== 'api') return true
    if (parsedLastResponseError) return true
    return false
  }, [orderSource, parsedLastResponseError, selectedMarket])
  const submitFeedbackToneClass = parsedLastResponseError
    ? 'submit-feedback-error'
    : lastResponse
      ? 'submit-feedback-success'
      : 'submit-feedback-neutral'
  const submitFeedbackMotionClass = isSubmitting
    ? 'submit-feedback-live'
    : parsedLastResponseError
      ? 'submit-feedback-error-glow'
      : lastResponse
        ? 'submit-feedback-success-glow'
        : ''
  const highlightedBalanceSet = useMemo(() => new Set(highlightedBalances), [highlightedBalances])
  const highlightedOrderSet = useMemo(() => new Set(highlightedOrders), [highlightedOrders])
  const highlightedFillSet = useMemo(() => new Set(highlightedFills), [highlightedFills])
  const closureSummary = useMemo(
    () => [balanceChangeSummary, orderChangeSummary, fillChangeSummary].filter(Boolean).join(' · '),
    [balanceChangeSummary, fillChangeSummary, orderChangeSummary],
  )

  const recordLatency = useCallback((key: string, durationMs: number) => {
    const bounded = Number.isFinite(durationMs) ? Math.max(0, Math.min(durationMs, 60_000)) : 0
    setLatencySamples((current) => {
      const previous = current[key] ?? []
      const next = [bounded, ...previous].slice(0, 20)
      return { ...current, [key]: next }
    })
  }, [])

  const percentile = useCallback((values: number[], p: number) => {
    if (values.length === 0) return null
    const sorted = [...values].sort((a, b) => a - b)
    const idx = Math.min(sorted.length - 1, Math.max(0, Math.round((p / 100) * (sorted.length - 1))))
    return sorted[idx]
  }, [])

  const latencyDashboard = useMemo(() => {
    const rows = [
      { key: 'loadOverview', label: '刷新总览' },
      { key: 'loadSelectedMarket', label: '刷新选中市场' },
      { key: 'submitOrder', label: '提交' },
      { key: 'cancelOrder', label: '撤单' },
    ]
    return rows.map((row) => {
      const samples = latencySamples[row.key] ?? []
      const latest = samples[0] ?? null
      const p50 = percentile(samples, 50)
      const p95 = percentile(samples, 95)
      return {
        ...row,
        latest,
        p50,
        p95,
        count: samples.length,
      }
    })
  }, [latencySamples, percentile])

  const toggleFavorite = useCallback((marketId: string) => {
    setFavoriteMarketIds((current) => {
      if (current.includes(marketId)) return current.filter((id) => id !== marketId)
      return [marketId, ...current].slice(0, 200)
    })
  }, [])

  const [dragFavoriteId, setDragFavoriteId] = useState<string | null>(null)
  const [dragOverFavoriteId, setDragOverFavoriteId] = useState<string | null>(null)

  const reorderFavorites = useCallback((dragId: string, overId: string) => {
    if (dragId === overId) return
    setFavoriteMarketIds((current) => {
      const dragIndex = current.indexOf(dragId)
      const overIndex = current.indexOf(overId)
      if (dragIndex < 0 || overIndex < 0) return current
      const next = [...current]
      next.splice(dragIndex, 1)
      next.splice(overIndex, 0, dragId)
      return next
    })
  }, [])

  const clearMarketSearch = useCallback(() => {
    setMarketSearch('')
    appendMemory('筛选', '已清空市场搜索条件。')
  }, [appendMemory])

  const isFavoriteDragEnabled = useMemo(() => onlyFavorites || marketSearch.trim().length === 0, [marketSearch, onlyFavorites])

  const buildShareLink = useCallback(() => {
    const base = `${window.location.origin}${window.location.pathname}`
    const hashRoute = '#/trading'
    const params = new URLSearchParams()
    if (selectedMarket?.id) params.set('m', selectedMarket.id)
    params.set('side', form.side)
    if (Number.isFinite(Number(form.price))) params.set('p', String(Number(form.price) || 0))
    if (Number.isFinite(Number(form.amount))) params.set('a', String(Number(form.amount) || 0))
    if (form.outcome) params.set('o', String(form.outcome))
    params.set('k', kindFilter)
    if (onlyFavorites) params.set('fav', '1')
    if (focusLock) params.set('lock', '1')
    if (densityMode === 'comfortable') params.set('d', 'c')
    if (showBrief) params.set('brief', '1')
    if (showInsights) params.set('ins', '1')
    return `${base}${hashRoute}?${params.toString()}`
  }, [densityMode, focusLock, form.amount, form.outcome, form.price, form.side, kindFilter, onlyFavorites, selectedMarket?.id, showBrief, showInsights])

  const copyShareLink = useCallback(async () => {
    const link = buildShareLink()
    const ok = await copyToClipboard(link)
    setNotice(ok ? '已复制分享链接（包含当前市场与票据参数）。' : '复制失败（请检查浏览器权限）。')
    appendMemory('分享链接', ok ? '已复制分享链接。' : '复制分享链接失败。')
  }, [appendMemory, buildShareLink])
  const terminalFooterItems = useMemo(
    () => [
      {
        label: '当前市场',
        value: selectedMarket ? `${pair.base}/${pair.quote}` : '未选择',
        hint: selectedMarket ? kindLabel[selectedMarket.kind] : '等待选择',
      },
      {
        label: '执行链路',
        value: orderSource === 'api' ? '真实接口' : '本地回退',
        hint: parsedLastResponseError ? '最新动作存在错误' : '链路可继续操作',
      },
      {
        label: '账户闭环',
        value: closureSummary || '等待下一次动作',
        hint: '余额 / 挂单 / 成交变化',
      },
      {
        label: '快捷操作',
        value: 'B / S / 1 / 2 / 3 / 4 / Enter',
        hint: '键盘直达',
      },
    ],
    [closureSummary, orderSource, pair.base, pair.quote, parsedLastResponseError, selectedMarket],
  )
  const selectedTrade = useMemo(() => trades.find((trade) => trade.id === selectedTradeId) ?? trades[0] ?? null, [selectedTradeId, trades])
  const selectedOrder = useMemo(() => orders.find((order) => order.id === selectedOrderId) ?? orders[0] ?? null, [orders, selectedOrderId])
  const hoveredTrade = useMemo(() => trades.find((trade) => trade.id === hoveredTradeId) ?? null, [hoveredTradeId, trades])
  const hoveredOrder = useMemo(() => orders.find((order) => order.id === hoveredOrderId) ?? null, [hoveredOrderId, orders])
  const crossesSpread = useMemo(() => {
    if (!bestBid || !bestAsk || !ticketPrice) return false
    return form.side === 'buy' ? ticketPrice >= bestAsk : ticketPrice <= bestBid
  }, [bestAsk, bestBid, form.side, ticketPrice])
  const affordabilityState = useMemo(() => {
    if (projectedAvailable === null) return 'unknown'
    if (projectedAvailable < 0) return 'insufficient'
    if (projectedAvailable === 0) return 'full-use'
    return 'safe'
  }, [projectedAvailable])
  const executionReadiness = useMemo(() => {
    if (!selectedMarket) return '未选择市场'
    if (parsedLastResponseError) return '需先处理上一次错误'
    if (!selectedMarket.tradingEnabled) return '市场暂停'
    if (affordabilityState === 'insufficient') return '余额不足'
    if (crossesSpread) return '价格将更接近立即成交'
    return '可作为挂单进入簿'
  }, [affordabilityState, crossesSpread, parsedLastResponseError, selectedMarket])
  const topContextItems = useMemo(
    () => [
      {
        label: '市场',
        value: selectedMarket ? `${pair.base}/${pair.quote}` : '未选择',
        hint: selectedMarket ? `${kindLabel[selectedMarket.kind]} · ${selectedMarketStateLabel}` : '等待选择',
      },
      {
        label: '执行链路',
        value: orderSource === 'api' ? '真实链路' : '本地回退',
        hint: parsedLastResponseError ? '最近一次动作返回错误' : executionStateLabel,
      },
      {
        label: '点差',
        value: spread ? formatNumber(spread, priceDigits) : '-',
        hint: spread ? `${formatNumber(spreadBps, 2)} bps` : '暂无盘口',
      },
      {
        label: '可用资产',
        value: currentImpactAvailable === null ? '-' : formatNumber(currentImpactAvailable, impactAsset === pair.quote ? quoteDigits : amountDigits),
        hint: impactAsset,
      },
      {
        label: '执行判断',
        value: executionReadiness,
        hint: crossesSpread ? '偏向成交' : '偏向挂单',
      },
    ],
    [
      currentImpactAvailable,
      executionReadiness,
      executionStateLabel,
      impactAsset,
      orderSource,
      pair.base,
      pair.quote,
      parsedLastResponseError,
      priceDigits,
      quoteDigits,
      amountDigits,
      selectedMarket,
      selectedMarketStateLabel,
      spread,
      spreadBps,
      crossesSpread,
    ],
  )
  const recentPriceDeltaPct = useMemo(() => {
    if (trades.length < 2) return 0
    const latest = trades[0]?.price ?? 0
    const earliest = trades[trades.length - 1]?.price ?? 0
    if (!latest || !earliest) return 0
    return ((latest - earliest) / earliest) * 100
  }, [trades])
  const liquidityScore = useMemo(() => {
    if (selectedMarket?.kind === 'otc') return visibleOtcQuotes.length > 0 ? 74 : 42
    if (selectedMarket?.kind === 'earn') return earnPositions.length > 0 ? 68 : 56
    const depthFactor = clamp((bookBidDepth + bookAskDepth) * 3, 0, 38)
    const spreadFactor = clamp(42 - spreadBps * 1.8, 0, 42)
    const sourceBonus = orderSource === 'api' ? 12 : 4
    return Math.round(clamp(depthFactor + spreadFactor + sourceBonus, 8, 98))
  }, [bookAskDepth, bookBidDepth, earnPositions.length, orderSource, selectedMarket?.kind, spreadBps, visibleOtcQuotes.length])
  const flowScore = useMemo(() => {
    if (selectedMarket?.kind === 'earn') return 61
    if (selectedMarket?.kind === 'otc') return visibleOtcQuotes.length > 0 ? 66 : 40
    return Math.round(clamp(50 + bookImbalance * 1.6 + recentPriceDeltaPct * 9, 5, 96))
  }, [bookImbalance, recentPriceDeltaPct, selectedMarket?.kind, visibleOtcQuotes.length])
  const balanceSafetyScore = useMemo(() => {
    if (projectedAvailable === null) return 52
    if (projectedAvailable < 0) return 6
    if (projectedAvailable === 0) return 44
    return Math.round(clamp(62 + (projectedAvailable / Math.max(estimatedDebit || 1, 1)) * 20, 12, 97))
  }, [estimatedDebit, projectedAvailable])
  const executionConfidenceScore = useMemo(() => {
    const base = orderSource === 'api' ? 78 : 56
    const errorPenalty = parsedLastResponseError ? 28 : 0
    const statePenalty = selectedMarket?.tradingEnabled ? 0 : 24
    const balancePenalty = affordabilityState === 'insufficient' ? 30 : affordabilityState === 'unknown' ? 10 : 0
    return Math.round(clamp(base - errorPenalty - statePenalty - balancePenalty + (crossesSpread ? 6 : 0), 5, 97))
  }, [affordabilityState, crossesSpread, orderSource, parsedLastResponseError, selectedMarket?.tradingEnabled])
  const marketBrief = useMemo(() => {
    if (!selectedMarket) {
      return {
        tag: '等待市场',
        title: '先选择一个市场，再生成智能交易简报',
        summary: '当前终端会根据盘口、成交、余额和链路状态自动生成简报与执行提示。',
      }
    }

    if (selectedMarket.kind === 'earn') {
      return {
        tag: '收益模式',
        title: `${pair.base} 理财模式更偏向仓位管理`,
        summary: `当前更适合关注申购/赎回节奏、预计收益与可赎回金额，而不是短时价格博弈。`,
      }
    }

    if (selectedMarket.kind === 'otc') {
      return {
        tag: '报价模式',
        title: `${pair.base}/${pair.quote} 当前以场外报价为主`,
        summary: visibleOtcQuotes.length > 0
          ? `已有 ${visibleOtcQuotes.length} 条可用报价，适合做大额询价、对手方选择与报价筛选。`
          : '当前暂无可点击报价，更适合先由你发起一笔试探性报价来观察回执。 ',
      }
    }

    const spreadNarrative = spreadBps <= 2 ? '点差非常紧凑' : spreadBps <= 8 ? '点差保持可接受范围' : '点差偏宽，需要更谨慎地入场'
    const flowNarrative = bookImbalance >= 8 ? '买盘深度更强' : bookImbalance <= -8 ? '卖盘深度更强' : '买卖盘相对均衡'
    const trendNarrative = recentPriceDeltaPct >= 0.25 ? '短线价格有抬升迹象' : recentPriceDeltaPct <= -0.25 ? '短线价格仍在回落' : '短线价格仍在震荡整理'
    return {
      tag: orderSource === 'api' ? '真实链路' : '回退模式',
      title: `${pair.base}/${pair.quote} 当前呈现 ${flowNarrative}`,
      summary: `${spreadNarrative}，${trendNarrative}。${crossesSpread ? '你当前输入更接近立即成交。' : '你当前输入更接近排队挂单。'}`,
    }
  }, [bookImbalance, crossesSpread, orderSource, pair.base, pair.quote, recentPriceDeltaPct, selectedMarket, spreadBps, visibleOtcQuotes.length])
  const signalMatrix = useMemo(
    () => [
      {
        label: '流动性',
        score: liquidityScore,
        detail: liquidityScore >= 75 ? '深度够厚，适合连续操作' : liquidityScore >= 55 ? '深度尚可，注意点差变化' : '深度偏薄，适合降低冲击',
      },
      {
        label: '买卖倾斜',
        score: flowScore,
        detail: flowScore >= 60 ? '买方节奏更主动' : flowScore <= 40 ? '卖方节奏更主动' : '多空力量相对均衡',
      },
      {
        label: '余额安全',
        score: balanceSafetyScore,
        detail: balanceSafetyScore >= 70 ? '余额覆盖充足' : balanceSafetyScore >= 40 ? '余额紧贴边界' : '余额不足，建议收缩规模',
      },
      {
        label: '执行置信',
        score: executionConfidenceScore,
        detail: executionConfidenceScore >= 70 ? '可以直接执行并观察回执' : executionConfidenceScore >= 45 ? '建议先小单试探' : '建议先处理链路或余额问题',
      },
    ],
    [balanceSafetyScore, executionConfidenceScore, flowScore, liquidityScore],
  )
  const executionModes = useMemo(
    () => [
      {
        label: 'Maker 模式',
        title: crossesSpread ? '当前不适合' : '当前优先推荐',
        detail: crossesSpread ? '你的价格已接近对手盘，容易立刻成交。' : '当前输入更适合排队挂单，保持价格主动权。',
      },
      {
        label: 'Taker 模式',
        title: crossesSpread ? '更接近实时成交' : '需要更主动价格',
        detail: crossesSpread ? '更适合快速确认撮合与资金链路是否通畅。' : '若想快速成交，可考虑向最优买一/卖一靠拢。',
      },
      {
        label: selectedMarket?.kind === 'otc' ? 'OTC 模式' : selectedMarket?.kind === 'earn' ? 'Earn 模式' : '试探模式',
        title: selectedMarket?.kind === 'otc' ? '重视对手方筛选' : selectedMarket?.kind === 'earn' ? '重视收益与赎回窗口' : '先小单再放大',
        detail:
          selectedMarket?.kind === 'otc'
            ? '优先比对报价状态、方向与时间，再决定是否点击带入。'
            : selectedMarket?.kind === 'earn'
              ? '优先确认申购/赎回方向、预计收益和可赎回信息。'
              : '先用小规模确认回执与余额变化，再决定是否连续下单。',
      },
    ],
    [crossesSpread, selectedMarket?.kind],
  )
  const cockpitFacts = useMemo(
    () => [
      {
        label: '微结构状态',
        value: bookImbalance >= 8 ? '买方主导' : bookImbalance <= -8 ? '卖方主导' : '平衡整理',
      },
      {
        label: '短线变化',
        value: `${formatSignedNumber(recentPriceDeltaPct, 2)}%`,
      },
      {
        label: '建议节奏',
        value: executionConfidenceScore >= 70 ? '可直接执行' : executionConfidenceScore >= 45 ? '先小单试探' : '先修复条件',
      },
      {
        label: '风险姿态',
        value: balanceSafetyScore >= 70 ? '可控' : balanceSafetyScore >= 40 ? '紧边界' : '高风险',
      },
    ],
    [balanceSafetyScore, bookImbalance, executionConfidenceScore, recentPriceDeltaPct],
  )
  const microPulseData = useMemo(
    () =>
      trades
        .slice(0, 12)
        .reverse()
        .map((trade, index) => ({
          tick: index + 1,
          price: trade.price,
          amount: trade.amount,
        })),
    [trades],
  )
  const sweepPreview = useMemo(() => {
    if (!orderBook || ticketAmount <= 0 || selectedMarket?.kind === 'otc' || selectedMarket?.kind === 'earn') {
      return null
    }

    const levels = form.side === 'buy' ? orderBook.asks : orderBook.bids
    let remaining = ticketAmount
    let filled = 0
    let notional = 0
    let worstPrice = 0

    for (const level of levels) {
      if (remaining <= 0) break
      const take = Math.min(remaining, level.amount)
      filled += take
      notional += take * level.price
      remaining -= take
      worstPrice = level.price
    }

    if (filled <= 0) {
      return {
        fillRatio: 0,
        averagePrice: 0,
        slippageBps: 0,
        worstPrice: 0,
        residual: ticketAmount,
      }
    }

    const averagePrice = notional / filled
    const anchor = form.side === 'buy' ? bestAsk || averagePrice : bestBid || averagePrice
    const slippageBps = anchor > 0 ? Math.abs((averagePrice - anchor) / anchor) * 10_000 : 0

    return {
      fillRatio: (filled / ticketAmount) * 100,
      averagePrice,
      slippageBps,
      worstPrice,
      residual: Math.max(ticketAmount - filled, 0),
    }
  }, [bestAsk, bestBid, form.side, orderBook, selectedMarket?.kind, ticketAmount])
  const queuePressureEstimate = useMemo(() => {
    if (!orderBook || ticketPrice <= 0 || ticketAmount <= 0 || selectedMarket?.kind === 'otc' || selectedMarket?.kind === 'earn') {
      return null
    }

    const bookSide = form.side === 'buy' ? orderBook.bids : orderBook.asks
    const betterLevels =
      form.side === 'buy'
        ? bookSide.filter((level) => level.price > ticketPrice)
        : bookSide.filter((level) => level.price < ticketPrice)
    const sameLevel = bookSide.find((level) => level.price === ticketPrice)
    const aheadVolume = betterLevels.reduce((sum, level) => sum + level.amount, 0) + (sameLevel?.amount ?? 0)
    const posture =
      crossesSpread
        ? '主动成交'
        : aheadVolume === 0
          ? '头部排队'
          : aheadVolume <= ticketAmount * 1.5
            ? '中等排队'
            : '深度排队'

    return {
      aheadVolume,
      posture,
    }
  }, [crossesSpread, form.side, orderBook, selectedMarket?.kind, ticketAmount, ticketPrice])
  const lifecycleTimeline = useMemo(
    () => [
      {
        label: '选市场',
        title: selectedMarket ? `${pair.base}/${pair.quote}` : '等待选择',
        detail: selectedMarket ? `${kindLabel[selectedMarket.kind]} · ${selectedMarketStateLabel}` : '先在左侧选择一个交易品类与市场',
      },
      {
        label: '设参数',
        title: ticketAmount > 0 ? `${form.side === 'buy' ? '买入' : '卖出'} ${formatNumber(ticketAmount, amountDigits)}` : '等待输入',
        detail: ticketPrice > 0 ? `价格 ${formatNumber(ticketPrice, priceDigits)}` : '先输入价格与数量',
      },
      {
        label: '执行判断',
        title: executionReadiness,
        detail: queuePressureEstimate ? `${queuePressureEstimate.posture} · 前方排队 ${formatNumber(queuePressureEstimate.aheadVolume, amountDigits)}` : '等待盘口与参数同时就绪',
      },
      {
        label: '回执结果',
        title: parsedLastResponseError ? '出现错误' : lastResponse ? '已收到回执' : '等待提交',
        detail: parsedLastResponseError ? (parsedLastResponseError.code ?? '请查看错误体') : closureSummary || '提交后会在这里形成闭环',
      },
    ],
    [
      amountDigits,
      closureSummary,
      executionReadiness,
      form.side,
      kindLabel,
      lastResponse,
      pair.base,
      pair.quote,
      parsedLastResponseError,
      priceDigits,
      queuePressureEstimate,
      selectedMarket,
      selectedMarketStateLabel,
      ticketAmount,
      ticketPrice,
    ],
  )
  const recentPriceRange = useMemo(() => {
    if (trades.length === 0) {
      return { low: 0, high: 0 }
    }
    const prices = trades.map((trade) => trade.price)
    return {
      low: Math.min(...prices),
      high: Math.max(...prices),
    }
  }, [trades])
  const priceMagnetZones = useMemo(() => {
    const anchors = [
      { label: '买一', price: bestBid, strength: bookBidDepth > bookAskDepth ? '强' : '中' },
      { label: '中价', price: midPrice, strength: spreadBps <= 5 ? '强' : '中' },
      { label: '卖一', price: bestAsk, strength: bookAskDepth > bookBidDepth ? '强' : '中' },
      { label: '区间高', price: recentPriceRange.high, strength: recentPriceRange.high > 0 ? '观察' : '弱' },
    ]

    return anchors
      .filter((item) => item.price > 0)
      .map((item) => ({
        ...item,
        distanceBps: ticketPrice > 0 ? Math.abs((ticketPrice - item.price) / item.price) * 10_000 : 0,
      }))
      .sort((left, right) => left.distanceBps - right.distanceBps)
      .slice(0, 4)
  }, [bestAsk, bestBid, bookAskDepth, bookBidDepth, midPrice, recentPriceRange.high, spreadBps, ticketPrice])
  const depthHeatBands = useMemo(() => {
    if (!orderBook || selectedMarket?.kind === 'otc' || selectedMarket?.kind === 'earn') return []
    const askLevels = orderBook.asks.slice(0, 4).map((level) => ({ side: 'ask' as const, ...level }))
    const bidLevels = orderBook.bids.slice(0, 4).map((level) => ({ side: 'bid' as const, ...level }))
    const merged = [...askLevels.reverse(), ...bidLevels]
    const maxAmount = Math.max(...merged.map((level) => level.amount), 1)
    return merged.map((level, index) => ({
      ...level,
      intensity: Math.max(10, (level.amount / maxAmount) * 100),
      key: `${level.side}-${index}-${level.price}`,
    }))
  }, [orderBook, selectedMarket?.kind])
  const depthCurveData = useMemo(() => {
    if (!orderBook || selectedMarket?.kind === 'otc' || selectedMarket?.kind === 'earn') return []

    const topBids = orderBook.bids.slice(0, 24)
    const topAsks = orderBook.asks.slice(0, 24)

    let bidCum = 0
    const bidPoints = topBids.map((level) => {
      bidCum += level.amount
      return { price: Number(level.price.toFixed(priceDigits)), bid: bidCum, ask: null as number | null }
    })

    let askCum = 0
    const askPoints = topAsks.map((level) => {
      askCum += level.amount
      return { price: Number(level.price.toFixed(priceDigits)), bid: null as number | null, ask: askCum }
    })

    const merged = [...bidPoints, ...askPoints].sort((a, b) => a.price - b.price)
    const collapsed: Array<{ price: number; bid: number | null; ask: number | null }> = []
    for (const point of merged) {
      const previous = collapsed[collapsed.length - 1]
      if (previous && previous.price === point.price) {
        collapsed[collapsed.length - 1] = {
          price: point.price,
          bid: point.bid ?? previous.bid,
          ask: point.ask ?? previous.ask,
        }
      } else {
        collapsed.push(point)
      }
    }
    return collapsed
  }, [orderBook, priceDigits, selectedMarket?.kind])
  const tacticalCues = useMemo(
    () => [
      {
        label: '价位磁吸',
        value: priceMagnetZones[0] ? `${priceMagnetZones[0].label} 附近` : '等待价格',
        hint: priceMagnetZones[0] ? `距离 ${formatNumber(priceMagnetZones[0].distanceBps, 1)} bps` : '输入价格后生成',
      },
      {
        label: '排队判断',
        value: queuePressureEstimate?.posture ?? '等待估算',
        hint: queuePressureEstimate ? `前方 ${formatNumber(queuePressureEstimate.aheadVolume, amountDigits)}` : '需要盘口与价格',
      },
      {
        label: '吃单成本',
        value: sweepPreview ? `${formatNumber(sweepPreview.slippageBps, 2)} bps` : '待估算',
        hint: sweepPreview ? `均价 ${formatNumber(sweepPreview.averagePrice, priceDigits)}` : '仅标准订单簿品类支持',
      },
    ],
    [amountDigits, priceDigits, priceMagnetZones, queuePressureEstimate, sweepPreview],
  )
  const tapeSummary = useMemo(
    () => {
      if (!selectedTrade) return []
      const perspective = classifyTradePerspective(selectedTrade, userId)
      return [
        { label: '成交价', value: formatNumber(selectedTrade.price, priceDigits) },
        { label: '成交量', value: formatNumber(selectedTrade.amount, amountDigits) },
        { label: '归属', value: perspective.label ?? '市场成交' },
        { label: '对手', value: perspective.counterparty ?? '-' },
      ]
    },
    [amountDigits, priceDigits, selectedTrade, userId],
  )
  const orderWorkbench = useMemo(
    () =>
      selectedOrder
        ? [
            { label: '方向', value: selectedOrder.side.toUpperCase() },
            { label: '价格', value: formatNumber(selectedOrder.price, priceDigits) },
            { label: '剩余', value: formatNumber(selectedOrder.remaining, amountDigits) },
            { label: '状态', value: selectedOrder.status },
          ]
        : [],
    [amountDigits, priceDigits, selectedOrder],
  )
  const anomalyRadarData = useMemo(
    () => [
      { metric: '流动性', score: liquidityScore },
      { metric: '平衡度', score: Math.round(100 - Math.min(Math.abs(bookImbalance) * 3.2, 92)) },
      { metric: '执行', score: executionConfidenceScore },
      { metric: '安全', score: balanceSafetyScore },
      { metric: '稳定性', score: orderSource === 'api' ? (parsedLastResponseError ? 38 : 84) : 56 },
    ],
    [balanceSafetyScore, bookImbalance, executionConfidenceScore, liquidityScore, orderSource, parsedLastResponseError],
  )
  const anomalyFeed = useMemo(
    () => [
      {
        label: '点差状态',
        value: spreadBps >= 12 ? '异常偏宽' : spreadBps >= 5 ? '轻度扩张' : '正常紧凑',
        tone: spreadBps >= 12 ? 'danger' : spreadBps >= 5 ? 'neutral' : 'success',
      },
      {
        label: '盘口倾斜',
        value: Math.abs(bookImbalance) >= 18 ? '单边明显' : Math.abs(bookImbalance) >= 8 ? '轻度偏斜' : '相对平衡',
        tone: Math.abs(bookImbalance) >= 18 ? 'danger' : Math.abs(bookImbalance) >= 8 ? 'neutral' : 'success',
      },
      {
        label: '风控事件',
        value: riskEvents.length >= 6 ? '活跃' : riskEvents.length > 0 ? '可见' : '平静',
        tone: riskEvents.length >= 6 ? 'danger' : riskEvents.length > 0 ? 'neutral' : 'success',
      },
      {
        label: '链路状态',
        value: parsedLastResponseError ? '最近回执异常' : orderSource === 'api' ? '真实链路稳定' : '当前处于回退模式',
        tone: parsedLastResponseError ? 'danger' : orderSource === 'api' ? 'success' : 'neutral',
      },
    ],
    [bookImbalance, orderSource, parsedLastResponseError, riskEvents.length, spreadBps],
  )
  const memoryNarrative = useMemo(() => {
    const latest = memoryEvents[0]
    if (!latest) {
      return '当前还没有新的终端记忆；你的一次选价、一次预设或一次提交，都会在这里留下操作故事线。'
    }
    return `${latest.time} · ${latest.label}：${latest.detail}`
  }, [memoryEvents])
  const leftRailFillItems = useMemo(
    () => [
      {
        label: '当前焦点',
        value: selectedMarket ? selectedMarket.name : '未选市场',
        hint: selectedMarket ? `${kindLabel[selectedMarket.kind]} · ${selectedMarketStateLabel}` : '等待从左栏选择市场',
      },
      {
        label: '市场覆盖',
        value: `${filteredMarkets.length}/${markets.length}`,
        hint: onlyFavorites ? '当前仅看收藏' : marketSearch.trim() ? `搜索：${marketSearch.trim()}` : '当前无额外搜索',
      },
      {
        label: '链路状态',
        value: orderSource === 'api' ? '真实链路' : '本地回退',
        hint: liveBackendCount > 0 ? `${liveBackendCount} 个真实后端` : '当前为演示路径',
      },
      {
        label: '操作建议',
        value: filteredMarkets.length <= 1 ? '建议切换更多市场' : '可继续浏览市场',
        hint: focusLock ? '焦点已锁定' : '焦点未锁定',
      },
    ],
    [
      filteredMarkets.length,
      focusLock,
      kindLabel,
      liveBackendCount,
      marketSearch,
      markets.length,
      onlyFavorites,
      orderSource,
      selectedMarket,
      selectedMarketStateLabel,
    ],
  )
  const pulseDigestItems = useMemo(
    () => [
      {
        label: '短线变化',
        value: `${formatSignedNumber(recentPriceDeltaPct, 2)}%`,
      },
      {
        label: '盘口失衡',
        value: `${formatSignedNumber(bookImbalance, 2)}%`,
      },
      {
        label: '点差',
        value: spread ? `${formatNumber(spread, priceDigits)} / ${formatNumber(spreadBps, 2)}bps` : '-',
      },
      {
        label: '执行置信',
        value: `${executionConfidenceScore}/100`,
      },
    ],
    [bookImbalance, executionConfidenceScore, priceDigits, recentPriceDeltaPct, spread, spreadBps],
  )
  const focusSpotlight = useMemo(() => {
    if (selectedOrder) {
      return {
        eyebrow: 'Order Spotlight',
        title: `${selectedOrder.side.toUpperCase()} ${formatNumber(selectedOrder.remaining, amountDigits)} @ ${formatNumber(selectedOrder.price, priceDigits)}`,
        summary: `这笔挂单当前状态为 ${selectedOrder.status}，已成交 ${formatNumber(selectedOrder.filled, amountDigits)}，剩余 ${formatNumber(selectedOrder.remaining, amountDigits)}。`,
        bullets: [
          `如果预期改变，可直接撤单并重新定价。`,
          `若希望更快成交，可参考买一 / 卖一或中间价重新挂单。`,
          `继续观察底部动作回放，确认撤单与余额变化是否闭环。`,
        ],
      }
    }

    if (selectedTrade) {
      return {
        eyebrow: 'Tape Spotlight',
        title: `${formatNumber(selectedTrade.price, priceDigits)} @ ${formatNumber(selectedTrade.amount, amountDigits)}`,
        summary: `当前聚焦的成交由 ${selectedTrade.buyer ?? 'buyer'} 与 ${selectedTrade.seller ?? 'seller'} 完成，可用于快速带入价格或观察最新成交节奏。`,
        bullets: [
          `点击成交可沿着最新成交节奏修正价格。`,
          `若盘口正在收窄，优先看右侧执行预演和排队姿态。`,
          `若最近回执异常，先核对错误体，再决定是否继续追价。`,
        ],
      }
    }

    if (priceMagnetZones[0]) {
      return {
        eyebrow: 'Market Spotlight',
        title: `${priceMagnetZones[0].label} 附近存在价格吸引带`,
        summary: `当前输入价格距离 ${priceMagnetZones[0].label} 约 ${formatNumber(priceMagnetZones[0].distanceBps, 1)} bps，可作为调价参考。`,
        bullets: [
          `优先检查盘口热力带与深度区块是否同步支持。`,
          `若要保持 maker 姿态，尽量贴近而不直接跨价。`,
          `如果你还没开始操作，可从右侧一键策略预设起步。`,
        ],
      }
    }

    return {
      eyebrow: 'Session Spotlight',
      title: '等待下一次有效动作',
      summary: '你可以从左侧市场列表、中央盘口或右侧策略预设开始，让终端逐步形成完整的操作故事线。',
      bullets: [
        `先选市场，再确认右侧影响资产与可用余额。`,
        `再用盘口、成交或快捷填充把价格与数量带入票据。`,
        `最后看执行反馈、闭环摘要和底部动作回放。`,
      ],
    }
  }, [amountDigits, priceDigits, priceMagnetZones, selectedOrder, selectedTrade])
  const strategyPresets = useMemo(() => {
    const buyBalance = quoteBalance?.available ?? 0
    const sellBalance = baseBalance?.available ?? 0
    const safeBuyAmount = ticketPrice > 0 ? buyBalance / ticketPrice : 0
    const edgeAmount = form.side === 'buy' ? safeBuyAmount : sellBalance

    return [
      {
        id: 'maker-shadow',
        label: 'Maker Shadow',
        title: '贴近最优价挂单',
        detail: '尽量保持排队优先，同时不跨价。',
        nextPrice: form.side === 'buy' ? bestBid || ticketPrice : bestAsk || ticketPrice,
        nextAmount: ticketAmount || 0.25,
      },
      {
        id: 'mid-probe',
        label: 'Mid Probe',
        title: '用中间价做试探',
        detail: '适合先看回执、确认链路和市场反馈。',
        nextPrice: midPrice || ticketPrice,
        nextAmount: ticketAmount || 0.1,
      },
      {
        id: 'sweep-lite',
        label: 'Sweep Lite',
        title: '轻量扫单',
        detail: '用更主动价格快速验证成交路径。',
        nextPrice: form.side === 'buy' ? bestAsk || ticketPrice : bestBid || ticketPrice,
        nextAmount: Math.max(ticketAmount || 0, 0.25),
      },
      {
        id: 'balance-edge',
        label: 'Balance Edge',
        title: '贴边额度',
        detail: '以当前可用余额估算上限，用于边界压力测试。',
        nextPrice: ticketPrice || midPrice || bestAsk || bestBid || 0,
        nextAmount: edgeAmount > 0 ? edgeAmount : ticketAmount || 0.1,
      },
    ]
  }, [baseBalance?.available, bestAsk, bestBid, form.side, midPrice, quoteBalance?.available, ticketAmount, ticketPrice])
  const workspaceLanes = useMemo(
    () => [
      {
        label: '左侧',
        title: '市场与账户',
        hint: '先选品类、确认可交易状态，再看余额与动作回放。',
      },
      {
        label: '中间',
        title: '盘口与成交',
        hint: '围绕中价、点差、盘口深度和成交流做判断。',
      },
      {
        label: '右侧',
        title: '下单与确认',
        hint: '最后落到下单、余额影响、错误体和挂单管理。',
      },
    ],
    [],
  )
  const selectedTradeFacts = useMemo(
    () =>
      selectedTrade
        ? [
            { label: '成交价', value: formatNumber(selectedTrade.price, priceDigits) },
            { label: '成交量', value: formatNumber(selectedTrade.amount, amountDigits) },
            { label: '买方', value: selectedTrade.buyer ?? 'buyer' },
            { label: '卖方', value: selectedTrade.seller ?? 'seller' },
          ]
        : [],
    [amountDigits, priceDigits, selectedTrade],
  )
  const selectedOrderFacts = useMemo(
    () =>
      selectedOrder
        ? [
            { label: '方向', value: selectedOrder.side.toUpperCase() },
            { label: '价格', value: formatNumber(selectedOrder.price, priceDigits) },
            { label: '剩余', value: formatNumber(selectedOrder.remaining, amountDigits) },
            { label: '已成交', value: formatNumber(selectedOrder.filled, amountDigits) },
          ]
        : [],
    [amountDigits, priceDigits, selectedOrder],
  )
  const actionChecklist = useMemo(
    () => [
      {
        label: '市场状态',
        value: selectedMarketStateLabel,
        tone: selectedMarket?.tradingEnabled ? 'positive' : 'negative',
      },
      {
        label: '余额检查',
        value:
          affordabilityState === 'insufficient'
            ? '不足'
            : affordabilityState === 'full-use'
              ? '刚好用尽'
              : affordabilityState === 'safe'
                ? '充足'
                : '待确认',
        tone: affordabilityState === 'insufficient' ? 'negative' : affordabilityState === 'safe' ? 'positive' : 'neutral',
      },
      {
        label: '成交倾向',
        value: crossesSpread ? '更接近成交' : '更接近挂单',
        tone: crossesSpread ? 'positive' : 'neutral',
      },
    ],
    [affordabilityState, crossesSpread, selectedMarket?.tradingEnabled, selectedMarketStateLabel],
  )
  const sessionFocusItems = useMemo(
    () => [
      {
        label: '当前用户',
        value: session?.displayName ?? userId,
        hint: session?.role ?? 'trader',
      },
      {
        label: '当前市场',
        value: selectedMarket ? `${pair.base}/${pair.quote}` : '未选择',
        hint: selectedMarket?.id ?? '等待选择',
      },
      {
        label: '链路模式',
        value: orderSource === 'api' ? '真实接口' : '本地回退',
        hint: orderSource === 'api' ? '可直接观察真实返回' : '用于后端异常时不中断演示',
      },
    ],
    [orderSource, pair.base, pair.quote, selectedMarket, session?.displayName, session?.role, userId],
  )
  const activityMetrics = useMemo(
    () => [
      { label: '盘口档位', value: String((orderBook?.bids.length ?? 0) + (orderBook?.asks.length ?? 0)) },
      { label: '最近成交', value: String(trades.length) },
      { label: '用户成交', value: String(fills.length) },
      { label: '挂单数量', value: String(orders.length) },
    ],
    [fills.length, orderBook, orders.length, trades.length],
  )
  const tradeRangeMetrics = useMemo(() => {
    if (trades.length === 0) {
      return [
        { label: '最近区间', value: '-' },
        { label: '均笔数量', value: '-' },
        { label: '最新一笔', value: '-' },
      ]
    }
    const prices = trades.map((trade) => trade.price)
    const amounts = trades.map((trade) => trade.amount)
    const lastTrade = trades[0]
    const averageAmount = amounts.reduce((sum, amount) => sum + amount, 0) / amounts.length
    return [
      { label: '最近区间', value: `${formatNumber(Math.min(...prices), priceDigits)} - ${formatNumber(Math.max(...prices), priceDigits)}` },
      { label: '均笔数量', value: formatNumber(averageAmount, amountDigits) },
      { label: '最新一笔', value: `${formatNumber(lastTrade.price, priceDigits)} / ${formatNumber(lastTrade.amount, amountDigits)}` },
    ]
  }, [amountDigits, priceDigits, trades])
  const bookSummaryItems = useMemo(
    () => [
      { label: '价格精度', value: `${priceDigits} 位` },
      { label: '数量精度', value: `${amountDigits} 位` },
      { label: '盘口深度', value: `${orderBook?.bids.length ?? 0}/${orderBook?.asks.length ?? 0}` },
    ],
    [amountDigits, orderBook, priceDigits],
  )
  const ticketWorkflowItems = useMemo(
    () => [
      {
        label: '1. 设定方向',
        value: form.side === 'buy' ? '买入' : '卖出',
        hint: '先确定交易意图',
      },
      {
        label: '2. 检查资金',
        value:
          affordabilityState === 'insufficient'
            ? '余额不足'
            : affordabilityState === 'unknown'
              ? '待确认'
              : '可执行',
        hint: '确认扣减后可用余额',
      },
      {
        label: '3. 确认结果',
        value: parsedLastResponseError ? '先处理错误' : closureSummary ? '已有闭环' : '等待提交',
        hint: '提交后观察返回体和挂单变化',
      },
    ],
    [affordabilityState, closureSummary, form.side, parsedLastResponseError],
  )
  const orderStatusMetrics = useMemo(() => {
    const working = orders.filter((order) => !['filled', 'cancelled', 'rejected'].includes(order.status.toLowerCase())).length
    const partiallyFilled = orders.filter((order) => order.filled > 0 && order.remaining > 0).length
    const remainingTotal = orders.reduce((sum, order) => sum + order.remaining, 0)
    return [
      { label: '有效挂单', value: String(working) },
      { label: '部分成交', value: String(partiallyFilled) },
      { label: '剩余总量', value: formatNumber(remainingTotal, amountDigits) },
    ]
  }, [amountDigits, orders])
  const bookPressureMetrics = useMemo(() => {
    const totalDepth = bookBidDepth + bookAskDepth
    const bidShare = totalDepth > 0 ? (bookBidDepth / totalDepth) * 100 : 0
    const askShare = totalDepth > 0 ? (bookAskDepth / totalDepth) * 100 : 0
    return [
      { label: '买盘占比', value: `${formatNumber(bidShare, 1)}%` },
      { label: '卖盘占比', value: `${formatNumber(askShare, 1)}%` },
      { label: '总深度', value: formatNumber(totalDepth, amountDigits) },
    ]
  }, [amountDigits, bookAskDepth, bookBidDepth])
  const executionPreviewItems = useMemo(
    () => [
      {
        label: '提交动作',
        value: selectedMarket?.kind === 'otc' ? '发送报价' : selectedMarket?.kind === 'earn' ? (form.side === 'buy' ? '申购' : '赎回') : '发送订单',
      },
      {
        label: '预估结果',
        value: crossesSpread ? '更可能立即成交' : '更可能进入挂单簿',
      },
      {
        label: '关注回执',
        value: parsedLastResponseError ? '先处理错误体' : '提交后看返回体与余额变化',
      },
    ],
    [crossesSpread, form.side, parsedLastResponseError, selectedMarket?.kind],
  )
  const submitPreviewLine = useMemo(() => {
    if (!selectedMarket) return '等待选择市场'
    if (selectedMarket.kind === 'otc') {
      return `将以 ${form.side === 'buy' ? '买方' : '卖方'} 方向发送 OTC 报价，价格 ${formatNumber(ticketPrice, priceDigits)}，数量 ${formatNumber(ticketAmount, amountDigits)}。`
    }
    if (selectedMarket.kind === 'earn') {
      return `将以 ${form.side === 'buy' ? '申购' : '赎回'} 理财方式处理 ${formatNumber(ticketAmount, amountDigits)} ${pair.base}。`
    }
    return `将以 ${form.side === 'buy' ? '买入' : '卖出'} 方向提交 ${formatNumber(ticketAmount, amountDigits)}，价格 ${formatNumber(ticketPrice, priceDigits)}。`
  }, [amountDigits, form.side, pair.base, priceDigits, selectedMarket, ticketAmount, ticketPrice])
  const displayedAskRows = useMemo(() => {
    if (!orderBook) return []
    let cumulative = 0
    return orderBook.asks.slice(0, 8).map((level, index) => {
      cumulative += level.amount
      return { ...level, cumulative, levelIndex: index + 1 }
    }).reverse()
  }, [orderBook])
  const displayedBidRows = useMemo(() => {
    if (!orderBook) return []
    let cumulative = 0
    return orderBook.bids.slice(0, 8).map((level, index) => {
      cumulative += level.amount
      return { ...level, cumulative, levelIndex: index + 1 }
    })
  }, [orderBook])
  const focusTickSize = useMemo(() => (priceDigits > 0 ? 1 / Math.pow(10, priceDigits) : 1), [priceDigits])
  const focusPrice = useMemo(
    () => hoverLens?.price ?? hoveredTrade?.price ?? selectedTrade?.price ?? hoveredOrder?.price ?? selectedOrder?.price ?? (ticketPrice || null),
    [hoverLens?.price, hoveredOrder?.price, hoveredTrade?.price, selectedOrder?.price, selectedTrade?.price, ticketPrice],
  )
  const focusSourceLabel = useMemo(() => {
    if (hoverLens) return '盘口焦点'
    if (hoveredTrade) return '成交悬停'
    if (selectedTrade) return '已选成交'
    if (hoveredOrder) return '挂单悬停'
    if (selectedOrder) return '已选挂单'
    if (ticketPrice) return '当前票据'
    return '未锁定'
  }, [hoverLens, hoveredOrder, hoveredTrade, selectedOrder, selectedTrade, ticketPrice])
  const linkedBookCount = useMemo(
    () =>
      [...displayedAskRows, ...displayedBidRows].filter((level) => isNearPrice(level.price, focusPrice, focusTickSize, 2)).length,
    [displayedAskRows, displayedBidRows, focusPrice, focusTickSize],
  )
  const linkedTradeCount = useMemo(
    () => trades.filter((trade) => isNearPrice(trade.price, focusPrice, focusTickSize, 2)).length,
    [focusPrice, focusTickSize, trades],
  )
  const linkedOrderCount = useMemo(
    () => orders.filter((order) => isNearPrice(order.price, focusPrice, focusTickSize, 2)).length,
    [focusPrice, focusTickSize, orders],
  )
  const focusStripMetrics = useMemo(
    () => [
      { label: '焦点来源', value: focusSourceLabel },
      { label: '追踪价格', value: focusPrice ? formatNumber(focusPrice, priceDigits) : '-' },
      { label: '盘口联动', value: `${linkedBookCount} 行` },
      { label: '成交联动', value: `${linkedTradeCount} 行` },
      { label: '挂单联动', value: `${linkedOrderCount} 行` },
    ],
    [focusPrice, focusSourceLabel, linkedBookCount, linkedOrderCount, linkedTradeCount, priceDigits],
  )
  const orderBookIntelItems = useMemo(
    () => [
      {
        label: '点差状态',
        value: spread ? `${formatNumber(spreadBps, 2)} bps` : '-',
        hint: spreadBps >= 12 ? '当前偏宽，适合更谨慎挂单' : spreadBps >= 5 ? '中性可观察' : '盘口相对紧凑',
      },
      {
        label: '队列姿态',
        value: queuePressureEstimate?.posture ?? '等待估算',
        hint: queuePressureEstimate ? `前方 ${formatNumber(queuePressureEstimate.aheadVolume, amountDigits)}` : '输入价格与数量后生成',
      },
      {
        label: '焦点联动',
        value: focusPrice ? formatNumber(focusPrice, priceDigits) : '-',
        hint: `${linkedBookCount} 盘口 / ${linkedTradeCount} 成交 / ${linkedOrderCount} 挂单`,
      },
      {
        label: '深度倾斜',
        value: `${formatSignedNumber(bookImbalance, 2)}%`,
        hint: bookImbalance >= 8 ? '买盘更强' : bookImbalance <= -8 ? '卖盘更强' : '买卖盘相对均衡',
      },
    ],
    [
      amountDigits,
      bookImbalance,
      focusPrice,
      linkedBookCount,
      linkedOrderCount,
      linkedTradeCount,
      priceDigits,
      queuePressureEstimate,
      spread,
      spreadBps,
    ],
  )
  const executionContextItems = useMemo(
    () => [
      {
        label: '当前市场',
        value: selectedMarket ? `${pair.base}/${pair.quote}` : '未选择',
        hint: selectedMarket ? kindLabel[selectedMarket.kind] : '先选择交易品类',
      },
      {
        label: '交易状态',
        value: selectedMarketStateLabel,
        hint: orderSource === 'api' ? '真实链路' : '本地回退',
      },
      {
        label: '影响资产',
        value: impactAsset,
        hint:
          currentImpactAvailable === null
            ? '等待余额确认'
            : `当前可用 ${formatNumber(currentImpactAvailable, impactAsset === pair.quote ? quoteDigits : amountDigits)}`,
      },
      {
        label: '预计可用',
        value:
          projectedAvailable === null
            ? '-'
            : formatSignedNumber(projectedAvailable, impactAsset === pair.quote ? quoteDigits : amountDigits),
        hint: projectedAvailable !== null && projectedAvailable < 0 ? '提交后将不足' : '提交后余额预估',
      },
    ],
    [
      amountDigits,
      currentImpactAvailable,
      impactAsset,
      kindLabel,
      orderSource,
      pair.base,
      pair.quote,
      projectedAvailable,
      quoteDigits,
      selectedMarket,
      selectedMarketStateLabel,
    ],
  )
  const orderBookIntelNarrative = useMemo(() => {
    const spreadTone = spreadBps >= 12 ? '点差偏宽' : spreadBps >= 5 ? '点差中性' : '点差紧凑'
    const queueTone = queuePressureEstimate?.posture ?? '等待队列估算'
    const biasTone = bookImbalance >= 8 ? '买盘主导' : bookImbalance <= -8 ? '卖盘主导' : '盘口均衡'
    return `${spreadTone} · ${queueTone} · ${biasTone}`
  }, [bookImbalance, queuePressureEstimate, spreadBps])
  const executionGuidanceItems = useMemo(
    () =>
      executionPreviewItems.map((item) => ({
        ...item,
        hint:
          item.label === '提交动作'
            ? selectedMarket ? `${kindLabel[selectedMarket.kind]} 主线动作` : '等待市场选择'
            : item.label === '预估结果'
              ? closureSummary || '等待最新闭环'
              : parsedLastResponseError
                ? '优先检查错误体与余额变化'
                : '提交后查看订单、成交与余额联动',
      })),
    [closureSummary, executionPreviewItems, kindLabel, parsedLastResponseError, selectedMarket],
  )
  const sessionBridgeItems = useMemo(
    () => [
      {
        label: '最近记忆',
        value: memoryEvents[0]?.label ?? '待生成',
        hint: memoryEvents[0]?.time ?? '执行一次动作后生成',
      },
      {
        label: '工作区域',
        value: workspaceLanes[1]?.title ?? '中枢',
        hint: workspaceLanes[1]?.hint ?? '围绕盘口与执行推进',
      },
      {
        label: '异常状态',
        value: anomalyFeed[0]?.value ?? '正常',
        hint: anomalyFeed[3]?.value ?? '链路稳定',
      },
      {
        label: '动作闭环',
        value: closureSummary || '等待下一次提交',
        hint: executionStateLabel,
      },
    ],
    [anomalyFeed, closureSummary, executionStateLabel, memoryEvents, workspaceLanes],
  )
  const impactContextItems = useMemo(
    () => [
      {
        label: '余额判断',
        value: actionChecklist[1]?.value ?? '待确认',
        hint: projectedAvailable !== null && projectedAvailable < 0 ? '提交后将不足' : '当前仍可继续规划',
      },
      {
        label: '成交倾向',
        value: actionChecklist[2]?.value ?? '待确认',
        hint: queuePressureEstimate?.posture ?? '等待盘口估算',
      },
      {
        label: '闭环状态',
        value: ticketWorkflowItems[2]?.value ?? '等待提交',
        hint: ticketWorkflowItems[2]?.hint ?? '提交后查看返回体',
      },
      {
        label: '当前提示',
        value: notice,
        hint: parsedLastResponseError ? '最近一次返回包含错误' : '当前反馈稳定',
      },
    ],
    [actionChecklist, notice, parsedLastResponseError, projectedAvailable, queuePressureEstimate, ticketWorkflowItems],
  )
  const replayBridgeItems = useMemo(
    () => [
      {
        label: '最新记忆',
        value: memoryEvents[0]?.label ?? '等待触发',
        hint: memoryEvents[0]?.time ?? '点选盘口或提交动作后生成',
      },
      {
        label: '闭环状态',
        value: closureSummary || '等待下一次动作',
        hint: executionReadiness,
      },
      {
        label: '成交/挂单',
        value: `${fills.length} / ${orders.length}`,
        hint: '成交回放 / 当前挂单',
      },
      {
        label: '执行链路',
        value: orderSource === 'api' ? '真实接口' : '本地回退',
        hint: selectedMarket ? `${pair.base}/${pair.quote}` : '等待市场选择',
      },
    ],
    [closureSummary, executionReadiness, fills.length, memoryEvents, orderSource, orders.length, pair.base, pair.quote, selectedMarket],
  )
  const coverageBridgeItems = useMemo(
    () => [
      {
        label: '覆盖率',
        value: `${filteredMarkets.length}/${markets.length}`,
        hint: onlyFavorites ? '当前为收藏视图' : kindFilter === 'all' ? '全市场视图' : `${kindLabel[kindFilter]} 过滤`,
      },
      {
        label: '链路可用',
        value: `${liveBackendCount} 实盘 / ${tradableCount} 可交易`,
        hint: marketSearch.trim() ? `搜索：${marketSearch.trim()}` : '当前筛选无搜索词',
      },
      {
        label: '当前焦点',
        value: selectedMarket ? selectedMarket.name : '未选市场',
        hint: selectedMarket ? selectedMarketStateLabel : '等待选择',
      },
      {
        label: 'OTC / Earn',
        value: `${selectedOtcQuotes.length} / ${earnPositions.length}`,
        hint: '报价数 / 理财持仓数',
      },
    ],
    [
      earnPositions.length,
      filteredMarkets.length,
      kindFilter,
      kindLabel,
      liveBackendCount,
      marketSearch,
      markets.length,
      onlyFavorites,
      selectedMarket,
      selectedMarketStateLabel,
      selectedOtcQuotes.length,
      tradableCount,
    ],
  )
  const continuityDigestItems = useMemo(
    () => [
      {
        label: '余额变更',
        value: balanceChangeSummary ?? '暂无变化',
      },
      {
        label: '挂单变更',
        value: orderChangeSummary ?? '暂无变化',
      },
      {
        label: '成交回放',
        value: fillChangeSummary ?? '暂无新增',
      },
      {
        label: '终端提示',
        value: parsedLastResponseError ? parsedLastResponseError.code ?? '请检查错误体' : notice || executionStateLabel,
      },
    ],
    [balanceChangeSummary, executionStateLabel, fillChangeSummary, notice, orderChangeSummary, parsedLastResponseError],
  )
  const marketChartBridgeItems = useMemo(
    () => [
      {
        label: '价格轴',
        value:
          selectedMarket?.kind === 'earn'
            ? '本金 / 收益'
            : selectedMarket?.kind === 'otc'
              ? '报价规模'
              : priceChartSummary.latest === null
                ? '等待价格轨迹'
                : `${formatNumber(priceChartSummary.latest, priceDigits)} / ${formatSignedNumber(priceChartSummary.changePct ?? 0, 2)}%`,
        hint: selectedMarket?.kind === 'otc' ? `${visibleOtcQuotes.length} 条报价` : selectedMarket?.kind === 'earn' ? `${earnPositionStats.length} 条持仓` : '最新价 / 区间变化',
      },
      {
        label: '量能视角',
        value:
          selectedMarket?.kind === 'otc'
            ? visibleOtcQuotes.length === 0
              ? '-'
              : formatNumber(visibleOtcQuotes.reduce((sum, item) => sum + item.amount, 0), amountDigits)
            : selectedMarket?.kind === 'earn'
              ? formatNumber(earnTotalPrincipal, 6)
              : formatNumber(priceChartSummary.volume, amountDigits),
        hint: selectedMarket?.kind === 'otc' ? '可见报价总规模' : selectedMarket?.kind === 'earn' ? '当前本金总额' : '累计成交量',
      },
      {
        label: '参考锚点',
        value: midPrice ? `${formatNumber(midPrice, priceDigits)} / ${ticketPrice ? formatNumber(ticketPrice, priceDigits) : '-'}` : '-',
        hint: '中间价 / 当前票据价',
      },
      {
        label: '图表上下文',
        value: closureSummary || '等待下一次动作',
        hint: selectedMarket ? `${pair.base}/${pair.quote}` : '等待市场选择',
      },
    ],
    [
      amountDigits,
      closureSummary,
      earnPositionStats.length,
      earnTotalPrincipal,
      midPrice,
      pair.base,
      pair.quote,
      priceChartSummary.changePct,
      priceChartSummary.latest,
      priceChartSummary.volume,
      priceDigits,
      selectedMarket,
      ticketPrice,
      visibleOtcQuotes,
    ],
  )
  const accountMixBridgeItems = useMemo(
    () => [
      {
        label: '资产数量',
        value: String(balanceChartData.length),
        hint: '账户中可见资产项',
      },
      {
        label: '可用 / 冻结',
        value: `${formatNumber(totalAvailable, 6)} / ${formatNumber(totalHold, 6)}`,
        hint: '可用余额 / 挂单冻结',
      },
      {
        label: '回放状态',
        value: fills.length > 0 ? `${fills.length} 条成交` : '暂无成交',
        hint: fillChangeSummary ?? '等待真实撮合或演示动作',
      },
      {
        label: '账户闭环',
        value: closureSummary || '等待下一次动作',
        hint: balanceChangeSummary ?? '余额暂未变化',
      },
    ],
    [balanceChangeSummary, balanceChartData.length, closureSummary, fillChangeSummary, fills.length, totalAvailable, totalHold],
  )
  const quantLensBridgeItems = useMemo(
    () => [
      {
        label: '流动性分',
        value: `${liquidityScore}`,
        hint: orderBookIntelNarrative,
      },
      {
        label: '执行信心',
        value: `${executionConfidenceScore}`,
        hint: executionReadiness,
      },
      {
        label: '账户安全',
        value: `${balanceSafetyScore}`,
        hint: projectedAvailable !== null && projectedAvailable < 0 ? '提交后余额可能不足' : '余额侧仍可控',
      },
      {
        label: '异常闭环',
        value: parsedLastResponseError ? parsedLastResponseError.code ?? 'error' : 'none',
        hint: anomalyFeed.map((item) => item.value).slice(0, 2).join(' · '),
      },
    ],
    [
      anomalyFeed,
      balanceSafetyScore,
      executionConfidenceScore,
      executionReadiness,
      liquidityScore,
      orderBookIntelNarrative,
      parsedLastResponseError,
      projectedAvailable,
    ],
  )
  const ticketDigestItems = useMemo(
    () => [
      {
        label: '提交判断',
        value: executionReadiness,
        hint: needsSubmitConfirm ? '当前需要二次确认' : '可直接进入提交',
      },
      {
        label: '影响资产',
        value: impactAsset,
        hint: currentImpactAvailable === null ? '等待余额确认' : `当前可用 ${formatNumber(currentImpactAvailable, impactAsset === pair.quote ? quoteDigits : amountDigits)}`,
      },
      {
        label: '票据规模',
        value: ticketPrice ? `${formatNumber(ticketPrice, priceDigits)} × ${formatNumber(ticketAmount, amountDigits)}` : '-',
        hint: `名义价值 ${formatNumber(ticketNotional, quoteDigits)}`,
      },
      {
        label: '闭环状态',
        value: closureSummary || '等待下一次动作',
        hint: parsedLastResponseError ? parsedLastResponseError.code ?? '请检查错误体' : executionStateLabel,
      },
    ],
    [
      amountDigits,
      closureSummary,
      currentImpactAvailable,
      executionReadiness,
      executionStateLabel,
      impactAsset,
      needsSubmitConfirm,
      parsedLastResponseError,
      pair.quote,
      priceDigits,
      quoteDigits,
      ticketAmount,
      ticketNotional,
      ticketPrice,
    ],
  )
  const impactRailDigestItems = useMemo(
    () => [
      {
        label: '可用变化',
        value: projectedAvailable === null ? '-' : formatSignedNumber(projectedAvailable, impactAsset === pair.quote ? quoteDigits : amountDigits),
        hint: balanceChangeSummary ?? '等待余额侧反馈',
      },
      {
        label: '成交倾向',
        value: crossesSpread ? '更接近立刻成交' : '更接近挂单入簿',
        hint: queuePressureEstimate?.posture ?? '等待盘口估算',
      },
      {
        label: '执行提示',
        value: notice || executionStateLabel,
        hint: orderSource === 'api' ? '真实接口链路' : '本地回退模式',
      },
    ],
    [
      amountDigits,
      balanceChangeSummary,
      crossesSpread,
      executionStateLabel,
      impactAsset,
      notice,
      orderSource,
      pair.quote,
      projectedAvailable,
      queuePressureEstimate,
      quoteDigits,
    ],
  )
  const orderRailDigestItems = useMemo(
    () => [
      {
        label: '选中挂单',
        value: selectedOrder ? selectedOrder.status : '未选择',
        hint: selectedOrder ? `${selectedOrder.side.toUpperCase()} @ ${formatNumber(selectedOrder.price, priceDigits)}` : '点击下方挂单行聚焦',
      },
      {
        label: '挂单变化',
        value: orderChangeSummary || '暂无变更',
        hint: `${orders.length} 条当前挂单`,
      },
      {
        label: '联动焦点',
        value: focusPrice ? formatNumber(focusPrice, priceDigits) : '-',
        hint: `${linkedBookCount} 盘口 / ${linkedTradeCount} 成交 / ${linkedOrderCount} 挂单`,
      },
    ],
    [
      focusPrice,
      linkedBookCount,
      linkedOrderCount,
      linkedTradeCount,
      orderChangeSummary,
      orders.length,
      priceDigits,
      selectedOrder,
    ],
  )
  const telemetryDigestItems = useMemo(
    () => [
      {
        label: '风控快照',
        value: `${fundingRates.length} / ${riskEvents.length}`,
        hint: '资金费率 / 风险事件',
      },
      {
        label: '最慢链路',
        value:
          latencyDashboard
            .slice()
            .sort((left, right) => (right.p95 ?? -1) - (left.p95 ?? -1))[0]?.label ?? '等待样本',
        hint: '按 p95 排序',
      },
      {
        label: '总体状态',
        value: parsedLastResponseError ? '需排查' : '稳定',
        hint: parsedLastResponseError ? parsedLastResponseError.code ?? '存在错误返回' : '最近链路无错误体',
      },
    ],
    [fundingRates.length, latencyDashboard, parsedLastResponseError, riskEvents.length],
  )
  const responseDigestItems = useMemo(
    () => [
      {
        label: '返回体状态',
        value: lastResponse ? '已有返回' : '等待动作',
        hint: parsedLastResponseError ? '最近一次返回带错误体' : '最近一次返回可用于复盘',
      },
      {
        label: '动作日志',
        value: `${actionLog.length} 条`,
        hint: actionLog[0]?.title ?? '等待下一次动作',
      },
      {
        label: '终端记忆',
        value: `${memoryEvents.length} 条`,
        hint: memoryEvents[0]?.label ?? '等待形成故事线',
      },
    ],
    [actionLog, lastResponse, memoryEvents, parsedLastResponseError],
  )
  const responseSummaryItems = useMemo(
    () => [
      { label: '执行状态', value: executionStateLabel },
      { label: '链路模式', value: orderSource === 'api' ? 'API' : 'Fallback' },
      { label: '错误代码', value: parsedLastResponseError?.code ?? 'none' },
      { label: '动作日志', value: String(actionLog.length) },
    ],
    [actionLog.length, executionStateLabel, orderSource, parsedLastResponseError?.code],
  )
  const marketHeroAsideMetrics = useMemo(
    () => [
      { label: '执行状态', value: executionStateLabel },
      { label: '余额检查', value: projectedAvailable === null ? '待确认' : projectedAvailable < 0 ? '不足' : '可执行' },
      { label: '我的挂单', value: String(orders.length) },
      { label: '当前票据', value: ticketPrice ? `${formatNumber(ticketPrice, priceDigits)} × ${formatNumber(ticketAmount, amountDigits)}` : '-' },
    ],
    [amountDigits, executionStateLabel, orders.length, priceDigits, projectedAvailable, ticketAmount, ticketPrice],
  )
  const marketHeroLinkMetrics = useMemo(
    () => [
      { label: '买盘深度', value: formatNumber(bookBidDepth, 6), tone: 'bid' as const },
      { label: '卖盘深度', value: formatNumber(bookAskDepth, 6), tone: 'ask' as const },
      { label: '深度失衡', value: `${formatSignedNumber(bookImbalance, 2)}%`, tone: bookImbalance >= 0 ? 'bid' as const : 'ask' as const },
      { label: '最新闭环', value: closureSummary || '等待下一次动作', tone: 'neutral' as const },
    ],
    [bookAskDepth, bookBidDepth, bookImbalance, closureSummary],
  )
  const selectedTradePerspective = useMemo(
    () => (selectedTrade ? classifyTradePerspective(selectedTrade, userId) : null),
    [selectedTrade, userId],
  )

  const exportSession = useCallback(() => {
    const payload = {
      exportedAt: new Date().toISOString(),
      userId,
      selectedMarket: selectedMarket
        ? { id: selectedMarket.id, name: selectedMarket.name, kind: selectedMarket.kind }
        : null,
      ui: {
        densityMode,
        showBrief,
        showInsights,
        kindFilter,
        marketSearch,
        onlyFavorites,
        favorites: favoriteMarketIds,
      },
      form,
      notice,
      closureSummary,
      actionLog,
      memoryEvents,
      lastResponse,
      latencySamples,
    }

    const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json;charset=utf-8' })
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = `terminal-session-${new Date().toISOString().replace(/[:.]/g, '-')}.json`
    document.body.appendChild(anchor)
    anchor.click()
    anchor.remove()
    URL.revokeObjectURL(url)
    appendMemory('会话导出', '已导出终端会话 JSON（用于复盘/审计/分享问题复现）。')
  }, [
    actionLog,
    appendMemory,
    closureSummary,
    densityMode,
    favoriteMarketIds,
    form,
    kindFilter,
    latencySamples,
    lastResponse,
    marketSearch,
    memoryEvents,
    notice,
    onlyFavorites,
    showBrief,
    showInsights,
    selectedMarket,
    userId,
  ])

  const copyLastResponse = useCallback(async () => {
    const text = lastResponse ? pretty(lastResponse) : ''
    if (!text) {
      setNotice('暂无可复制的返回体。')
      return
    }
    const ok = await copyToClipboard(text)
    setNotice(ok ? '已复制最近一次返回体。' : '复制失败（请检查浏览器权限）。')
    appendMemory('复制', ok ? '已复制最近返回体。' : '复制失败。')
  }, [appendMemory, lastResponse])

  const copyErrorBody = useCallback(async () => {
    const payload = parsedLastResponseError ? (lastResponse ?? parsedLastResponseError) : lastResponse
    const text = payload ? pretty(payload) : ''
    if (!text) {
      setNotice('暂无可复制的错误体。')
      return
    }
    const ok = await copyToClipboard(text)
    setNotice(ok ? '已复制错误体。' : '复制失败（请检查浏览器权限）。')
    appendMemory('复制', ok ? '已复制错误体。' : '复制失败。')
  }, [appendMemory, lastResponse, parsedLastResponseError])

  const terminalCommands = useMemo<TerminalCommand[]>(() => {
    const hasMarket = Boolean(selectedMarket)
    const isTradableKind = selectedMarket?.kind !== 'otc' && selectedMarket?.kind !== 'earn'

    return [
      {
        id: 'refresh',
        label: '刷新全链路数据',
        detail: '重新拉取市场、盘口、成交、挂单、余额与风控快照。',
        shortcut: 'R',
        run: () => void refreshAll(),
      },
      {
        id: 'toggle-brief',
        label: showBrief ? '隐藏终端概览（Brief）' : '显示终端概览（Brief）',
        detail: '把“Smart Brief / Signal Matrix / Execution Modes”折叠起来，首屏更像专业交易终端。',
        run: () => setShowBrief((current) => !current),
      },
      {
        id: 'toggle-insights',
        label: showInsights ? '隐藏洞察面板（Insights）' : '显示洞察面板（Insights）',
        detail: '控制“价位磁场 / 脉冲 / 体征卡片”等高级信息密度，让首屏更清爽。',
        run: () => setShowInsights((current) => !current),
      },
      {
        id: 'toggle-side',
        label: '切换买入 / 卖出',
        detail: '在不改价格与数量的前提下切换方向。',
        shortcut: 'B / S',
        disabled: !hasMarket,
        run: () => setForm((current) => ({ ...current, side: current.side === 'buy' ? 'sell' : 'buy' })),
      },
      {
        id: 'fill-best-bid',
        label: '带入买一价格',
        detail: '把票据价格贴近买一，适合 maker 试探。',
        shortcut: '1',
        disabled: !hasMarket || !bestBid,
        run: () => applyQuickFill('bestBid'),
      },
      {
        id: 'fill-best-ask',
        label: '带入卖一价格',
        detail: '把票据价格贴近卖一，适合 maker 试探。',
        shortcut: '2',
        disabled: !hasMarket || !bestAsk,
        run: () => applyQuickFill('bestAsk'),
      },
      {
        id: 'fill-mid',
        label: '带入中间价',
        detail: '把票据价格锚定中间价，用于回执与闭环验证。',
        shortcut: '3',
        disabled: !hasMarket || !midPrice,
        run: () => applyQuickFill('mid'),
      },
      {
        id: 'fill-max',
        label: '填充最大数量（按余额）',
        detail: '按当前方向与余额估算最大下单数量。',
        shortcut: '4',
        disabled: !hasMarket,
        run: () => applyQuickFill('max'),
      },
      {
        id: 'submit',
        label: selectedMarket?.kind === 'otc' ? '提交 OTC 报价' : selectedMarket?.kind === 'earn' ? '提交理财动作' : '提交订单',
        detail: '执行当前票据参数，并观察回执 / 余额 / 挂单闭环变化。',
        shortcut: 'Enter',
        disabled: !hasMarket || isSubmitting,
        run: () => void handleSubmitIntent(),
      },
      {
        id: 'cancel-selected',
        label: '撤销当前选中挂单',
        detail: selectedOrder ? `撤销 ${selectedOrder.id}，并观察闭环摘要变化。` : '先在右侧选择一条挂单作为焦点。',
        shortcut: '在挂单列表中选择',
        disabled: !hasMarket || !selectedOrder || isSubmitting,
        run: () => (selectedOrder ? void cancelOrder(selectedOrder.id) : undefined),
      },
      {
        id: 'demo-counterparty',
        label: '生成对手单（演示）',
        detail: '注入一个对手方向订单，用于验证真实撮合与成交回放。',
        shortcut: '右侧按钮',
        disabled: !hasMarket || !isTradableKind || isSubmitting,
        run: () => void runCounterpartyDemo(),
      },
      {
        id: 'demo-selftrade',
        label: '生成自成交（演示）',
        detail: '同账户对向下单，用于观察自成交防护与错误体。',
        shortcut: '右侧按钮',
        disabled: !hasMarket || !isTradableKind || isSubmitting,
        run: () => void runSelfTradeDemo(),
      },
      {
        id: 'toggle-density',
        label: densityMode === 'compact' ? '切换到舒适密度' : '切换到紧凑密度',
        detail: '影响盘口/成交/挂单表格的行距与字号密度，并自动记忆你的偏好。',
        shortcut: 'UI 按钮',
        disabled: false,
        run: () => setDensityMode((current) => (current === 'compact' ? 'comfortable' : 'compact')),
      },
      {
        id: 'toggle-favorites',
        label: onlyFavorites ? '显示全部市场' : '仅看收藏市场',
        detail: '在左侧市场列表中开启/关闭“仅看收藏”。',
        shortcut: 'UI 按钮',
        disabled: false,
        run: () => setOnlyFavorites((current) => !current),
      },
      {
        id: 'toggle-focus-lock',
        label: focusLock ? '解锁市场焦点' : '锁定市场焦点',
        detail: '锁定后，即使你在左侧搜索/筛选，当前选中市场也不会被自动切换。',
        shortcut: 'UI 按钮',
        disabled: false,
        run: () => setFocusLock((current) => !current),
      },
      {
        id: 'clear-market-search',
        label: '清空市场搜索',
        detail: '清空左侧市场搜索条件。',
        shortcut: 'UI 按钮',
        disabled: marketSearch.trim().length === 0,
        run: () => clearMarketSearch(),
      },
      {
        id: 'export-session',
        label: '导出终端会话',
        detail: '导出 action log / memory / last response / latency 等上下文，用于复盘或报障。',
        shortcut: 'Footer 按钮',
        disabled: false,
        run: () => exportSession(),
      },
      {
        id: 'copy-share-link',
        label: '复制分享链接',
        detail: '复制当前市场 + 票据参数的链接，用于复现或分享当前上下文。',
        shortcut: 'UI 按钮',
        disabled: !hasMarket,
        run: () => void copyShareLink(),
      },
    ]
  }, [
    applyQuickFill,
    bestAsk,
    bestBid,
    cancelOrder,
    clearMarketSearch,
    copyShareLink,
    densityMode,
    exportSession,
    focusLock,
    handleSubmitIntent,
    isSubmitting,
    marketSearch,
    midPrice,
    onlyFavorites,
    refreshAll,
    runCounterpartyDemo,
    runSelfTradeDemo,
    selectedMarket,
    selectedOrder,
    showBrief,
    showInsights,
  ])

  const filteredCommands = useMemo(() => {
    const query = paletteQuery.trim().toLowerCase()
    if (!query) return terminalCommands
    return terminalCommands.filter((command) => `${command.label} ${command.detail}`.toLowerCase().includes(query))
  }, [paletteQuery, terminalCommands])

  const pinnedCommandIds = useMemo(
    () => [
      'refresh',
      'submit',
      'cancel-selected',
      'toggle-brief',
      'toggle-insights',
      'toggle-density',
      'toggle-favorites',
      'toggle-focus-lock',
      'export-session',
      'copy-share-link',
    ],
    [],
  )
  const [recentCommandIds, setRecentCommandIds] = useState<string[]>(() => {
    try {
      const raw = window.localStorage.getItem('terminal:recentCommands')
      const parsed = raw ? (JSON.parse(raw) as unknown) : []
      if (Array.isArray(parsed)) {
        return parsed.filter((item) => typeof item === 'string').slice(0, 8) as string[]
      }
    } catch {
      // ignore
    }
    return []
  })

  const addRecentCommand = useCallback((commandId: string) => {
    setRecentCommandIds((current) => {
      const next = [commandId, ...current.filter((id) => id !== commandId)].slice(0, 8)
      try {
        window.localStorage.setItem('terminal:recentCommands', JSON.stringify(next))
      } catch {
        // ignore
      }
      return next
    })
  }, [])

  const paletteSections = useMemo(() => {
    if (paletteQuery.trim().length > 0) {
      return [{ id: 'search', title: `搜索结果 (${filteredCommands.length})`, items: filteredCommands }]
    }

    const byId = new Map(terminalCommands.map((command) => [command.id, command]))
    const pinned = pinnedCommandIds.map((id) => byId.get(id)).filter(Boolean) as TerminalCommand[]
    const recent = recentCommandIds.map((id) => byId.get(id)).filter(Boolean) as TerminalCommand[]
    const pinnedSet = new Set(pinned.map((item) => item.id))
    const recentSet = new Set(recent.map((item) => item.id))
    const rest = terminalCommands.filter((item) => !pinnedSet.has(item.id) && !recentSet.has(item.id))

    return [
      { id: 'pinned', title: '常用', items: pinned },
      { id: 'recent', title: '最近', items: recent },
      { id: 'all', title: `全部命令 (${rest.length})`, items: rest },
    ].filter((section) => section.items.length > 0)
  }, [filteredCommands, paletteQuery, pinnedCommandIds, recentCommandIds, terminalCommands])

  const paletteItems = useMemo(() => paletteSections.flatMap((section) => section.items), [paletteSections])
  const paletteItemsRef = useRef(paletteItems)
  useEffect(() => {
    paletteItemsRef.current = paletteItems
    setPaletteIndex((current) => Math.min(current, Math.max(paletteItems.length - 1, 0)))
  }, [paletteItems])

  function applyStrategyPreset(presetId: string) {
    const preset = strategyPresets.find((item) => item.id === presetId)
    if (!preset) return

    setForm((current) => ({
      ...current,
      price: preset.nextPrice > 0 ? String(Number(preset.nextPrice.toFixed(priceDigits))) : current.price,
      amount: preset.nextAmount > 0 ? String(Number(preset.nextAmount.toFixed(amountDigits))) : current.amount,
    }))
    setNotice(`已应用策略预设：${preset.title}`)
    appendLog('策略预设', `${preset.label} 已带入价格和数量。`, 'success')
    appendMemory('策略预设', `${preset.title} 已带入 ${Number(preset.nextPrice.toFixed(priceDigits))} / ${Number(preset.nextAmount.toFixed(amountDigits))}`)
  }

  const loadOverview = useCallback(async () => {
    setIsLoading(true)
    const start = performance.now()
    try {
      const [marketResult, nextBalances, nextQuotes, nextEarn, nextFunding, nextRisk, nextFills] = await Promise.all([
        exchangeAPI.getMarkets(),
        exchangeAPI.getBalances(userId).catch(() => []),
        exchangeAPI.listOtcQuotes().catch(() => []),
        exchangeAPI.getEarnPositions(userId).catch(() => []),
        exchangeAPI.listFundingRates().catch(() => []),
        exchangeAPI.listRiskEvents(10).catch(() => []),
        exchangeAPI.getFills(userId).catch(() => []),
      ])
      setMarkets(marketResult.items)
      setBalances(nextBalances)
      setOtcQuotes(nextQuotes)
      setEarnPositions(nextEarn)
      setFundingRates(nextFunding)
      setRiskEvents(nextRisk)
      setFills(nextFills)
      setSelectedMarketId((current) => current || marketResult.items[0]?.id || '')
    } finally {
      recordLatency('loadOverview', performance.now() - start)
      setIsLoading(false)
    }
  }, [recordLatency, userId])

  const loadSelectedMarket = useCallback(async () => {
    if (!selectedMarket) return
    const start = performance.now()

    if (selectedMarket.kind === 'otc' || selectedMarket.kind === 'earn') {
      setOrderBook(null)
      setTrades([])
    } else {
      const [nextBook, nextTrades] = await Promise.all([
        exchangeAPI.getOrderBook(selectedMarket.id, 0).catch(() => null),
        exchangeAPI.getTrades(selectedMarket.id, 20).catch(() => ({ items: [], source: 'mock' as const })),
      ])
      setOrderBook(nextBook)
      setTrades(nextTrades.items)
    }

    setOrders(await exchangeAPI.getOrders(userId, selectedMarket.id).catch(() => []))
    recordLatency('loadSelectedMarket', performance.now() - start)
  }, [selectedMarket, userId])

  useEffect(() => {
    void loadOverview()
  }, [loadOverview])

  useEffect(() => {
    if (didApplyShareLinkRef.current) return
    didApplyShareLinkRef.current = true

    const hash = window.location.hash
    const queryIndex = hash.indexOf('?')
    if (queryIndex < 0) return

    const query = hash.slice(queryIndex + 1)
    const params = new URLSearchParams(query)
    const marketId = params.get('m')
    const side = params.get('side')
    const price = params.get('p')
    const amount = params.get('a')
    const outcome = params.get('o')
    const kind = params.get('k')
    const fav = params.get('fav')
    const lock = params.get('lock')
    const density = params.get('d')
    const brief = params.get('brief')
    const insights = params.get('ins')

    if (kind && kindOptions.some((opt) => opt.value === kind)) {
      setKindFilter(kind as KindFilter)
    }
    if (fav === '1') setOnlyFavorites(true)
    if (lock === '1') setFocusLock(true)
    if (density === 'c') setDensityMode('comfortable')
    if (brief === '1') setShowBrief(true)
    if (insights === '1') setShowInsights(true)

    if (marketId) {
      setSelectedMarketId(marketId)
    }

    setForm((current) => ({
      ...current,
      side: side === 'sell' ? 'sell' : 'buy',
      price: price ?? current.price,
      amount: amount ?? current.amount,
      outcome: outcome ?? current.outcome,
    }))
    appendMemory('分享链接', '已从分享链接加载市场与票据参数。')
  }, [appendMemory])

  useEffect(() => {
    if (!selectedMarketId) {
      setSelectedMarketId(filteredMarkets[0]?.id ?? '')
      return
    }

    const existsInAllMarkets = markets.some((item) => item.id === selectedMarketId)
    if (!existsInAllMarkets) {
      setSelectedMarketId(filteredMarkets[0]?.id ?? '')
      return
    }

    if (focusLock) {
      return
    }

    if (!filteredMarkets.some((item) => item.id === selectedMarketId)) {
      setSelectedMarketId(filteredMarkets[0]?.id ?? '')
    }
  }, [filteredMarkets, focusLock, markets, selectedMarketId])

  useEffect(() => {
    void loadSelectedMarket()
  }, [loadSelectedMarket])

  useEffect(() => {
    if (selectedMarket) {
      setForm((current) => ({ ...current, outcome: String(selectedMarket.outcomes[0] ?? 0) }))
    }
  }, [selectedMarket])

  useEffect(() => {
    if (!trades.some((trade) => trade.id === selectedTradeId)) {
      setSelectedTradeId(trades[0]?.id ?? null)
    }
  }, [selectedTradeId, trades])

  useEffect(() => {
    if (hoveredTradeId && !trades.some((trade) => trade.id === hoveredTradeId)) {
      setHoveredTradeId(null)
    }
  }, [hoveredTradeId, trades])

  useEffect(() => {
    if (!orders.some((order) => order.id === selectedOrderId)) {
      setSelectedOrderId(orders[0]?.id ?? null)
    }
  }, [orders, selectedOrderId])

  useEffect(() => {
    if (hoveredOrderId && !orders.some((order) => order.id === hoveredOrderId)) {
      setHoveredOrderId(null)
    }
  }, [hoveredOrderId, orders])

  useEffect(() => {
    if (!submitArmed) return
    setSubmitArmed(false)
    if (submitArmTimerRef.current) {
      window.clearTimeout(submitArmTimerRef.current)
      submitArmTimerRef.current = null
    }
  }, [form.amount, form.outcome, form.price, form.side, selectedMarketId])

  useEffect(() => {
    return () => {
      if (submitArmTimerRef.current) {
        window.clearTimeout(submitArmTimerRef.current)
        submitArmTimerRef.current = null
      }
    }
  }, [])

  useEffect(
    () => () => {
      if (balanceHighlightTimerRef.current) clearTimeout(balanceHighlightTimerRef.current)
      if (orderHighlightTimerRef.current) clearTimeout(orderHighlightTimerRef.current)
      if (fillHighlightTimerRef.current) clearTimeout(fillHighlightTimerRef.current)
    },
    [],
  )

  useEffect(() => {
    const nextSnapshot = new Map(balances.map((item) => [item.asset, { available: item.available, hold: item.hold }]))
    const previousSnapshot = balanceSnapshotRef.current

    if (!previousSnapshot) {
      balanceSnapshotRef.current = nextSnapshot
      return
    }

    const changedAssets: string[] = []
    const summaryParts: string[] = []

    for (const item of balances) {
      const previous = previousSnapshot.get(item.asset)
      if (!previous) {
        changedAssets.push(item.asset)
        summaryParts.push(`${item.asset} 新增可用 ${formatNumber(item.available, 6)}`)
        continue
      }

      const availableDelta = item.available - previous.available
      const holdDelta = item.hold - previous.hold
      if (Math.abs(availableDelta) > 1e-9 || Math.abs(holdDelta) > 1e-9) {
        changedAssets.push(item.asset)
        const parts: string[] = []
        if (Math.abs(availableDelta) > 1e-9) parts.push(`可用 ${formatSignedNumber(availableDelta, 6)}`)
        if (Math.abs(holdDelta) > 1e-9) parts.push(`冻结 ${formatSignedNumber(holdDelta, 6)}`)
        summaryParts.push(`${item.asset} ${parts.join(' / ')}`)
      }
    }

    if (changedAssets.length > 0) {
      setHighlightedBalances(changedAssets)
      setBalanceChangeSummary(`余额变化：${summaryParts.slice(0, 3).join(' · ')}`)
      if (balanceHighlightTimerRef.current) clearTimeout(balanceHighlightTimerRef.current)
      balanceHighlightTimerRef.current = window.setTimeout(() => setHighlightedBalances([]), 2600)
    }

    balanceSnapshotRef.current = nextSnapshot
  }, [balances])

  useEffect(() => {
    const nextSnapshot = new Map(
      orders.map((order) => [
        order.id,
        {
          remaining: order.remaining,
          filled: order.filled,
          status: order.status,
        },
      ]),
    )
    const previousSnapshot = orderSnapshotRef.current

    if (!previousSnapshot) {
      orderSnapshotRef.current = nextSnapshot
      return
    }

    let added = 0
    let updated = 0
    let removed = 0
    const changedIds: string[] = []

    for (const order of orders) {
      const previous = previousSnapshot.get(order.id)
      if (!previous) {
        added += 1
        changedIds.push(order.id)
        continue
      }

      if (
        Math.abs(order.remaining - previous.remaining) > 1e-9 ||
        Math.abs(order.filled - previous.filled) > 1e-9 ||
        order.status !== previous.status
      ) {
        updated += 1
        changedIds.push(order.id)
      }
    }

    for (const orderId of previousSnapshot.keys()) {
      if (!nextSnapshot.has(orderId)) removed += 1
    }

    if (added > 0 || updated > 0 || removed > 0) {
      const summaryParts = [
        added > 0 ? `新增 ${added}` : null,
        updated > 0 ? `更新 ${updated}` : null,
        removed > 0 ? `移除 ${removed}` : null,
      ].filter(Boolean)
      setOrderChangeSummary(`挂单变化：${summaryParts.join(' · ')}`)
      if (changedIds.length > 0) {
        setHighlightedOrders(changedIds)
        if (orderHighlightTimerRef.current) clearTimeout(orderHighlightTimerRef.current)
        orderHighlightTimerRef.current = window.setTimeout(() => setHighlightedOrders([]), 2600)
      }
    }

    orderSnapshotRef.current = nextSnapshot
  }, [orders])

  useEffect(() => {
    const nextSnapshot = new Set(fills.map((fill) => fill.id))
    const previousSnapshot = fillSnapshotRef.current

    if (!previousSnapshot) {
      fillSnapshotRef.current = nextSnapshot
      return
    }

    const newFillIds = fills.filter((fill) => !previousSnapshot.has(fill.id)).map((fill) => fill.id)
    if (newFillIds.length > 0) {
      setHighlightedFills(newFillIds)
      setFillChangeSummary(`成交回放新增 ${newFillIds.length} 条`)
      if (fillHighlightTimerRef.current) clearTimeout(fillHighlightTimerRef.current)
      fillHighlightTimerRef.current = window.setTimeout(() => setHighlightedFills([]), 2600)
    }

    fillSnapshotRef.current = nextSnapshot
  }, [fills])

  useEffect(() => {
    function handlePaletteKeydown(event: KeyboardEvent) {
      const key = event.key.toLowerCase()
      const wantsPalette = (event.ctrlKey || event.metaKey) && key === 'k'

      if (wantsPalette) {
        event.preventDefault()
        setPaletteOpen((current) => {
          const next = !current
          if (next) {
            setPaletteQuery('')
            setPaletteIndex(0)
          }
          return next
        })
        return
      }

      if (!paletteOpen) return

      if (event.key === 'Escape') {
        event.preventDefault()
        setPaletteOpen(false)
        return
      }

      if (event.key === 'ArrowDown') {
        event.preventDefault()
        setPaletteIndex((current) => Math.min(current + 1, Math.max(paletteItemsRef.current.length - 1, 0)))
        return
      }

      if (event.key === 'ArrowUp') {
        event.preventDefault()
        setPaletteIndex((current) => Math.max(current - 1, 0))
        return
      }

      if (event.key === 'Enter') {
        const command = paletteItemsRef.current[paletteIndex]
        if (!command || command.disabled) return
        event.preventDefault()
        void command.run()
        appendMemory('指令面板', `执行：${command.label}`)
        addRecentCommand(command.id)
        setPaletteOpen(false)
      }
    }

    document.addEventListener('keydown', handlePaletteKeydown)
    return () => {
      document.removeEventListener('keydown', handlePaletteKeydown)
    }
  }, [addRecentCommand, appendMemory, paletteIndex, paletteOpen])

  useEffect(() => {
    if (!paletteOpen) return
    const timer = window.setTimeout(() => {
      paletteInputRef.current?.focus()
    }, 30)
    return () => window.clearTimeout(timer)
  }, [paletteOpen])

  async function refreshAll() {
    await loadOverview()
    await loadSelectedMarket()
  }

  function selectBookLevel(price: number, amount: number, side: OrderSide, sourceLabel: string) {
    setForm((current) => ({
      ...current,
      price: String(price),
      amount: String(amount),
      side,
    }))
    setNotice(`已从${sourceLabel}带入 ${formatNumber(price, priceDigits)} / ${formatNumber(amount, amountDigits)}，并切换为${side === 'buy' ? '买入' : '卖出'}。`)
    appendMemory('盘口选价', `${sourceLabel} → ${side.toUpperCase()} ${formatNumber(amount, amountDigits)} @ ${formatNumber(price, priceDigits)}`)
  }

  function updateHoverLens(next: HoverLensState | null) {
    setHoverLens(next)
  }

  function selectTradePrice(price: number) {
    setForm((current) => ({
      ...current,
      price: String(price),
    }))
    setNotice(`已从最近成交带入价格 ${formatNumber(price, priceDigits)}；数量保持不变。`)
    appendMemory('成交跟价', `带入最近成交价 ${formatNumber(price, priceDigits)}，数量保持不变`)
  }

  function selectOtcQuote(quote: OtcQuoteRecord) {
    setForm((current) => ({
      ...current,
      price: String(quote.price),
      amount: String(quote.amount),
      side: quote.side === 'buy' ? 'sell' : 'buy',
      outcome: String(quote.outcome),
    }))
    setNotice(`已带入 OTC 报价 ${formatNumber(quote.price, priceDigits)} / ${formatNumber(quote.amount, amountDigits)}，当前方向切换为${quote.side === 'buy' ? '卖出应答' : '买入应答'}。`)
    appendMemory('OTC 报价', `选择 ${quote.side.toUpperCase()} 报价，反向应答 ${formatNumber(quote.amount, amountDigits)} @ ${formatNumber(quote.price, priceDigits)}`)
  }

  function applyQuickFill(mode: 'bestBid' | 'bestAsk' | 'mid' | 'max') {
    if (!selectedMarket) return

    if (mode === 'bestBid' && bestBid) {
      setForm((current) => ({ ...current, price: String(bestBid) }))
      setNotice(`已带入买一价格 ${formatNumber(bestBid, priceDigits)}`)
      appendMemory('快速填充', `带入买一 ${formatNumber(bestBid, priceDigits)}`)
      return
    }

    if (mode === 'bestAsk' && bestAsk) {
      setForm((current) => ({ ...current, price: String(bestAsk) }))
      setNotice(`已带入卖一价格 ${formatNumber(bestAsk, priceDigits)}`)
      appendMemory('快速填充', `带入卖一 ${formatNumber(bestAsk, priceDigits)}`)
      return
    }

    if (mode === 'mid' && midPrice) {
      setForm((current) => ({ ...current, price: String(midPrice) }))
      setNotice(`已带入中间价 ${formatNumber(midPrice, priceDigits)}`)
      appendMemory('快速填充', `带入中间价 ${formatNumber(midPrice, priceDigits)}`)
      return
    }

    if (mode === 'max') {
      const nextAmount = form.side === 'buy'
        ? quoteBalance && ticketPrice > 0
          ? quoteBalance.available / ticketPrice
          : 0
        : baseBalance?.available ?? 0

      setForm((current) => ({
        ...current,
        amount: nextAmount > 0 ? String(Number(nextAmount.toFixed(6))) : current.amount,
      }))
      setNotice(`已按当前余额填充最大数量 ${formatNumber(nextAmount, 6)}`)
      appendMemory('快速填充', `按当前余额估算最大数量 ${formatNumber(nextAmount, 6)}`)
    }
  }

  async function submitOrder() {
    if (!selectedMarket) return
    setIsSubmitting(true)
    const start = performance.now()
    const result = await exchangeAPI.submitOrder({
      userId,
      marketId: selectedMarket.id,
      side: form.side,
      price: ticketPrice,
      amount: ticketAmount,
      outcome: Number(form.outcome) || 0,
    })
    setLastResponse(result.raw ?? result)
    setNotice(result.message)
    appendLog('提交订单', result.message, result.ok ? 'success' : 'danger')
    appendMemory('提交订单', `${selectedMarket.name} · ${form.side.toUpperCase()} ${formatNumber(ticketAmount, amountDigits)} @ ${formatNumber(ticketPrice, priceDigits)} · ${result.ok ? '已送出' : '返回异常'}`)
    recordLatency('submitOrder', performance.now() - start)
    setIsSubmitting(false)
    await refreshAll()
  }

  async function cancelOrder(orderId: string) {
    if (!selectedMarket) return
    setIsSubmitting(true)
    const start = performance.now()
    const result = await exchangeAPI.cancelOrder(orderId, selectedMarket.id, Number(form.outcome) || 0)
    setLastResponse(result.raw ?? result)
    setNotice(result.message)
    appendLog('撤单', result.message, result.ok ? 'success' : 'danger')
    appendMemory('撤单', `${orderId} · ${result.ok ? '撤单完成' : '撤单失败'}`)
    recordLatency('cancelOrder', performance.now() - start)
    setIsSubmitting(false)
    await refreshAll()
  }

  async function handleSubmitIntent() {
    if (!selectedMarket) return
    if (!needsSubmitConfirm) {
      await submitOrder()
      return
    }

    if (!submitArmed) {
      setSubmitArmed(true)
      setNotice(orderSource !== 'api' ? '当前处于回退/非真实链路：再次点击确认提交。' : '检测到异常/暂停状态：再次点击确认提交。')
      appendMemory('提交保护', '已进入二次确认窗口（再次点击提交才会真正发送）。')
      if (submitArmTimerRef.current) window.clearTimeout(submitArmTimerRef.current)
      submitArmTimerRef.current = window.setTimeout(() => setSubmitArmed(false), 3500)
      return
    }

    setSubmitArmed(false)
    if (submitArmTimerRef.current) window.clearTimeout(submitArmTimerRef.current)
    await submitOrder()
  }

  async function runCounterpartyDemo() {
    if (!selectedMarket || selectedMarket.kind === 'otc' || selectedMarket.kind === 'earn') return
    setIsSubmitting(true)
    const result = await exchangeAPI.submitOrder(
      {
        userId: 'admin',
        marketId: selectedMarket.id,
        side: form.side === 'buy' ? 'sell' : 'buy',
        price: ticketPrice,
        amount: ticketAmount,
        outcome: Number(form.outcome) || 0,
      },
      'admin',
    )
    setLastResponse(result.raw ?? result)
    setNotice(result.message)
    appendLog('对手单演示', result.message, result.ok ? 'success' : 'danger')
    appendMemory('对手单演示', `${result.ok ? '已注入对手方向订单' : '演示失败'} · ${form.side === 'buy' ? 'SELL' : 'BUY'}`)
    setIsSubmitting(false)
    await refreshAll()
  }

  async function runSelfTradeDemo() {
    if (!selectedMarket || selectedMarket.kind === 'otc' || selectedMarket.kind === 'earn') return
    setIsSubmitting(true)
    const result = await exchangeAPI.submitOrder({
      userId,
      marketId: selectedMarket.id,
      side: form.side === 'buy' ? 'sell' : 'buy',
      price: ticketPrice,
      amount: ticketAmount,
      outcome: Number(form.outcome) || 0,
    })
    setLastResponse(result.raw ?? result)
    setNotice(result.message)
    appendLog('自成交演示', result.message, result.ok ? 'success' : 'danger')
    appendMemory('自成交演示', `${result.ok ? '已触发同账户对向提交' : '演示失败'} · ${form.side === 'buy' ? 'SELL' : 'BUY'}`)
    setIsSubmitting(false)
    await refreshAll()
  }

  useKeyboardShortcuts(
    [
      { key: 'b', action: () => setForm((current) => ({ ...current, side: 'buy' })), description: '切换买入' },
      { key: 's', action: () => setForm((current) => ({ ...current, side: 'sell' })), description: '切换卖出' },
      { key: 'r', action: () => void refreshAll(), description: '刷新数据' },
      { key: '1', action: () => applyQuickFill('bestBid'), description: '带入买一' },
      { key: '2', action: () => applyQuickFill('bestAsk'), description: '带入卖一' },
      { key: '3', action: () => applyQuickFill('mid'), description: '带入中间价' },
      { key: '4', action: () => applyQuickFill('max'), description: '填充最大数量' },
      { key: 'Enter', action: () => void handleSubmitIntent(), description: needsSubmitConfirm ? '提交（需二次确认）' : '提交订单', disabled: isSubmitting || !selectedMarket },
      { key: 'Escape', action: () => setNotice('已清除快捷操作焦点'), description: '清除提示' },
    ],
    true,
  )

  return (
    <AppShell title="交易终端" subtitle="专业交易工作台" mode="terminal">
      <StatusBanner
        compact
        tone={notice.includes('error') || notice.includes('失败') ? 'danger' : orderSource === 'mock' ? 'warning' : 'neutral'}
        eyebrow="状态"
        title={selectedMarket?.name ?? '未选择市场'}
        message={notice}
        trailing={
          <button type="button" onClick={() => void refreshAll()} className="action-light px-4 py-2">
            {isLoading ? '刷新中…' : '刷新数据'}
          </button>
        }
      />

      {/* ── Compact Toolbar ── */}
      <section className="mt-3 flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <span className="mono-chip">工作区</span>
          <button type="button" onClick={() => setShowBrief((c) => !c)} className={`rounded-full border px-3 py-1.5 text-xs transition ${showBrief ? 'border-black bg-black text-white' : 'border-black bg-white text-black hover:bg-neutral-100'}`}>
            {showBrief ? '概览 ON' : '概览 OFF'}
          </button>
          <button type="button" onClick={() => setShowInsights((c) => !c)} className={`rounded-full border px-3 py-1.5 text-xs transition ${showInsights ? 'border-black bg-black text-white' : 'border-black bg-white text-black hover:bg-neutral-100'}`}>
            {showInsights ? '洞察 ON' : '洞察 OFF'}
          </button>
        </div>
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1 text-[11px] text-neutral-500">
            <span className="mono-chip">B/S</span><span>方向</span>
            <span className="mono-chip">1-4</span><span>快带</span>
            <span className="mono-chip">Enter</span><span>提交</span>
          </div>
          <button type="button" onClick={() => setPaletteOpen(true)} className="action-light px-3 py-1.5 text-xs">⌘K 指令</button>
          <span className="mono-chip">密度</span>
          <button type="button" onClick={() => setDensityMode('compact')} className={`rounded-full border px-3 py-1.5 text-xs transition ${densityMode === 'compact' ? 'border-black bg-black text-white' : 'border-black bg-white text-black hover:bg-neutral-100'}`}>紧凑</button>
          <button type="button" onClick={() => setDensityMode('comfortable')} className={`rounded-full border px-3 py-1.5 text-xs transition ${densityMode === 'comfortable' ? 'border-black bg-black text-white' : 'border-black bg-white text-black hover:bg-neutral-100'}`}>舒适</button>
        </div>
      </section>

      {/* ═══════════ COMPACT MARKET BAR ═══════════ */}
      <div className="trading-market-bar mt-4">
        <div className="trading-market-bar-left">
          <h2 className="text-lg font-bold tracking-tight text-black truncate">
            {selectedMarket ? `${pair.base}/${pair.quote}` : '未选择市场'}
          </h2>
          <span className="chip-soft text-xs">{selectedMarket ? kindLabel[selectedMarket.kind] : '-'}</span>
          <span className="chip-soft text-xs">{orderSource === 'api' ? '真实链路' : '本地回退'}</span>
          <span className={`text-xs font-medium ${selectedMarket?.tradingEnabled ? 'signal-positive' : 'signal-negative'}`}>
            {selectedMarket?.tradingEnabled ? '● 交易开启' : '○ 交易关闭'}
          </span>
        </div>
        <div className="trading-market-bar-right">
          <div className="trading-bar-metric trading-bar-metric-hero">
            <div className="trading-bar-metric-label">Mid Price</div>
            <div className="trading-bar-metric-value text-xl">{midPrice ? formatNumber(midPrice, priceDigits) : '-'}</div>
          </div>
          <div className="trading-bar-metric">
            <div className="trading-bar-metric-label">买一</div>
            <div className="trading-bar-metric-value book-price-bid">{bestBid ? formatNumber(bestBid, priceDigits) : '-'}</div>
          </div>
          <div className="trading-bar-metric">
            <div className="trading-bar-metric-label">卖一</div>
            <div className="trading-bar-metric-value book-price-ask">{bestAsk ? formatNumber(bestAsk, priceDigits) : '-'}</div>
          </div>
          <div className="trading-bar-metric">
            <div className="trading-bar-metric-label">点差</div>
            <div className="trading-bar-metric-value">{spread ? `${formatNumber(spread, priceDigits)} (${formatNumber(spreadBps, 1)}bp)` : '-'}</div>
          </div>
          <div className="trading-bar-signals">
            {[
              { label: 'Focus', tone: focusPrice ? 'live' : 'idle' },
              { label: 'Exec', tone: parsedLastResponseError ? 'danger' : crossesSpread ? 'live' : 'idle' },
              { label: 'Drift', tone: priceChartSummary.changePct !== null && priceChartSummary.changePct < 0 ? 'ask' : 'bid' },
              { label: 'Latency', tone: (latencyDashboard[0]?.latest ?? 0) > 120 ? 'danger' : 'live' },
            ].map((s) => (
              <div key={s.label} className="trading-bar-signal" title={s.label}>
                <span className={`trading-bar-dot ${s.tone === 'danger' ? 'trading-bar-dot-danger' : s.tone === 'live' || s.tone === 'bid' ? 'trading-bar-dot-live' : s.tone === 'ask' ? 'trading-bar-dot-ask' : ''}`} />
                <span className="trading-bar-signal-label">{s.label}</span>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* ═══════════ MAIN 3-COLUMN GRID ═══════════ */}
      <div className="terminal-main-layout mt-4">

        {/* ── LEFT RAIL: Market Catalog + Balances ── */}
        <div className="terminal-rail-stack terminal-seam-stack">
          <div className="surface-card terminal-side-card p-5">
            <div className="eyebrow">Market Catalog</div>
            <h3 className="mt-1 terminal-section-title">市场目录</h3>
            <div className="mt-3 flex flex-wrap gap-1">
              {kindOptions.map((option) => {
                const count = option.value === 'all' ? markets.length : markets.filter((m) => m.kind === option.value).length
                return (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => setKindFilter(option.value)}
                  className={`rounded-full border px-2.5 py-1 text-xs transition ${kindFilter === option.value ? 'border-black bg-black text-white' : 'border-black bg-white text-black hover:bg-neutral-100'}`}
                >
                  {option.label} {count > 0 ? `(${count})` : ''}
                </button>
                )
              })}
            </div>
            <div className="mt-3 flex items-center gap-2">
              <input
                value={marketSearch}
                onChange={(event) => setMarketSearch(event.target.value)}
                className="field-shell flex-1 text-xs"
                placeholder="搜索市场..."
              />
              <button
                type="button"
                onClick={() => setOnlyFavorites((c) => !c)}
                className={`rounded-full border px-2.5 py-1 text-xs transition ${onlyFavorites ? 'border-black bg-black text-white' : 'border-black bg-white text-black hover:bg-neutral-100'}`}
              >
                {onlyFavorites ? '★ 收藏' : '☆ 收藏'}
              </button>
            </div>
            <div className="terminal-scroll mt-3 max-h-[320px] space-y-1 overflow-auto">
              {filteredMarkets.map((market) => {
                const mPair = (() => { const parts = market.name.split(/[/_-]/); return { base: parts[0] ?? '?', quote: parts[1] ?? '?' } })()
                const isFav = favoriteMarketIds.includes(market.id)
                return (
                  <button
                    key={market.id}
                    type="button"
                    onClick={() => setSelectedMarketId(market.id)}
                    draggable={isFav}
                    onDragStart={() => setDragFavoriteId(market.id)}
                    onDragOver={(e) => { e.preventDefault(); setDragOverFavoriteId(market.id) }}
                    onDragEnd={() => { if (dragFavoriteId && dragOverFavoriteId && dragFavoriteId !== dragOverFavoriteId) { setFavoriteMarketIds((current) => { const next = current.filter((id) => id !== dragFavoriteId); const targetIndex = next.indexOf(dragOverFavoriteId); if (targetIndex === -1) return [...next, dragFavoriteId]; next.splice(targetIndex, 0, dragFavoriteId); return next }); } setDragFavoriteId(null); setDragOverFavoriteId(null) }}
                    className={`watchlist-row ${selectedMarketId === market.id ? 'watchlist-row-active' : ''}`}
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="font-medium text-black truncate">{mPair.base}/{mPair.quote}</span>
                        <span className="chip-soft text-[10px]">{kindLabel[market.kind]}</span>
                      </div>
                    </div>
                    <div className="flex items-center gap-1">
                      {market.backendAvailable ? <span className="inline-block w-1.5 h-1.5 rounded-full bg-emerald-500" title="真实链路" /> : <span className="inline-block w-1.5 h-1.5 rounded-full bg-neutral-300" title="演示" />}
                      <button type="button" onClick={(e) => { e.stopPropagation(); toggleFavorite(market.id) }} className="text-xs px-1 hover:scale-110 transition" title={isFav ? '取消收藏' : '收藏'}>
                        {isFav ? '★' : '☆'}
                      </button>
                    </div>
                  </button>
                )
              })}
            </div>
          </div>

          <div className="surface-card terminal-side-card p-5">
            <div className="eyebrow">Balances</div>
            <h3 className="mt-1 terminal-section-title">余额</h3>
            <div className="mt-3 space-y-2">
              {balances.length === 0 ? (
                <div className="text-sm text-neutral-500">暂无余额数据</div>
              ) : (
                balances.slice(0, 6).map((balance) => (
                  <div key={balance.asset} className={`flex items-center justify-between text-sm ${highlightedBalanceSet.has(balance.asset) ? 'row-flash row-flash-positive' : ''}`}>
                    <span className="font-medium text-black">{balance.asset}</span>
                    <div className="text-right">
                      <span className="data-mono text-black">{formatNumber(balance.available, 6)}</span>
                      {balance.hold > 0 ? <span className="ml-2 text-xs text-neutral-500">冻结 {formatNumber(balance.hold, 6)}</span> : null}
                    </div>
                  </div>
                ))
              )}
            </div>
            {balanceChangeSummary ? <div className="mt-2 text-xs text-neutral-500">{balanceChangeSummary}</div> : null}
          </div>

          <div className="surface-card terminal-side-card p-5">
            <div className="eyebrow">Session</div>
            <h3 className="mt-1 terminal-section-title">会话</h3>
            <div className="mt-3 grid grid-cols-2 gap-2 text-xs">
              <div><span className="text-neutral-500">挂单</span> <span className="ml-1 data-mono font-medium">{orders.length}</span></div>
              <div><span className="text-neutral-500">成交</span> <span className="ml-1 data-mono font-medium">{fills.length}</span></div>
              <div><span className="text-neutral-500">操作</span> <span className="ml-1 data-mono font-medium">{actionLog.length}</span></div>
              <div><span className="text-neutral-500">记忆</span> <span className="ml-1 data-mono font-medium">{memoryEvents.length}</span></div>
            </div>
          </div>
        </div>

        {/* ── CENTER: Order Book + Chart Tabs ── */}
        <div className="terminal-center-stack" style={{ gap: '0.75rem' }}>
          <div className="trading-center-split">
            {/* Order Book Column */}
            <div className="trading-book-col">
              <div className="section-frame h-full">
                <div className="flex items-center justify-between mb-3">
                  <div>
                    <div className="eyebrow">Order Book</div>
                    <div className="text-sm font-semibold text-black">盘口深度</div>
                  </div>
                  <div className="premium-micro">{depthHeatBands.length} levels</div>
                </div>
                {selectedMarket?.kind === 'otc' || selectedMarket?.kind === 'earn' ? (
                  <EmptyStatePanel title="当前品类不展示盘口" description="OTC 与理财产品不使用标准订单簿视图。" />
                ) : orderBook ? (
                  <div className="space-y-0">
                    <div className="terminal-panel terminal-scroll max-h-[520px] overflow-auto">
                      <div className={`terminal-head ${densityMode === 'compact' ? 'terminal-head-dense' : 'terminal-head-comfy'} grid-cols-[1fr_0.7fr_0.5fr]`}>
                        <div>价格</div>
                        <div>数量</div>
                        <div>累计</div>
                      </div>
                      {displayedAskRows.map((level) => {
                        const width = `${Math.max((level.amount / maxAskAmount) * 100, 8)}%`
                        const rowLinked = isNearPrice(level.price, focusPrice, focusTickSize, 2)
                        return (
                          <button
                            key={`ask-${level.levelIndex}`}
                            type="button"
                            onClick={() => selectBookLevel(level.price, level.amount, 'buy', '卖盘')}
                            onMouseEnter={() => updateHoverLens({ source: 'ask', side: 'buy', price: level.price, amount: level.amount, cumulative: level.cumulative, levelIndex: level.levelIndex, hint: `卖盘 ${level.levelIndex} 档` })}
                            onMouseLeave={() => updateHoverLens(null)}
                            className={`terminal-row ${densityMode === 'compact' ? 'terminal-row-dense' : 'terminal-row-comfy'} terminal-row-interactive book-row grid-cols-[1fr_0.7fr_0.5fr] text-left ${rowLinked ? 'terminal-row-linked' : ''} ${hoverLens?.price === level.price ? 'terminal-row-focused' : ''}`}
                          >
                            <div className="book-bar book-bar-ask" style={{ width }} />
                            <div className="relative z-10 terminal-number font-semibold book-price-ask">{formatNumber(level.price, priceDigits)}</div>
                            <div className="relative z-10 terminal-number-right text-black">{formatNumber(level.amount, amountDigits)}</div>
                            <div className="relative z-10 terminal-number-right text-neutral-500">{formatNumber(level.cumulative, amountDigits)}</div>
                          </button>
                        )
                      })}
                      <div className="book-mid-divider grid-cols-[1fr_0.7fr_0.5fr]">
                        <div className="data-mono text-lg font-bold tracking-tight text-black">{midPrice ? formatNumber(midPrice, priceDigits) : '-'}</div>
                        <div className={`text-xs font-medium ${toneClass(spread === 0, spread > 0)}`}>点差 {spread ? formatNumber(spread, priceDigits) : '-'}</div>
                        <div className="text-xs text-neutral-500">{spread ? `${formatNumber(spreadBps, 1)}bp` : '-'}</div>
                      </div>
                      {displayedBidRows.map((level) => {
                        const width = `${Math.max((level.amount / maxBidAmount) * 100, 8)}%`
                        const rowLinked = isNearPrice(level.price, focusPrice, focusTickSize, 2)
                        return (
                          <button
                            key={`bid-${level.levelIndex}`}
                            type="button"
                            onClick={() => selectBookLevel(level.price, level.amount, 'sell', '买盘')}
                            onMouseEnter={() => updateHoverLens({ source: 'bid', side: 'sell', price: level.price, amount: level.amount, cumulative: level.cumulative, levelIndex: level.levelIndex, hint: `买盘 ${level.levelIndex} 档` })}
                            onMouseLeave={() => updateHoverLens(null)}
                            className={`terminal-row ${densityMode === 'compact' ? 'terminal-row-dense' : 'terminal-row-comfy'} terminal-row-interactive book-row grid-cols-[1fr_0.7fr_0.5fr] text-left ${rowLinked ? 'terminal-row-linked' : ''} ${hoverLens?.price === level.price ? 'terminal-row-focused' : ''}`}
                          >
                            <div className="book-bar book-bar-bid" style={{ width }} />
                            <div className="relative z-10 terminal-number font-semibold book-price-bid">{formatNumber(level.price, priceDigits)}</div>
                            <div className="relative z-10 terminal-number-right text-black">{formatNumber(level.amount, amountDigits)}</div>
                            <div className="relative z-10 terminal-number-right text-neutral-500">{formatNumber(level.cumulative, amountDigits)}</div>
                          </button>
                        )
                      })}
                    </div>
                    {/* Depth Heatmap */}
                    {depthHeatBands.length > 0 ? (
                      <div className="depth-heatmap-grid mt-3">
                        {depthHeatBands.map((level) => (
                          <button
                            key={level.key}
                            type="button"
                            onClick={() => selectBookLevel(level.price, level.amount, level.side === 'ask' ? 'buy' : 'sell', level.side === 'ask' ? '卖盘热力带' : '买盘热力带')}
                            onMouseEnter={() => updateHoverLens({ source: 'heat', side: level.side === 'ask' ? 'buy' : 'sell', price: level.price, amount: level.amount, hint: `${level.side === 'ask' ? '卖盘' : '买盘'}热力带` })}
                            onMouseLeave={() => updateHoverLens(null)}
                            className={`depth-heatmap-pill ${level.side === 'ask' ? 'depth-heatmap-pill-ask' : 'depth-heatmap-pill-bid'}`}
                            style={{ opacity: `${Math.min(1, 0.28 + level.intensity / 100)}` }}
                          >
                            <span className="depth-heatmap-price">{formatNumber(level.price, priceDigits)}</span>
                            <span className="depth-heatmap-amount">{formatNumber(level.amount, amountDigits)}</span>
                          </button>
                        ))}
                      </div>
                    ) : null}
                  </div>
                ) : (
                  <EmptyStatePanel title="暂无盘口数据" description="当前市场还没有订单簿快照。" />
                )}
              </div>
            </div>

            {/* Chart Tabs Column */}
            <div className="trading-chart-col">
              <div className="section-frame h-full">
                <div className="flex items-center justify-between mb-3">
                  <div className="flex items-center gap-2">
                    {(['chart', 'depth', 'pulse'] as const).map((tab) => (
                      <button
                        key={tab}
                        type="button"
                        onClick={() => setCenterTab(tab)}
                        className={`rounded-full border px-3 py-1.5 text-xs font-medium transition ${centerTab === tab ? 'border-black bg-black text-white' : 'border-neutral-300 bg-white text-neutral-600 hover:bg-neutral-50'}`}
                      >
                        {tab === 'chart' ? '📈 价格图表' : tab === 'depth' ? '📊 深度曲线' : '⚡ 短线脉冲'}
                      </button>
                    ))}
                  </div>
                  <div className="premium-micro">
                    {centerTab === 'chart' ? `${priceChartData.length} points` : centerTab === 'depth' ? `${depthCurveData.length} levels` : `${microPulseData.length} ticks`}
                  </div>
                </div>

                {/* Chart Tab Content */}
                {centerTab === 'chart' ? (
                  <div>
                    <div className="chart-metric-strip mb-3">
                      {(selectedMarket?.kind === 'earn'
                        ? [
                            { label: '本金', value: formatNumber(earnTotalPrincipal, 6) },
                            { label: '收益', value: formatNumber(earnTotalYield, 6), tone: 'positive' as const },
                            { label: '可赎回', value: formatNumber(earnTotalRedeemable, 6) },
                          ]
                        : selectedMarket?.kind === 'otc'
                          ? [
                              { label: '报价数', value: String(visibleOtcQuotes.length) },
                              { label: '平均规模', value: visibleOtcQuotes.length === 0 ? '-' : formatNumber(visibleOtcQuotes.reduce((sum, item) => sum + item.amount, 0) / visibleOtcQuotes.length, amountDigits) },
                            ]
                          : priceChartMetricItems
                      ).map((item) => (
                        <div key={item.label} className="chart-metric-card">
                          <div className="chart-metric-label">{item.label}</div>
                          <div className={`chart-metric-value ${(item as { tone?: string }).tone === 'positive' ? 'signal-positive' : (item as { tone?: string }).tone === 'negative' ? 'signal-negative' : ''}`}>{item.value}</div>
                        </div>
                      ))}
                    </div>
                    <div className="chart-shell h-[340px]">
                      {selectedMarket?.kind === 'earn' ? (
                        earnPositionStats.length === 0 ? (
                          <EmptyStatePanel title="暂无理财图表" description="等待理财持仓后展示。" />
                        ) : (
                          <ResponsiveContainer width="100%" height="100%">
                            <BarChart data={earnPositionStats}>
                              <CartesianGrid vertical={false} stroke="#ececec" />
                              <XAxis dataKey="name" axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                              <YAxis axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                              <Tooltip content={<PremiumTooltip />} />
                              <Bar dataKey="本金" fill="#111111" radius={[10, 10, 0, 0]} />
                              <Bar dataKey="估算收益" fill="#737373" radius={[10, 10, 0, 0]} />
                            </BarChart>
                          </ResponsiveContainer>
                        )
                      ) : selectedMarket?.kind === 'otc' ? (
                        otcChartData.length === 0 ? (
                          <EmptyStatePanel title="暂无 OTC 图表" description="等待报价出现后展示。" />
                        ) : (
                          <ResponsiveContainer width="100%" height="100%">
                            <BarChart data={otcChartData}>
                              <CartesianGrid vertical={false} stroke="#ececec" />
                              <XAxis dataKey="name" axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                              <YAxis axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                              <Tooltip content={<PremiumTooltip />} />
                              <Bar dataKey="报价规模" fill="#111111" radius={[10, 10, 0, 0]} />
                            </BarChart>
                          </ResponsiveContainer>
                        )
                      ) : priceChartData.length === 0 ? (
                        <EmptyStatePanel title="暂无价格走势" description="等待成交产生后展示。" />
                      ) : (
                        <ResponsiveContainer width="100%" height="100%">
                          <ComposedChart data={priceChartData}>
                            <CartesianGrid vertical={false} stroke="#ececec" />
                            <XAxis dataKey="name" axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                            <YAxis yAxisId="price" axisLine={false} tickLine={false} fontSize={12} stroke="#525252" domain={['dataMin', 'dataMax']} />
                            <YAxis yAxisId="volume" orientation="right" hide domain={[0, 'dataMax']} />
                            <Tooltip content={<PremiumTooltip />} />
                            <Bar yAxisId="volume" dataKey="数量" fill="#d4d4d4" radius={[8, 8, 0, 0]} opacity={0.9} />
                            {midPrice ? <ReferenceLine yAxisId="price" y={midPrice} stroke="#737373" strokeDasharray="4 4" ifOverflow="extendDomain" /> : null}
                            {ticketPrice ? <ReferenceLine yAxisId="price" y={ticketPrice} stroke="#111111" strokeDasharray="2 3" ifOverflow="extendDomain" /> : null}
                            <Line yAxisId="price" type="monotone" dataKey="价格" stroke={priceChartSummary.changePct !== null && priceChartSummary.changePct < 0 ? '#b91c1c' : '#111111'} strokeWidth={2.4} dot={false} activeDot={{ r: 3, fill: '#111111' }} />
                          </ComposedChart>
                        </ResponsiveContainer>
                      )}
                    </div>
                  </div>
                ) : centerTab === 'depth' ? (
                  <div>
                    <div className="flex items-center justify-between mb-3">
                      <div className="text-sm font-semibold text-black">累积深度曲线</div>
                      <div className="text-xs text-neutral-500">黑线为买盘, 虚线为卖盘</div>
                    </div>
                    <div className="chart-shell h-[340px]">
                      {depthCurveData.length === 0 ? (
                        <EmptyStatePanel title="等待盘口快照" description="有盘口数据后将生成深度曲线。" />
                      ) : (
                        <ResponsiveContainer width="100%" height="100%">
                          <LineChart data={depthCurveData} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
                            <CartesianGrid vertical={false} stroke="#ececec" />
                            <XAxis dataKey="price" axisLine={false} tickLine={false} fontSize={11} stroke="#525252" />
                            <YAxis axisLine={false} tickLine={false} fontSize={11} stroke="#525252" />
                            <Tooltip content={<PremiumTooltip />} />
                            <Line type="monotone" dataKey="bid" stroke="#111111" strokeWidth={2.2} dot={false} />
                            <Line type="monotone" dataKey="ask" stroke="#525252" strokeWidth={2.2} dot={false} strokeDasharray="4 3" />
                          </LineChart>
                        </ResponsiveContainer>
                      )}
                    </div>
                    <div className="mt-3 grid grid-cols-4 gap-2">
                      {bookSummaryItems.map((item) => (
                        <div key={item.label} className="surface-soft px-3 py-2 rounded-xl">
                          <div className="text-[10px] text-neutral-500">{item.label}</div>
                          <div className="data-mono text-sm font-medium text-black">{item.value}</div>
                        </div>
                      ))}
                    </div>
                  </div>
                ) : (
                  <div>
                    <div className="flex items-center justify-between mb-3">
                      <div className="text-sm font-semibold text-black">短线脉冲</div>
                      <div className="flex items-center gap-3 text-xs text-neutral-500">
                        <span>流动性 <span className="data-mono font-medium text-black">{liquidityScore}/100</span></span>
                        <span>执行置信 <span className="data-mono font-medium text-black">{executionConfidenceScore}/100</span></span>
                      </div>
                    </div>
                    <div className="chart-shell h-[200px]">
                      {microPulseData.length === 0 ? (
                        <EmptyStatePanel title="等待成交" description="成交后生成脉冲线。" />
                      ) : (
                        <ResponsiveContainer width="100%" height="100%">
                          <LineChart data={microPulseData} margin={{ top: 4, right: 0, left: 0, bottom: 0 }}>
                            <Tooltip content={<PremiumTooltip />} />
                            {focusPrice ? <ReferenceLine y={focusPrice} stroke="#737373" strokeDasharray="3 3" ifOverflow="extendDomain" /> : null}
                            <Line type="monotone" dataKey="price" stroke="#111111" strokeWidth={2} dot={false} />
                          </LineChart>
                        </ResponsiveContainer>
                      )}
                    </div>
                    <div className="mt-3 grid grid-cols-2 gap-2">
                      {pulseDigestItems.map((item) => (
                        <div key={item.label} className="surface-soft px-3 py-2 rounded-xl">
                          <div className="text-[10px] text-neutral-500">{item.label}</div>
                          <div className="data-mono text-sm font-medium text-black">{item.value}</div>
                        </div>
                      ))}
                    </div>
                    {priceMagnetZones.length > 0 ? (
                      <div className="mt-3 grid grid-cols-2 gap-2">
                        {priceMagnetZones.slice(0, 4).map((zone) => (
                          <div key={`${zone.label}-${zone.price}`} className="surface-soft px-3 py-2 rounded-xl">
                            <div className="text-[10px] text-neutral-500">{zone.label}</div>
                            <div className="data-mono text-sm font-medium">{formatNumber(zone.price, priceDigits)}</div>
                            <div className="text-[10px] text-neutral-400">{formatNumber(zone.distanceBps, 1)} bps · {zone.strength}</div>
                          </div>
                        ))}
                      </div>
                    ) : null}
                  </div>
                )}
              </div>
            </div>
          </div>

          {/* Hover Lens (compact) */}
          {hoverLens ? (
            <div className="trading-hover-lens">
              <span className={`font-medium ${hoverLens.side === 'buy' ? 'book-price-bid' : 'book-price-ask'}`}>{hoverLens.side.toUpperCase()}</span>
              <span className="data-mono">{formatNumber(hoverLens.price, priceDigits)}</span>
              <span className="text-neutral-500">×</span>
              <span className="data-mono">{formatNumber(hoverLens.amount, amountDigits)}</span>
              <span className="text-neutral-500">·</span>
              <span className="text-neutral-500 text-xs">{hoverLens.hint}</span>
              <button type="button" onClick={() => { selectBookLevel(hoverLens.price, hoverLens.amount, hoverLens.side, 'Hover Lens'); appendMemory('Hover Lens', `带入 ${hoverLens.side.toUpperCase()} @ ${formatNumber(hoverLens.price, priceDigits)}`) }} className="action-light px-2 py-0.5 text-xs ml-auto">
                带入票据
              </button>
            </div>
          ) : null}
        </div>

        {/* ── RIGHT RAIL: Order Entry ── */}
        <div className="terminal-rail-stack terminal-seam-stack terminal-rail-right sticky-rail">
          <div className="ticket-panel ticket-panel-premium">
            <div className="eyebrow">交易票据</div>
            <h3 className="mt-1 terminal-section-title">下单</h3>
            <div className="mt-3 flex flex-wrap gap-2">
              <span className="chip-soft">{selectedMarket ? kindLabel[selectedMarket.kind] : '-'}</span>
              <span className="chip-soft">{orderSource === 'api' ? '真实链路' : '回退模式'}</span>
            </div>

            {/* Side Toggle */}
            <div className="mt-3 segmented-wrap">
              <button type="button" onClick={() => setForm((c) => ({ ...c, side: 'buy' }))} className={`segmented-btn ${form.side === 'buy' ? 'segmented-btn-active' : ''}`}>买入</button>
              <button type="button" onClick={() => setForm((c) => ({ ...c, side: 'sell' }))} className={`segmented-btn ${form.side === 'sell' ? 'segmented-btn-active' : ''}`}>卖出</button>
            </div>

            {/* Quick fills */}
            <div className="terminal-quick-grid mt-3">
              <button type="button" onClick={() => applyQuickFill('bestBid')} className="action-light px-3 py-1.5 text-xs">买一</button>
              <button type="button" onClick={() => applyQuickFill('bestAsk')} className="action-light px-3 py-1.5 text-xs">卖一</button>
              <button type="button" onClick={() => applyQuickFill('mid')} className="action-light px-3 py-1.5 text-xs">中间价</button>
              <button type="button" onClick={() => applyQuickFill('max')} className="action-light px-3 py-1.5 text-xs">最大量</button>
            </div>

            {/* Price Input */}
            <div className="mt-3">
              <div className="field-label">价格</div>
              <div className="field-stepper">
                <button type="button" onClick={() => { const tick = Math.pow(10, -priceDigits); const next = Math.max(0, (Number(form.price) || 0) - tick); setForm((c) => ({ ...c, price: String(Number(next.toFixed(priceDigits))) })) }} className="field-stepper-btn">−tick</button>
                <button type="button" onClick={() => { const tick = Math.pow(10, -priceDigits) * 5; const next = Math.max(0, (Number(form.price) || 0) - tick); setForm((c) => ({ ...c, price: String(Number(next.toFixed(priceDigits))) })) }} className="field-stepper-btn">−5</button>
                <button type="button" onClick={() => { const tick = Math.pow(10, -priceDigits) * 5; const next = (Number(form.price) || 0) + tick; setForm((c) => ({ ...c, price: String(Number(next.toFixed(priceDigits))) })) }} className="field-stepper-btn">+5</button>
                <button type="button" onClick={() => { const tick = Math.pow(10, -priceDigits); const next = (Number(form.price) || 0) + tick; setForm((c) => ({ ...c, price: String(Number(next.toFixed(priceDigits))) })) }} className="field-stepper-btn">+tick</button>
                <button type="button" onClick={() => { if (!midPrice) return; const next = midPrice * (form.side === 'buy' ? 0.999 : 1.001); setForm((c) => ({ ...c, price: String(Number(next.toFixed(priceDigits))) })) }} className="field-stepper-btn" disabled={!midPrice}>mid±10bp</button>
              </div>
              <input value={form.price} onChange={(e) => setForm((c) => ({ ...c, price: e.target.value }))} className="field-shell" placeholder="输入价格" />
            </div>

            {/* Amount Input */}
            <div className="mt-3">
              <div className="field-label">数量</div>
              <div className="field-stepper">
                <button type="button" onClick={() => { const step = Math.pow(10, -Math.min(amountDigits, 6)); const next = Math.max(0, (Number(form.amount) || 0) - step); setForm((c) => ({ ...c, amount: String(Number(next.toFixed(Math.min(amountDigits, 6)))) })) }} className="field-stepper-btn">−step</button>
                <button type="button" onClick={() => { const step = Math.pow(10, -Math.min(amountDigits, 6)); const next = (Number(form.amount) || 0) + step; setForm((c) => ({ ...c, amount: String(Number(next.toFixed(Math.min(amountDigits, 6)))) })) }} className="field-stepper-btn">+step</button>
                <button type="button" onClick={() => { const next = (Number(form.amount) || 0) * 0.5; setForm((c) => ({ ...c, amount: String(Number(next.toFixed(Math.min(amountDigits, 6)))) })) }} className="field-stepper-btn">×0.5</button>
                <button type="button" onClick={() => { const next = (Number(form.amount) || 0) * 2; setForm((c) => ({ ...c, amount: String(Number(next.toFixed(Math.min(amountDigits, 6)))) })) }} className="field-stepper-btn">×2</button>
              </div>
              <input value={form.amount} onChange={(e) => setForm((c) => ({ ...c, amount: e.target.value }))} className="field-shell" placeholder="输入数量" />
            </div>

            {/* Outcome (for multi-outcome) */}
            <div className="mt-3">
              <div className="field-label">Outcome</div>
              <input value={form.outcome} onChange={(e) => setForm((c) => ({ ...c, outcome: e.target.value }))} className="field-shell" placeholder="输入 outcome" />
            </div>

            {/* Impact Preview */}
            <div className="ticket-summary mt-3">
              <div className="grid gap-2 text-sm">
                <div className="flex justify-between"><span className="text-neutral-600">预估价值</span><span className="data-mono font-medium">{formatNumber(ticketNotional, quoteDigits)}</span></div>
                <div className="flex justify-between"><span className="text-neutral-600">预计扣减</span><span className="data-mono font-medium">{formatNumber(estimatedDebit, form.side === 'buy' ? quoteDigits : amountDigits)} {impactAsset}</span></div>
                <div className="flex justify-between"><span className="text-neutral-600">扣减后可用</span><span className={`data-mono font-medium ${toneClass(projectedAvailable !== null && projectedAvailable >= 0, projectedAvailable !== null && projectedAvailable < 0)}`}>{projectedAvailable === null ? '-' : formatSignedNumber(projectedAvailable, impactAsset === pair.quote ? quoteDigits : amountDigits)} {impactAsset}</span></div>
                <div className="flex justify-between"><span className="text-neutral-600">执行判断</span><span className={`font-medium ${crossesSpread ? 'signal-positive' : 'text-black'}`}>{crossesSpread ? '跨价成交' : '挂单入簿'}</span></div>
              </div>
            </div>

            {/* Sweep Preview */}
            {sweepPreview ? (
              <div className="mt-3 surface-soft p-3 rounded-2xl">
                <div className="signal-meter-track">
                  <div className="impact-fill-bar" style={{ width: `${Math.max(6, sweepPreview.fillRatio)}%` }} />
                </div>
                <div className="mt-2 grid grid-cols-2 gap-2 text-xs">
                  <div><span className="text-neutral-500">吃单比</span> <span className="data-mono font-medium">{formatNumber(sweepPreview.fillRatio, 1)}%</span></div>
                  <div><span className="text-neutral-500">均价</span> <span className="data-mono font-medium">{sweepPreview.averagePrice ? formatNumber(sweepPreview.averagePrice, priceDigits) : '-'}</span></div>
                  <div><span className="text-neutral-500">滑点</span> <span className="data-mono font-medium">{formatNumber(sweepPreview.slippageBps, 2)} bps</span></div>
                  <div><span className="text-neutral-500">排队</span> <span className="data-mono font-medium">{queuePressureEstimate?.posture ?? '-'}</span></div>
                </div>
              </div>
            ) : null}

            {/* Error Display */}
            {parsedLastResponseError ? (
              <div className="ticket-summary mt-3">
                <div className="eyebrow">Execution Error</div>
                <div className="mt-2 text-sm text-neutral-700">{parsedLastResponseError.message ?? parsedLastResponseError.error ?? '未提供错误说明'}</div>
                <button type="button" onClick={() => void copyErrorBody()} className="action-light mt-2 px-3 py-1.5 text-xs">复制错误体</button>
              </div>
            ) : null}

            {/* Submit Button */}
            <button type="button" disabled={isSubmitting || !selectedMarket} onClick={() => void handleSubmitIntent()} className="action-dark ticket-submit-primary mt-3 w-full disabled:cursor-not-allowed disabled:opacity-60">
              {submitArmed && needsSubmitConfirm
                ? '再次点击确认提交'
                : selectedMarket?.kind === 'otc'
                  ? '提交 OTC 报价'
                  : selectedMarket?.kind === 'earn'
                    ? (form.side === 'buy' ? '申购理财' : '赎回理财')
                    : needsSubmitConfirm
                      ? '提交（需二次确认）'
                      : '提交订单'}
            </button>

            {/* Feedback */}
            <div className={`submit-feedback mt-3 ${submitFeedbackToneClass} ${submitFeedbackMotionClass}`}>
              <div className="flex items-center justify-between gap-2">
                <div className="text-sm font-medium text-black truncate">{isSubmitting ? '提交中…' : notice}</div>
                <div className={`mono-chip text-xs ${parsedLastResponseError ? 'signal-negative' : orderSource === 'api' ? 'signal-positive' : ''}`}>{executionStateLabel}</div>
              </div>
            </div>

            {/* Strategy Presets */}
            <div className="mt-3">
              <div className="text-xs font-medium text-neutral-500 mb-2">策略预设</div>
              <div className="grid grid-cols-2 gap-1.5">
                {strategyPresets.map((preset) => (
                  <button key={preset.id} type="button" onClick={() => applyStrategyPreset(preset.id)} className="preset-lab-item text-left">
                    <div className="preset-lab-label">{preset.label}</div>
                    <div className="preset-lab-title text-xs">{preset.title}</div>
                  </button>
                ))}
              </div>
            </div>

            {/* Demo Actions */}
            {selectedMarket && selectedMarket.kind !== 'otc' && selectedMarket.kind !== 'earn' ? (
              <div className="mt-3 grid gap-2">
                <button type="button" disabled={isSubmitting} onClick={() => void runCounterpartyDemo()} className="action-light w-full text-xs disabled:cursor-not-allowed disabled:opacity-60">生成对手单演示</button>
                <button type="button" disabled={isSubmitting} onClick={() => void runSelfTradeDemo()} className="action-light w-full text-xs disabled:cursor-not-allowed disabled:opacity-60">生成自成交演示</button>
              </div>
            ) : null}
          </div>
        </div>
      </div>

      {/* ═══════════ BOTTOM TABS ═══════════ */}
      <div className="section-frame mt-5">
        <div className="flex items-center gap-2 mb-3">
          {([
            { key: 'orders' as const, label: '挂单', count: orders.length },
            { key: 'trades' as const, label: '最近成交', count: trades.length },
            { key: 'fills' as const, label: '成交回放', count: fills.length },
            { key: 'risk' as const, label: '风控快照', count: fundingRates.length + riskEvents.length },
            { key: 'log' as const, label: '操作日志', count: actionLog.length },
          ]).map((tab) => (
            <button
              key={tab.key}
              type="button"
              onClick={() => setBottomTab(tab.key)}
              className={`rounded-full border px-3 py-1.5 text-xs font-medium transition ${bottomTab === tab.key ? 'border-black bg-black text-white' : 'border-neutral-300 bg-white text-neutral-600 hover:bg-neutral-50'}`}
            >
              {tab.label} {tab.count > 0 ? <span className="ml-1 opacity-60">({tab.count})</span> : null}
            </button>
          ))}
        </div>

        {/* Orders Tab */}
        {bottomTab === 'orders' ? (
          <div className="terminal-panel terminal-scroll max-h-[320px] overflow-auto">
            <div className={`terminal-head ${densityMode === 'compact' ? 'terminal-head-dense' : 'terminal-head-comfy'} grid-cols-[1fr_0.4fr_0.4fr_0.3fr]`}>
              <div>挂单</div>
              <div>状态</div>
              <div>剩余</div>
              <div>动作</div>
            </div>
            {orders.length === 0 ? (
              <div className="p-6"><EmptyStatePanel title="暂无挂单" description="当前市场下没有属于你的挂单。" /></div>
            ) : (
              orders.map((order) => (
                <button
                  key={order.id}
                  type="button"
                  onClick={() => { setSelectedOrderId(order.id); appendMemory('挂单聚焦', `${order.side.toUpperCase()} ${formatNumber(order.remaining, amountDigits)} @ ${formatNumber(order.price, priceDigits)}`) }}
                  className={`terminal-row ${densityMode === 'compact' ? 'terminal-row-dense' : 'terminal-row-comfy'} terminal-row-interactive grid-cols-[1fr_0.4fr_0.4fr_0.3fr] text-left ${highlightedOrderSet.has(order.id) ? 'row-flash row-flash-neutral' : ''} ${selectedOrder?.id === order.id ? 'terminal-row-selected' : ''}`}
                >
                  <div>
                    <div className={`font-semibold ${order.side === 'buy' ? 'book-price-bid' : 'book-price-ask'}`}>
                      {order.side.toUpperCase()} {formatNumber(order.remaining, amountDigits)} @ {formatNumber(order.price, priceDigits)}
                    </div>
                    <div className="mt-0.5 text-xs text-neutral-500">已成交 {formatNumber(order.filled, amountDigits)} · {formatTime(order.createdAt)}</div>
                  </div>
                  <div className="text-sm text-neutral-700">{order.status}</div>
                  <div className="terminal-number-right text-black">{formatNumber(order.remaining, amountDigits)}</div>
                  <div>
                    <button type="button" disabled={isSubmitting} onClick={(e) => { e.stopPropagation(); void cancelOrder(order.id) }} className="action-light px-3 py-1 text-xs disabled:cursor-not-allowed disabled:opacity-60">撤单</button>
                  </div>
                </button>
              ))
            )}
          </div>
        ) : bottomTab === 'trades' ? (
          <div className="terminal-panel terminal-scroll max-h-[320px] overflow-auto">
            <div className={`terminal-head ${densityMode === 'compact' ? 'terminal-head-dense' : 'terminal-head-comfy'} grid-cols-[0.7fr_0.5fr_0.7fr_0.5fr_0.7fr]`}>
              <div>价格</div>
              <div>数量</div>
              <div>时间</div>
              <div>归属</div>
              <div>对手</div>
            </div>
            {trades.length === 0 ? (
              <div className="p-6"><EmptyStatePanel title="暂无成交" description="等待真实成交。" /></div>
            ) : (
              trades.map((trade, index) => {
                const perspective = classifyTradePerspective(trade, userId)
                return (
                  <button
                    key={trade.id}
                    type="button"
                    onClick={() => { setSelectedTradeId(trade.id); selectTradePrice(trade.price) }}
                    className={`terminal-row ${densityMode === 'compact' ? 'terminal-row-dense' : 'terminal-row-comfy'} terminal-row-interactive grid-cols-[0.7fr_0.5fr_0.7fr_0.5fr_0.7fr] text-left ${selectedTrade?.id === trade.id ? 'terminal-row-selected' : ''}`}
                  >
                    <div className={`terminal-number font-semibold ${index % 2 === 0 ? 'book-price-bid' : 'book-price-ask'}`}>{formatNumber(trade.price, priceDigits)}</div>
                    <div className="terminal-number-right text-black">{formatNumber(trade.amount, amountDigits)}</div>
                    <div className="text-xs text-neutral-500">{formatTime(trade.timestamp)}</div>
                    <div className={`text-xs font-medium ${perspective.tone === 'buy' ? 'book-price-bid' : perspective.tone === 'sell' ? 'book-price-ask' : 'text-neutral-500'}`}>{perspective.label}</div>
                    <div className="truncate text-xs text-neutral-500">{perspective.counterparty}</div>
                  </button>
                )
              })
            )}
          </div>
        ) : bottomTab === 'fills' ? (
          <div className="terminal-panel terminal-scroll max-h-[320px] overflow-auto">
            <div className={`terminal-head ${densityMode === 'compact' ? 'terminal-head-dense' : 'terminal-head-comfy'} grid-cols-[0.5fr_1fr_0.7fr]`}>
              <div>方向</div>
              <div>成交</div>
              <div>时间</div>
            </div>
            {fills.length === 0 ? (
              <div className="p-6"><EmptyStatePanel title="暂无成交回放" description="真实撮合后展示成交明细。" /></div>
            ) : (
              fills.map((fill) => (
                <div
                  key={fill.id}
                  className={`terminal-row ${densityMode === 'compact' ? 'terminal-row-dense' : 'terminal-row-comfy'} grid-cols-[0.5fr_1fr_0.7fr] ${highlightedFillSet.has(fill.id) ? 'row-flash row-flash-positive' : ''}`}
                >
                  <div className={`font-semibold ${fill.side === 'Buy' ? 'book-price-bid' : 'book-price-ask'}`}>{fill.side}</div>
                  <div className="terminal-number text-black">{formatNumber(fill.amount, amountDigits)} @ {formatNumber(fill.price, priceDigits)}</div>
                  <div className="text-xs text-neutral-500">{formatTime(fill.timestamp)}</div>
                </div>
              ))
            )}
          </div>
        ) : bottomTab === 'risk' ? (
          <div className="p-4 space-y-3 max-h-[320px] overflow-auto">
            <div className="grid gap-3 sm:grid-cols-2">
              {latencyDashboard.map((row) => (
                <div key={row.key} className="surface-soft px-4 py-3 rounded-xl">
                  <div className="text-xs text-neutral-500">{row.label}</div>
                  <div className="mt-1 data-mono font-semibold text-black">{row.latest === null ? '-' : `${formatNumber(row.latest, 0)}ms`}</div>
                  <div className="mt-0.5 text-[10px] text-neutral-400">p50 {row.p50 === null ? '-' : `${formatNumber(row.p50, 0)}ms`} · p95 {row.p95 === null ? '-' : `${formatNumber(row.p95, 0)}ms`}</div>
                </div>
              ))}
            </div>
            {fundingRates.slice(0, 2).map((item, index) => (
              <pre key={`funding-${index}`} className="overflow-auto rounded-xl border border-neutral-200 bg-white p-3 text-xs leading-6 text-neutral-700">{pretty(item)}</pre>
            ))}
            {riskEvents.slice(0, 2).map((item, index) => (
              <pre key={`risk-${index}`} className="overflow-auto rounded-xl border border-neutral-200 bg-white p-3 text-xs leading-6 text-neutral-700">{pretty(item)}</pre>
            ))}
            {fundingRates.length === 0 && riskEvents.length === 0 ? <EmptyStatePanel compact title="暂无风控快照" description="当前接口没有返回数据。" /> : null}
          </div>
        ) : (
          <div className="p-4 space-y-2 max-h-[320px] overflow-auto">
            {actionLog.length === 0 ? (
              <EmptyStatePanel compact title="暂无操作日志" description="提交、撤单或演示动作后记录。" />
            ) : (
              actionLog.slice(0, 20).map((entry) => (
                <div key={entry.id} className="surface-soft px-4 py-3 rounded-xl">
                  <div className="flex items-center justify-between gap-3">
                    <div className="text-sm font-medium text-black">{entry.title}</div>
                    <div className="text-xs text-neutral-500">{entry.time}</div>
                  </div>
                  <div className={`mt-1 text-sm ${entry.tone === 'danger' ? 'signal-negative' : entry.tone === 'success' ? 'signal-positive' : 'text-neutral-700'}`}>{entry.message}</div>
                </div>
              ))
            )}
          </div>
        )}
      </div>

      {/* ═══════════ EXPLORATION ZONE — Scrollable Enrichment ═══════════ */}
      <div className="mt-8 space-y-6">
        <div className="text-center">
          <div className="eyebrow">Exploration Zone</div>
          <div className="mt-1 text-sm text-neutral-500">深度分析与全局视野</div>
        </div>

        {/* ─── 1. Quant Lens / Anomaly Radar ─── */}
        <div className="section-frame">
          <div className="terminal-section-head">
            <div className="min-w-0">
              <div className="eyebrow">Quant Lens</div>
              <h3 className="mt-2 terminal-section-title">异常雷达与市场体征</h3>
            </div>
            <div className="premium-micro">adaptive diagnostics</div>
          </div>
          <div className="mt-5 grid gap-5 xl:grid-cols-[0.92fr_1.08fr]">
            <div className="chart-shell h-[320px]">
              <ResponsiveContainer width="100%" height="100%">
                <RadarChart data={anomalyRadarData}>
                  <PolarGrid stroke="#d4d4d4" />
                  <PolarAngleAxis dataKey="metric" tick={{ fill: '#525252', fontSize: 12 }} />
                  <Radar dataKey="score" stroke="#111111" fill="#111111" fillOpacity={0.12} strokeWidth={2} />
                  <Tooltip content={<PremiumTooltip />} />
                </RadarChart>
              </ResponsiveContainer>
            </div>
            <div className="quant-feed-grid">
              {anomalyFeed.map((item) => (
                <div key={item.label} className="quant-feed-card">
                  <div className="quant-feed-label">{item.label}</div>
                  <div className={`quant-feed-value ${item.tone === 'danger' ? 'signal-negative' : item.tone === 'success' ? 'signal-positive' : 'text-black'}`}>{item.value}</div>
                </div>
              ))}
              <div className="quant-feed-note">
                <div className="quant-feed-label">设计意图</div>
                <div className="quant-feed-copy">把点差、深度、余额、风控和链路状态浓缩成可扫一眼的异常画像。</div>
              </div>
            </div>
          </div>
        </div>

        {/* ─── 2. Account Structure ─── */}
        <div className="section-frame">
          <div className="terminal-section-head">
            <div className="min-w-0">
              <div className="eyebrow">Account Mix</div>
              <h3 className="mt-2 terminal-section-title">账户资产结构</h3>
            </div>
            <div className="premium-micro">{balanceChartData.length} 资产</div>
          </div>
          <div className="mt-5 grid gap-5 xl:grid-cols-2">
            <div className="chart-shell h-[280px]">
              {balanceChartData.length === 0 ? (
                <EmptyStatePanel title="暂无资产结构" description="等待余额数据。" />
              ) : (
                <ResponsiveContainer width="100%" height="100%">
                  <PieChart>
                    <Pie data={balanceChartData} dataKey="value" nameKey="name" innerRadius={64} outerRadius={92} paddingAngle={2}>
                      {balanceChartData.map((entry, index) => (
                        <Cell key={`${entry.name}-${index}`} fill={palette[index % palette.length]} />
                      ))}
                    </Pie>
                    <Tooltip content={<PremiumTooltip />} />
                  </PieChart>
                </ResponsiveContainer>
              )}
            </div>
            <div className="grid gap-3 content-start">
              {balanceChartData.map((entry, index) => (
                <div key={entry.name} className="flex items-center justify-between surface-soft px-4 py-3 rounded-xl">
                  <div className="flex items-center gap-2">
                    <span className="inline-block w-3 h-3 rounded-full" style={{ backgroundColor: palette[index % palette.length] }} />
                    <span className="font-medium text-sm text-black">{entry.name}</span>
                  </div>
                  <span className="data-mono text-sm">{formatNumber(entry.value, 6)}</span>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* ─── 3. Market Replay ─── */}
        <div className="section-frame">
          <div className="terminal-section-head">
            <div className="min-w-0">
              <div className="eyebrow">Market Replay</div>
              <h3 className="mt-2 terminal-section-title">成交回放与价格轨迹</h3>
            </div>
            <div className="premium-micro">{trades.length} 笔成交</div>
          </div>
          <div className="mt-5 h-[260px]">
            {trades.length === 0 ? (
              <EmptyStatePanel title="暂无成交回放" description="等待成交产生后回放。" />
            ) : (
              <ResponsiveContainer width="100%" height="100%">
                <ComposedChart
                  data={trades.slice().reverse().slice(-24).map((trade, index) => ({
                    name: `${index + 1}`,
                    price: trade.price,
                    amount: trade.amount,
                  }))}
                  margin={{ top: 8, right: 8, left: -18, bottom: 0 }}
                >
                  <CartesianGrid vertical={false} stroke="#ececec" />
                  <XAxis dataKey="name" axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                  <YAxis yAxisId="left" axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                  <YAxis yAxisId="right" orientation="right" axisLine={false} tickLine={false} fontSize={12} stroke="#a3a3a3" />
                  <Tooltip content={<PremiumTooltip />} />
                  {focusPrice ? <ReferenceLine yAxisId="left" y={focusPrice} stroke="#737373" strokeDasharray="3 3" ifOverflow="extendDomain" /> : null}
                  <Bar yAxisId="right" dataKey="amount" fill="#d4d4d4" radius={[8, 8, 0, 0]} />
                  <Line yAxisId="left" type="monotone" dataKey="price" stroke="#111111" strokeWidth={2} dot={false} />
                </ComposedChart>
              </ResponsiveContainer>
            )}
          </div>
          <div className="mt-4 grid grid-cols-2 gap-3 xl:grid-cols-4">
            {[
              { label: '最近闭环', value: closureSummary || '等待下一次动作' },
              { label: '执行准备度', value: executionReadiness },
              { label: '价格姿态', value: crossesSpread ? '更接近立即成交' : '更像挂单入簿' },
              { label: '链路模式', value: orderSource === 'api' ? '真实接口' : '本地回退' },
            ].map((item) => (
              <div key={item.label} className="surface-soft px-4 py-3 rounded-xl">
                <div className="text-[10px] text-neutral-500">{item.label}</div>
                <div className="mt-1 text-sm font-medium text-black">{item.value}</div>
              </div>
            ))}
          </div>
        </div>

        {/* ─── 4. Market Registry ─── */}
        <div className="section-frame">
          <div className="terminal-section-head">
            <div className="min-w-0">
              <div className="eyebrow">Registry & Coverage</div>
              <h3 className="mt-2 terminal-section-title">市场注册表与全局覆盖</h3>
            </div>
            <div className="premium-micro">{filteredMarkets.length} 市场</div>
          </div>
          <div className="mt-5 grid gap-5 xl:grid-cols-[1.2fr_0.8fr]">
            <div className="table-shell">
              <div className="table-head grid-cols-[1fr_0.4fr_0.4fr_0.3fr]">
                <div>市场</div>
                <div>品类</div>
                <div>链路</div>
                <div>状态</div>
              </div>
              {filteredMarkets.slice(0, 12).map((market) => (
                <div key={market.id} className={`table-row grid-cols-[1fr_0.4fr_0.4fr_0.3fr] ${selectedMarket?.id === market.id ? 'table-row-active' : ''}`}>
                  <div className="min-w-0">
                    <div className="truncate font-medium text-black">{market.name}</div>
                    <div className="mt-0.5 truncate text-xs text-neutral-500">{market.id}</div>
                  </div>
                  <div className="text-sm text-neutral-700">{kindLabel[market.kind]}</div>
                  <div className="text-sm text-neutral-700">{market.backendAvailable ? '真实' : '演示'}</div>
                  <div className={market.tradingEnabled ? 'signal-positive' : 'signal-negative'}>
                    {market.tradingEnabled ? '可交易' : '暂停'}
                  </div>
                </div>
              ))}
            </div>
            <div className="grid gap-3 sm:grid-cols-2 content-start">
              {[
                { label: '筛选后市场', value: String(filteredMarkets.length), hint: '当前左栏可见范围' },
                { label: '真实后端', value: String(liveBackendCount), hint: '具备真实链路' },
                { label: '可交易', value: String(tradableCount), hint: '当前允许下单' },
                { label: '收藏市场', value: String(favoriteMarketIds.length), hint: '已加入关注' },
              ].map((item) => (
                <div key={item.label} className="surface-card px-4 py-4 rounded-2xl">
                  <div className="eyebrow">{item.label}</div>
                  <div className="mt-2 text-lg font-semibold tracking-tight text-black">{item.value}</div>
                  <div className="mt-1 text-xs text-neutral-500">{item.hint}</div>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* ─── 5. Execution Context & Session Flow ─── */}
        <div className="section-frame">
          <div className="terminal-section-head">
            <div className="min-w-0">
              <div className="eyebrow">Execution Context</div>
              <h3 className="mt-2 terminal-section-title">执行上下文与会话流</h3>
            </div>
            <div className="premium-micro">lifecycle · memory · latency</div>
          </div>
          <div className="mt-5 grid gap-5 xl:grid-cols-3">
            {/* Lifecycle Timeline */}
            <div className="surface-soft p-5 rounded-2xl">
              <div className="eyebrow mb-3">生命周期</div>
              <div className="space-y-3">
                {lifecycleTimeline.map((step, index) => (
                  <div key={step.label} className="flex gap-3">
                    <div className={`flex-shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold ${index < 2 ? 'bg-black text-white' : 'bg-neutral-200 text-neutral-600'}`}>{index + 1}</div>
                    <div className="min-w-0">
                      <div className="text-xs font-medium text-black">{step.label}</div>
                      <div className="text-xs text-neutral-500">{step.title}</div>
                      <div className="text-xs text-neutral-400">{step.detail}</div>
                    </div>
                  </div>
                ))}
              </div>
            </div>
            {/* Terminal Memory */}
            <div className="surface-soft p-5 rounded-2xl">
              <div className="eyebrow mb-3">操作记忆 · {memoryEvents.length} 条</div>
              <div className="space-y-2 max-h-[300px] overflow-auto">
                {memoryEvents.length === 0 ? (
                  <div className="text-sm text-neutral-500">尚无操作轨迹</div>
                ) : (
                  memoryEvents.slice(0, 8).map((entry) => (
                    <div key={entry.id} className="surface-card px-3 py-2.5 rounded-xl">
                      <div className="flex items-center justify-between gap-2">
                        <div className="text-xs font-medium text-black truncate">{entry.label}</div>
                        <div className="text-[10px] text-neutral-500 flex-shrink-0">{entry.time}</div>
                      </div>
                      <div className="mt-1 text-xs text-neutral-600 truncate">{entry.detail}</div>
                    </div>
                  ))
                )}
              </div>
            </div>
            {/* Latency Dashboard */}
            <div className="surface-soft p-5 rounded-2xl">
              <div className="eyebrow mb-3">链路延迟</div>
              <div className="space-y-2">
                {latencyDashboard.map((row) => (
                  <div key={row.key} className="flex items-center justify-between">
                    <span className="text-xs font-medium text-black">{row.label}</span>
                    <div className="text-right">
                      <span className="data-mono text-xs font-medium text-black">{row.latest === null ? '-' : `${formatNumber(row.latest, 0)}ms`}</span>
                      <span className="ml-2 text-[10px] text-neutral-400">p50 {row.p50 === null ? '-' : `${formatNumber(row.p50, 0)}ms`} · p95 {row.p95 === null ? '-' : `${formatNumber(row.p95, 0)}ms`}</span>
                    </div>
                  </div>
                ))}
              </div>
              <div className="mt-4 grid grid-cols-2 gap-2">
                {terminalFooterItems.slice(0, 4).map((item) => (
                  <div key={item.label} className="surface-card px-3 py-2.5 rounded-xl">
                    <div className="text-[10px] text-neutral-500">{item.label}</div>
                    <div className="mt-1 text-sm font-semibold text-black">{item.value}</div>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* ─── 6. Microstructure Intelligence ─── */}
        <div className="section-frame">
          <div className="terminal-section-head">
            <div className="min-w-0">
              <div className="eyebrow">Microstructure Intel</div>
              <h3 className="mt-2 terminal-section-title">盘口微观洞察</h3>
            </div>
            <div className="premium-micro">{orderBookIntelItems.length} metrics · {tacticalCues.length} cues</div>
          </div>
          <div className="mt-5 grid gap-5 xl:grid-cols-2">
            <div>
              <div className="grid grid-cols-2 gap-3">
                {orderBookIntelItems.map((item) => (
                  <div key={item.label} className="book-intel-stat surface-soft px-4 py-3 rounded-xl">
                    <div className="text-[10px] text-neutral-500">{item.label}</div>
                    <div className="mt-1 data-mono text-sm font-semibold text-black">{item.value}</div>
                    <div className="mt-0.5 text-[10px] text-neutral-400">{item.hint}</div>
                  </div>
                ))}
              </div>
              {orderBookIntelNarrative ? (
                <div className="mt-3 surface-soft px-4 py-3 rounded-xl">
                  <div className="text-[10px] text-neutral-500">Quick Read</div>
                  <div className="mt-1 text-sm text-neutral-700">{orderBookIntelNarrative}</div>
                </div>
              ) : null}
            </div>
            <div className="grid grid-cols-2 gap-3 content-start">
              {tacticalCues.map((cue) => (
                <div key={cue.label} className="surface-soft px-4 py-3 rounded-xl">
                  <div className="text-[10px] text-neutral-500">{cue.label}</div>
                  <div className="mt-1 text-sm font-semibold text-black">{cue.value}</div>
                  <div className="mt-0.5 text-[10px] text-neutral-400">{cue.hint}</div>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* ─── 7. Focus Spotlight ─── */}
        <div className="section-frame">
          <div className="terminal-section-head">
            <div className="min-w-0">
              <div className="eyebrow">{focusSpotlight.eyebrow}</div>
              <h3 className="mt-2 terminal-section-title">{focusSpotlight.title}</h3>
            </div>
            <div className="premium-micro">联动焦点</div>
          </div>
          <div className="mt-5 grid gap-5 xl:grid-cols-[1fr_1fr]">
            <div>
              <div className="text-sm leading-7 text-neutral-700">{focusSpotlight.summary}</div>
              <div className="mt-3 space-y-2">
                {focusSpotlight.bullets.map((item) => (
                  <div key={item} className="flex items-start gap-2">
                    <span className="mt-1.5 inline-block w-1.5 h-1.5 rounded-full bg-black flex-shrink-0" />
                    <span className="text-sm text-neutral-700">{item}</span>
                  </div>
                ))}
              </div>
            </div>
            <div className="grid grid-cols-2 gap-3 content-start">
              {executionContextItems.map((item) => (
                <div key={item.label} className="surface-soft px-4 py-3 rounded-xl">
                  <div className="text-[10px] text-neutral-500">{item.label}</div>
                  <div className="mt-1 text-sm font-semibold text-black">{item.value}</div>
                  <div className="mt-0.5 text-[10px] text-neutral-400">{item.hint}</div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* ═══════════ COMMAND PALETTE ═══════════ */}
      {paletteOpen ? (
        <div
          className="palette-overlay"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setPaletteOpen(false)
          }}
        >
          <div className="palette-shell">
            <div className="flex items-end justify-between gap-4">
              <div>
                <div className="eyebrow">Command Palette</div>
                <div className="mt-2 text-base font-semibold tracking-[-0.03em] text-black">指令面板</div>
              </div>
              <div className="premium-micro">Ctrl/Cmd + K</div>
            </div>
            <div className="mt-4">
              <input
                ref={paletteInputRef}
                value={paletteQuery}
                onChange={(event) => {
                  setPaletteQuery(event.target.value)
                  setPaletteIndex(0)
                }}
                className="palette-input"
                placeholder="搜索命令：例如 刷新 / 买一 / 提交 / 撤单"
              />
            </div>
            <div className="palette-list mt-4">
              {paletteItems.length === 0 ? (
                <div className="palette-empty">
                  <div className="text-sm font-medium text-black">没有匹配的命令</div>
                  <div className="mt-2 text-sm leading-7 text-neutral-600">换个关键词试试，例如 "撤单"、"买一"、"演示"。</div>
                </div>
              ) : (
                paletteSections.map((section) => (
                  <div key={section.id} className="palette-section">
                    <div className="palette-section-title">{section.title}</div>
                    <div className="palette-section-items">
                      {section.items.map((command) => {
                        const flatIndex = paletteItems.findIndex((item) => item.id === command.id)
                        return (
                          <button
                            key={command.id}
                            type="button"
                            disabled={command.disabled}
                            onClick={() => {
                              if (command.disabled) return
                              void command.run()
                              appendMemory('指令面板', `执行：${command.label}`)
                              addRecentCommand(command.id)
                              setPaletteOpen(false)
                            }}
                            className={`palette-item ${flatIndex === paletteIndex ? 'palette-item-active' : ''}`}
                          >
                            <div className="min-w-0">
                              <div className="palette-item-title">
                                {command.label}
                                {command.disabled ? <span className="palette-item-disabled">不可用</span> : null}
                              </div>
                              <div className="palette-item-copy">{command.detail}</div>
                            </div>
                            <div className="palette-item-shortcut">{command.shortcut ?? ''}</div>
                          </button>
                        )
                      })}
                    </div>
                  </div>
                ))
              )}
            </div>
            <div className="palette-foot mt-4">
              <span className="mono-chip">↑ ↓</span>
              <span className="text-xs text-neutral-600">选择</span>
              <span className="mono-chip">Enter</span>
              <span className="text-xs text-neutral-600">执行</span>
              <span className="mono-chip">Esc</span>
              <span className="text-xs text-neutral-600">关闭</span>
            </div>
          </div>
        </div>
      ) : null}
    </AppShell>
  )
}
