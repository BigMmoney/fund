import { useState, useEffect, useCallback, useRef } from 'react'
import axios from 'axios'
import { 
  AlertTriangle, Bell,
  Search, X, Plus, Minus, ArrowUpRight, ArrowDownRight
} from 'lucide-react'
import { CRYPTO_ASSETS, STOCK_ASSETS, COMMODITY_ASSETS } from '@/config'
import { formatCurrency, formatPercentage, formatRelativeTime } from '@/lib/utils'
import { useTheme } from '@/contexts/ThemeContext'
import { useLanguage } from '@/contexts/LanguageContext'
import { useNotifications } from '@/contexts/NotificationContext'
import { useAccount } from '@/contexts/AccountContext'
import { useMarketStatus } from '@/contexts/MarketStatusContext'
import { GlobalNavbar } from '@/components/GlobalNavbar'
import { ChartTypeSelector, getAssetIcon } from '@/components/ProfessionalChart'
import { LightweightChart } from '@/components/LightweightChart'
import { AccountBreakdown } from '@/components/AccountBreakdown'
import { RiskMeter } from '@/components/RiskMeter'
import { MarketStatusBar } from '@/components/MarketStatusBar'
import { EventTags } from '@/components/EventTags'
import { 
  generateMarketEvents, 
  calculateExecutionFee,
  simulateOrderExecution,
  generateRealisticOrderBook,
  type MarketEvent
} from '@/lib/tradingLogic'

interface PriceData {
  [key: string]: {
    price: number
    change: number
    changePercent: number
    volume: number
    high24h: number
    low24h: number
    bid: number
    ask: number
    spread: number
  }
}

interface Order {
  id: string
  symbol: string
  side: 'BUY' | 'SELL'
  type: 'LIMIT' | 'MARKET' | 'STOP-MKT' | 'STOP-LMT' | 'IOC' | 'FOK'
  price: number
  qty: number
  filled: number
  status: 'NEW' | 'PARTIAL' | 'FILLED' | 'CANCELLED'
  time: string
  isOpen: boolean // true = opening position, false = closing
}

interface Position {
  symbol: string
  qty: number
  avgPrice: number
  currentPrice: number
  pnl: number
  pnlPercent: number
  value: number
  // New professional fields
  liquidationPrice?: number
  positionType: 'STRATEGY' | 'MANUAL'
  pricePnL?: number
  feePnL?: number
  fundingPnL?: number
  openTime?: number
}

interface Trade {
  id: string
  symbol: string
  side: 'BUY' | 'SELL'
  price: number
  qty: number
  time: string
}

interface Alert {
  id: string
  symbol: string
  type: 'PRICE_ABOVE' | 'PRICE_BELOW' | 'VOLUME_SPIKE'
  value: number
  active: boolean
}

export function TradingTerminal() {
  useTheme()
  const { t } = useLanguage()
  const { addNotification } = useNotifications()
  const account = useAccount()
  const { getMarketStatus } = useMarketStatus()
  const [priceData, setPriceData] = useState<PriceData>({})
  const [selectedSymbol, setSelectedSymbol] = useState('BTC')
  const [orderSide, setOrderSide] = useState<'BUY' | 'SELL'>('BUY')
  const [orderType, setOrderType] = useState<'LIMIT' | 'MARKET' | 'STOP-MKT' | 'STOP-LMT' | 'IOC' | 'FOK'>('LIMIT')
  const [orderPrice, setOrderPrice] = useState('')
  const [orderQty, setOrderQty] = useState('')
  const [orders, setOrders] = useState<Order[]>([])
  const [positions, setPositions] = useState<Position[]>([])
  const [trades, setTrades] = useState<Trade[]>([])
  const [alerts, setAlerts] = useState<Alert[]>([])
  const [totalPnL, setTotalPnL] = useState(0)
  const [hotkeys] = useState(true)
  const [marketEvents, setMarketEvents] = useState<MarketEvent[]>([])
  
  // New professional trading states
  const [qtyMode, setQtyMode] = useState<'COIN' | 'USD' | 'EQUITY'>('COIN')
  const [accountMode] = useState<'LIVE' | 'PAPER' | 'INTERNAL'>('LIVE')
  const [killSwitchActive, setKillSwitchActive] = useState(false)
  const [maxLossToday] = useState(25000)
  const [realizedLossToday] = useState(0)
  const [lastOrderAckTime] = useState<number>(Date.now())
  const [marketDataLag] = useState(12)
  const [tradingLag] = useState(45)
  const [venueLatencies] = useState({ binance: 8, okx: 15, kraken: 28 })
  const [packetLossPercent] = useState(0.02)
  const [isReplayMode] = useState(false)
  
  // New global status bar states
  const [currentVenue, setCurrentVenue] = useState<'AUTO' | 'BINANCE' | 'OKX' | 'KRAKEN'>('AUTO')
  const [dataSource, setDataSource] = useState<'PRIMARY' | 'BACKUP'>('PRIMARY')
  const [priceScaleMode, setPriceScaleMode] = useState<'linear' | 'log'>('linear')
  const [showCompare, setShowCompare] = useState(false)
  const [compareSymbol] = useState<string | null>(null)
  
  // Watchlist filter states
  const [watchlistFilter, setWatchlistFilter] = useState<'ALL' | 'POSITIONS' | 'MOVERS' | 'FAVORITES'>('ALL')
  const [cryptoCollapsed, setCryptoCollapsed] = useState(false)
  const [equityCollapsed, setEquityCollapsed] = useState(false)
  const [commodityCollapsed, setCommodityCollapsed] = useState(false)
  const [favorites] = useState<string[]>(['BTC', 'ETH', 'AAPL', 'XAU', 'XAG'])
  
  // News state - symbol-specific with sentiment indicators
  interface NewsItem {
    id: string
    symbol: string
    headline: string
    source: string
    time: Date
    sentiment: 'BULLISH' | 'BEARISH' | 'NEUTRAL'
    riskImpact: 'UP' | 'DOWN' | 'NEUTRAL'
    category: 'MARKET' | 'REGULATORY' | 'TECHNICAL' | 'MACRO' | 'EARNINGS' | 'ONCHAIN' | 'INSTITUTIONAL'
    reliability: number // 0-100
  }
  
  // Social Sentiment Data - X, YouTube, Reddit, TikTok (增强版)
  interface SocialSentiment {
    symbol: string
    // X (Twitter)
    xMentions: number
    xSentiment: number // -100 to 100
    xTrending: boolean
    xTopics: { tag: string; type: 'FOMO' | 'FUD' | 'ANALYSIS' | 'MEME' | 'NEWS' }[]
    xVelocity: number  // 提及增速 %/hour
    // YouTube
    youtubeSentiment: number // -100 to 100
    youtubeViews24h: number
    youtubeCreators: string[]
    youtubeNewVideos: number  // 24h内新视频数
    // Reddit
    redditMentions: number
    redditSentiment: number // -100 to 100
    redditSubreddits: string[]
    redditUpvoteRatio: number  // 平均点赞率
    // TikTok (新增)
    tiktokMentions: number
    tiktokSentiment: number // -100 to 100
    tiktokViews24h: number
    tiktokTrending: boolean
    tiktokHashtags: string[]
    // Overall
    overallSocialScore: number // -100 to 100
    lastUpdated: number  // 最后更新时间戳
  }
  
  // 平台权重配置
  interface PlatformWeights {
    x: number
    youtube: number
    reddit: number
    tiktok: number
  }
  
  // 走势吻合度分析结果
  interface TrendMatchAnalysis {
    score: number  // 0-100
    verdict: 'ALIGNED' | 'DIVERGENT' | 'NEUTRAL'
    details: {
      platform: string
      sentiment: number
      priceDirection: 'UP' | 'DOWN' | 'FLAT'
      match: boolean
    }[]
    confidence: number
    lastCalculated: number
  }
  
  // Symbol-specific indicators
  interface SymbolIndicators {
    symbol: string
    momentum: number // -100 to 100
    volatility: 'LOW' | 'MEDIUM' | 'HIGH' | 'EXTREME'
    volatilityValue: number
    whaleActivity: 'ACCUMULATING' | 'DISTRIBUTING' | 'NEUTRAL'
    whaleNetFlow: number // in millions
    fundingRate: number
    openInterestChange: number // percentage
    liquidations24h: { long: number; short: number }
    trendMatchScore: number // 0-100 - how well sentiment matches price trend
  }
  
  const [newsData] = useState<NewsItem[]>([
    // BTC News - Multiple Sources
    { id: 'n1', symbol: 'BTC', headline: 'BlackRock Bitcoin ETF sees record $1.2B daily inflow', source: 'Bloomberg', time: new Date(Date.now() - 300000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'INSTITUTIONAL', reliability: 95 },
    { id: 'n2', symbol: 'BTC', headline: 'SEC postpones decision on spot Bitcoin ETF options', source: 'Reuters', time: new Date(Date.now() - 900000), sentiment: 'BEARISH', riskImpact: 'UP', category: 'REGULATORY', reliability: 98 },
    { id: 'n3', symbol: 'BTC', headline: 'Bitcoin hash rate reaches new ATH of 620 EH/s', source: 'Glassnode', time: new Date(Date.now() - 1800000), sentiment: 'BULLISH', riskImpact: 'NEUTRAL', category: 'ONCHAIN', reliability: 92 },
    { id: 'n4', symbol: 'BTC', headline: 'MicroStrategy acquires additional 12,000 BTC', source: 'CNBC', time: new Date(Date.now() - 3600000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'INSTITUTIONAL', reliability: 96 },
    { id: 'n5', symbol: 'BTC', headline: 'Fed signals potential rate cut in September', source: 'WSJ', time: new Date(Date.now() - 7200000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'MACRO', reliability: 97 },
    { id: 'n6', symbol: 'BTC', headline: 'Mt. Gox creditors begin receiving Bitcoin distributions', source: 'Decrypt', time: new Date(Date.now() - 10800000), sentiment: 'BEARISH', riskImpact: 'UP', category: 'MARKET', reliability: 85 },
    { id: 'n19', symbol: 'BTC', headline: 'Whale wallets accumulate 45,000 BTC in 72 hours', source: 'Santiment', time: new Date(Date.now() - 1200000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'ONCHAIN', reliability: 88 },
    { id: 'n20', symbol: 'BTC', headline: 'CME Bitcoin futures open interest hits record high', source: 'The Block', time: new Date(Date.now() - 2100000), sentiment: 'NEUTRAL', riskImpact: 'UP', category: 'MARKET', reliability: 90 },
    { id: 'n21', symbol: 'BTC', headline: 'Fidelity reports institutional demand surge for BTC', source: 'Financial Times', time: new Date(Date.now() - 4500000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'INSTITUTIONAL', reliability: 94 },
    { id: 'n22', symbol: 'BTC', headline: 'Bitcoin mining difficulty adjustment incoming +3.2%', source: 'CryptoQuant', time: new Date(Date.now() - 5400000), sentiment: 'NEUTRAL', riskImpact: 'NEUTRAL', category: 'TECHNICAL', reliability: 91 },
    // ETH News
    { id: 'n7', symbol: 'ETH', headline: 'Ethereum L2 TVL surpasses $40B milestone', source: 'DefiLlama', time: new Date(Date.now() - 600000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'TECHNICAL', reliability: 89 },
    { id: 'n8', symbol: 'ETH', headline: 'Vitalik proposes new gas limit increase', source: 'CoinDesk', time: new Date(Date.now() - 1200000), sentiment: 'NEUTRAL', riskImpact: 'NEUTRAL', category: 'TECHNICAL', reliability: 87 },
    { id: 'n9', symbol: 'ETH', headline: 'Grayscale Ethereum ETF sees $50M outflows', source: 'Bloomberg', time: new Date(Date.now() - 2400000), sentiment: 'BEARISH', riskImpact: 'UP', category: 'INSTITUTIONAL', reliability: 95 },
    { id: 'n10', symbol: 'ETH', headline: 'Major DeFi protocol announces ETH staking rewards boost', source: 'Messari', time: new Date(Date.now() - 4800000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'MARKET', reliability: 82 },
    { id: 'n23', symbol: 'ETH', headline: 'Ethereum burns 2,500 ETH in single day, highest in months', source: 'Ultrasound.money', time: new Date(Date.now() - 1500000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'ONCHAIN', reliability: 93 },
    { id: 'n24', symbol: 'ETH', headline: 'Base L2 transaction volume exceeds Ethereum mainnet', source: 'Dune Analytics', time: new Date(Date.now() - 3200000), sentiment: 'BULLISH', riskImpact: 'NEUTRAL', category: 'TECHNICAL', reliability: 90 },
    // SOL News
    { id: 'n11', symbol: 'SOL', headline: 'Solana processes 65M transactions in 24h, new record', source: 'The Block', time: new Date(Date.now() - 450000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'TECHNICAL', reliability: 88 },
    { id: 'n12', symbol: 'SOL', headline: 'Jump Trading reduces Solana validator operations', source: 'CoinDesk', time: new Date(Date.now() - 1500000), sentiment: 'BEARISH', riskImpact: 'UP', category: 'MARKET', reliability: 86 },
    { id: 'n25', symbol: 'SOL', headline: 'Solana DEX volume surpasses Ethereum for 3rd consecutive day', source: 'DefiLlama', time: new Date(Date.now() - 800000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'MARKET', reliability: 91 },
    { id: 'n26', symbol: 'SOL', headline: 'Major memecoin launch drives Solana gas fees 10x', source: 'Blockworks', time: new Date(Date.now() - 2800000), sentiment: 'NEUTRAL', riskImpact: 'UP', category: 'MARKET', reliability: 84 },
    // AAPL News
    { id: 'n13', symbol: 'AAPL', headline: 'Apple Vision Pro sales exceed analyst expectations', source: 'CNBC', time: new Date(Date.now() - 800000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'EARNINGS', reliability: 93 },
    { id: 'n14', symbol: 'AAPL', headline: 'EU imposes €1.8B antitrust fine on Apple', source: 'Reuters', time: new Date(Date.now() - 2000000), sentiment: 'BEARISH', riskImpact: 'UP', category: 'REGULATORY', reliability: 98 },
    { id: 'n27', symbol: 'AAPL', headline: 'Apple AI features driving iPhone 16 upgrade cycle', source: 'Morgan Stanley', time: new Date(Date.now() - 1100000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'INSTITUTIONAL', reliability: 89 },
    { id: 'n28', symbol: 'AAPL', headline: 'Warren Buffett trims Apple stake by additional 5%', source: 'SEC Filing', time: new Date(Date.now() - 4200000), sentiment: 'BEARISH', riskImpact: 'UP', category: 'INSTITUTIONAL', reliability: 100 },
    // TSLA News  
    { id: 'n15', symbol: 'TSLA', headline: 'Tesla Cybertruck deliveries accelerate in Q2', source: 'Bloomberg', time: new Date(Date.now() - 700000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'EARNINGS', reliability: 94 },
    { id: 'n16', symbol: 'TSLA', headline: 'NHTSA opens investigation into Tesla Autopilot', source: 'WSJ', time: new Date(Date.now() - 3000000), sentiment: 'BEARISH', riskImpact: 'UP', category: 'REGULATORY', reliability: 97 },
    { id: 'n29', symbol: 'TSLA', headline: 'Tesla FSD v13 receives approval in Germany', source: 'Electrek', time: new Date(Date.now() - 1300000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'REGULATORY', reliability: 82 },
    { id: 'n30', symbol: 'TSLA', headline: 'Cathie Wood ARK adds $50M TSLA position', source: 'ARK Invest', time: new Date(Date.now() - 2600000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'INSTITUTIONAL', reliability: 100 },
    // NVDA News
    { id: 'n17', symbol: 'NVDA', headline: 'NVIDIA announces next-gen Blackwell GPU availability', source: 'TechCrunch', time: new Date(Date.now() - 500000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'TECHNICAL', reliability: 88 },
    { id: 'n18', symbol: 'NVDA', headline: 'China restrictions may impact NVIDIA Q4 revenue', source: 'Reuters', time: new Date(Date.now() - 2500000), sentiment: 'BEARISH', riskImpact: 'UP', category: 'REGULATORY', reliability: 96 },
    { id: 'n31', symbol: 'NVDA', headline: 'Microsoft increases NVIDIA chip orders by 40%', source: 'The Information', time: new Date(Date.now() - 900000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'INSTITUTIONAL', reliability: 85 },
    { id: 'n32', symbol: 'NVDA', headline: 'NVIDIA stock added to Dow Jones Industrial Average', source: 'CNBC', time: new Date(Date.now() - 3800000), sentiment: 'BULLISH', riskImpact: 'DOWN', category: 'MARKET', reliability: 100 },
  ])
  
  // Social Sentiment Data (增强版 - 含TikTok和实时更新)
  const [socialSentiment, setSocialSentiment] = useState<Record<string, SocialSentiment>>({
    'BTC': {
      symbol: 'BTC',
      xMentions: 125840,
      xSentiment: 68,
      xTrending: true,
      xVelocity: 15.2,
      xTopics: [
        { tag: '#Bitcoin', type: 'NEWS' },
        { tag: '#BTCto100k', type: 'FOMO' },
        { tag: '#CryptoWinter', type: 'FUD' },
        { tag: '#BitcoinETF', type: 'ANALYSIS' },
      ],
      youtubeSentiment: 72,
      youtubeViews24h: 2450000,
      youtubeCreators: ['Coin Bureau', 'Ben Cowen', 'DataDash'],
      youtubeNewVideos: 28,
      redditMentions: 8540,
      redditSentiment: 58,
      redditSubreddits: ['r/Bitcoin', 'r/CryptoCurrency', 'r/BitcoinMarkets'],
      redditUpvoteRatio: 0.82,
      tiktokMentions: 45200,
      tiktokSentiment: 75,
      tiktokViews24h: 8500000,
      tiktokTrending: true,
      tiktokHashtags: ['#bitcoin', '#crypto', '#btc', '#investing'],
      overallSocialScore: 68,
      lastUpdated: Date.now(),
    },
    'ETH': {
      symbol: 'ETH',
      xMentions: 78500,
      xSentiment: 45,
      xTrending: false,
      xVelocity: 3.5,
      xTopics: [
        { tag: '#Ethereum', type: 'NEWS' },
        { tag: '#ETH2', type: 'ANALYSIS' },
        { tag: '#DeFi', type: 'NEWS' },
      ],
      youtubeSentiment: 52,
      youtubeViews24h: 980000,
      youtubeCreators: ['Bankless', 'The Daily Gwei'],
      youtubeNewVideos: 12,
      redditMentions: 5230,
      redditSentiment: 48,
      redditSubreddits: ['r/ethereum', 'r/ethfinance'],
      redditUpvoteRatio: 0.75,
      tiktokMentions: 18500,
      tiktokSentiment: 52,
      tiktokViews24h: 2200000,
      tiktokTrending: false,
      tiktokHashtags: ['#ethereum', '#eth', '#defi'],
      overallSocialScore: 49,
      lastUpdated: Date.now(),
    },
    'SOL': {
      symbol: 'SOL',
      xMentions: 45200,
      xSentiment: 78,
      xTrending: true,
      xVelocity: 28.5,
      xTopics: [
        { tag: '#Solana', type: 'FOMO' },
        { tag: '#SOL', type: 'MEME' },
        { tag: '#SolanaMemecoin', type: 'FOMO' },
      ],
      youtubeSentiment: 82,
      youtubeViews24h: 1250000,
      youtubeCreators: ['Altcoin Daily', 'Crypto Banter'],
      youtubeNewVideos: 22,
      redditMentions: 3820,
      redditSentiment: 71,
      redditSubreddits: ['r/solana', 'r/CryptoCurrency'],
      redditUpvoteRatio: 0.88,
      tiktokMentions: 68000,
      tiktokSentiment: 85,
      tiktokViews24h: 15000000,
      tiktokTrending: true,
      tiktokHashtags: ['#solana', '#sol', '#memecoin', '#crypto'],
      overallSocialScore: 79,
      lastUpdated: Date.now(),
    },
    'AAPL': {
      symbol: 'AAPL',
      xMentions: 32100,
      xSentiment: 35,
      xTrending: false,
      xVelocity: -2.1,
      xTopics: [
        { tag: '#Apple', type: 'NEWS' },
        { tag: '#AAPL', type: 'ANALYSIS' },
      ],
      youtubeSentiment: 42,
      youtubeViews24h: 520000,
      youtubeCreators: ['MKBHD', 'Linus Tech Tips'],
      youtubeNewVideos: 5,
      redditMentions: 1850,
      redditSentiment: 38,
      redditSubreddits: ['r/apple', 'r/stocks', 'r/wallstreetbets'],
      redditUpvoteRatio: 0.72,
      tiktokMentions: 12000,
      tiktokSentiment: 45,
      tiktokViews24h: 1800000,
      tiktokTrending: false,
      tiktokHashtags: ['#apple', '#iphone', '#stocks'],
      overallSocialScore: 40,
      lastUpdated: Date.now(),
    },
    'TSLA': {
      symbol: 'TSLA',
      xMentions: 89400,
      xSentiment: 55,
      xTrending: true,
      xVelocity: 12.8,
      xTopics: [
        { tag: '#Tesla', type: 'NEWS' },
        { tag: '#TSLA', type: 'FOMO' },
        { tag: '#ElonMusk', type: 'MEME' },
        { tag: '#Cybertruck', type: 'NEWS' },
      ],
      youtubeSentiment: 62,
      youtubeViews24h: 1850000,
      youtubeCreators: ['Tesla Daily', 'Solving The Money Problem'],
      youtubeNewVideos: 18,
      redditMentions: 12500,
      redditSentiment: 45,
      redditSubreddits: ['r/teslainvestorsclub', 'r/wallstreetbets', 'r/stocks'],
      redditUpvoteRatio: 0.68,
      tiktokMentions: 95000,
      tiktokSentiment: 58,
      tiktokViews24h: 22000000,
      tiktokTrending: true,
      tiktokHashtags: ['#tesla', '#elonmusk', '#cybertruck', '#ev'],
      overallSocialScore: 55,
      lastUpdated: Date.now(),
    },
    'NVDA': {
      symbol: 'NVDA',
      xMentions: 56800,
      xSentiment: 82,
      xTrending: true,
      xVelocity: 22.5,
      xTopics: [
        { tag: '#NVIDIA', type: 'FOMO' },
        { tag: '#AI', type: 'NEWS' },
        { tag: '#Blackwell', type: 'ANALYSIS' },
      ],
      youtubeSentiment: 85,
      youtubeViews24h: 1680000,
      youtubeCreators: ['Meet Kevin', 'Tom Nash'],
      youtubeNewVideos: 25,
      redditMentions: 8900,
      redditSentiment: 78,
      redditSubreddits: ['r/nvidia', 'r/wallstreetbets', 'r/stocks'],
      redditUpvoteRatio: 0.91,
      tiktokMentions: 42000,
      tiktokSentiment: 88,
      tiktokViews24h: 9500000,
      tiktokTrending: true,
      tiktokHashtags: ['#nvidia', '#ai', '#stocks', '#investing'],
      overallSocialScore: 83,
      lastUpdated: Date.now(),
    },
  })
  
  // 走势吻合度分析状态
  const [trendMatchAnalysis, setTrendMatchAnalysis] = useState<Record<string, TrendMatchAnalysis>>({})
  
  // 平台权重（可调整）
  const [platformWeights] = useState<PlatformWeights>({
    x: 0.30,       // X权重30%
    youtube: 0.25, // YouTube权重25%
    reddit: 0.25,  // Reddit权重25%
    tiktok: 0.20   // TikTok权重20%
  })
  
  // Symbol Indicators with Trend Match Score
  const [symbolIndicators] = useState<Record<string, SymbolIndicators>>({
    'BTC': {
      symbol: 'BTC',
      momentum: 65,
      volatility: 'MEDIUM',
      volatilityValue: 42,
      whaleActivity: 'ACCUMULATING',
      whaleNetFlow: 125.5,
      fundingRate: 0.012,
      openInterestChange: 8.5,
      liquidations24h: { long: 12.5, short: 45.2 },
      trendMatchScore: 78,
    },
    'ETH': {
      symbol: 'ETH',
      momentum: 32,
      volatility: 'MEDIUM',
      volatilityValue: 38,
      whaleActivity: 'NEUTRAL',
      whaleNetFlow: -18.2,
      fundingRate: 0.008,
      openInterestChange: 2.1,
      liquidations24h: { long: 8.2, short: 15.6 },
      trendMatchScore: 52,
    },
    'SOL': {
      symbol: 'SOL',
      momentum: 82,
      volatility: 'HIGH',
      volatilityValue: 68,
      whaleActivity: 'ACCUMULATING',
      whaleNetFlow: 45.8,
      fundingRate: 0.025,
      openInterestChange: 15.2,
      liquidations24h: { long: 5.5, short: 28.4 },
      trendMatchScore: 85,
    },
    'AAPL': {
      symbol: 'AAPL',
      momentum: -15,
      volatility: 'LOW',
      volatilityValue: 18,
      whaleActivity: 'DISTRIBUTING',
      whaleNetFlow: -85.2,
      fundingRate: 0,
      openInterestChange: -2.5,
      liquidations24h: { long: 0, short: 0 },
      trendMatchScore: 35,
    },
    'TSLA': {
      symbol: 'TSLA',
      momentum: 45,
      volatility: 'HIGH',
      volatilityValue: 72,
      whaleActivity: 'NEUTRAL',
      whaleNetFlow: 12.5,
      fundingRate: 0,
      openInterestChange: 5.8,
      liquidations24h: { long: 0, short: 0 },
      trendMatchScore: 62,
    },
    'NVDA': {
      symbol: 'NVDA',
      momentum: 88,
      volatility: 'MEDIUM',
      volatilityValue: 45,
      whaleActivity: 'ACCUMULATING',
      whaleNetFlow: 220.5,
      fundingRate: 0,
      openInterestChange: 12.5,
      liquidations24h: { long: 0, short: 0 },
      trendMatchScore: 92,
    },
  })
  
  // ==================== MARKET STATE - THE NEURAL CENTER ====================
  // This is the master decision block that governs ALL other signals
  interface MarketState {
    trend: 'TREND_UP' | 'TREND_DOWN' | 'RANGE' | 'BREAKOUT'
    confidence: number // 0-1
    horizon: string // e.g., "15-30m", "1-4h"
    riskMode: 'NORMAL' | 'CAUTION' | 'PROTECT'
    riskReason?: string
    maxPositionSize?: number // in base currency
    factors: { name: string; value: string; direction: '+' | '-' | '=' }[]
    // 新增机构级字段
    regime: 'TRENDING' | 'RANGE' | 'EVENT' | 'VOLATILE'
    decisionTime: number // timestamp when decision was made
    riskBudgetUsed: number // 0-100 percentage
  }
  
  const [marketState] = useState<Record<string, MarketState>>({
    'BTC': {
      trend: 'TREND_UP',
      confidence: 0.78,
      horizon: '15-30m',
      riskMode: 'NORMAL',
      maxPositionSize: 2.5,
      regime: 'TRENDING',
      decisionTime: Date.now() - 8 * 60 * 1000, // 8 minutes ago
      riskBudgetUsed: 63,
      factors: [
        { name: 'ADX', value: '36', direction: '+' },
        { name: 'MA Slope', value: '+0.8%', direction: '+' },
        { name: 'Vol Confirm', value: 'YES', direction: '+' },
      ]
    },
    'ETH': {
      trend: 'RANGE',
      confidence: 0.52,
      horizon: '1-2h',
      riskMode: 'CAUTION',
      riskReason: 'Low conviction + wide spread',
      maxPositionSize: 8.0,
      regime: 'RANGE',
      decisionTime: Date.now() - 8 * 60 * 1000, // 8 minutes ago
      riskBudgetUsed: 45,
      factors: [
        { name: 'ADX', value: '18', direction: '=' },
        { name: 'MA Slope', value: '+0.1%', direction: '=' },
        { name: 'Vol Confirm', value: 'WEAK', direction: '-' },
      ]
    },
    'SOL': {
      trend: 'BREAKOUT',
      confidence: 0.85,
      horizon: '5-15m',
      riskMode: 'CAUTION',
      riskReason: 'High volatility spike',
      maxPositionSize: 150,
      regime: 'VOLATILE',
      decisionTime: Date.now() - 3 * 60 * 1000, // 3 minutes ago
      riskBudgetUsed: 85,
      factors: [
        { name: 'ADX', value: '52', direction: '+' },
        { name: 'MA Slope', value: '+2.1%', direction: '+' },
        { name: 'Vol Confirm', value: 'STRONG', direction: '+' },
      ]
    },
    'AAPL': {
      trend: 'TREND_DOWN',
      confidence: 0.42,
      horizon: '1-4h',
      riskMode: 'PROTECT',
      riskReason: 'VaR limit exceeded (2.8%)',
      maxPositionSize: 0,
      regime: 'EVENT',
      decisionTime: Date.now() - 25 * 60 * 1000, // 25 minutes ago - expired
      riskBudgetUsed: 100,
      factors: [
        { name: 'ADX', value: '22', direction: '-' },
        { name: 'MA Slope', value: '-0.4%', direction: '-' },
        { name: 'Vol Confirm', value: 'NO', direction: '-' },
      ]
    },
    'TSLA': {
      trend: 'RANGE',
      confidence: 0.58,
      horizon: '30m-1h',
      riskMode: 'CAUTION',
      riskReason: 'Earnings approaching',
      maxPositionSize: 25,
      regime: 'EVENT',
      decisionTime: Date.now() - 12 * 60 * 1000, // 12 minutes ago
      riskBudgetUsed: 72,
      factors: [
        { name: 'ADX', value: '28', direction: '=' },
        { name: 'MA Slope', value: '+0.3%', direction: '+' },
        { name: 'Vol Confirm', value: 'MIXED', direction: '=' },
      ]
    },
    'NVDA': {
      trend: 'TREND_UP',
      confidence: 0.92,
      horizon: '4h-1d',
      riskMode: 'NORMAL',
      maxPositionSize: 50,
      regime: 'TRENDING',
      decisionTime: Date.now() - 5 * 60 * 1000, // 5 minutes ago
      riskBudgetUsed: 45,
      factors: [
        { name: 'ADX', value: '48', direction: '+' },
        { name: 'MA Slope', value: '+1.5%', direction: '+' },
        { name: 'Vol Confirm', value: 'STRONG', direction: '+' },
      ]
    },
  })
  
  // Get current market state for selected symbol
  const currentMarketState = marketState[selectedSymbol] || {
    trend: 'RANGE',
    confidence: 0.5,
    horizon: '1h',
    riskMode: 'NORMAL' as const,
    factors: [],
    regime: 'RANGE' as const,
    decisionTime: Date.now(),
    riskBudgetUsed: 50
  }
  
  // Decision countdown timer
  const getDecisionCountdown = () => {
    const horizonMinutes = currentMarketState.horizon.includes('30m') ? 30 :
                          currentMarketState.horizon.includes('15') ? 15 :
                          currentMarketState.horizon.includes('1h') ? 60 :
                          currentMarketState.horizon.includes('4h') ? 240 : 30
    const elapsedMs = Date.now() - (currentMarketState.decisionTime || Date.now())
    const remainingMs = horizonMinutes * 60 * 1000 - elapsedMs
    if (remainingMs <= 0) return { expired: true, text: 'EXPIRED' }
    const mins = Math.floor(remainingMs / 60000)
    const secs = Math.floor((remainingMs % 60000) / 1000)
    return { expired: false, text: `${mins}m ${secs}s` }
  }
  
  // Data health impact on confidence
  const getDataHealthImpact = () => {
    // Simulated data quality check
    const latency = 12 // ms
    if (latency > 100) return { status: 'DEGRADED', adjustment: -5, caution: true }
    if (latency > 50) return { status: 'FAIR', adjustment: -2, caution: false }
    return { status: 'GOOD', adjustment: 0, caution: false }
  }
  
  // Calculate max allowed size based on risk mode
  const getMaxAllowedSize = () => {
    if (currentMarketState.riskMode === 'PROTECT') return 0
    if (currentMarketState.riskMode === 'CAUTION') {
      return currentMarketState.maxPositionSize || 0.35
    }
    return Infinity
  }
  
  // Check if order would be blocked
  const getOrderBlockReason = () => {
    if (killSwitchActive) return 'Kill switch active'
    if (currentMarketState.riskMode === 'PROTECT') {
      return `Risk mode PROTECT: ${currentMarketState.riskReason || 'Position opening blocked'}`
    }
    const qty = parseFloat(orderQty) || 0
    const maxSize = getMaxAllowedSize()
    if (qty > maxSize && currentMarketState.riskMode === 'CAUTION') {
      return `Size exceeds limit: Max ${maxSize} allowed (${currentMarketState.riskReason})`
    }
    return null
  }
  
  // Liquidity assessment for order book
  const getLiquidityStatus = () => {
    const totalBidSize = orderBook.bids.reduce((sum, b) => sum + (b.qty || 0), 0)
    const totalAskSize = orderBook.asks.reduce((sum, a) => sum + (a.qty || 0), 0)
    const totalLiquidity = totalBidSize + totalAskSize
    if (totalLiquidity > 50) return { status: 'DEEP', color: 'text-[#00ff88]' }
    if (totalLiquidity > 20) return { status: 'GOOD', color: 'text-[#00aa66]' }
    if (totalLiquidity > 10) return { status: 'THIN', color: 'text-[#ffaa00]' }
    return { status: 'VERY THIN', color: 'text-[#ff4444]' }
  }
  
  // Sentiment Impact Assessment
  const getSentimentImpact = () => {
    const social = socialSentiment[selectedSymbol]
    if (!social) return { impact: 'NEUTRAL', weight: 'LOW' as const }
    
    const score = social.overallSocialScore
    const trend = currentMarketState.trend
    
    // Check if sentiment aligns with trend
    const isBullishTrend = trend === 'TREND_UP' || trend === 'BREAKOUT'
    const isBullishSentiment = score > 60
    
    if (isBullishTrend && isBullishSentiment) return { impact: 'SUPPORTIVE', weight: 'MEDIUM' as const }
    if (!isBullishTrend && !isBullishSentiment) return { impact: 'SUPPORTIVE', weight: 'MEDIUM' as const }
    if (Math.abs(score - 50) < 10) return { impact: 'NEUTRAL', weight: 'LOW' as const }
    return { impact: 'CONTRARY', weight: 'LOW' as const }
  }

  // ==================== MARKET LABEL SYSTEM (增强版) ====================
  // 主标签（Primary）：互斥，最多1个
  // 辅标签（Secondary）：最多2个
  type PrimaryLabel = 
    | 'FLASH_CRASH' | 'FLASH_PUMP'           // P0
    | 'LIQUIDITY_VACUUM'                      // P1
    | 'VOL_SPIKE_UP' | 'VOL_SPIKE_DOWN' | 'VOL_SPIKE_TWOWAY'  // P2
    | 'BREAKOUT_CONFIRMED' | 'BREAKDOWN_CONFIRMED'  // P3
    | 'FALSE_BREAKOUT' | 'BULL_TRAP' | 'BEAR_TRAP'  // P4
    | 'TREND_UP' | 'TREND_DOWN'               // P5
    | 'RANGE' | 'CHOP'                        // P6
    | null
  
  type SecondaryLabel = 
    | 'LARGE_TRADE'      // 大单成交
    | 'IMBALANCE_BID' | 'IMBALANCE_ASK'  // 订单流失衡
    | 'VOL_CONFIRM'      // 成交量确认
    | 'NEWS_DRIVEN'      // 新闻驱动
    | 'HALT' | 'LIMIT'   // 熔断/限价
    | 'WHALE_ACCUMULATING' | 'WHALE_DISTRIBUTING'  // 鲸鱼行为
    | 'FUNDING_EXTREME'  // 资金费率极端
  
  interface MarketLabel {
    primary: PrimaryLabel
    primaryPriority: number  // 0-6
    secondary: SecondaryLabel[]
    primaryText: string      // 人话描述
    primaryIcon: string      // 图标
    primaryColor: string     // 颜色
    cooldownRemaining: number // 剩余冷却K线数
    confirmCount: number     // 确认K线数
    confidence: number       // 置信度 0-100
    reasoning: string[]      // 推理依据
  }

  // 标签状态追踪（用于冷却和确认）- 使用state以支持更新
  interface LabelTracker {
    lastPrimary: PrimaryLabel
    lastTriggerTime: number
    confirmWindow: number   // 需要确认的K线数
    cooldownBars: number    // 冷却K线数
    consecutiveSignals: number  // 连续同向信号数
    priceHistory: number[]  // 最近价格历史（用于趋势判断）
    volHistory: number[]    // 成交量历史
    breakoutLevel: number | null  // 突破关键位
    lastBreakoutTime: number  // 上次突破时间
  }
  
  const [labelTrackers, setLabelTrackers] = useState<Record<string, LabelTracker>>({
    'BTC': { lastPrimary: null, lastTriggerTime: 0, confirmWindow: 2, cooldownBars: 5, consecutiveSignals: 0, priceHistory: [], volHistory: [], breakoutLevel: null, lastBreakoutTime: 0 },
    'ETH': { lastPrimary: null, lastTriggerTime: 0, confirmWindow: 2, cooldownBars: 5, consecutiveSignals: 0, priceHistory: [], volHistory: [], breakoutLevel: null, lastBreakoutTime: 0 },
    'SOL': { lastPrimary: null, lastTriggerTime: 0, confirmWindow: 2, cooldownBars: 3, consecutiveSignals: 0, priceHistory: [], volHistory: [], breakoutLevel: null, lastBreakoutTime: 0 },
    'AAPL': { lastPrimary: null, lastTriggerTime: 0, confirmWindow: 3, cooldownBars: 8, consecutiveSignals: 0, priceHistory: [], volHistory: [], breakoutLevel: null, lastBreakoutTime: 0 },
    'TSLA': { lastPrimary: null, lastTriggerTime: 0, confirmWindow: 2, cooldownBars: 5, consecutiveSignals: 0, priceHistory: [], volHistory: [], breakoutLevel: null, lastBreakoutTime: 0 },
    'NVDA': { lastPrimary: null, lastTriggerTime: 0, confirmWindow: 2, cooldownBars: 5, consecutiveSignals: 0, priceHistory: [], volHistory: [], breakoutLevel: null, lastBreakoutTime: 0 },
  })

  // 实时市场数据状态 - 模拟K线更新
  interface RealtimeMetrics {
    symbol: string
    price: number
    volume: number
    bidDepth: number      // 买盘深度（前5档总量）
    askDepth: number      // 卖盘深度
    spread: number        // 点差
    lastTradeSize: number // 最近成交量
    timestamp: number
  }
  
  const [realtimeData, setRealtimeData] = useState<Record<string, RealtimeMetrics>>({})
  
  // 模拟市场事件队列
  interface MarketEventSignal {
    id: string
    symbol: string
    type: 'PRICE_SPIKE' | 'DEPTH_DROP' | 'LARGE_ORDER' | 'NEWS_IMPACT' | 'WHALE_MOVE' | 'FUNDING_RESET'
    magnitude: number  // 强度 0-100
    direction: 'UP' | 'DOWN' | 'NEUTRAL'
    timestamp: number
    ttl: number  // 生存时间(ms)
  }
  
  const [eventQueue, setEventQueue] = useState<MarketEventSignal[]>([])

  // 模拟市场数据（增强版 - 基于事件驱动）
  interface MarketMetrics {
    return1k: number        // 1根K线收益率
    returnDelta: number     // 短时收益率（多根K线累计）
    return5k: number        // 5根K线收益率
    timeToMove: number      // 价格变动用时（秒）
    depthTopN: number       // Top N档深度变化%
    depthImbalance: number  // 深度不平衡 -1到1 (正=买盘厚)
    slippageEst: number     // 预估滑点
    spreadMultiple: number  // 点差倍数（相对常态）
    atrZscore: number       // ATR Z分数
    rangeZscore: number     // 振幅Z分数
    realizedVolChange: number  // 已实现波动率变化%
    priceBreakLevel: boolean   // 是否突破关键位
    breakDirection: 'UP' | 'DOWN' | null  // 突破方向
    breakHoldBars: number      // 站稳K线数
    volZscore: number         // 成交量Z分数
    orderImbalance: number    // 订单流失衡 -1到1
    retraceRatio: number      // 回撤比例
    adx: number               // ADX值
    adxSlope: number          // ADX斜率（上升/下降）
    maSlope: number           // 均线斜率
    priceAboveMA: boolean     // 价格在均线上方
    maDeviation: number       // 价格偏离均线的程度%
    rangeWidth: number        // 区间宽度
    breakAttempts: number     // 突破尝试次数
    largeTrade: boolean       // 是否有大单
    largeTradeDirection: 'BUY' | 'SELL' | null  // 大单方向
    hasNews: boolean          // 是否有新闻
    newsImpact: 'BULLISH' | 'BEARISH' | 'NEUTRAL'  // 新闻影响
    isHalt: boolean           // 是否熔断
    fundingRate: number       // 资金费率
    whaleNetFlow: number      // 鲸鱼净流入
    liquidationPressure: 'LONG' | 'SHORT' | 'BALANCED'  // 爆仓压力
    consecutiveBars: number   // 连续同向K线数
  }

  // 计算价格变化率（基于历史数据）
  const calculateReturn = (prices: number[], periods: number): number => {
    if (prices.length < periods + 1) return 0
    const current = prices[prices.length - 1]
    const past = prices[prices.length - 1 - periods]
    return ((current - past) / past) * 100
  }
  
  // 计算Z分数
  const calculateZScore = (value: number, mean: number, std: number): number => {
    if (std === 0) return 0
    return (value - mean) / std
  }
  
  // 检测突破
  const detectBreakout = (price: number, symbol: string): { isBreak: boolean; direction: 'UP' | 'DOWN' | null; level: number } => {
    const priceInfo = priceData[symbol]
    if (!priceInfo) return { isBreak: false, direction: null, level: 0 }
    
    const high24h = priceInfo.high24h
    const low24h = priceInfo.low24h
    const range = high24h - low24h
    const upperThreshold = high24h - range * 0.05  // 突破上沿95%
    const lowerThreshold = low24h + range * 0.05   // 突破下沿5%
    
    if (price > upperThreshold) {
      return { isBreak: true, direction: 'UP', level: high24h }
    }
    if (price < lowerThreshold) {
      return { isBreak: true, direction: 'DOWN', level: low24h }
    }
    return { isBreak: false, direction: null, level: 0 }
  }

  // 获取增强版市场指标
  const getMarketMetrics = (symbol: string): MarketMetrics => {
    const indicators = symbolIndicators[symbol]
    const state = marketState[symbol]
    const priceInfo = priceData[symbol]
    const tracker = labelTrackers[symbol]
    const events = eventQueue.filter(e => e.symbol === symbol && Date.now() - e.timestamp < e.ttl)
    
    // 基于现有数据和事件生成市场指标
    const volLevel = indicators?.volatilityValue || 30
    const momentum = indicators?.momentum || 0
    const isHigh = indicators?.volatility === 'HIGH' || indicators?.volatility === 'EXTREME'
    const currentPrice = priceInfo?.price || 0
    const funding = indicators?.fundingRate || 0
    const whaleFlow = indicators?.whaleNetFlow || 0
    
    // 检测是否有极端事件
    const hasPriceSpike = events.some(e => e.type === 'PRICE_SPIKE' && e.magnitude > 70)
    const hasDepthDrop = events.some(e => e.type === 'DEPTH_DROP' && e.magnitude > 60)
    const hasLargeOrder = events.some(e => e.type === 'LARGE_ORDER')
    const hasWhaleMove = events.some(e => e.type === 'WHALE_MOVE')
    
    // 价格历史分析
    const prices = tracker?.priceHistory || []
    const return1k = prices.length >= 2 ? calculateReturn(prices, 1) : (momentum / 100) * 1.5
    const return5k = prices.length >= 6 ? calculateReturn(prices, 5) : return1k * 3
    
    // 连续同向K线计算
    let consecutiveBars = 0
    if (prices.length >= 3) {
      const direction = prices[prices.length - 1] > prices[prices.length - 2] ? 1 : -1
      for (let i = prices.length - 1; i > 0; i--) {
        const thisDir = prices[i] > prices[i - 1] ? 1 : -1
        if (thisDir === direction) consecutiveBars++
        else break
      }
    }
    
    // 突破检测
    const breakout = detectBreakout(currentPrice, symbol)
    const timeSinceBreakout = tracker?.lastBreakoutTime ? (Date.now() - tracker.lastBreakoutTime) / 60000 : Infinity
    const breakHoldBars = breakout.isBreak && timeSinceBreakout < 10 ? Math.min(Math.floor(timeSinceBreakout), 5) : 0
    
    // 回撤比例计算
    const retraceRatio = breakout.isBreak && breakout.level ? 
      Math.abs(currentPrice - breakout.level) / (breakout.level * 0.01) : 0.2
    
    // 深度分析 - 基于指标模拟（避免orderBook未定义）
    const depthImbalance = momentum > 30 ? 0.25 : momentum < -30 ? -0.25 : (momentum / 100) * 0.3
    
    // ADX斜率（模拟）
    const adxBase = state?.trend === 'TREND_UP' || state?.trend === 'TREND_DOWN' ? 32 : 
                   state?.trend === 'RANGE' ? 15 : 28
    const adxSlope = momentum > 50 ? 2 : momentum < -50 ? -2 : 0
    
    // 新闻影响分析
    const recentNews = newsData.filter(n => n.symbol === symbol && Date.now() - n.time.getTime() < 3600000)
    const newsImpact: 'BULLISH' | 'BEARISH' | 'NEUTRAL' = 
      recentNews.filter(n => n.sentiment === 'BULLISH').length > recentNews.filter(n => n.sentiment === 'BEARISH').length 
        ? 'BULLISH' 
        : recentNews.filter(n => n.sentiment === 'BEARISH').length > 0 ? 'BEARISH' : 'NEUTRAL'
    
    // 爆仓压力分析
    const liqLong = indicators?.liquidations24h?.long || 0
    const liqShort = indicators?.liquidations24h?.short || 0
    const liquidationPressure: 'LONG' | 'SHORT' | 'BALANCED' = 
      liqLong > liqShort * 1.5 ? 'LONG' : liqShort > liqLong * 1.5 ? 'SHORT' : 'BALANCED'
    
    return {
      return1k: hasPriceSpike ? (events.find(e => e.type === 'PRICE_SPIKE')?.direction === 'UP' ? 3.5 : -3.5) : return1k,
      returnDelta: hasPriceSpike ? (events.find(e => e.type === 'PRICE_SPIKE')?.direction === 'UP' ? 4.0 : -4.0) : return1k * 1.5,
      return5k,
      timeToMove: hasPriceSpike ? 10 : isHigh ? 25 : 60,
      depthTopN: hasDepthDrop ? -55 : volLevel > 60 ? -35 : -10,
      depthImbalance,
      slippageEst: hasDepthDrop ? 0.12 : volLevel > 60 ? 0.06 : 0.015,
      spreadMultiple: hasDepthDrop ? 3.5 : isHigh ? 2.2 : 1.0,
      atrZscore: hasPriceSpike ? 3.2 : volLevel > 60 ? 2.3 : volLevel > 40 ? 1.1 : 0.4,
      rangeZscore: hasPriceSpike ? 2.8 : isHigh ? 2.0 : 0.6,
      realizedVolChange: hasPriceSpike ? 120 : isHigh ? 70 : 15,
      priceBreakLevel: breakout.isBreak || state?.trend === 'BREAKOUT',
      breakDirection: breakout.direction,
      breakHoldBars: state?.trend === 'BREAKOUT' ? 3 : breakHoldBars,
      volZscore: hasLargeOrder ? 2.5 : momentum > 50 ? 1.4 : 0.4,
      orderImbalance: depthImbalance,
      retraceRatio: state?.trend === 'RANGE' ? 0.65 : retraceRatio,
      adx: adxBase,
      adxSlope,
      maSlope: momentum / 100 * 2,
      priceAboveMA: momentum > 0,
      maDeviation: Math.abs(momentum / 100 * 5),
      rangeWidth: state?.trend === 'RANGE' ? 0.4 : 1.8,
      breakAttempts: state?.trend === 'RANGE' ? 3 : 0,
      largeTrade: hasLargeOrder || hasWhaleMove || indicators?.whaleActivity === 'ACCUMULATING' || indicators?.whaleActivity === 'DISTRIBUTING',
      largeTradeDirection: hasLargeOrder ? (events.find(e => e.type === 'LARGE_ORDER')?.direction === 'UP' ? 'BUY' : 'SELL') : 
                          hasWhaleMove ? (events.find(e => e.type === 'WHALE_MOVE')?.direction === 'UP' ? 'BUY' : 'SELL') : null,
      hasNews: recentNews.length > 0,
      newsImpact,
      isHalt: false,
      fundingRate: funding,
      whaleNetFlow: whaleFlow,
      liquidationPressure,
      consecutiveBars
    }
  }

  // 主标签检测函数（增强版）- 按优先级从高到低检测，带置信度和推理
  const detectPrimaryLabel = (metrics: MarketMetrics, tracker: LabelTracker): {
    label: PrimaryLabel
    priority: number
    text: string
    icon: string
    color: string
    confidence: number
    reasoning: string[]
  } => {
    const now = Date.now()
    const barDurationMs = 60000 // 1分钟K线
    const inCooldown = (now - tracker.lastTriggerTime) < (tracker.cooldownBars * barDurationMs)
    const lastPriority = getPriorityForLabel(tracker.lastPrimary)
    const reasoning: string[] = []
    
    // ========== P0: 闪崩/闪拉 - 极端速度的单边价格冲击 ==========
    const p0Conditions = {
      extremeReturn: Math.abs(metrics.return1k) > 2.5 || Math.abs(metrics.returnDelta) > 3.0,
      fastMove: metrics.timeToMove < 30,
      consecutiveCandles: metrics.consecutiveBars >= 2,
      highVolume: metrics.volZscore > 1.5
    }
    const p0Score = Object.values(p0Conditions).filter(Boolean).length
    
    if (p0Score >= 3) {
      const isUp = metrics.return1k > 0 || metrics.returnDelta > 0
      reasoning.push(`极端收益率: ${metrics.return1k.toFixed(2)}%`)
      reasoning.push(`价格变动速度: ${metrics.timeToMove}s`)
      if (p0Conditions.consecutiveCandles) reasoning.push(`连续${metrics.consecutiveBars}根同向K线`)
      if (p0Conditions.highVolume) reasoning.push(`成交量Z分数: ${metrics.volZscore.toFixed(1)}`)
      
      return {
        label: isUp ? 'FLASH_PUMP' : 'FLASH_CRASH',
        priority: 0,
        text: isUp ? '极速上涨：流动性风险↑' : '极速下跌：流动性风险↑',
        icon: isUp ? '⚡' : '💥',
        color: isUp ? 'bg-[#00ff00] text-black' : 'bg-[#ff0000] text-white',
        confidence: Math.min(95, 60 + p0Score * 10),
        reasoning
      }
    }
    
    // ========== P1: 流动性抽干 - 深度骤降导致滑点暴增 ==========
    const p1Conditions = {
      depthDrop: metrics.depthTopN < -40,
      spreadWide: metrics.spreadMultiple > 2.0,
      highSlippage: metrics.slippageEst > 0.05,
      depthImbalance: Math.abs(metrics.depthImbalance) > 0.4
    }
    const p1Score = Object.values(p1Conditions).filter(Boolean).length
    
    if (p1Score >= 2 && (!inCooldown || lastPriority > 1)) {
      reasoning.push(`深度变化: ${metrics.depthTopN.toFixed(0)}%`)
      reasoning.push(`点差倍数: ${metrics.spreadMultiple.toFixed(1)}x`)
      if (p1Conditions.highSlippage) reasoning.push(`预估滑点: ${(metrics.slippageEst * 100).toFixed(2)}%`)
      
      return {
        label: 'LIQUIDITY_VACUUM',
        priority: 1,
        text: '深度骤降：滑点风险↑',
        icon: '🕳️',
        color: 'bg-[#ff6600] text-white',
        confidence: Math.min(90, 55 + p1Score * 12),
        reasoning
      }
    }
    
    // ========== P2: 异常波动 - 波动显著超出常态 ==========
    const p2Conditions = {
      highATR: metrics.atrZscore > 2.0,
      highRange: metrics.rangeZscore > 2.0,
      volChange: metrics.realizedVolChange > 60,
      notP0: Math.abs(metrics.return1k) < 2.5  // 确保不是P0级别
    }
    const p2Score = Object.values(p2Conditions).filter(Boolean).length
    
    if (p2Score >= 2 && (!inCooldown || lastPriority > 2)) {
      const direction = metrics.return1k > 0.5 ? 'UP' : metrics.return1k < -0.5 ? 'DOWN' : 'TWOWAY'
      reasoning.push(`ATR Z分数: ${metrics.atrZscore.toFixed(1)}`)
      reasoning.push(`波动率变化: +${metrics.realizedVolChange.toFixed(0)}%`)
      if (direction !== 'TWOWAY') reasoning.push(`方向偏向: ${direction === 'UP' ? '上涨' : '下跌'}`)
      
      return {
        label: direction === 'UP' ? 'VOL_SPIKE_UP' : direction === 'DOWN' ? 'VOL_SPIKE_DOWN' : 'VOL_SPIKE_TWOWAY',
        priority: 2,
        text: direction === 'TWOWAY' ? '波动异常：不确定性↑' : 
              direction === 'UP' ? '波动放大：向上扩张' : '波动放大：向下扩张',
        icon: '📊',
        color: 'bg-[#ff9900] text-black',
        confidence: Math.min(85, 50 + p2Score * 12),
        reasoning
      }
    }
    
    // ========== P3: 突破确认 - 站稳关键位并延续 ==========
    const p3Conditions = {
      breakLevel: metrics.priceBreakLevel,
      holdBars: metrics.breakHoldBars >= 2,
      volConfirm: metrics.volZscore > 1.0,
      imbalanceConfirm: (metrics.breakDirection === 'UP' && metrics.orderImbalance > 0.2) ||
                        (metrics.breakDirection === 'DOWN' && metrics.orderImbalance < -0.2),
      lowRetrace: metrics.retraceRatio < 0.3
    }
    const p3Score = Object.values(p3Conditions).filter(Boolean).length
    
    if (p3Score >= 3 && (!inCooldown || lastPriority > 3)) {
      const isUp = metrics.breakDirection === 'UP' || metrics.return1k > 0
      reasoning.push(`突破关键位，已站稳${metrics.breakHoldBars}根K线`)
      if (p3Conditions.volConfirm) reasoning.push(`成交量确认 (Z=${metrics.volZscore.toFixed(1)})`)
      if (p3Conditions.imbalanceConfirm) reasoning.push(`订单流${isUp ? '买盘' : '卖盘'}主导`)
      reasoning.push(`回撤幅度: ${(metrics.retraceRatio * 100).toFixed(0)}%`)
      
      return {
        label: isUp ? 'BREAKOUT_CONFIRMED' : 'BREAKDOWN_CONFIRMED',
        priority: 3,
        text: isUp ? '突破确认：站稳关键位' : '跌破确认：失守关键位',
        icon: isUp ? '🚀' : '📉',
        color: isUp ? 'bg-[#00cc66] text-white' : 'bg-[#cc3333] text-white',
        confidence: Math.min(88, 50 + p3Score * 10),
        reasoning
      }
    }
    
    // ========== P4: 假突破 - 突破失败并回吐 ==========
    const p4Conditions = {
      hadBreak: metrics.priceBreakLevel || metrics.breakHoldBars > 0,
      highRetrace: metrics.retraceRatio > 0.5,
      reverseImbalance: (metrics.breakDirection === 'UP' && metrics.orderImbalance < -0.1) ||
                        (metrics.breakDirection === 'DOWN' && metrics.orderImbalance > 0.1),
      volFading: metrics.volZscore < 0.8
    }
    const p4Score = Object.values(p4Conditions).filter(Boolean).length
    
    if (p4Score >= 2 && metrics.retraceRatio > 0.5 && (!inCooldown || lastPriority > 4)) {
      const isBullTrap = metrics.breakDirection === 'UP' || metrics.return1k < 0
      reasoning.push(`回撤比例: ${(metrics.retraceRatio * 100).toFixed(0)}%`)
      if (p4Conditions.reverseImbalance) reasoning.push(`订单流反转`)
      if (p4Conditions.volFading) reasoning.push(`成交量萎缩`)
      
      return {
        label: isBullTrap ? 'BULL_TRAP' : 'BEAR_TRAP',
        priority: 4,
        text: isBullTrap ? '多头陷阱：冲高回落，谨防追涨' : '空头陷阱：急跌反弹，谨防追空',
        icon: '⚠️',
        color: 'bg-[#ffcc00] text-black',
        confidence: Math.min(80, 45 + p4Score * 12),
        reasoning
      }
    }
    
    // ========== P5: 趋势延续 - 结构性上行/下行持续 ==========
    const p5Conditions = {
      highADX: metrics.adx > 25,
      adxRising: metrics.adxSlope > 0,
      maAligned: Math.abs(metrics.maSlope) > 0.5,
      priceWithTrend: (metrics.maSlope > 0 && metrics.priceAboveMA) || 
                      (metrics.maSlope < 0 && !metrics.priceAboveMA),
      lowDeviation: metrics.maDeviation < 3,
      consecutiveCandles: metrics.consecutiveBars >= 2
    }
    const p5Score = Object.values(p5Conditions).filter(Boolean).length
    
    if (p5Score >= 3 && (!inCooldown || lastPriority > 5)) {
      const isUp = metrics.maSlope > 0 && metrics.priceAboveMA
      reasoning.push(`ADX: ${metrics.adx.toFixed(0)} (${metrics.adxSlope > 0 ? '上升' : '平稳'})`)
      reasoning.push(`均线斜率: ${metrics.maSlope > 0 ? '+' : ''}${(metrics.maSlope * 100).toFixed(1)}%`)
      if (p5Conditions.consecutiveCandles) reasoning.push(`连续${metrics.consecutiveBars}根${isUp ? '阳' : '阴'}线`)
      
      return {
        label: isUp ? 'TREND_UP' : 'TREND_DOWN',
        priority: 5,
        text: isUp ? '趋势延续：顺势做多' : '趋势延续：顺势做空',
        icon: isUp ? '📈' : '📉',
        color: isUp ? 'bg-[#00aa66]/80 text-white' : 'bg-[#aa3333]/80 text-white',
        confidence: Math.min(82, 48 + p5Score * 8),
        reasoning
      }
    }
    
    // ========== P6: 震荡/盘整 - 无方向 ==========
    const p6Conditions = {
      lowADX: metrics.adx < 20,
      narrowRange: metrics.rangeWidth < 0.8,
      noMATrend: Math.abs(metrics.maSlope) < 0.3,
      priceOscillating: metrics.maDeviation < 1.5,
      multipleBreakFails: metrics.breakAttempts > 2
    }
    const p6Score = Object.values(p6Conditions).filter(Boolean).length
    
    if (p6Score >= 2) {
      const isChop = metrics.breakAttempts > 2 || metrics.volZscore > 1.5
      reasoning.push(`ADX: ${metrics.adx.toFixed(0)} (弱趋势)`)
      reasoning.push(`区间宽度: ${(metrics.rangeWidth * 100).toFixed(0)}%`)
      if (p6Conditions.multipleBreakFails) reasoning.push(`${metrics.breakAttempts}次突破失败`)
      
      return {
        label: isChop ? 'CHOP' : 'RANGE',
        priority: 6,
        text: isChop ? '震荡行情：波动无序，不宜追单' : '区间盘整：等待方向明确',
        icon: '↔️',
        color: 'bg-[#666]/80 text-white',
        confidence: Math.min(75, 45 + p6Score * 8),
        reasoning
      }
    }
    
    // 默认：无标签
    return {
      label: null,
      priority: 99,
      text: '',
      icon: '',
      color: '',
      confidence: 0,
      reasoning: []
    }
  }
  
  // 获取标签优先级
  const getPriorityForLabel = (label: PrimaryLabel): number => {
    if (!label) return 99
    if (label === 'FLASH_CRASH' || label === 'FLASH_PUMP') return 0
    if (label === 'LIQUIDITY_VACUUM') return 1
    if (label.startsWith('VOL_SPIKE')) return 2
    if (label === 'BREAKOUT_CONFIRMED' || label === 'BREAKDOWN_CONFIRMED') return 3
    if (label === 'FALSE_BREAKOUT' || label === 'BULL_TRAP' || label === 'BEAR_TRAP') return 4
    if (label === 'TREND_UP' || label === 'TREND_DOWN') return 5
    if (label === 'RANGE' || label === 'CHOP') return 6
    return 99
  }

  // 辅标签检测（增强版）- 最多返回2个
  const detectSecondaryLabels = (metrics: MarketMetrics, primaryPriority: number): {
    label: SecondaryLabel
    text: string
    icon: string
    color: string
    reasoning: string
  }[] => {
    const labels: { label: SecondaryLabel; text: string; icon: string; color: string; reasoning: string }[] = []
    
    // 大单成交 - P0时禁配避免信息爆炸
    if (metrics.largeTrade && primaryPriority > 0) {
      const dir = metrics.largeTradeDirection
      labels.push({
        label: 'LARGE_TRADE',
        text: dir ? (dir === 'BUY' ? '大单买入' : '大单卖出') : '大单成交',
        icon: '🐋',
        color: dir === 'BUY' ? 'bg-[#00aaff]/60 text-white' : dir === 'SELL' ? 'bg-[#ff6666]/60 text-white' : 'bg-[#00aaff]/60 text-white',
        reasoning: `检测到大额${dir === 'BUY' ? '买单' : dir === 'SELL' ? '卖单' : '成交'}`
      })
    }
    
    // 鲸鱼行为 - 基于净流入
    if (Math.abs(metrics.whaleNetFlow) > 50 && primaryPriority > 1) {
      const isAccum = metrics.whaleNetFlow > 0
      labels.push({
        label: isAccum ? 'WHALE_ACCUMULATING' : 'WHALE_DISTRIBUTING',
        text: isAccum ? '鲸鱼吸筹' : '鲸鱼出货',
        icon: isAccum ? '🟢' : '🔴',
        color: isAccum ? 'bg-[#00aa66]/50 text-white' : 'bg-[#aa3333]/50 text-white',
        reasoning: `鲸鱼净流${isAccum ? '入' : '出'}: ${Math.abs(metrics.whaleNetFlow).toFixed(1)}M`
      })
    }
    
    // 订单流失衡
    if (Math.abs(metrics.orderImbalance) > 0.25 && labels.length < 2) {
      labels.push({
        label: metrics.orderImbalance > 0 ? 'IMBALANCE_BID' : 'IMBALANCE_ASK',
        text: metrics.orderImbalance > 0 ? '买盘主导' : '卖盘主导',
        icon: metrics.orderImbalance > 0 ? '📗' : '📕',
        color: metrics.orderImbalance > 0 ? 'bg-[#00aa66]/50 text-white' : 'bg-[#aa3333]/50 text-white',
        reasoning: `订单流失衡: ${(metrics.orderImbalance * 100).toFixed(0)}%`
      })
    }
    
    // 成交量确认 - P6震荡时不显示
    if (metrics.volZscore > 1.2 && primaryPriority !== 6 && labels.length < 2) {
      labels.push({
        label: 'VOL_CONFIRM',
        text: '放量确认',
        icon: '📶',
        color: 'bg-[#9966ff]/50 text-white',
        reasoning: `成交量Z分数: ${metrics.volZscore.toFixed(1)}`
      })
    }
    
    // 资金费率极端
    if (Math.abs(metrics.fundingRate) > 0.02 && labels.length < 2) {
      labels.push({
        label: 'FUNDING_EXTREME',
        text: metrics.fundingRate > 0 ? '资金费高' : '资金费负',
        icon: metrics.fundingRate > 0 ? '💰' : '💸',
        color: metrics.fundingRate > 0 ? 'bg-[#ffaa00]/50 text-black' : 'bg-[#00aaff]/50 text-white',
        reasoning: `资金费率: ${(metrics.fundingRate * 100).toFixed(3)}%`
      })
    }
    
    // 新闻驱动 - P0/P1时不抢镜
    if (metrics.hasNews && primaryPriority > 1 && labels.length < 2) {
      labels.push({
        label: 'NEWS_DRIVEN',
        text: metrics.newsImpact === 'BULLISH' ? '利好新闻' : metrics.newsImpact === 'BEARISH' ? '利空新闻' : '新闻事件',
        icon: '📰',
        color: metrics.newsImpact === 'BULLISH' ? 'bg-[#00aa66]/50 text-white' : 
               metrics.newsImpact === 'BEARISH' ? 'bg-[#aa3333]/50 text-white' : 'bg-[#ff66aa]/50 text-white',
        reasoning: `检测到${metrics.newsImpact === 'BULLISH' ? '利好' : metrics.newsImpact === 'BEARISH' ? '利空' : '中性'}新闻`
      })
    }
    
    // 熔断/限价
    if (metrics.isHalt && labels.length < 2) {
      labels.push({
        label: 'HALT',
        text: '交易暂停',
        icon: '⛔',
        color: 'bg-[#ff0000]/80 text-white',
        reasoning: '交易所暂停交易'
      })
    }
    
    return labels.slice(0, 2)
  }

  // 获取当前symbol的完整标签（增强版 - 含置信度和推理）
  const getMarketLabels = (symbol: string): MarketLabel => {
    const metrics = getMarketMetrics(symbol)
    const tracker = labelTrackers[symbol] || { 
      lastPrimary: null, 
      lastTriggerTime: 0, 
      confirmWindow: 2, 
      cooldownBars: 5,
      consecutiveSignals: 0,
      priceHistory: [],
      volHistory: [],
      breakoutLevel: null,
      lastBreakoutTime: 0
    }
    
    const primary = detectPrimaryLabel(metrics, tracker)
    const secondary = detectSecondaryLabels(metrics, primary.priority)
    
    return {
      primary: primary.label,
      primaryPriority: primary.priority,
      secondary: secondary.map(s => s.label),
      primaryText: primary.text,
      primaryIcon: primary.icon,
      primaryColor: primary.color,
      cooldownRemaining: Math.max(0, tracker.cooldownBars - Math.floor((Date.now() - tracker.lastTriggerTime) / 60000)),
      confirmCount: tracker.confirmWindow,
      confidence: primary.confidence,
      reasoning: primary.reasoning
    }
  }
  
  // 当前symbol的标签
  const currentLabels = getMarketLabels(selectedSymbol)
  
  // 模拟事件生成器 - 定期生成市场事件
  useEffect(() => {
    const eventInterval = setInterval(() => {
      const symbols = ['BTC', 'ETH', 'SOL', 'AAPL', 'TSLA', 'NVDA']
      const randomSymbol = symbols[Math.floor(Math.random() * symbols.length)]
      const indicators = symbolIndicators[randomSymbol]
      
      // 基于市场状态概率生成事件
      const volLevel = indicators?.volatilityValue || 30
      const eventProbability = volLevel > 60 ? 0.3 : volLevel > 40 ? 0.15 : 0.05
      
      if (Math.random() < eventProbability) {
        const eventTypes: MarketEventSignal['type'][] = ['PRICE_SPIKE', 'DEPTH_DROP', 'LARGE_ORDER', 'WHALE_MOVE']
        const type = eventTypes[Math.floor(Math.random() * eventTypes.length)]
        const direction: 'UP' | 'DOWN' = Math.random() > 0.5 ? 'UP' : 'DOWN'
        
        const newEvent: MarketEventSignal = {
          id: `evt-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
          symbol: randomSymbol,
          type,
          magnitude: 50 + Math.random() * 50,
          direction,
          timestamp: Date.now(),
          ttl: type === 'PRICE_SPIKE' ? 30000 : 60000  // 30秒到1分钟
        }
        
        setEventQueue(prev => [...prev.filter(e => Date.now() - e.timestamp < e.ttl), newEvent].slice(-20))
      }
    }, 5000)  // 每5秒检查一次
    
    return () => clearInterval(eventInterval)
  }, [symbolIndicators])
  
  // 价格历史追踪 - 用于趋势分析
  useEffect(() => {
    const priceTrackInterval = setInterval(() => {
      const currentPrice = priceData[selectedSymbol]?.price
      if (currentPrice) {
        setLabelTrackers(prev => ({
          ...prev,
          [selectedSymbol]: {
            ...prev[selectedSymbol],
            priceHistory: [...(prev[selectedSymbol]?.priceHistory || []).slice(-20), currentPrice]
          }
        }))
      }
    }, 10000)  // 每10秒记录一次价格
    
    return () => clearInterval(priceTrackInterval)
  }, [selectedSymbol, priceData])

  // ==================== SENTIMENT 实时更新系统 ====================
  
  // 计算走势吻合度分数
  const calculateTrendMatchScore = useCallback((symbol: string): TrendMatchAnalysis => {
    const social = socialSentiment[symbol]
    const indicators = symbolIndicators[symbol]
    const price = priceData[symbol]
    
    if (!social || !indicators || !price) {
      return {
        score: 50,
        verdict: 'NEUTRAL',
        details: [],
        confidence: 0,
        lastCalculated: Date.now()
      }
    }
    
    // 判断价格方向
    const priceDirection: 'UP' | 'DOWN' | 'FLAT' = 
      price.changePercent > 1 ? 'UP' : 
      price.changePercent < -1 ? 'DOWN' : 'FLAT'
    
    // 各平台情绪与价格方向匹配分析
    const details: TrendMatchAnalysis['details'] = []
    
    // X平台分析
    const xBullish = social.xSentiment > 55
    const xMatch = (xBullish && priceDirection === 'UP') || (!xBullish && priceDirection === 'DOWN') || priceDirection === 'FLAT'
    details.push({
      platform: 'X',
      sentiment: social.xSentiment,
      priceDirection,
      match: xMatch
    })
    
    // YouTube分析
    const ytBullish = social.youtubeSentiment > 55
    const ytMatch = (ytBullish && priceDirection === 'UP') || (!ytBullish && priceDirection === 'DOWN') || priceDirection === 'FLAT'
    details.push({
      platform: 'YouTube',
      sentiment: social.youtubeSentiment,
      priceDirection,
      match: ytMatch
    })
    
    // Reddit分析
    const rdBullish = social.redditSentiment > 55
    const rdMatch = (rdBullish && priceDirection === 'UP') || (!rdBullish && priceDirection === 'DOWN') || priceDirection === 'FLAT'
    details.push({
      platform: 'Reddit',
      sentiment: social.redditSentiment,
      priceDirection,
      match: rdMatch
    })
    
    // TikTok分析
    const ttBullish = social.tiktokSentiment > 55
    const ttMatch = (ttBullish && priceDirection === 'UP') || (!ttBullish && priceDirection === 'DOWN') || priceDirection === 'FLAT'
    details.push({
      platform: 'TikTok',
      sentiment: social.tiktokSentiment,
      priceDirection,
      match: ttMatch
    })
    
    // 加权计算总分
    const xScore = xMatch ? social.xSentiment : (100 - social.xSentiment)
    const ytScore = ytMatch ? social.youtubeSentiment : (100 - social.youtubeSentiment)
    const rdScore = rdMatch ? social.redditSentiment : (100 - social.redditSentiment)
    const ttScore = ttMatch ? social.tiktokSentiment : (100 - social.tiktokSentiment)
    
    const weightedScore = 
      xScore * platformWeights.x +
      ytScore * platformWeights.youtube +
      rdScore * platformWeights.reddit +
      ttScore * platformWeights.tiktok
    
    // 匹配数量
    const matchCount = details.filter(d => d.match).length
    
    // 判断结论
    const verdict: 'ALIGNED' | 'DIVERGENT' | 'NEUTRAL' = 
      matchCount >= 3 ? 'ALIGNED' : 
      matchCount <= 1 ? 'DIVERGENT' : 'NEUTRAL'
    
    // 置信度（基于一致性）
    const confidence = matchCount * 25
    
    return {
      score: Math.round(weightedScore),
      verdict,
      details,
      confidence,
      lastCalculated: Date.now()
    }
  }, [socialSentiment, symbolIndicators, priceData, platformWeights])
  
  // 每分钟更新走势吻合度
  useEffect(() => {
    const updateTrendMatch = () => {
      const symbols = ['BTC', 'ETH', 'SOL', 'AAPL', 'TSLA', 'NVDA']
      const newAnalysis: Record<string, TrendMatchAnalysis> = {}
      symbols.forEach(symbol => {
        newAnalysis[symbol] = calculateTrendMatchScore(symbol)
      })
      setTrendMatchAnalysis(newAnalysis)
    }
    
    updateTrendMatch()
    const interval = setInterval(updateTrendMatch, 60000) // 每分钟更新
    
    return () => clearInterval(interval)
  }, [calculateTrendMatchScore])
  
  // 每小时模拟更新新闻和社交情绪数据
  useEffect(() => {
    const updateSentimentData = () => {
      setSocialSentiment(prev => {
        const updated = { ...prev }
        Object.keys(updated).forEach(symbol => {
          const current = updated[symbol]
          // 模拟小幅波动更新
          const fluctuation = () => Math.floor((Math.random() - 0.5) * 10)
          const mentionChange = () => Math.floor(Math.random() * 5000)
          
          updated[symbol] = {
            ...current,
            xMentions: current.xMentions + mentionChange(),
            xSentiment: Math.max(-100, Math.min(100, current.xSentiment + fluctuation())),
            xVelocity: parseFloat((current.xVelocity + (Math.random() - 0.5) * 5).toFixed(1)),
            youtubeSentiment: Math.max(-100, Math.min(100, current.youtubeSentiment + fluctuation())),
            youtubeViews24h: current.youtubeViews24h + Math.floor(Math.random() * 100000),
            redditMentions: current.redditMentions + Math.floor(Math.random() * 500),
            redditSentiment: Math.max(-100, Math.min(100, current.redditSentiment + fluctuation())),
            tiktokMentions: current.tiktokMentions + Math.floor(Math.random() * 2000),
            tiktokSentiment: Math.max(-100, Math.min(100, current.tiktokSentiment + fluctuation())),
            tiktokViews24h: current.tiktokViews24h + Math.floor(Math.random() * 500000),
            overallSocialScore: Math.max(-100, Math.min(100, current.overallSocialScore + fluctuation())),
            lastUpdated: Date.now()
          }
        })
        return updated
      })
    }
    
    // 立即更新一次，然后每小时更新
    const interval = setInterval(updateSentimentData, 3600000) // 1小时
    
    // 每30秒小幅更新（模拟实时）
    const realtimeInterval = setInterval(() => {
      setSocialSentiment(prev => {
        const symbol = selectedSymbol
        const current = prev[symbol]
        if (!current) return prev
        
        return {
          ...prev,
          [symbol]: {
            ...current,
            xMentions: current.xMentions + Math.floor(Math.random() * 100),
            lastUpdated: Date.now()
          }
        }
      })
    }, 30000) // 30秒
    
    return () => {
      clearInterval(interval)
      clearInterval(realtimeInterval)
    }
  }, [selectedSymbol])
  
  // 获取当前symbol的走势吻合度
  const currentTrendMatch = trendMatchAnalysis[selectedSymbol] || {
    score: 50,
    verdict: 'NEUTRAL' as const,
    details: [],
    confidence: 0,
    lastCalculated: Date.now()
  }
  
  // 格式化最后更新时间
  const formatLastUpdate = (timestamp: number): string => {
    const diff = Date.now() - timestamp
    if (diff < 60000) return '刚刚'
    if (diff < 3600000) return `${Math.floor(diff / 60000)}分钟前`
    return `${Math.floor(diff / 3600000)}小时前`
  }

  // ==================== END SENTIMENT 实时更新系统 ====================

  // ==================== END MARKET LABEL SYSTEM ====================

  const [chartTimeframe, setChartTimeframe] = useState('1h')
  const [chartType, setChartType] = useState<string>('candle')
  const [leverage, setLeverage] = useState(1)
  const [riskPercent] = useState(2)
  const [activeTab, setActiveTab] = useState<'positions' | 'orders' | 'trades' | 'alerts' | 'account' | 'risk'>('positions')
  const [searchQuery, setSearchQuery] = useState('')
  const [accountBalance] = useState(1250000)
  const [dailyPnL] = useState(3650)
  const [weeklyPnL] = useState(28500)
  const [winRate] = useState(68.5)
  const [sharpeRatio] = useState(2.34)
  const priceRef = useRef<number>(0)
  const symbolRef = useRef<string>('')

  // 生成订单簿 - 使用真实数据
  const generateOrderBook = useCallback((basePrice: number) => {
    const volatility = (Math.random() * 0.02 + 0.005); // 0.5% - 2.5%
    const realisticBook = generateRealisticOrderBook(basePrice, volatility)
    
    // Convert to old format for compatibility
    const bids = realisticBook.bids.map((level, i) => ({
      price: level.price,
      qty: level.size,
      total: 0,
      myOrder: i === 3 || i === 7
    }))
    const asks = realisticBook.asks.map((level, i) => ({
      price: level.price,
      qty: level.size,
      total: 0,
      myOrder: i === 2
    }))
    
    let bidTotal = 0, askTotal = 0
    bids.forEach(b => { bidTotal += b.qty; b.total = bidTotal })
    asks.forEach(a => { askTotal += a.qty; a.total = askTotal })
    
    // Convert order book events to market events for display
    if (realisticBook.events.length > 0) {
      setMarketEvents(prev => [...realisticBook.events.map((evt, i) => ({
        id: `event-${Date.now()}-${i}`,
        type: 'LARGE_TRADE' as const,
        label: `${evt.type} ${evt.side} ${evt.quantity.toFixed(2)} @ ${evt.price.toFixed(2)}`,
        severity: evt.type === 'CANCEL' ? 'warning' as const : 'info' as const,
        timestamp: evt.timestamp,
        description: `Order book ${evt.type.toLowerCase()}: ${evt.side} ${evt.quantity.toFixed(2)} units at $${evt.price.toFixed(2)}`
      })), ...prev].slice(0, 10))
    }
    
    return { bids, asks }
  }, [])

  const [orderBook, setOrderBook] = useState(() => generateOrderBook(95000))

  // ==================== 免费实时 API 集成 ====================
  // Free Real-time API Integration: CoinGecko (crypto) + Multiple Stock APIs
  
  // 股票价格缓存 - 用于平滑更新
  const stockPriceCache = useRef<Record<string, { price: number; lastUpdate: number }>>({})
  
  // 获取加密货币价格 - CoinGecko 免费 API
  const fetchCryptoPrices = async () => {
    try {
      const response = await axios.get(
        'https://api.coingecko.com/api/v3/coins/markets', {
          params: {
            vs_currency: 'usd',
            ids: 'bitcoin,ethereum,solana,binancecoin,ripple,cardano,dogecoin,polkadot,polygon,avalanche-2,chainlink,uniswap',
            order: 'market_cap_desc',
            per_page: 20,
            page: 1,
            sparkline: false,
            price_change_percentage: '24h'
          },
          timeout: 10000
        }
      )
      return response.data
    } catch (error) {
      console.warn('CoinGecko API error, using fallback data:', error)
      return null
    }
  }

  // ==================== 真实股票API集成 ====================
  // Real Stock API Integration: Yahoo Finance (via proxy) / Finnhub / Twelve Data
  
  // 方法1: Yahoo Finance 通过 AllOrigins 代理 (无需API key)
  const fetchYahooFinanceQuote = async (symbol: string) => {
    try {
      const url = `https://query1.finance.yahoo.com/v8/finance/chart/${symbol}?interval=1d&range=1d`
      const proxyUrl = `https://api.allorigins.win/raw?url=${encodeURIComponent(url)}`
      const response = await axios.get(proxyUrl, { timeout: 5000 })
      const result = response.data?.chart?.result?.[0]
      if (result) {
        const meta = result.meta
        return {
          symbol: symbol,
          price: meta.regularMarketPrice || meta.previousClose,
          change: (meta.regularMarketPrice || 0) - (meta.previousClose || 0),
          changePercent: ((meta.regularMarketPrice - meta.previousClose) / meta.previousClose * 100) || 0,
          high: meta.regularMarketDayHigh || meta.regularMarketPrice,
          low: meta.regularMarketDayLow || meta.regularMarketPrice,
          volume: meta.regularMarketVolume || 0,
          previousClose: meta.previousClose || 0,
          isRealData: true
        }
      }
    } catch (error) {
      console.warn(`Yahoo Finance error for ${symbol}:`, error)
    }
    return null
  }

  // 方法2: Finnhub 免费API (需要免费注册获取API key)
  // 免费获取: https://finnhub.io/register
  const FINNHUB_API_KEY = '' // 用户可以填入自己的免费API key
  
  const fetchFinnhubQuote = async (symbol: string) => {
    if (!FINNHUB_API_KEY) return null
    try {
      const response = await axios.get(
        `https://finnhub.io/api/v1/quote?symbol=${symbol}&token=${FINNHUB_API_KEY}`,
        { timeout: 5000 }
      )
      const data = response.data
      if (data && data.c) {
        return {
          symbol: symbol,
          price: data.c, // Current price
          change: data.d, // Change
          changePercent: data.dp, // Change percent
          high: data.h, // High
          low: data.l, // Low
          volume: 0, // Finnhub quote doesn't include volume
          previousClose: data.pc,
          isRealData: true
        }
      }
    } catch (error) {
      console.warn(`Finnhub error for ${symbol}:`, error)
    }
    return null
  }

  // 方法3: Twelve Data 免费API (800次/天免费)
  // 免费获取: https://twelvedata.com/
  const TWELVE_DATA_API_KEY = '' // 用户可以填入自己的免费API key
  
  const fetchTwelveDataQuote = async (symbol: string) => {
    if (!TWELVE_DATA_API_KEY) return null
    try {
      const response = await axios.get(
        `https://api.twelvedata.com/quote?symbol=${symbol}&apikey=${TWELVE_DATA_API_KEY}`,
        { timeout: 5000 }
      )
      const data = response.data
      if (data && data.close) {
        return {
          symbol: symbol,
          price: parseFloat(data.close),
          change: parseFloat(data.change) || 0,
          changePercent: parseFloat(data.percent_change) || 0,
          high: parseFloat(data.high) || parseFloat(data.close),
          low: parseFloat(data.low) || parseFloat(data.close),
          volume: parseInt(data.volume) || 0,
          previousClose: parseFloat(data.previous_close) || parseFloat(data.close),
          isRealData: true
        }
      }
    } catch (error) {
      console.warn(`Twelve Data error for ${symbol}:`, error)
    }
    return null
  }

  // 智能股票数据获取 - 多源回退
  const fetchStockQuote = async (symbol: string) => {
    // 优先使用 Finnhub (如果有API key)
    let quote = await fetchFinnhubQuote(symbol)
    if (quote) return quote

    // 其次使用 Twelve Data (如果有API key)
    quote = await fetchTwelveDataQuote(symbol)
    if (quote) return quote

    // 最后使用 Yahoo Finance 代理
    quote = await fetchYahooFinanceQuote(symbol)
    if (quote) return quote

    // 回退到缓存或模拟数据
    return null
  }

  // 基于真实市场价格的高保真模拟
  const generateRealisticStockPrice = (symbol: string, basePrice: number) => {
    const cache = stockPriceCache.current[symbol]
    const now = Date.now()
    
    // 如果有缓存且未过期（30秒内），基于缓存价格微调
    if (cache && now - cache.lastUpdate < 30000) {
      // 模拟真实市场的微小波动 (±0.1%)
      const microMovement = (Math.random() - 0.5) * cache.price * 0.002
      const newPrice = cache.price + microMovement
      stockPriceCache.current[symbol] = { price: newPrice, lastUpdate: now }
      return newPrice
    }
    
    // 否则使用基准价格
    stockPriceCache.current[symbol] = { price: basePrice, lastUpdate: now }
    return basePrice
  }

  // 获取所有股票价格 - 优先真实数据，回退模拟
  const fetchStockPrices = async () => {
    const stockSymbols = ['AAPL', 'MSFT', 'GOOGL', 'AMZN', 'TSLA', 'NVDA', 'META', 'JPM', 'V', 'WMT', 'DIS', 'BA']
    
    // 2026年1月的真实基准价格估算
    const basePrices: Record<string, number> = {
      'AAPL': 188.50, 'MSFT': 425.30, 'GOOGL': 178.20, 'AMZN': 198.40,
      'TSLA': 252.80, 'NVDA': 512.60, 'META': 592.40, 'JPM': 198.50,
      'V': 288.70, 'WMT': 168.30, 'DIS': 118.40, 'BA': 188.90
    }

    const results = await Promise.all(
      stockSymbols.map(async (symbol) => {
        // 尝试获取真实数据
        const realQuote = await fetchStockQuote(symbol)
        
        if (realQuote) {
          return {
            symbol,
            price: realQuote.price,
            change: realQuote.change,
            changePercent: realQuote.changePercent,
            volume: realQuote.volume || Math.floor(Math.random() * 50000000) + 10000000,
            high: realQuote.high,
            low: realQuote.low,
            isRealData: true
          }
        }

        // 使用高保真模拟
        const price = generateRealisticStockPrice(symbol, basePrices[symbol] || 100)
        const dayChange = (Math.random() - 0.5) * price * 0.03 // ±1.5% 日波动
        
        return {
          symbol,
          price: price,
          change: dayChange,
          changePercent: (dayChange / price) * 100,
          volume: Math.floor(Math.random() * 50000000) + 10000000,
          high: price * (1 + Math.random() * 0.015),
          low: price * (1 - Math.random() * 0.015),
          isRealData: false
        }
      })
    )

    return results
  }

  // 获取贵金属价格 - 黄金使用纽约期货COMEX，其他保持原数据源
  const fetchMetalsPrices = async () => {
    // 贵金属基准价格 (USD/oz) - 2026年1月
    const basePrices: Record<string, number> = {
      'XAU': 2758.50,  // 纽约COMEX黄金期货 GC
      'XAG': 32.50,    // 白银
      'XPT': 1050,     // 铂金
      'XPD': 1080      // 钯金
    }
    
    // 生成逼真的价格波动（白银、铂金、钯金用）
    const generateRealisticPrices = () => {
      const now = Date.now()
      return Object.entries(basePrices).map(([symbol, basePrice]) => {
        // 基于时间的波动（更真实）
        const timeWave = Math.sin(now / 60000) * 0.002  // 分钟波动
        const microNoise = (Math.random() - 0.5) * 0.003 // 微小随机
        const volatility = symbol === 'XAU' ? 0.015 : 0.025 // 金波动小于其他贵金属
        
        const priceChange = basePrice * (timeWave + microNoise)
        const price = basePrice + priceChange
        const dayChange = (Math.random() - 0.5) * basePrice * volatility
        
        return {
          symbol,
          price: Number(price.toFixed(2)),
          change: Number(dayChange.toFixed(2)),
          changePercent: Number(((dayChange / basePrice) * 100).toFixed(2)),
          volume: Math.floor(Math.random() * 80000000) + 40000000,
          high: Number((price * 1.008).toFixed(2)),
          low: Number((price * 0.992).toFixed(2)),
          isRealData: false,
          lastUpdate: now
        }
      })
    }
    
    // 获取COMEX黄金价格
    const fetchComexGold = async () => {
      try {
        // Yahoo Finance COMEX黄金期货 GC=F
        const response = await axios.get(
          'https://query1.finance.yahoo.com/v7/finance/quote?symbols=GC=F',
          { 
            timeout: 5000,
            headers: { 'User-Agent': 'Mozilla/5.0' }
          }
        )
        
        if (response.data?.quoteResponse?.result?.[0]) {
          const quote = response.data.quoteResponse.result[0]
          return {
            symbol: 'XAU',
            price: Number((quote.regularMarketPrice || 2758.50).toFixed(2)),
            change: Number((quote.regularMarketChange || 0).toFixed(2)),
            changePercent: Number((quote.regularMarketChangePercent || 0).toFixed(2)),
            volume: quote.regularMarketVolume || 150000,
            high: Number((quote.regularMarketDayHigh || quote.regularMarketPrice * 1.005).toFixed(2)),
            low: Number((quote.regularMarketDayLow || quote.regularMarketPrice * 0.995).toFixed(2)),
            isRealData: true,
            lastUpdate: Date.now(),
            exchange: 'COMEX',
            contractMonth: quote.shortName || 'Feb 2026'
          }
        }
      } catch (error) {
        console.log('COMEX Gold API unavailable')
      }
      return null
    }
    
    try {
      // 尝试使用 metals.live 免费API获取白银、铂金、钯金
      const response = await axios.get('https://api.metals.live/v1/spot', {
        timeout: 3000
      })
      
      // 同时获取COMEX黄金价格
      const comexGold = await fetchComexGold()
      
      if (response.data && Array.isArray(response.data)) {
        const metalMap: Record<string, string> = {
          'gold': 'XAU',
          'silver': 'XAG',
          'platinum': 'XPT',
          'palladium': 'XPD'
        }
        
        // 只取白银、铂金、钯金（不取gold）
        const otherMetals = response.data
          .filter((m: any) => m.metal?.toLowerCase() !== 'gold')
          .map((m: any) => {
            const symbol = metalMap[m.metal?.toLowerCase()] || 'XAG'
            const price = m.price || basePrices[symbol]
            return {
              symbol,
              price: Number(price.toFixed(2)),
              change: Number(((Math.random() - 0.5) * price * 0.02).toFixed(2)),
              changePercent: Number(((Math.random() - 0.5) * 2).toFixed(2)),
              volume: Math.floor(Math.random() * 100000000) + 50000000,
              high: Number((price * 1.01).toFixed(2)),
              low: Number((price * 0.99).toFixed(2)),
              isRealData: true,
              lastUpdate: Date.now()
            }
          })
        
        // 黄金使用COMEX数据，其他使用metals.live
        if (comexGold) {
          return [comexGold, ...otherMetals]
        } else {
          // COMEX不可用时，使用模拟的COMEX黄金价格
          const now = Date.now()
          const timeWave = Math.sin(now / 60000) * 0.002
          const microNoise = (Math.random() - 0.5) * 0.003
          const goldBase = basePrices['XAU']
          const goldPrice = goldBase + goldBase * (timeWave + microNoise)
          const dayChange = (Math.random() - 0.5) * goldBase * 0.015
          
          const simulatedGold = {
            symbol: 'XAU',
            price: Number(goldPrice.toFixed(2)),
            change: Number(dayChange.toFixed(2)),
            changePercent: Number(((dayChange / goldBase) * 100).toFixed(2)),
            volume: Math.floor(Math.random() * 150000) + 80000,
            high: Number((goldPrice * 1.006).toFixed(2)),
            low: Number((goldPrice * 0.994).toFixed(2)),
            isRealData: false,
            lastUpdate: now,
            exchange: 'COMEX',
            contractMonth: 'Feb 2026'
          }
          return [simulatedGold, ...otherMetals]
        }
      }
    } catch (error) {
      // API失败，使用模拟数据
      console.log('Metals API unavailable, using simulated data')
    }
    
    // 回退到模拟数据（使用更真实的波动）
    return generateRealisticPrices()
  }

  // 合并获取价格数据
  const fetchPrices = async () => {
    try {
      const [cryptoData, stockData, metalsData] = await Promise.all([
        fetchCryptoPrices(),
        fetchStockPrices(),
        fetchMetalsPrices()
      ])

      const newPrices: PriceData = {}

      // 处理加密货币数据
      if (cryptoData && Array.isArray(cryptoData)) {
        cryptoData.forEach((c: any) => {
          const price = c.current_price ?? 0
          const spread = price * 0.0001
          const symbol = c.symbol?.toUpperCase() || 'UNKNOWN'
          newPrices[symbol] = {
            price: price,
            change: c.price_change_24h ?? 0,
            changePercent: c.price_change_percentage_24h ?? 0,
            volume: c.total_volume ?? 0,
            high24h: c.high_24h ?? price,
            low24h: c.low_24h ?? price,
            bid: price - spread/2,
            ask: price + spread/2,
            spread: spread
          }
        })
      } else {
        // 回退数据
        const fallbackCrypto = [
          { symbol: 'BTC', price: 95000 + Math.random() * 2000 },
          { symbol: 'ETH', price: 3500 + Math.random() * 100 },
          { symbol: 'SOL', price: 180 + Math.random() * 10 },
          { symbol: 'BNB', price: 620 + Math.random() * 20 },
          { symbol: 'XRP', price: 2.2 + Math.random() * 0.2 },
          { symbol: 'ADA', price: 0.95 + Math.random() * 0.1 },
          { symbol: 'DOGE', price: 0.32 + Math.random() * 0.05 },
          { symbol: 'DOT', price: 7.5 + Math.random() * 0.5 },
          { symbol: 'MATIC', price: 0.85 + Math.random() * 0.1 },
          { symbol: 'AVAX', price: 38 + Math.random() * 3 },
          { symbol: 'LINK', price: 22 + Math.random() * 2 },
          { symbol: 'UNI', price: 12 + Math.random() * 1 },
        ]
        fallbackCrypto.forEach(c => {
          const spread = c.price * 0.0001
          newPrices[c.symbol] = {
            price: c.price,
            change: (Math.random() - 0.5) * c.price * 0.05,
            changePercent: (Math.random() - 0.5) * 5,
            volume: Math.floor(Math.random() * 5000000000) + 1000000000,
            high24h: c.price * 1.03,
            low24h: c.price * 0.97,
            bid: c.price - spread/2,
            ask: c.price + spread/2,
            spread: spread
          }
        })
      }

      // 处理股票数据
      stockData.forEach((s: any) => {
        const price = s.price ?? 0
        const spread = price * 0.0001
        newPrices[s.symbol] = {
          price: price,
          change: s.change ?? 0,
          changePercent: s.changePercent ?? 0,
          volume: s.volume ?? 0,
          high24h: s.high ?? price,
          low24h: s.low ?? price,
          bid: price - spread/2,
          ask: price + spread/2,
          spread: spread
        }
      })

      // 处理贵金属数据
      metalsData.forEach((m: any) => {
        const price = m.price ?? 0
        const spread = price * 0.0002 // 贵金属点差稍大
        newPrices[m.symbol] = {
          price: price,
          change: m.change ?? 0,
          changePercent: m.changePercent ?? 0,
          volume: m.volume ?? 0,
          high24h: m.high ?? price,
          low24h: m.low ?? price,
          bid: price - spread/2,
          ask: price + spread/2,
          spread: spread
        }
      })

      setPriceData(newPrices)
      
      // Generate market events for current symbol - NEW
      if (newPrices[selectedSymbol]) {
        const currentData = newPrices[selectedSymbol]
        const prevData = priceData[selectedSymbol]
        const previousPrice = prevData?.price || currentData.price
        
        const events = generateMarketEvents(
          selectedSymbol,
          currentData.price,
          previousPrice,
          currentData.volume,
          Math.random() * 0.0002 - 0.0001, // Random funding rate for demo
          currentData.bid,
          currentData.ask
        )
        
        if (events.length > 0) {
          setMarketEvents(prev => [...events, ...prev].slice(0, 10))
        }
      }
      
      // 更新持仓的当前价格
      setPositions(prev => prev.map(pos => {
        const currentPrice = newPrices[pos.symbol]?.price || pos.currentPrice
        const pnl = (currentPrice - pos.avgPrice) * pos.qty
        const pnlPercent = ((currentPrice - pos.avgPrice) / pos.avgPrice) * 100
        return { ...pos, currentPrice, pnl, pnlPercent, value: currentPrice * Math.abs(pos.qty) }
      }))
    } catch (error) {
      console.error('Price fetch error:', error)
    }
  }

  // 初始化
  useEffect(() => {
    const now = Date.now()
    setPositions([
      { symbol: 'BTC', qty: 2.5, avgPrice: 94500, currentPrice: 95200, pnl: 1750, pnlPercent: 0.74, value: 238000, liquidationPrice: 76000, positionType: 'STRATEGY', pricePnL: 1820, feePnL: -45, fundingPnL: -25, openTime: now - 3600000 * 4 },
      { symbol: 'ETH', qty: 15, avgPrice: 3450, currentPrice: 3520, pnl: 1050, pnlPercent: 2.03, value: 52800, liquidationPrice: 2760, positionType: 'STRATEGY', pricePnL: 1080, feePnL: -22, fundingPnL: -8, openTime: now - 3600000 * 12 },
      { symbol: 'SOL', qty: 100, avgPrice: 175, currentPrice: 182, pnl: 700, pnlPercent: 4.0, value: 18200, liquidationPrice: 140, positionType: 'MANUAL', pricePnL: 720, feePnL: -15, fundingPnL: -5, openTime: now - 3600000 * 2 },
      { symbol: 'AAPL', qty: -50, avgPrice: 188, currentPrice: 185, pnl: 150, pnlPercent: 1.6, value: 9250, positionType: 'MANUAL', pricePnL: 150, feePnL: -8, fundingPnL: 0, openTime: now - 3600000 * 24 },
      { symbol: 'NVDA', qty: 20, avgPrice: 495, currentPrice: 505, pnl: 200, pnlPercent: 2.02, value: 10100, positionType: 'STRATEGY', pricePnL: 210, feePnL: -10, fundingPnL: 0, openTime: now - 3600000 * 6 },
    ])
    setTotalPnL(3850)

    setOrders([
      { id: 'ORD001', symbol: 'BTC', side: 'BUY', type: 'LIMIT', price: 94800, qty: 0.5, filled: 0, status: 'NEW', time: '10:32:15', isOpen: true },
      { id: 'ORD002', symbol: 'ETH', side: 'SELL', type: 'LIMIT', price: 3580, qty: 5, filled: 2, status: 'PARTIAL', time: '10:31:42', isOpen: false },
      { id: 'ORD003', symbol: 'SOL', side: 'BUY', type: 'STOP-MKT', price: 190, qty: 25, filled: 0, status: 'NEW', time: '10:28:03', isOpen: true },
    ])

    setTrades([
      { id: 'T001', symbol: 'BTC', side: 'BUY', price: 95150, qty: 0.25, time: '10:45:32' },
      { id: 'T002', symbol: 'ETH', side: 'SELL', price: 3515, qty: 3, time: '10:42:18' },
      { id: 'T003', symbol: 'SOL', side: 'BUY', price: 181.5, qty: 50, time: '10:38:55' },
      { id: 'T004', symbol: 'AAPL', side: 'SELL', price: 185.2, qty: 25, time: '10:35:12' },
      { id: 'T005', symbol: 'NVDA', side: 'BUY', price: 504, qty: 10, time: '10:30:45' },
    ])

    setAlerts([
      { id: 'A001', symbol: 'BTC', type: 'PRICE_ABOVE', value: 100000, active: true },
      { id: 'A002', symbol: 'ETH', type: 'PRICE_BELOW', value: 3000, active: true },
      { id: 'A003', symbol: 'SOL', type: 'VOLUME_SPIKE', value: 5000000000, active: false },
    ])
  }, [])

  useEffect(() => {
    fetchPrices()
    // 每2秒刷新一次价格 / Refresh prices every 2 seconds
    const interval = setInterval(fetchPrices, 2000)
    return () => clearInterval(interval)
  }, [])

  useEffect(() => {
    const currentPrice = priceData[selectedSymbol]?.price || 95000
    // 当资产切换或价格变化超过阈值时更新订单簿
    const symbolChanged = symbolRef.current !== selectedSymbol
    const priceThreshold = currentPrice * 0.0001 // 0.01% 阈值
    const priceChanged = Math.abs(currentPrice - priceRef.current) > priceThreshold
    
    if (symbolChanged || priceChanged) {
      setOrderBook(generateOrderBook(currentPrice))
      setOrderPrice((currentPrice ?? 0).toFixed(2))
      priceRef.current = currentPrice
      symbolRef.current = selectedSymbol
    }
  }, [selectedSymbol, priceData, generateOrderBook])

  // 键盘快捷键
  useEffect(() => {
    if (!hotkeys) return
    
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return
      
      switch(e.key.toUpperCase()) {
        case 'B': setOrderSide('BUY'); break
        case 'S': setOrderSide('SELL'); break
        case 'M': setOrderType('MARKET'); break
        case 'L': setOrderType('LIMIT'); break
        case 'ENTER': handlePlaceOrder(); break
        case 'ESCAPE': handleCancelAll(); break
        case '1': setLeverage(1); break
        case '2': setLeverage(2); break
        case '5': setLeverage(5); break
      }
    }
    
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [hotkeys, orderPrice, orderQty, orderSide, orderType, selectedSymbol])

  // 生产级订单提交 / Production-grade order submission
  const handlePlaceOrder = async () => {
    if (!orderQty) {
      addNotification('warning', 'Invalid Order / 无效订单', 'Please enter quantity / 请输入数量')
      return
    }
    
    const price = parseFloat(orderPrice) || priceData[selectedSymbol]?.price || 0
    const qty = parseFloat(orderQty)
    
    if (qty <= 0) {
      addNotification('warning', 'Invalid Quantity / 无效数量', 'Quantity must be positive / 数量必须大于0')
      return
    }
    
    if (orderType === 'LIMIT' && (!orderPrice || parseFloat(orderPrice) <= 0)) {
      addNotification('warning', 'Invalid Price / 无效价格', 'Limit order requires price / 限价单需要价格')
      return
    }

    // Check market status - NEW
    const marketStatus = getMarketStatus(selectedSymbol)
    
    // Check if order type is allowed
    if (orderType === 'MARKET' && !marketStatus.canPlaceMarketOrder) {
      addNotification('warning', 'Market Order Blocked / 市价单被阻止', 
        `${selectedSymbol} market orders are not allowed during ${marketStatus.status}. ${marketStatus.reason || ''}`)
      return
    }
    
    if (!marketStatus.canPlaceLimitOrder && orderType === 'LIMIT') {
      addNotification('warning', 'Limit Order Blocked / 限价单被阻止', 
        `${selectedSymbol} orders are not allowed during ${marketStatus.status}. ${marketStatus.reason || ''}`)
      return
    }

    // Determine account type
    const isCrypto = CRYPTO_ASSETS.some(a => a.symbol === selectedSymbol)
    const accountTypeForOrder = isCrypto ? 'CRYPTO_MARGIN' : 'EQUITY_CASH' as const
    const selectedAccount = isCrypto ? account.cryptoAccount : account.equityAccount

    const orderValue = price * qty
    const maxLeverage = isCrypto ? 20 : 1 // Equity has no leverage (cash account)
    
    // Validate leverage limit
    if (leverage > maxLeverage) {
      addNotification('warning', 'Leverage Limit / 杠杆限制', 
        `${isCrypto ? 'Crypto' : 'Equity (cash account)'} max leverage is ${maxLeverage}x`)
      return
    }
    
    // Validate order using new AccountContext
    const validation = account.validateOrder({
      accountType: accountTypeForOrder,
      symbol: selectedSymbol,
      side: orderSide,
      quantity: qty,
      price,
      leverage
    })
    if (!validation.valid) {
      addNotification('warning', 'Order Validation Failed / 订单验证失败', validation.error || 'Unknown error')
      return
    }

    const marginRequired = orderValue / leverage
    if (selectedAccount.cash < marginRequired) {
      addNotification('warning', 'Insufficient Margin / 保证金不足', 
        `Required: ${formatCurrency(marginRequired)}, Available: ${formatCurrency(selectedAccount.cash)}`)
      return
    }
    
    // Determine if this order opens or closes a position
    const isOpeningPosition = !activePosition || 
      (orderSide === 'BUY' && activePosition.qty >= 0) || 
      (orderSide === 'SELL' && activePosition.qty <= 0)
    
    const newOrder: Order = {
      id: `ORD${Date.now()}`,
      symbol: selectedSymbol,
      side: orderSide,
      type: orderType as Order['type'],
      price: price,
      qty: qty,
      filled: 0,
      status: 'NEW',
      time: new Date().toLocaleTimeString('en-US', { hour12: false }),
      isOpen: isOpeningPosition
    }
    
    setOrders(prev => [newOrder, ...prev])
    addNotification('info', 'Order Placed / 订单已提交', `Executing with simulated delays (300-1200ms)...`)

    // Simulate order execution with realistic delays, failures, and partial fills
    const currentBid = priceData[selectedSymbol]?.bid || price
    const currentAsk = priceData[selectedSymbol]?.ask || price
    
    // Get market status for liquidity/spread multipliers
    const currentMarketStatusForOrder = getMarketStatus(selectedSymbol)
    
    // Call the async order execution simulator
    // Map new order types for the simulator
    const simOrderType = orderType.startsWith('STOP') ? 'STOP' : orderType
    simulateOrderExecution({
      quantity: qty,
      limitPrice: price,
      currentBid,
      currentAsk,
      orderType: simOrderType as 'LIMIT' | 'MARKET' | 'STOP' | 'IOC' | 'FOK',
      side: orderSide,
      liquidityMultiplier: currentMarketStatusForOrder.liquidityMultiplier,
      spreadMultiplier: currentMarketStatusForOrder.spreadMultiplier,
      canPlaceMarketOrder: currentMarketStatusForOrder.canPlaceMarketOrder,
    }).then((execution) => {
      // Apply the execution delay for realism
      setTimeout(() => {
        if (execution.status === 'REJECTED') {
          // Order failed
          setOrders(prev => prev.map(o => 
            o.id === newOrder.id ? { ...o, status: 'CANCELLED' as const } : o
          ))
          addNotification('error', 'Order Rejected / 订单被拒', execution.failureReason || 'Unknown error')
          return
        }

        if (execution.status === 'REQUOTED') {
          // Order needs requote
          setOrders(prev => prev.map(o => 
            o.id === newOrder.id ? { ...o, status: 'CANCELLED' as const } : o
          ))
          addNotification('warning', 'Requote / 重新报价', 
            `${execution.failureReason}. New price: ${formatCurrency(execution.requotePrice || price)}`)
          return
        }

        // Calculate fees
        const executionFee = calculateExecutionFee(execution.executedQty, execution.executedPrice, 0.001)

        // Update order status
        const isFilled = execution.status === 'FILLED'
        setOrders(prev => prev.map(o => 
          o.id === newOrder.id 
            ? { 
                ...o, 
                filled: execution.executedQty, 
                status: isFilled ? 'FILLED' as const : 'PARTIAL' as const,
                price: execution.executedPrice
              } 
            : o
        ))

        // Record trade (local state for UI)
        const newTrade: Trade = {
          id: `T${Date.now()}`,
          symbol: selectedSymbol,
          side: orderSide,
          price: execution.executedPrice,
          qty: execution.executedQty,
          time: new Date().toLocaleTimeString('en-US', { hour12: false })
        }
        setTrades(prev => [newTrade, ...prev].slice(0, 100))

        // Add trade to account context (generates id and fee internally)
        account.addTrade({
          symbol: selectedSymbol,
          accountType: accountTypeForOrder,
          quantity: execution.executedQty,
          executionPrice: execution.executedPrice,
          side: orderSide,
          timestamp: Date.now(),
        })

        // Update position
        updatePosition(selectedSymbol, orderSide, execution.executedQty, execution.executedPrice, isCrypto ? 'crypto' : 'equity')

        addNotification(
          'trade',
          `Order ${isFilled ? 'Filled' : 'Partially Filled'} / 订单${isFilled ? '成交' : '部分成交'}`,
          `${orderSide} ${execution.executedQty.toFixed(4)} ${selectedSymbol} @ ${formatCurrency(execution.executedPrice)} (fee: ${formatCurrency(executionFee)})`,
          { symbol: selectedSymbol, side: orderSide, qty: execution.executedQty, price: execution.executedPrice }
        )

        if (execution.status === 'PARTIALLY_FILLED') {
          addNotification('warning', 'Partial Fill / 部分成交', 
            `Only ${execution.executedQty.toFixed(4)} of ${qty} filled. Remaining ${execution.remainingQty.toFixed(4)} cancelled.`)
        }
      }, execution.executionDelay)
    })
    
    setOrderQty('')
    if (orderType !== 'LIMIT') setOrderPrice('')
  }
  
  // Enhanced position update with account context
  const updatePosition = (symbol: string, side: 'BUY' | 'SELL', qty: number, price: number, _accountType: 'crypto' | 'equity') => {
    setPositions(prev => {
      const existing = prev.find(p => p.symbol === symbol)
      if (existing) {
        const newQty = side === 'BUY' ? existing.qty + qty : existing.qty - qty
        if (Math.abs(newQty) < 0.0001) {
          // Close position
          return prev.filter(p => p.symbol !== symbol)
        }
        const newAvgPrice = side === 'BUY' 
          ? (existing.avgPrice * existing.qty + price * qty) / (existing.qty + qty)
          : existing.avgPrice
        
        const currentPrice = priceData[symbol]?.price || price
        const unrealizedPnL = (currentPrice - newAvgPrice) * newQty

        return prev.map(p => p.symbol === symbol ? {
          ...p,
          qty: newQty,
          avgPrice: newAvgPrice,
          currentPrice: currentPrice,
          pnl: unrealizedPnL,
          pnlPercent: ((currentPrice - newAvgPrice) / newAvgPrice) * 100,
          value: Math.abs(newQty * currentPrice)
        } : p)
      } else if (side === 'BUY') {
        const currentPrice = priceData[symbol]?.price || price
        return [...prev, {
          symbol,
          qty,
          avgPrice: price,
          currentPrice,
          pnl: 0,
          pnlPercent: 0,
          value: qty * currentPrice,
          positionType: 'MANUAL' as const,
          openTime: Date.now()
        }]
      }
      return prev
    })
  }

  const handleCancelOrder = (orderId: string) => {
    setOrders(prev => prev.map(o => o.id === orderId ? { ...o, status: 'CANCELLED' as const } : o))
    addNotification('info', 'Order Cancelled / 订单已取消', `Order ${orderId.slice(-6)} cancelled / 订单 ${orderId.slice(-6)} 已取消`)
  }

  const handleCancelAll = () => {
    const activeOrders = orders.filter(o => o.status === 'NEW' || o.status === 'PARTIAL')
    setOrders(prev => prev.map(o => 
      o.status === 'NEW' || o.status === 'PARTIAL' 
        ? { ...o, status: 'CANCELLED' as const } 
        : o
    ))
    if (activeOrders.length > 0) {
      addNotification('info', 'All Orders Cancelled / 全部订单已取消', `${activeOrders.length} orders cancelled / ${activeOrders.length} 个订单已取消`)
    }
  }

  const handleClosePosition = (symbol: string) => {
    const position = positions.find(p => p.symbol === symbol)
    if (position) {
      setPositions(prev => prev.filter(p => p.symbol !== symbol))
      addNotification('trade', 'Position Closed / 持仓已平', 
        `${symbol} ${position.qty > 0 ? 'LONG' : 'SHORT'} closed, P&L: ${formatCurrency(position.pnl)}`)
    }
  }

  const calculatePositionSize = () => {
    const currentPrice = priceData[selectedSymbol]?.price || 0
    if (!currentPrice) return 0
    const riskAmount = accountBalance * (riskPercent / 100)
    return (riskAmount * leverage) / currentPrice
  }

  const currentData = priceData[selectedSymbol]
  // allAssets used for reference - filtering done inline in watchlist
  const _allAssets = [...CRYPTO_ASSETS, ...STOCK_ASSETS]
  void _allAssets // suppress unused warning

  const totalPositionValue = positions.reduce((sum, p) => sum + p.value, 0)
  const activeOrdersCount = orders.filter(o => o.status === 'NEW' || o.status === 'PARTIAL').length

  // Get market status for selected symbol
  const currentMarketStatus = getMarketStatus(selectedSymbol)
  
  // Determine account type based on selected asset
  const isCryptoAsset = CRYPTO_ASSETS.some(a => a.symbol === selectedSymbol)
  const currentAccountType = isCryptoAsset ? 'crypto' : 'equity'
  const selectedAccount = isCryptoAsset ? account.cryptoAccount : account.equityAccount
  
  // Calculate risk metrics for active position
  const activePosition = positions.find(p => p.symbol === selectedSymbol)

  return (
    <div className="h-screen bg-[#0a0a0a] text-[#e0e0e0] font-mono text-xs flex flex-col overflow-hidden">
      {/* 全局导航栏 / Global Navigation */}
      <GlobalNavbar 
        accountBalance={accountBalance}
        dailyPnL={dailyPnL}
        weeklyPnL={weeklyPnL}
        winRate={winRate}
        sharpeRatio={sharpeRatio}
        showMetrics={true}
        compact={true}
      />

      {/* 全局状态栏 / Global Status Bar - MODE | ENV | ACCOUNT | VENUE | DATA | LAT | CLOCK */}
      <div className="h-auto md:h-7 bg-[#0a0a0a] border-b border-[#1a1a1a] flex flex-wrap md:flex-nowrap items-center justify-between px-2 py-1 md:py-0 text-[9px] font-mono gap-1 md:gap-0">
        <div className="flex items-center gap-2 flex-wrap">
          {/* MODE */}
          <span className="text-[#555] hidden md:inline">MODE:</span>
          <span className={`px-1.5 py-0.5 rounded font-bold ${
            isReplayMode ? 'bg-[#ff00ff30] text-[#ff00ff] border border-[#ff00ff]' : 
            'bg-[#00aa6630] text-[#00aa66] border border-[#00aa66]'
          }`}>
            {isReplayMode ? 'REPLAY' : 'LIVE'}
          </span>
          
          <div className="h-3 w-px bg-[#333] hidden md:block" />
          
          {/* ENV - Strong visual for LIVE/PAPER */}
          <span className="text-[#555] hidden md:inline">ENV:</span>
          <span className={`px-2 py-0.5 rounded font-bold text-[10px] ${
            accountMode === 'LIVE' 
              ? 'bg-[#ff4444] text-white border-2 border-[#ff6666] animate-pulse' 
              : accountMode === 'PAPER' 
                ? 'bg-[#ffaa00] text-[#000] border border-[#ffcc00]' 
                : 'bg-[#00aaff30] text-[#00aaff] border border-[#00aaff]'
          }`}>
            {accountMode === 'LIVE' ? '⚠ LIVE ⚠' : accountMode}
          </span>
          
          <div className="h-3 w-px bg-[#333] hidden md:block" />
          
          {/* ACCOUNT */}
          <span className="text-[#555] hidden md:inline">ACCT:</span>
          <span className={`px-1.5 py-0.5 rounded ${
            currentAccountType === 'crypto' ? 'bg-[#ffaa0020] text-[#aa8800]' : 'bg-[#00aaff20] text-[#0088cc]'
          }`}>
            {currentAccountType.toUpperCase()}
          </span>
          <span className="text-[#888]">{formatCurrency(selectedAccount.equity)}</span>
          
          <div className="h-3 w-px bg-[#333]" />
          
          {/* VENUE */}
          <span className="text-[#555]">VENUE:</span>
          <select 
            value={currentVenue} 
            onChange={(e) => setCurrentVenue(e.target.value as any)}
            className="bg-[#1a1a1a] border border-[#333] px-1 py-0.5 rounded text-[#888] hover:border-[#555] focus:outline-none cursor-pointer"
          >
            <option value="AUTO">AUTO</option>
            <option value="BINANCE">BINANCE</option>
            <option value="OKX">OKX</option>
            <option value="KRAKEN">KRAKEN</option>
          </select>
          
          <div className="h-3 w-px bg-[#333]" />
          
          {/* DATA Source Toggle */}
          <span className="text-[#555]">DATA:</span>
          <button 
            onClick={() => setDataSource(d => d === 'PRIMARY' ? 'BACKUP' : 'PRIMARY')}
            className={`px-1.5 py-0.5 rounded border ${
              dataSource === 'PRIMARY' 
                ? 'bg-[#00aa6620] text-[#00aa66] border-[#00aa66]' 
                : 'bg-[#ffaa0020] text-[#ffaa00] border-[#ffaa00]'
            }`}
          >
            {dataSource}
          </button>
          
          <div className="h-3 w-px bg-[#333]" />
          
          {/* LAT - Latency */}
          <span className="text-[#555]">LAT:</span>
          <span className={`${marketDataLag < 50 ? 'text-[#00aa66]' : marketDataLag < 200 ? 'text-[#ffaa00]' : 'text-[#ff4444]'}`}>
            {marketDataLag}ms
          </span>
        </div>
        
        <div className="flex items-center gap-3">
          {/* Kill Switch */}
          <button 
            onClick={() => setKillSwitchActive(!killSwitchActive)}
            className={`px-2 py-0.5 rounded font-bold ${
              killSwitchActive 
                ? 'bg-[#ff4444] text-white animate-pulse border border-[#ff6666]' 
                : 'bg-[#1a1a1a] text-[#555] border border-[#333] hover:border-[#ff4444] hover:text-[#ff4444]'
            }`}
          >
            {killSwitchActive ? '⚠ KILL' : 'KILL'}
          </button>
          
          <div className="h-3 w-px bg-[#333]" />
          
          {/* CLOCK - UTC + Local */}
          <span className="text-[#555]">UTC:</span>
          <span className="text-[#888]">{new Date().toLocaleTimeString('en-US', { hour12: false, timeZone: 'UTC' })}</span>
          <span className="text-[#555]">|</span>
          <span className="text-[#666]">LOC:</span>
          <span className="text-[#aaa]">{new Date().toLocaleTimeString('en-US', { hour12: false })}</span>
        </div>
      </div>

      {/* 交易工具栏 / Trading Toolbar - Compact */}
      <div className="h-5 bg-[#0d0d0d] border-b border-[#1a1a1a] hidden md:flex items-center justify-between px-3 text-[9px] font-mono">
        <div className="flex items-center gap-3">
          <span className="text-[#888]">{selectedSymbol}</span>
          <span className="text-[#666]">{leverage}x</span>
          
          {/* Market Status Badge */}
          <span className={`px-1.5 py-0.5 rounded text-[8px] font-bold ${
            currentMarketStatus.status === 'OPEN' ? 'bg-[#00aa6630] text-[#00aa66]' : 
            currentMarketStatus.status === 'AFTER_HOURS' ? 'bg-[#00aaff30] text-[#00aaff]' : 
            'bg-[#cc333330] text-[#cc3333]'
          }`}>
            {currentMarketStatus.status.replace('_', ' ')}
          </span>
        </div>
        <div className="flex items-center gap-3 text-[10px] font-mono">
          {/* Max Loss Today */}
          <span className={`${realizedLossToday > maxLossToday * 0.8 ? 'text-[#ff4444]' : 'text-[#666]'}`}>
            MaxLoss: <span className={realizedLossToday > maxLossToday * 0.5 ? 'text-[#ffaa00]' : 'text-[#888]'}>{formatCurrency(realizedLossToday)}</span>/<span className="text-[#ff4444]">{formatCurrency(maxLossToday)}</span>
          </span>
          <span className="text-[#666]">Equity: <span className="text-[#00ff88] font-mono">{formatCurrency(selectedAccount.equity)}</span></span>
          <span className="text-[#666]">Cash: <span className="text-white font-mono">{formatCurrency(selectedAccount.cash)}</span></span>
          <span className="text-[#666]">Pos: <span className="text-white font-mono">{positions.length}</span></span>
          <span className="text-[#666]">Ord: <span className="text-white font-mono">{activeOrdersCount}</span></span>
        </div>
      </div>

      {/* 移动端简化工具栏 */}
      <div className="md:hidden h-8 bg-[#0d0d0d] border-b border-[#1a1a1a] flex items-center justify-between px-2 text-[10px] font-mono overflow-x-auto">
        <div className="flex items-center gap-2 whitespace-nowrap">
          <span className="font-bold text-white">{selectedSymbol}</span>
          <span className={`px-1.5 py-0.5 rounded text-[8px] font-bold ${
            currentMarketStatus.status === 'OPEN' ? 'bg-[#00aa6630] text-[#00aa66]' : 'bg-[#cc333330] text-[#cc3333]'
          }`}>
            {currentMarketStatus.status === 'OPEN' ? '开盘' : '休市'}
          </span>
        </div>
        <div className="flex items-center gap-2 whitespace-nowrap">
          <span className="text-[#00ff88]">{formatCurrency(selectedAccount.equity)}</span>
        </div>
      </div>

      <div className="flex-1 flex overflow-hidden">
        {/* 左侧市场列表 - 移动端隐藏 */}
        <div className="hidden md:flex w-64 border-r border-[#1a1a1a] flex-col bg-[#0d0d0d]">
          {/* Search Bar */}
          <div className="h-7 border-b border-[#1a1a1a] flex items-center px-2 gap-2">
            <Search className="h-3 w-3 text-[#666]" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search..."
              className="flex-1 bg-transparent text-[#888] focus:outline-none text-[10px]"
            />
          </div>
          
          {/* Quick Filters */}
          <div className="h-6 border-b border-[#1a1a1a] flex items-center px-1 gap-1 text-[8px]">
            {(['ALL', 'POSITIONS', 'MOVERS', 'FAVORITES'] as const).map((f) => (
              <button
                key={f}
                onClick={() => setWatchlistFilter(f)}
                className={`px-1.5 py-0.5 rounded ${watchlistFilter === f ? 'bg-[#333] text-[#fff]' : 'text-[#555] hover:text-[#888]'}`}
              >
                {f === 'POSITIONS' ? 'POS' : f === 'FAVORITES' ? '★' : f}
              </button>
            ))}
          </div>
          
          {/* Column Headers */}
          <div className="h-5 border-b border-[#222] flex items-center px-2 text-[8px] text-[#555] font-mono">
            <span className="w-14">SYM</span>
            <span className="w-16 text-right">LAST</span>
            <span className="w-12 text-right">%CHG</span>
            <span className="w-12 text-right">SPRD</span>
          </div>
          
          <div className="flex-1 overflow-y-auto">
            {/* Crypto Group */}
            <div 
              onClick={() => setCryptoCollapsed(!cryptoCollapsed)}
              className="h-5 bg-[#111] border-b border-[#1a1a1a] flex items-center px-2 cursor-pointer hover:bg-[#151515]"
            >
              <span className="text-[8px] text-[#ffaa00] font-bold flex items-center gap-1">
                {cryptoCollapsed ? '▶' : '▼'} CRYPTO
              </span>
              <span className="ml-auto text-[8px] text-[#555]">{CRYPTO_ASSETS.length}</span>
            </div>
            {!cryptoCollapsed && CRYPTO_ASSETS.filter(a => {
              if (watchlistFilter === 'POSITIONS') return positions.some(p => p.symbol === a.symbol)
              if (watchlistFilter === 'FAVORITES') return favorites.includes(a.symbol)
              if (watchlistFilter === 'MOVERS') return Math.abs(priceData[a.symbol]?.changePercent || 0) > 3
              if (searchQuery) return a.symbol.toLowerCase().includes(searchQuery.toLowerCase())
              return true
            }).map((asset) => {
              const data = priceData[asset.symbol]
              const isSelected = selectedSymbol === asset.symbol
              const hasPosition = positions.some(p => p.symbol === asset.symbol)
              const isFavorite = favorites.includes(asset.symbol)
              // Anomaly detection
              const isStale = !data || (Date.now() - (data as any).lastUpdate > 30000)
              const isWideSpread = data && (data.spread / data.price) > 0.005
              const isHalt = false // Would come from market status
              
              return (
                <div
                  key={asset.id}
                  onClick={() => setSelectedSymbol(asset.symbol)}
                  className={`flex items-center px-2 py-1 cursor-pointer border-b border-[#151515] hover:bg-[#151515] text-[9px] font-mono ${
                    isSelected ? 'bg-[#0a1a0a] border-l-2 border-l-[#00aa66]' : ''
                  }`}
                >
                  <div className="w-14 flex items-center gap-1">
                    <span className={`font-semibold ${isSelected ? 'text-[#00aa66]' : 'text-[#ccc]'}`}>{asset.symbol}</span>
                    {hasPosition && <span className="text-[6px] text-[#00aaff]">●</span>}
                    {isFavorite && <span className="text-[6px] text-[#ffaa00]">★</span>}
                  </div>
                  <div className="w-16 text-right text-[#fff]">{data ? formatCurrency(data.price, 2) : '--'}</div>
                  <div className={`w-12 text-right ${(data?.changePercent ?? 0) >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}`}>
                    {data ? `${(data.changePercent ?? 0) >= 0 ? '+' : ''}${(data.changePercent ?? 0).toFixed(1)}%` : '--'}
                  </div>
                  <div className="w-12 text-right text-[#666]">
                    {data ? (data.spread ?? 0).toFixed(2) : '--'}
                  </div>
                  {/* Anomaly Labels */}
                  {(isStale || isWideSpread || isHalt) && (
                    <div className="ml-1 flex gap-0.5">
                      {isHalt && <span className="px-0.5 bg-[#ff4444] text-[#fff] text-[6px]">HALT</span>}
                      {isStale && <span className="px-0.5 bg-[#ffaa00] text-[#000] text-[6px]">STALE</span>}
                      {isWideSpread && <span className="px-0.5 bg-[#ff6600] text-[#fff] text-[6px]">WIDE</span>}
                    </div>
                  )}
                </div>
              )
            })}
            
            {/* Equity Group */}
            <div 
              onClick={() => setEquityCollapsed(!equityCollapsed)}
              className="h-5 bg-[#111] border-b border-[#1a1a1a] flex items-center px-2 cursor-pointer hover:bg-[#151515]"
            >
              <span className="text-[8px] text-[#00aaff] font-bold flex items-center gap-1">
                {equityCollapsed ? '▶' : '▼'} EQUITY
              </span>
              <span className="ml-auto text-[8px] text-[#555]">{STOCK_ASSETS.length}</span>
            </div>
            {!equityCollapsed && STOCK_ASSETS.filter(a => {
              if (watchlistFilter === 'POSITIONS') return positions.some(p => p.symbol === a.symbol)
              if (watchlistFilter === 'FAVORITES') return favorites.includes(a.symbol)
              if (watchlistFilter === 'MOVERS') return Math.abs(priceData[a.symbol]?.changePercent || 0) > 3
              if (searchQuery) return a.symbol.toLowerCase().includes(searchQuery.toLowerCase())
              return true
            }).map((asset) => {
              const data = priceData[asset.symbol]
              const isSelected = selectedSymbol === asset.symbol
              const hasPosition = positions.some(p => p.symbol === asset.symbol)
              const isFavorite = favorites.includes(asset.symbol)
              const isStale = !data || (Date.now() - (data as any).lastUpdate > 30000)
              const isWideSpread = data && (data.spread / data.price) > 0.003
              
              return (
                <div
                  key={asset.id}
                  onClick={() => setSelectedSymbol(asset.symbol)}
                  className={`flex items-center px-2 py-1 cursor-pointer border-b border-[#151515] hover:bg-[#151515] text-[9px] font-mono ${
                    isSelected ? 'bg-[#0a1a0a] border-l-2 border-l-[#00aa66]' : ''
                  }`}
                >
                  <div className="w-14 flex items-center gap-1">
                    <span className={`font-semibold ${isSelected ? 'text-[#00aa66]' : 'text-[#ccc]'}`}>{asset.symbol}</span>
                    {hasPosition && <span className="text-[6px] text-[#00aaff]">●</span>}
                    {isFavorite && <span className="text-[6px] text-[#ffaa00]">★</span>}
                  </div>
                  <div className="w-16 text-right text-[#fff]">{data ? formatCurrency(data.price, 2) : '--'}</div>
                  <div className={`w-12 text-right ${(data?.changePercent ?? 0) >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}`}>
                    {data ? `${(data.changePercent ?? 0) >= 0 ? '+' : ''}${(data.changePercent ?? 0).toFixed(1)}%` : '--'}
                  </div>
                  <div className="w-12 text-right text-[#666]">
                    {data ? (data.spread ?? 0).toFixed(2) : '--'}
                  </div>
                  {(isStale || isWideSpread) && (
                    <div className="ml-1 flex gap-0.5">
                      {isStale && <span className="px-0.5 bg-[#ffaa00] text-[#000] text-[6px]">STALE</span>}
                      {isWideSpread && <span className="px-0.5 bg-[#ff6600] text-[#fff] text-[6px]">WIDE</span>}
                    </div>
                  )}
                </div>
              )
            })}
            
            {/* Commodity Group - 贵金属 */}
            <div 
              onClick={() => setCommodityCollapsed(!commodityCollapsed)}
              className="h-5 bg-[#111] border-b border-[#1a1a1a] flex items-center px-2 cursor-pointer hover:bg-[#151515]"
            >
              <span className="text-[8px] text-[#ffd700] font-bold flex items-center gap-1">
                {commodityCollapsed ? '▶' : '▼'} 🥇 METALS
              </span>
              <span className="ml-auto text-[8px] text-[#555]">{COMMODITY_ASSETS.length}</span>
            </div>
            {!commodityCollapsed && COMMODITY_ASSETS.filter(a => {
              if (watchlistFilter === 'POSITIONS') return positions.some(p => p.symbol === a.symbol)
              if (watchlistFilter === 'FAVORITES') return favorites.includes(a.symbol)
              if (watchlistFilter === 'MOVERS') return Math.abs(priceData[a.symbol]?.changePercent || 0) > 1.5
              if (searchQuery) return a.symbol.toLowerCase().includes(searchQuery.toLowerCase()) || a.name.toLowerCase().includes(searchQuery.toLowerCase())
              return true
            }).map((asset) => {
              const data = priceData[asset.symbol]
              const isSelected = selectedSymbol === asset.symbol
              const hasPosition = positions.some(p => p.symbol === asset.symbol)
              const isFavorite = favorites.includes(asset.symbol)
              const isStale = !data || (Date.now() - (data as any).lastUpdate > 60000)
              const isWideSpread = data && (data.spread / data.price) > 0.002
              
              return (
                <div
                  key={asset.id}
                  onClick={() => setSelectedSymbol(asset.symbol)}
                  className={`flex items-center px-2 py-1 cursor-pointer border-b border-[#151515] hover:bg-[#151515] text-[9px] font-mono ${
                    isSelected ? 'bg-[#1a1508] border-l-2 border-l-[#ffd700]' : ''
                  }`}
                >
                  <div className="w-14 flex items-center gap-1">
                    <span className="text-[10px]">{asset.icon}</span>
                    <span className={`font-semibold ${isSelected ? 'text-[#ffd700]' : 'text-[#ccc]'}`}>{asset.symbol}</span>
                    {hasPosition && <span className="text-[6px] text-[#00aaff]">●</span>}
                    {isFavorite && <span className="text-[6px] text-[#ffaa00]">★</span>}
                  </div>
                  <div className="w-16 text-right text-[#fff]">{data ? formatCurrency(data.price, 2) : '--'}</div>
                  <div className={`w-12 text-right ${(data?.changePercent ?? 0) >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}`}>
                    {data ? `${(data.changePercent ?? 0) >= 0 ? '+' : ''}${(data.changePercent ?? 0).toFixed(2)}%` : '--'}
                  </div>
                  <div className="w-12 text-right text-[#666]">
                    {data ? (data.spread ?? 0).toFixed(2) : '--'}
                  </div>
                  {(isStale || isWideSpread) && (
                    <div className="ml-1 flex gap-0.5">
                      {isStale && <span className="px-0.5 bg-[#ffaa00] text-[#000] text-[6px]">STALE</span>}
                      {isWideSpread && <span className="px-0.5 bg-[#ff6600] text-[#fff] text-[6px]">WIDE</span>}
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        </div>

        {/* 中间主区域 */}
        <div className="flex-1 flex flex-col">
          {/* ==================== ROW 1: Symbol + Price + Stats ==================== */}
          <div className="h-7 bg-[#0d0d0d] border-b border-[#1a1a1a] flex items-center px-3 font-mono">
            {/* Left: Symbol + Price */}
            <div className="flex items-center gap-3">
              <div className="flex items-center gap-1">
                {getAssetIcon(selectedSymbol)}
                <span className="text-sm font-bold text-[#00aa66]">{selectedSymbol}</span>
                <span className="text-[#444] text-[8px]">/USD</span>
              </div>
              
              {currentData && (
                <>
                  <span className="text-[#333]">|</span>
                  <span className="text-white text-sm font-bold">{formatCurrency(currentData.price)}</span>
                  <span className={`text-xs font-semibold ${currentData.changePercent >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}`}>
                    {currentData.changePercent >= 0 ? '↑' : '↓'}{formatPercentage(currentData.changePercent)}
                  </span>
                </>
              )}
            </div>
            
            {/* Center: Quick Stats */}
            {currentData && (
              <div className="flex items-center gap-3 ml-6 text-[9px]">
                <span className="text-[#555]">SPD <span className="text-[#aa7700]">{(currentData.spread ?? 0).toFixed(2)}</span></span>
                <span className="text-[#555]">VOL <span className="text-[#888]">${((currentData.volume ?? 0) / 1e9).toFixed(1)}B</span></span>
                {currentAccountType === 'crypto' && (
                  <>
                    <span className="text-[#555]">FR <span className="text-[#00aa66]">+0.01%</span></span>
                    <span className="text-[#555]">OI <span className="text-[#888]">${((currentData.volume ?? 0) * 0.3 / 1e9).toFixed(1)}B</span></span>
                  </>
                )}
              </div>
            )}
            
            {/* Right: Market Status */}
            <span className={`ml-auto px-2 py-0.5 rounded text-[8px] font-bold ${
              currentAccountType === 'crypto' ? 'bg-[#00aa6620] text-[#00aa66]' : 'bg-[#00aaff20] text-[#00aaff]'
            }`}>
              {currentAccountType === 'crypto' ? '24/7 OPEN' : currentMarketStatus.status}
            </span>
          </div>
          
          {/* ==================== ROW 2: MARKET STATE 神经中枢 ==================== */}
          <div className={`h-6 border-b flex items-center px-3 font-mono text-[9px] ${
            currentMarketState.riskMode === 'PROTECT' ? 'bg-[#200808] border-[#ff000050]' :
            currentMarketState.riskMode === 'CAUTION' ? 'bg-[#201408] border-[#ffaa0050]' :
            'bg-[#082010] border-[#00ff8840]'
          }`}>
            {/* Left Group: Core State Info */}
            <div className="flex items-center gap-4">
              {/* REGIME - 新增机构级标签 */}
              <div className="flex items-center gap-1.5">
                <span className="text-[#555] text-[10px]">REGIME</span>
                <span className={`font-bold px-2 py-0.5 rounded text-[11px] ${
                  currentMarketState.regime === 'TRENDING' ? 'bg-[#00aaff25] text-[#00aaff]' :
                  currentMarketState.regime === 'VOLATILE' ? 'bg-[#ff880025] text-[#ff8800]' :
                  currentMarketState.regime === 'EVENT' ? 'bg-[#ff444425] text-[#ff4444]' :
                  'bg-[#88888825] text-[#888]'
                }`}>
                  {currentMarketState.regime || 'RANGE'}
                </span>
              </div>
              
              {/* TREND */}
              <div className="flex items-center gap-1.5">
                <span className="text-[#555] text-[10px]">STATE</span>
                <span className={`font-bold px-2 py-0.5 rounded text-[11px] ${
                  currentMarketState.trend === 'TREND_UP' ? 'bg-[#00ff8830] text-[#00ff88]' :
                  currentMarketState.trend === 'TREND_DOWN' ? 'bg-[#ff444430] text-[#ff4444]' :
                  currentMarketState.trend === 'BREAKOUT' ? 'bg-[#00aaff30] text-[#00aaff]' :
                  'bg-[#88888830] text-[#888]'
                }`}>
                  {currentMarketState.trend === 'TREND_UP' ? '▲ UP' :
                   currentMarketState.trend === 'TREND_DOWN' ? '▼ DOWN' :
                   currentMarketState.trend === 'BREAKOUT' ? '⚡ BREAK' : '◆ RANGE'}
                </span>
              </div>
              
              {/* CONF + Data Health Adjustment */}
              <div className="flex items-center gap-1.5">
                <span className="text-[#555] text-[10px]">CONF</span>
                <span className={`font-mono font-bold text-[12px] ${
                  currentMarketState.confidence >= 0.7 ? 'text-[#00ff88]' :
                  currentMarketState.confidence >= 0.5 ? 'text-[#ffaa00]' : 'text-[#ff4444]'
                }`}>
                  {(currentMarketState.confidence * 100).toFixed(0)}%
                </span>
                {getDataHealthImpact().adjustment !== 0 && (
                  <span className="text-[7px] text-[#ff8800]">({getDataHealthImpact().adjustment}%)</span>
                )}
              </div>
              
              {/* VALID FOR - Decision Countdown */}
              <div className="flex items-center gap-1.5">
                <span className="text-[#555] text-[10px]">VALID</span>
                <span className={`font-mono text-[11px] ${
                  getDecisionCountdown().expired ? 'text-[#ff4444] animate-pulse' : 'text-[#888]'
                }`}>
                  {getDecisionCountdown().text}
                </span>
              </div>
              
              {/* HORIZON - simplified */}
              <div className="flex items-center gap-1.5">
                <span className="text-[#555] text-[10px]">HRZ</span>
                <span className="text-[#888] text-[11px]">{currentMarketState.horizon}</span>
              </div>
              
              {/* RISK */}
              <div className="flex items-center gap-1.5">
                <span className="text-[#555] text-[10px]">RISK</span>
                <span className={`font-bold px-2 py-0.5 rounded text-[8px] ${
                  currentMarketState.riskMode === 'PROTECT' ? 'bg-[#ff0000] text-white animate-pulse' :
                  currentMarketState.riskMode === 'CAUTION' ? 'bg-[#ff8800] text-black' :
                  'bg-[#00aa66] text-black'
                }`}>
                  {currentMarketState.riskMode}
                </span>
              </div>
            </div>
            
            {/* Divider */}
            <span className="mx-3 text-[#333]">│</span>
            
            {/* FACTORS */}
            <div className="flex items-center gap-3">
              <span className="text-[#555] text-[8px]">FACTORS</span>
              {currentMarketState.factors.slice(0, 3).map((f, i) => (
                <span key={i} className={`font-mono ${
                  f.direction === '+' ? 'text-[#00aa66]' : 
                  f.direction === '-' ? 'text-[#cc4444]' : 'text-[#666]'
                }`}>
                  {f.name} {f.value}{f.direction === '+' ? '↑' : f.direction === '-' ? '↓' : ''}
                </span>
              ))}
            </div>
            
            {/* Right: SENT */}
            <div className="ml-auto flex items-center gap-1">
              <span className="text-[#555] text-[8px]">SENT</span>
              <span className={`font-mono ${
                getSentimentImpact().impact === 'SUPPORTIVE' ? 'text-[#00aa66]' :
                getSentimentImpact().impact === 'CONTRARY' ? 'text-[#cc4444]' : 'text-[#888]'
              }`}>
                {getSentimentImpact().impact}
              </span>
            </div>
          </div>
          
          {/* ==================== ROW 2.5: MARKET LABELS 主标签+辅标签 (增强版) ==================== */}
          {currentLabels.primary && (
            <div className="h-8 bg-[#050505] border-b border-[#1a1a1a] flex items-center px-3 font-mono group/labels">
              {/* 主标签 - Primary Label (最多1个) */}
              <div className="flex items-center gap-2 relative">
                <span className="text-[#444] text-[8px] font-bold">SIGNAL</span>
                <div className={`flex items-center gap-1.5 px-2.5 py-1 rounded-sm text-[10px] font-bold ${currentLabels.primaryColor} cursor-help`}>
                  <span>{currentLabels.primaryIcon}</span>
                  <span>{currentLabels.primaryText}</span>
                </div>
                
                {/* 置信度指示器 */}
                <div className="flex items-center gap-1 ml-1">
                  <span className="text-[7px] text-[#555]">置信</span>
                  <span className={`text-[9px] font-bold ${
                    currentLabels.confidence >= 80 ? 'text-[#00ff88]' :
                    currentLabels.confidence >= 60 ? 'text-[#ffaa00]' :
                    'text-[#ff6666]'
                  }`}>
                    {currentLabels.confidence}%
                  </span>
                </div>
                
                {/* 优先级指示器 */}
                <span className="text-[7px] text-[#333] ml-1">P{currentLabels.primaryPriority}</span>
                
                {/* 冷却状态 */}
                {currentLabels.cooldownRemaining > 0 && (
                  <span className="text-[7px] text-[#555] ml-1">
                    CD: {currentLabels.cooldownRemaining}K
                  </span>
                )}
                
                {/* 推理依据 Tooltip */}
                {currentLabels.reasoning.length > 0 && (
                  <div className="hidden group-hover/labels:block absolute left-0 top-full z-50 mt-1 bg-[#1a1a1a] border border-[#333] rounded p-2 shadow-xl min-w-[200px]">
                    <div className="text-[8px] text-[#00aaff] font-bold mb-1 border-b border-[#222] pb-1">📊 推理依据</div>
                    {currentLabels.reasoning.map((r, i) => (
                      <div key={i} className="text-[8px] text-[#888] py-0.5 flex items-center gap-1">
                        <span className="text-[#555]">•</span>
                        <span>{r}</span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
              
              {/* 分隔符 */}
              <span className="mx-4 text-[#222]">│</span>
              
              {/* 辅标签 - Secondary Labels (最多2个) */}
              <div className="flex items-center gap-2">
                {(() => {
                  const metrics = getMarketMetrics(selectedSymbol)
                  const secondaryLabels = detectSecondaryLabels(metrics, currentLabels.primaryPriority)
                  
                  if (secondaryLabels.length === 0) {
                    return <span className="text-[#333] text-[8px]">无辅标签</span>
                  }
                  
                  return secondaryLabels.map((s, i) => (
                    <div key={i} className={`group/sec relative flex items-center gap-1 px-2 py-0.5 rounded text-[9px] cursor-help ${s.color}`}>
                      <span>{s.icon}</span>
                      <span>{s.text}</span>
                      {/* 辅标签推理 Tooltip */}
                      <div className="hidden group-hover/sec:block absolute left-0 top-full z-50 mt-1 bg-[#1a1a1a] border border-[#333] rounded px-2 py-1 shadow-xl whitespace-nowrap">
                        <div className="text-[8px] text-[#888]">{s.reasoning}</div>
                      </div>
                    </div>
                  ))
                })()}
              </div>
              
              {/* 分隔符 */}
              <span className="mx-4 text-[#222]">│</span>
              
              {/* 事件队列指示器 */}
              <div className="flex items-center gap-1">
                <span className="text-[#444] text-[7px]">EVT</span>
                <span className={`text-[8px] font-mono ${
                  eventQueue.filter(e => e.symbol === selectedSymbol && Date.now() - e.timestamp < e.ttl).length > 0 
                    ? 'text-[#ffaa00]' : 'text-[#333]'
                }`}>
                  {eventQueue.filter(e => e.symbol === selectedSymbol && Date.now() - e.timestamp < e.ttl).length}
                </span>
              </div>
              
              {/* 右侧：互斥规则说明 */}
              <div className="ml-auto flex items-center gap-2">
                {currentLabels.primaryPriority <= 2 && (
                  <span className="text-[7px] text-[#ff6600] bg-[#ff660015] px-1.5 py-0.5 rounded">
                    ⚠ 压制趋势类标签
                  </span>
                )}
                {currentLabels.primaryPriority === 0 && (
                  <span className="text-[7px] text-[#ff0000] bg-[#ff000020] px-1.5 py-0.5 rounded animate-pulse">
                    🔴 极端事件
                  </span>
                )}
                {currentLabels.primaryPriority === 3 && (
                  <span className="text-[7px] text-[#00ff88] bg-[#00ff8815] px-1.5 py-0.5 rounded">
                    🚀 突破确认
                  </span>
                )}
                {currentLabels.primaryPriority === 4 && (
                  <span className="text-[7px] text-[#ffcc00] bg-[#ffcc0015] px-1.5 py-0.5 rounded">
                    ⚠ 假突破风险
                  </span>
                )}
              </div>
            </div>
          )}
          
          {/* ==================== ROW 3: Chart Controls (简化) ==================== */}
          <div className="h-6 bg-[#0a0a0a] border-b border-[#1a1a1a] flex items-center px-3 font-mono">
            {/* Chart Type */}
            <ChartTypeSelector value={chartType} onChange={setChartType} />
            
            {/* Timeframes - 居中 */}
            <div className="flex-1 flex justify-center">
              <div className="flex items-center gap-1 bg-[#050505] rounded px-1 py-0.5">
                {[
                  { key: '1min', label: '1m' },
                  { key: '1h', label: '1H' },
                  { key: '24h', label: '1D' },
                  { key: '1month', label: '1M' },
                  { key: '1year', label: '1Y' }
                ].map((tf) => (
                  <button
                    key={tf.key}
                    onClick={() => setChartTimeframe(tf.key)}
                    className={`px-2.5 py-0.5 text-[9px] rounded ${
                      chartTimeframe === tf.key 
                        ? 'bg-[#333] text-white font-bold' 
                        : 'text-[#555] hover:text-[#aaa]'
                    }`}
                  >
                    {tf.label}
                  </button>
                ))}
              </div>
            </div>
            
            {/* Right: CMP + LIN/LOG */}
            <div className="flex items-center gap-1">
              <button 
                onClick={() => setShowCompare(!showCompare)}
                className={`px-2 py-0.5 text-[8px] rounded ${
                  showCompare ? 'bg-[#00aaff30] text-[#00aaff]' : 'bg-[#151515] text-[#555] hover:text-[#888]'
                }`}
              >
                CMP
              </button>
              <button 
                onClick={() => setPriceScaleMode(m => m === 'linear' ? 'log' : 'linear')}
                className={`px-2 py-0.5 text-[8px] rounded ${
                  priceScaleMode === 'log' ? 'bg-[#ffaa0030] text-[#ffaa00]' : 'bg-[#151515] text-[#555] hover:text-[#888]'
                }`}
              >
                {priceScaleMode === 'log' ? 'LOG' : 'LIN'}
              </button>
            </div>
          </div>

          {/* 专业图表区域 - TradingView Lightweight Charts */}
          <div className="h-52 bg-[#0a0a0a] border-b border-[#1a1a1a] relative">
            <LightweightChart
              key={`${selectedSymbol}-${chartTimeframe}-${priceScaleMode}`}
              symbol={selectedSymbol}
              basePrice={currentData?.price || 95000}
              chartType={chartType as any}
              timeframe={chartTimeframe}
              height={205}
              showVolume={true}
              showMA={true}
            />
          </div>

          {/* Critical Events Only - Filtered for Main Page (REJECT, KILL, DATA STALE, LARGE SLIPPAGE, LARGE FILL) */}
          {(() => {
            const criticalEvents = marketEvents.filter(e => 
              e.type === 'REJECT' || 
              e.type === 'KILL' || 
              e.severity === 'critical' ||
              e.label.includes('STALE') ||
              e.label.includes('SLIPPAGE') ||
              e.label.includes('LARGE')
            )
            return criticalEvents.length > 0 && (
              <div className="h-6 bg-[#0d0d0d] border-b border-[#1a1a1a] flex items-center px-3 gap-2 overflow-x-auto">
                <span className="text-[#666] text-[8px]">CRITICAL:</span>
                {criticalEvents.slice(0, 3).map((event, idx) => (
                  <span key={idx} className={`text-[8px] px-1.5 py-0.5 rounded ${
                    event.severity === 'critical' ? 'bg-[#ff000020] text-[#ff4444] border border-[#ff000040]' :
                    event.severity === 'warning' ? 'bg-[#ffaa0020] text-[#ffaa00] border border-[#ffaa0040]' :
                    'bg-[#00aaff20] text-[#00aaff] border border-[#00aaff40]'
                  }`}>
                    {event.label}
                  </span>
                ))}
                {criticalEvents.length > 3 && (
                  <span className="text-[7px] text-[#555]">+{criticalEvents.length - 3} more</span>
                )}
              </div>
            )
          })()}

          {/* 订单簿 + 交易面板 */}
          <div className="flex-1 flex overflow-hidden">
            {/* 订单簿 */}
            <div className="w-64 border-r border-[#1a1a1a] flex flex-col bg-[#0d0d0d]">
              <div className="h-6 bg-[#111] border-b border-[#1a1a1a] flex items-center justify-between px-2 text-[#666]">
                <span>ORDER BOOK</span>
                <div className="flex items-center gap-2">
                  <span className={`text-[8px] px-1 py-0.5 rounded border ${getLiquidityStatus().color} ${
                    getLiquidityStatus().status === 'DEEP' ? 'border-[#00ff8840] bg-[#00ff8810]' :
                    getLiquidityStatus().status === 'GOOD' ? 'border-[#00aa6640] bg-[#00aa6610]' :
                    getLiquidityStatus().status === 'THIN' ? 'border-[#ffaa0040] bg-[#ffaa0010]' :
                    'border-[#ff444440] bg-[#ff444410]'
                  }`}>
                    LIQ: {getLiquidityStatus().status}
                  </span>
                  <span className="text-[9px]">DEPTH</span>
                </div>
              </div>
              
              {/* 卖单 */}
              <div className="flex-1 overflow-hidden flex flex-col-reverse">
                {orderBook.asks.slice(0, 12).reverse().map((ask, i) => (
                  <div key={i} className="flex items-center px-2 py-0.5 hover:bg-[#151515] relative cursor-pointer" onClick={() => setOrderPrice((ask.price ?? 0).toFixed(2))}>
                    <div className="absolute left-0 top-0 bottom-0 bg-[#ff444415]" style={{ width: `${Math.min((ask.total ?? 0) / 300 * 100, 100)}%` }} />
                    <span className={`w-20 text-[10px] relative z-10 ${ask.myOrder ? 'text-[#ffaa00]' : 'text-[#ff4444]'}`}>{formatCurrency(ask.price ?? 0, 2)}</span>
                    <span className="w-14 text-right text-[10px] text-[#888] relative z-10">{(ask.qty ?? 0).toFixed(3)}</span>
                    <span className="w-14 text-right text-[9px] text-[#555] relative z-10">{(ask.total ?? 0).toFixed(1)}</span>
                    {ask.myOrder && <span className="absolute right-1 text-[8px] text-[#ffaa00]">★</span>}
                  </div>
                ))}
              </div>

              {/* 中间价格 */}
              <div className="h-8 bg-[#111] border-y border-[#222] flex items-center justify-center gap-2">
                <span className="text-[#fff] font-bold text-sm">{currentData ? formatCurrency(currentData.price) : '--'}</span>
                {currentData && (
                  <span className={`text-[10px] ${currentData.changePercent >= 0 ? 'text-[#00ff88]' : 'text-[#ff4444]'}`}>
                    {currentData.changePercent >= 0 ? '▲' : '▼'} {Math.abs(currentData.change || 0).toFixed(2)}
                  </span>
                )}
              </div>

              {/* 买单 */}
              <div className="flex-1 overflow-hidden">
                {orderBook.bids.slice(0, 12).map((bid, i) => (
                  <div key={i} className="flex items-center px-2 py-0.5 hover:bg-[#151515] relative cursor-pointer" onClick={() => setOrderPrice((bid.price ?? 0).toFixed(2))}>
                    <div className="absolute left-0 top-0 bottom-0 bg-[#00ff8815]" style={{ width: `${Math.min((bid.total ?? 0) / 300 * 100, 100)}%` }} />
                    <span className={`w-20 text-[10px] relative z-10 ${bid.myOrder ? 'text-[#ffaa00]' : 'text-[#00ff88]'}`}>{formatCurrency(bid.price ?? 0, 2)}</span>
                    <span className="w-14 text-right text-[10px] text-[#888] relative z-10">{(bid.qty ?? 0).toFixed(3)}</span>
                    <span className="w-14 text-right text-[9px] text-[#555] relative z-10">{(bid.total ?? 0).toFixed(1)}</span>
                    {bid.myOrder && <span className="absolute right-1 text-[8px] text-[#ffaa00]">★</span>}
                  </div>
                ))}
              </div>
            </div>

            {/* 交易面板 - 移动端隐藏 */}
            <div className="hidden lg:flex w-72 border-r border-[#1a1a1a] flex-col bg-[#0d0d0d]">
              <div className="h-6 bg-[#111] border-b border-[#1a1a1a] flex items-center justify-between px-2">
                <span className="text-[#666] text-[10px]">ORDER ENTRY</span>
                {/* Effective Leverage Display */}
                <span className="text-[9px] font-mono">
                  <span className="text-[#555]">EFF.LEV:</span>
                  <span className={`ml-1 ${leverage * (parseFloat(orderQty) || 0) * (parseFloat(orderPrice) || 0) / selectedAccount.equity > 5 ? 'text-[#ff4444]' : 'text-[#00aaff]'}`}>
                    {((leverage * (parseFloat(orderQty) || 0) * (parseFloat(orderPrice) || 0) / selectedAccount.equity) || 0).toFixed(2)}x
                  </span>
                </span>
              </div>
              
              <div className="p-2 space-y-2 flex-1 overflow-y-auto">
                {/* 买卖切换 with Open/Close indicator */}
                <div className="grid grid-cols-2 gap-1">
                  <button onClick={() => setOrderSide('BUY')} className={`py-1.5 font-bold text-xs relative ${orderSide === 'BUY' ? 'bg-[#00aa66] text-[#000]' : 'bg-[#1a1a1a] text-[#666] hover:bg-[#222]'}`}>
                    BUY [B]
                    {orderSide === 'BUY' && (
                      <span className="absolute -top-1 right-1 text-[7px] px-1 py-0 bg-[#000] text-[#00ff88] rounded">
                        {activePosition && activePosition.qty < 0 ? 'CLOSE' : 'OPEN'}
                      </span>
                    )}
                  </button>
                  <button onClick={() => setOrderSide('SELL')} className={`py-1.5 font-bold text-xs relative ${orderSide === 'SELL' ? 'bg-[#cc3333] text-[#fff]' : 'bg-[#1a1a1a] text-[#666] hover:bg-[#222]'}`}>
                    SELL [S]
                    {orderSide === 'SELL' && (
                      <span className="absolute -top-1 right-1 text-[7px] px-1 py-0 bg-[#000] text-[#ff4444] rounded">
                        {activePosition && activePosition.qty > 0 ? 'CLOSE' : 'OPEN'}
                      </span>
                    )}
                  </button>
                </div>

                {/* 订单类型 with STOP-MKT and STOP-LMT */}
                <div className="grid grid-cols-3 gap-0.5">
                  {(['LIMIT', 'MARKET', 'IOC'] as const).map((type) => (
                    <button key={type} onClick={() => setOrderType(type)} className={`py-0.5 text-[9px] font-mono ${orderType === type ? 'bg-[#333] text-[#fff]' : 'bg-[#1a1a1a] text-[#555] hover:bg-[#222]'}`}>{type}</button>
                  ))}
                </div>
                <div className="grid grid-cols-3 gap-0.5">
                  {(['STOP-MKT', 'STOP-LMT', 'FOK'] as const).map((type) => (
                    <button key={type} onClick={() => setOrderType(type)} className={`py-0.5 text-[9px] font-mono ${orderType === type ? 'bg-[#333] text-[#fff]' : 'bg-[#1a1a1a] text-[#555] hover:bg-[#222]'}`}>{type}</button>
                  ))}
                </div>

                {/* 杠杆 */}
                <div>
                  <div className="flex items-center justify-between mb-1">
                    <span className="text-[#555] text-[9px]">LEVERAGE</span>
                    <span className="text-[#00aaff] text-[9px] font-mono">{leverage}x</span>
                  </div>
                  <div className="flex gap-1">
                    {[1, 2, 3, 5, 10, 20].map((lev) => (
                      <button key={lev} onClick={() => setLeverage(lev)} className={`flex-1 py-0.5 text-[9px] font-mono ${leverage === lev ? 'bg-[#00aaff] text-[#000]' : 'bg-[#1a1a1a] text-[#666] hover:bg-[#222]'}`}>{lev}x</button>
                    ))}
                  </div>
                </div>

                {/* 价格 with Stop Trigger for STOP orders */}
                <div>
                  <label className="text-[#555] text-[9px]">{orderType.startsWith('STOP') ? 'TRIGGER PRICE' : 'PRICE'}</label>
                  <div className="flex">
                    <button onClick={() => setOrderPrice((p) => ((parseFloat(p) || 0) - 1).toFixed(2))} className="px-2 bg-[#1a1a1a] text-[#888] hover:bg-[#222]"><Minus className="h-3 w-3" /></button>
                    <input type="text" value={orderPrice} onChange={(e) => setOrderPrice(e.target.value)} className="flex-1 bg-[#111] border-y border-[#222] px-2 py-1 text-[#fff] text-center text-xs font-mono focus:outline-none" disabled={orderType === 'MARKET'} />
                    <button onClick={() => setOrderPrice((p) => ((parseFloat(p) || 0) + 1).toFixed(2))} className="px-2 bg-[#1a1a1a] text-[#888] hover:bg-[#222]"><Plus className="h-3 w-3" /></button>
                  </div>
                </div>

                {/* Limit Price for STOP-LMT */}
                {orderType === 'STOP-LMT' && (
                  <div>
                    <label className="text-[#555] text-[9px]">LIMIT PRICE</label>
                    <div className="flex">
                      <input type="text" value={orderPrice} onChange={(e) => setOrderPrice(e.target.value)} className="w-full bg-[#111] border border-[#222] px-2 py-1 text-[#fff] text-center text-xs font-mono focus:outline-none" />
                    </div>
                  </div>
                )}

                {/* 数量 with mode toggle */}
                <div>
                  <div className="flex items-center justify-between">
                    <span className="text-[#555] text-[9px]">QUANTITY</span>
                    <div className="flex items-center gap-1">
                      {(['COIN', 'USD', 'EQUITY'] as const).map((mode) => (
                        <button 
                          key={mode}
                          onClick={() => setQtyMode(mode)}
                          className={`px-1.5 py-0 text-[7px] font-mono ${qtyMode === mode ? 'bg-[#333] text-[#fff]' : 'text-[#555] hover:text-[#888]'}`}
                        >
                          {mode === 'EQUITY' ? '%' : mode}
                        </button>
                      ))}
                    </div>
                  </div>
                  <input 
                    type="text" 
                    value={orderQty} 
                    onChange={(e) => setOrderQty(e.target.value)} 
                    className="w-full bg-[#111] border border-[#222] px-2 py-1 text-[#fff] text-xs font-mono focus:outline-none focus:border-[#00ff88]" 
                    placeholder={qtyMode === 'COIN' ? '0.0000' : qtyMode === 'USD' ? '$0.00' : '0%'} 
                  />
                  <div className="flex items-center justify-between mt-0.5">
                    <button onClick={() => setOrderQty((calculatePositionSize() || 0).toFixed(4))} className="text-[8px] text-[#00aaff] hover:underline">RISK {riskPercent}%</button>
                    <span className="text-[8px] text-[#555] font-mono">
                      ≈ {qtyMode === 'COIN' 
                        ? formatCurrency((parseFloat(orderQty) || 0) * (parseFloat(orderPrice) || 0))
                        : qtyMode === 'USD'
                        ? `${((parseFloat(orderQty) || 0) / (parseFloat(orderPrice) || 1)).toFixed(4)} ${selectedSymbol}`
                        : formatCurrency((parseFloat(orderQty) || 0) / 100 * selectedAccount.equity)
                      }
                    </span>
                  </div>
                </div>

                {/* 快速数量 */}
                <div className="grid grid-cols-4 gap-0.5">
                  {(qtyMode === 'COIN' ? ['0.01', '0.1', '0.5', '1'] : qtyMode === 'USD' ? ['100', '500', '1K', '5K'] : ['5', '10', '25', '50']).map((qty) => (
                    <button key={qty} onClick={() => setOrderQty(qty.replace('K', '000'))} className="py-0.5 text-[9px] font-mono bg-[#1a1a1a] text-[#666] hover:bg-[#222]">{qty}{qtyMode === 'EQUITY' && '%'}</button>
                  ))}
                </div>

                {/* 订单预览 with Slippage */}
                <div className="bg-[#111] p-2 border border-[#1a1a1a] space-y-1">
                  <div className="flex justify-between text-[9px]">
                    <span className="text-[#555]">NOTIONAL</span>
                    <span className="text-[#fff] font-mono">{formatCurrency((parseFloat(orderPrice) || 0) * (parseFloat(orderQty) || 0))}</span>
                  </div>
                  <div className="flex justify-between text-[9px]">
                    <span className="text-[#555]">MARGIN REQ</span>
                    <span className="text-[#888] font-mono">{formatCurrency((parseFloat(orderPrice) || 0) * (parseFloat(orderQty) || 0) / leverage)}</span>
                  </div>
                  <div className="flex justify-between text-[9px]">
                    <span className="text-[#555]">FEE (0.02%)</span>
                    <span className="text-[#666] font-mono">{formatCurrency((parseFloat(orderPrice) || 0) * (parseFloat(orderQty) || 0) * 0.0002)}</span>
                  </div>
                  {/* Estimated Slippage */}
                  <div className="flex justify-between text-[9px]">
                    <span className="text-[#555]">EST. SLIPPAGE</span>
                    <span className={`font-mono ${(parseFloat(orderQty) || 0) > 0.5 ? 'text-[#ffaa00]' : 'text-[#666]'}`}>
                      ~{((parseFloat(orderQty) || 0) * 0.001 * 100).toFixed(3)}% ({formatCurrency((parseFloat(orderPrice) || 0) * (parseFloat(orderQty) || 0) * 0.00001)})
                    </span>
                  </div>
                </div>
                
                {/* ==================== RISK → ORDER 硬反馈行 (机构必看) ==================== */}
                <div className={`p-2 rounded border text-[9px] font-mono ${
                  currentMarketState.riskMode === 'PROTECT' ? 'bg-[#ff000010] border-[#ff000030] text-[#ff6666]' :
                  currentMarketState.riskMode === 'CAUTION' ? 'bg-[#ffaa0010] border-[#ffaa0030] text-[#ffaa00]' :
                  'bg-[#00ff8808] border-[#00ff8820] text-[#888]'
                }`}>
                  {currentMarketState.riskMode === 'NORMAL' ? (
                    <>
                      <div className="text-[#00aa66]">✓ Order allowed under current risk</div>
                      <div className="flex justify-between mt-0.5">
                        <span className="text-[#666]">Max size:</span>
                        <span className="text-[#888]">{(getMaxAllowedSize() === Infinity ? '∞' : getMaxAllowedSize().toFixed(2))} {selectedSymbol}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-[#666]">Risk budget used:</span>
                        <span className={`${(currentMarketState.riskBudgetUsed || 50) > 80 ? 'text-[#ffaa00]' : 'text-[#888]'}`}>
                          {currentMarketState.riskBudgetUsed || 50}%
                        </span>
                      </div>
                      {getDataHealthImpact().caution && (
                        <div className="text-[#ff8800] mt-0.5">⚠ Data degraded — execution caution enabled</div>
                      )}
                    </>
                  ) : currentMarketState.riskMode === 'CAUTION' ? (
                    <>
                      <div className="text-[#ffaa00]">⚠ Order adjusted due to {currentMarketState.riskReason || 'volatility'}</div>
                      <div className="flex justify-between mt-0.5">
                        <span className="text-[#666]">Allowed size:</span>
                        <span className="text-[#ffaa00]">{currentMarketState.maxPositionSize} {selectedSymbol}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-[#666]">Risk budget used:</span>
                        <span className="text-[#ff8800]">{currentMarketState.riskBudgetUsed || 85}%</span>
                      </div>
                    </>
                  ) : (
                    <>
                      <div className="text-[#ff4444] font-bold">🛡 Orders blocked: PROTECT mode active</div>
                      <div className="text-[#ff6666] mt-0.5">{currentMarketState.riskReason}</div>
                      <div className="text-[#ff8888] mt-0.5">Risk budget: 100% (limit reached)</div>
                    </>
                  )}
                </div>
                
                {/* ==================== RISK MODE CONTROL - Order Entry Restriction ==================== */}
                {currentMarketState.riskMode !== 'NORMAL' && (
                  <div className={`p-2 rounded border text-[9px] ${
                    currentMarketState.riskMode === 'PROTECT' 
                      ? 'bg-[#ff000015] border-[#ff000040] text-[#ff6666]' 
                      : 'bg-[#ffaa0015] border-[#ffaa0040] text-[#ffaa00]'
                  }`}>
                    <div className="flex items-center gap-1 font-bold">
                      <AlertTriangle className="h-3 w-3" />
                      Risk mode: {currentMarketState.riskMode}
                    </div>
                    {currentMarketState.riskMode === 'CAUTION' && (
                      <div className="text-[#888] mt-0.5">
                        Max size allowed: {currentMarketState.maxPositionSize} {selectedSymbol}
                      </div>
                    )}
                    {currentMarketState.riskReason && (
                      <div className="text-[#666] mt-0.5">
                        Reason: {currentMarketState.riskReason}
                      </div>
                    )}
                  </div>
                )}
                
                {/* Order Block Warning */}
                {getOrderBlockReason() && (
                  <div className="p-2 bg-[#ff000020] border border-[#ff000050] rounded text-[9px] text-[#ff4444]">
                    <div className="font-bold">⚠ Order blocked:</div>
                    <div className="text-[#ff8888]">{getOrderBlockReason()}</div>
                  </div>
                )}

                {/* 下单按钮 with OPEN/CLOSE and Risk Control */}
                <button 
                  onClick={handlePlaceOrder} 
                  disabled={!orderQty || killSwitchActive || currentMarketState.riskMode === 'PROTECT'} 
                  className={`w-full py-2.5 font-bold text-sm font-mono ${
                    killSwitchActive || currentMarketState.riskMode === 'PROTECT'
                      ? 'bg-[#333] text-[#666] cursor-not-allowed' 
                      : orderSide === 'BUY' 
                        ? 'bg-[#00aa66] text-[#000] hover:bg-[#009955]' 
                        : 'bg-[#cc3333] text-[#fff] hover:bg-[#bb2222]'
                  } disabled:opacity-50 disabled:cursor-not-allowed`}
                >
                  {killSwitchActive ? '⚠ TRADING DISABLED' : 
                   currentMarketState.riskMode === 'PROTECT' ? '🛡 PROTECT MODE - BLOCKED' : (
                    <>
                      {orderType === 'MARKET' ? 'MKT ' : orderType.startsWith('STOP') ? orderType + ' ' : ''}
                      {orderSide} 
                      {activePosition && ((orderSide === 'BUY' && activePosition.qty < 0) || (orderSide === 'SELL' && activePosition.qty > 0)) ? ' (CLOSE)' : ' (OPEN)'}
                    </>
                  )}
                </button>

                <button onClick={handleCancelAll} className="w-full py-1.5 text-[10px] font-mono bg-[#1a1a1a] text-[#ff4444] border border-[#ff444430] hover:bg-[#201515]">CANCEL ALL [{activeOrdersCount}]</button>
              </div>
            </div>

            {/* 右侧面板 */}
            <div className="flex-1 flex flex-col bg-[#0d0d0d]">
              {/* 标签页 */}
              <div className="h-6 bg-[#111] border-b border-[#1a1a1a] flex items-center px-2 gap-4 text-[10px]">
                {[
                  { key: 'positions', label: 'POSITIONS', count: positions.length },
                  { key: 'orders', label: 'ORDERS', count: activeOrdersCount },
                  { key: 'trades', label: 'TRADES', count: trades.length },
                  { key: 'alerts', label: 'ALERTS', count: alerts.filter(a => a.active).length },
                  { key: 'account', label: 'ACCOUNT', count: 0 },
                  { key: 'risk', label: 'RISK', count: activePosition ? 1 : 0 }
                ].map(tab => (
                  <button key={tab.key} onClick={() => setActiveTab(tab.key as any)} className={`flex items-center gap-1 ${activeTab === tab.key ? 'text-[#fff]' : 'text-[#666] hover:text-[#888]'}`}>
                    {tab.label}
                    {tab.count > 0 && <span className={`px-1 rounded text-[8px] ${activeTab === tab.key ? 'bg-[#00ff88] text-[#000]' : 'bg-[#333]'}`}>{tab.count}</span>}
                  </button>
                ))}
                <div className="ml-auto text-[#888]">VALUE: <span className="text-[#fff]">{formatCurrency(totalPositionValue)}</span></div>
              </div>

              {/* 内容区域 */}
              <div className="flex-1 overflow-auto">
                {activeTab === 'positions' && (
                  <table className="w-full">
                    <thead className="bg-[#111] sticky top-0">
                      <tr className="text-[#555] text-left text-[9px] font-mono">
                        <th className="px-2 py-1">SYMBOL</th>
                        <th className="px-2 py-1">TYPE</th>
                        <th className="px-2 py-1 text-right">SIZE</th>
                        <th className="px-2 py-1 text-right">ENTRY</th>
                        <th className="px-2 py-1 text-right">MARK</th>
                        <th className="px-2 py-1 text-right">LIQ</th>
                        <th className="px-2 py-1 text-right">P&L</th>
                        <th className="px-2 py-1 text-right">TIME</th>
                        <th className="px-2 py-1">ACTIONS</th>
                      </tr>
                    </thead>
                    <tbody>
                      {positions.map((pos) => {
                        const timeInTrade = pos.openTime ? Math.floor((Date.now() - pos.openTime) / 60000) : 0;
                        const timeStr = timeInTrade >= 60 ? `${Math.floor(timeInTrade / 60)}h ${timeInTrade % 60}m` : `${timeInTrade}m`;
                        return (
                          <tr key={pos.symbol} className="border-b border-[#151515] hover:bg-[#111] text-[10px] font-mono">
                            <td className="px-2 py-1.5 text-[#fff] font-bold">{pos.symbol}</td>
                            <td className="px-2 py-1.5">
                              <span className={`px-1 py-0.5 text-[7px] ${pos.positionType === 'STRATEGY' ? 'bg-[#00aaff20] text-[#00aaff]' : 'bg-[#ffaa0020] text-[#ffaa00]'}`}>
                                {pos.positionType === 'STRATEGY' ? 'STR' : 'MAN'}
                              </span>
                            </td>
                            <td className={`px-2 py-1.5 text-right ${pos.qty >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}`}>
                              {pos.qty >= 0 ? '+' : ''}{pos.qty}
                            </td>
                            <td className="px-2 py-1.5 text-right text-[#888]">{formatCurrency(pos.avgPrice)}</td>
                            <td className="px-2 py-1.5 text-right text-[#fff]">{formatCurrency(pos.currentPrice)}</td>
                            <td className={`px-2 py-1.5 text-right ${
                              pos.liquidationPrice && Math.abs(pos.currentPrice - pos.liquidationPrice) / pos.currentPrice < 0.1 
                                ? 'text-[#ff4444]' 
                                : 'text-[#666]'
                            }`}>
                              {pos.liquidationPrice ? formatCurrency(pos.liquidationPrice) : '--'}
                            </td>
                            <td className="px-2 py-1.5 text-right group relative">
                              <span className={`${(pos.pnl ?? 0) >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}`}>
                                {(pos.pnl ?? 0) >= 0 ? '+' : ''}{formatCurrency(pos.pnl ?? 0)}
                                <span className="text-[8px] ml-1">({(pos.pnlPercent ?? 0).toFixed(2)}%)</span>
                              </span>
                              {/* PnL Breakdown + Execution Aftermath Tooltip */}
                              <div className="hidden group-hover:block absolute right-0 top-full z-50 bg-[#1a1a1a] border border-[#333] p-2 text-[8px] whitespace-nowrap shadow-lg min-w-[180px]">
                                <div className="text-[#555] mb-1 border-b border-[#222] pb-1">P&L BREAKDOWN</div>
                                <div className="flex justify-between gap-4">
                                  <span className="text-[#666]">Price P&L:</span>
                                  <span className={pos.pricePnL && pos.pricePnL >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}>{formatCurrency(pos.pricePnL || 0)}</span>
                                </div>
                                <div className="flex justify-between gap-4">
                                  <span className="text-[#666]">Fees:</span>
                                  <span className="text-[#cc3333]">-{formatCurrency(Math.abs(pos.feePnL || 0))}</span>
                                </div>
                                <div className="flex justify-between gap-4">
                                  <span className="text-[#666]">Funding:</span>
                                  <span className={pos.fundingPnL && pos.fundingPnL >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}>{formatCurrency(pos.fundingPnL || 0)}</span>
                                </div>
                                {/* Execution Aftermath - CIO级别安心剂 */}
                                <div className="text-[#555] mt-2 mb-1 border-t border-[#222] pt-1">EXECUTION OUTCOME</div>
                                <div className="flex justify-between gap-4">
                                  <span className="text-[#666]">Expected slippage:</span>
                                  <span className="text-[#888]">0.02%</span>
                                </div>
                                <div className="flex justify-between gap-4">
                                  <span className="text-[#666]">Actual slippage:</span>
                                  <span className={(0.04 > 0.03) ? 'text-[#ffaa00]' : 'text-[#00aa66]'}>0.04%</span>
                                </div>
                                <div className="flex justify-between gap-4">
                                  <span className="text-[#666]">State consistency:</span>
                                  <span className="text-[#00aa66]">✓</span>
                                </div>
                                <div className="flex justify-between gap-4">
                                  <span className="text-[#666]">Fill rate:</span>
                                  <span className="text-[#00aa66]">100%</span>
                                </div>
                              </div>
                            </td>
                            <td className="px-2 py-1.5 text-right text-[#555]">{timeStr}</td>
                            <td className="px-2 py-1.5">
                              <div className="flex items-center gap-1">
                                <button 
                                  onClick={() => {
                                    setOrderQty((Math.abs(pos.qty) * 0.5).toFixed(4));
                                    setOrderSide(pos.qty > 0 ? 'SELL' : 'BUY');
                                  }}
                                  className="px-1 py-0.5 text-[7px] bg-[#1a1a1a] text-[#888] hover:bg-[#222] hover:text-[#fff]"
                                  title="Reduce 50%"
                                >
                                  REDUCE
                                </button>
                                <button 
                                  onClick={() => {
                                    setOrderQty(Math.abs(pos.qty).toFixed(4));
                                    setOrderSide(pos.qty > 0 ? 'BUY' : 'SELL');
                                  }}
                                  className="px-1 py-0.5 text-[7px] bg-[#1a1a1a] text-[#00aaff] hover:bg-[#222]"
                                  title="Add hedge position"
                                >
                                  HEDGE
                                </button>
                                <button 
                                  onClick={() => handleClosePosition(pos.symbol)} 
                                  className="px-1 py-0.5 text-[7px] bg-[#1a1a1a] text-[#ff4444] hover:bg-[#201515]"
                                  title="Close entire position"
                                >
                                  CLOSE
                                </button>
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                      {positions.length > 0 && (
                        <tr className="bg-[#111] font-bold text-[10px] font-mono">
                          <td className="px-2 py-1.5 text-[#fff]" colSpan={6}>TOTAL</td>
                          <td className={`px-2 py-1.5 text-right ${totalPnL >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}`}>{totalPnL >= 0 ? '+' : ''}{formatCurrency(totalPnL)}</td>
                          <td colSpan={2}></td>
                        </tr>
                      )}
                    </tbody>
                  </table>
                )}

                {activeTab === 'orders' && (
                  <table className="w-full">
                    <thead className="bg-[#111] sticky top-0">
                      <tr className="text-[#555] text-left text-[9px] font-mono">
                        <th className="px-2 py-1">TIME</th>
                        <th className="px-2 py-1">SYMBOL</th>
                        <th className="px-2 py-1">O/C</th>
                        <th className="px-2 py-1">SIDE</th>
                        <th className="px-2 py-1">TYPE</th>
                        <th className="px-2 py-1 text-right">PRICE</th>
                        <th className="px-2 py-1 text-right">QTY</th>
                        <th className="px-2 py-1">STATUS</th>
                        <th className="px-2 py-1"></th>
                      </tr>
                    </thead>
                    <tbody>
                      {orders.map((order) => (
                        <tr key={order.id} className="border-b border-[#151515] hover:bg-[#111] text-[10px] font-mono">
                          <td className="px-2 py-1.5 text-[#555]">{order.time}</td>
                          <td className="px-2 py-1.5 text-[#fff]">{order.symbol}</td>
                          <td className="px-2 py-1.5">
                            <span className={`px-1 py-0.5 text-[7px] ${order.isOpen ? 'bg-[#00aaff20] text-[#00aaff]' : 'bg-[#ffaa0020] text-[#ffaa00]'}`}>
                              {order.isOpen ? 'OPEN' : 'CLOSE'}
                            </span>
                          </td>
                          <td className={`px-2 py-1.5 ${order.side === 'BUY' ? 'text-[#00aa66]' : 'text-[#cc3333]'}`}>{order.side}</td>
                          <td className="px-2 py-1.5 text-[#666]">{order.type}</td>
                          <td className="px-2 py-1.5 text-right text-[#fff]">{formatCurrency(order.price)}</td>
                          <td className="px-2 py-1.5 text-right text-[#888]">{order.filled}/{order.qty}</td>
                          <td className="px-2 py-1.5">
                            <span className={`px-1 py-0.5 text-[8px] ${order.status === 'FILLED' ? 'bg-[#00aa6630] text-[#00aa66]' : order.status === 'PARTIAL' ? 'bg-[#aa770030] text-[#aa7700]' : order.status === 'CANCELLED' ? 'bg-[#cc333330] text-[#cc3333]' : 'bg-[#33333330] text-[#888]'}`}>{order.status}</span>
                          </td>
                          <td className="px-2 py-1.5">{(order.status === 'NEW' || order.status === 'PARTIAL') && <button onClick={() => handleCancelOrder(order.id)} className="text-[#cc3333] hover:text-[#ff4444]"><X className="h-3 w-3" /></button>}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}

                {activeTab === 'trades' && (
                  <table className="w-full">
                    <thead className="bg-[#111] sticky top-0">
                      <tr className="text-[#555] text-left text-[9px] font-mono">
                        <th className="px-2 py-1">TIME</th>
                        <th className="px-2 py-1">SYMBOL</th>
                        <th className="px-2 py-1">SIDE</th>
                        <th className="px-2 py-1 text-right">PRICE</th>
                        <th className="px-2 py-1 text-right">QTY</th>
                        <th className="px-2 py-1 text-right">VALUE</th>
                      </tr>
                    </thead>
                    <tbody>
                      {trades.map((trade) => (
                        <tr key={trade.id} className="border-b border-[#151515] hover:bg-[#111] text-[10px] font-mono">
                          <td className="px-2 py-1.5 text-[#555]">{trade.time}</td>
                          <td className="px-2 py-1.5 text-[#fff]">{trade.symbol}</td>
                          <td className={`px-2 py-1.5 ${trade.side === 'BUY' ? 'text-[#00aa66]' : 'text-[#cc3333]'}`}>{trade.side}</td>
                          <td className="px-2 py-1.5 text-right text-[#fff]">{formatCurrency(trade.price)}</td>
                          <td className="px-2 py-1.5 text-right text-[#888]">{trade.qty}</td>
                          <td className="px-2 py-1.5 text-right text-[#888]">{formatCurrency(trade.price * trade.qty)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}

                {activeTab === 'alerts' && (
                  <div className="p-2 space-y-2">
                    {alerts.map((alert) => (
                      <div key={alert.id} className={`flex items-center justify-between p-2 border ${alert.active ? 'border-[#00ff8830] bg-[#00ff8810]' : 'border-[#222] bg-[#111]'}`}>
                        <div className="flex items-center gap-2">
                          <Bell className={`h-3 w-3 ${alert.active ? 'text-[#00ff88]' : 'text-[#555]'}`} />
                          <span className="text-[#fff]">{alert.symbol}</span>
                          <span className="text-[#888] text-[10px]">{alert.type === 'PRICE_ABOVE' ? '>' : alert.type === 'PRICE_BELOW' ? '<' : 'VOL'} {formatCurrency(alert.value)}</span>
                        </div>
                        <button className="text-[#ff4444] hover:text-[#ff6666]"><X className="h-3 w-3" /></button>
                      </div>
                    ))}
                    <button className="w-full py-2 border border-dashed border-[#333] text-[#555] hover:border-[#555] hover:text-[#888] text-[10px] flex items-center justify-center gap-1"><Plus className="h-3 w-3" /> ADD ALERT</button>
                  </div>
                )}

                {/* Account Breakdown Tab - NEW */}
                {activeTab === 'account' && (
                  <div className="p-3 space-y-3">
                    <div className="grid grid-cols-2 gap-3">
                      <AccountBreakdown accountType="crypto" showChart={false} />
                      <AccountBreakdown accountType="equity" showChart={false} />
                    </div>
                    
                    {/* Market Status Section */}
                    <div className="mt-4">
                      <h4 className="text-[#888] text-[10px] mb-2">MARKET STATUS</h4>
                      <MarketStatusBar symbol={selectedSymbol} />
                    </div>
                  </div>
                )}

                {/* Risk Metrics Tab - NEW */}
                {activeTab === 'risk' && (
                  <div className="p-3 space-y-3 font-mono">
                    {/* Account vs Strategy Risk */}
                    <div className="grid grid-cols-2 gap-3">
                      <div className="bg-[#111] p-3 border border-[#1a1a1a]">
                        <h4 className="text-[#666] text-[9px] mb-2">ACCOUNT RISK</h4>
                        <div className="space-y-1.5 text-[10px]">
                          <div className="flex justify-between">
                            <span className="text-[#555]">Exposure:</span>
                            <span className="text-[#fff]">{formatCurrency(totalPositionValue)}</span>
                          </div>
                          <div className="flex justify-between">
                            <span className="text-[#555]">Margin Used:</span>
                            <span className="text-[#ffaa00]">{((totalPositionValue / leverage) / selectedAccount.equity * 100).toFixed(1)}%</span>
                          </div>
                          <div className="flex justify-between">
                            <span className="text-[#555]">Available:</span>
                            <span className="text-[#00aa66]">{formatCurrency(selectedAccount.cash)}</span>
                          </div>
                        </div>
                      </div>
                      <div className="bg-[#111] p-3 border border-[#1a1a1a]">
                        <h4 className="text-[#666] text-[9px] mb-2">STRATEGY RISK</h4>
                        <div className="space-y-1.5 text-[10px]">
                          <div className="flex justify-between">
                            <span className="text-[#555]">Strat Positions:</span>
                            <span className="text-[#fff]">{positions.filter(p => p.positionType === 'STRATEGY').length}</span>
                          </div>
                          <div className="flex justify-between">
                            <span className="text-[#555]">Strat Exposure:</span>
                            <span className="text-[#00aaff]">{formatCurrency(positions.filter(p => p.positionType === 'STRATEGY').reduce((a, p) => a + p.value, 0))}</span>
                          </div>
                          <div className="flex justify-between">
                            <span className="text-[#555]">Manual Exp:</span>
                            <span className="text-[#ffaa00]">{formatCurrency(positions.filter(p => p.positionType !== 'STRATEGY').reduce((a, p) => a + p.value, 0))}</span>
                          </div>
                        </div>
                      </div>
                    </div>

                    {/* VAR with Time Windows */}
                    <div className="bg-[#111] p-3 border border-[#1a1a1a]">
                      <h4 className="text-[#666] text-[9px] mb-2">VALUE AT RISK (95% CI)</h4>
                      <div className="grid grid-cols-3 gap-2 text-[10px]">
                        <div className="text-center p-2 bg-[#0d0d0d]">
                          <div className="text-[#555] text-[8px]">1D VAR</div>
                          <div className="text-[#ff4444]">-{formatCurrency(totalPositionValue * 0.02)}</div>
                        </div>
                        <div className="text-center p-2 bg-[#0d0d0d]">
                          <div className="text-[#555] text-[8px]">5D VAR</div>
                          <div className="text-[#ff4444]">-{formatCurrency(totalPositionValue * 0.045)}</div>
                        </div>
                        <div className="text-center p-2 bg-[#0d0d0d]">
                          <div className="text-[#555] text-[8px]">MAX DRAWDOWN</div>
                          <div className="text-[#ff4444]">-{formatCurrency(totalPositionValue * 0.08)}</div>
                        </div>
                      </div>
                    </div>

                    {activePosition ? (
                      <RiskMeter 
                        position={{
                          avgCostPrice: activePosition.avgPrice,
                          currentPrice: activePosition.currentPrice,
                          quantity: Math.abs(activePosition.qty),
                          leverage: leverage,
                          equity: selectedAccount.equity,
                          side: activePosition.qty > 0 ? 'LONG' : 'SHORT'
                        }}
                        showDetails={true}
                      />
                    ) : (
                      <div className="text-center text-[#666] py-4 bg-[#111] border border-[#1a1a1a]">
                        <p className="text-[10px]">No active position for {selectedSymbol}</p>
                      </div>
                    )}
                    
                    {/* System Status Panel */}
                    <div className="bg-[#111] p-3 border border-[#1a1a1a]">
                      <h4 className="text-[#666] text-[9px] mb-2">SYSTEM STATUS</h4>
                      <div className="space-y-2 text-[10px]">
                        {/* Venue Latencies */}
                        <div className="flex justify-between items-center">
                          <span className="text-[#555]">Venue Latency:</span>
                          <div className="flex gap-2">
                            <span className={`px-1 ${venueLatencies.binance < 50 ? 'text-[#00aa66]' : venueLatencies.binance < 100 ? 'text-[#ffaa00]' : 'text-[#ff4444]'}`}>
                              BIN:{venueLatencies.binance}ms
                            </span>
                            <span className={`px-1 ${venueLatencies.okx < 50 ? 'text-[#00aa66]' : venueLatencies.okx < 100 ? 'text-[#ffaa00]' : 'text-[#ff4444]'}`}>
                              OKX:{venueLatencies.okx}ms
                            </span>
                            <span className={`px-1 ${venueLatencies.kraken < 50 ? 'text-[#00aa66]' : venueLatencies.kraken < 100 ? 'text-[#ffaa00]' : 'text-[#ff4444]'}`}>
                              KRK:{venueLatencies.kraken}ms
                            </span>
                          </div>
                        </div>
                        
                        {/* Last Order Ack */}
                        <div className="flex justify-between">
                          <span className="text-[#555]">Last Order Ack:</span>
                          <span className="text-[#888]">{lastOrderAckTime ? `${lastOrderAckTime}ms ago` : '--'}</span>
                        </div>
                        
                        {/* Market Data vs Trading Lag */}
                        <div className="flex justify-between items-center">
                          <span className="text-[#555]">Lag:</span>
                          <div className="flex gap-3">
                            <span className={marketDataLag < 100 ? 'text-[#00aa66]' : marketDataLag < 500 ? 'text-[#ffaa00]' : 'text-[#ff4444]'}>
                              DATA: {marketDataLag}ms
                            </span>
                            <span className={tradingLag < 100 ? 'text-[#00aa66]' : tradingLag < 500 ? 'text-[#ffaa00]' : 'text-[#ff4444]'}>
                              TRADE: {tradingLag}ms
                            </span>
                          </div>
                        </div>
                        
                        {/* Packet Loss */}
                        <div className="flex justify-between items-center">
                          <span className="text-[#555]">Packet Loss:</span>
                          <span className={`${packetLossPercent < 0.1 ? 'text-[#00aa66]' : packetLossPercent < 1 ? 'text-[#ffaa00]' : 'text-[#ff4444]'}`}>
                            {packetLossPercent.toFixed(2)}%
                            {packetLossPercent >= 1 && <span className="ml-2 text-[#ff4444] animate-pulse">⚠ AUTO-DISABLE</span>}
                          </span>
                        </div>
                      </div>
                    </div>
                    
                    {/* PnL Breakdown for current account */}
                    <div className="bg-[#111] p-3 border border-[#1a1a1a]">
                      <h4 className="text-[#666] text-[9px] mb-3">PNL BREAKDOWN ({currentAccountType.toUpperCase()})</h4>
                      <div className="space-y-2 text-[10px]">
                        <div className="flex justify-between">
                          <span className="text-[#555]">Unrealized PnL:</span>
                          <span className={selectedAccount.unrealizedPnL >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}>
                            {formatCurrency(selectedAccount.unrealizedPnL)}
                          </span>
                        </div>
                        <div className="flex justify-between">
                          <span className="text-[#555]">Realized PnL:</span>
                          <span className={selectedAccount.realizedPnL >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}>
                            {formatCurrency(selectedAccount.realizedPnL)}
                          </span>
                        </div>
                        <div className="flex justify-between">
                          <span className="text-[#555]">Total Fees:</span>
                          <span className="text-[#aa7700]">-{formatCurrency(selectedAccount.totalFees)}</span>
                        </div>
                        <div className="flex justify-between">
                          <span className="text-[#555]">Funding Cost:</span>
                          <span className="text-[#aa7700]">-{formatCurrency(selectedAccount.totalFundingCost)}</span>
                        </div>
                        <div className="border-t border-[#222] pt-2 flex justify-between font-bold">
                          <span className="text-[#888]">Net PnL:</span>
                          <span className={
                            (selectedAccount.unrealizedPnL + selectedAccount.realizedPnL - selectedAccount.totalFees - selectedAccount.totalFundingCost) >= 0 
                              ? 'text-[#00aa66]' : 'text-[#cc3333]'
                          }>
                            {formatCurrency(
                              selectedAccount.unrealizedPnL + selectedAccount.realizedPnL - 
                              selectedAccount.totalFees - selectedAccount.totalFundingCost
                            )}
                          </span>
                        </div>
                      </div>
                    </div>
                  </div>
                )}
              </div>
              
              {/* NEWS & SOCIAL SENTIMENT PANEL - Symbol Specific */}
              <div className="h-56 border-t border-[#1a1a1a] flex flex-col">
                {/* Panel Header with Trend Match Score and Impact Weight */}
                <div className="h-7 bg-[#111] border-b border-[#1a1a1a] flex items-center justify-between px-2 text-[10px]">
                  <div className="flex items-center gap-3">
                    <span className="text-[#666]">SENTIMENT <span className="text-[#fff]">{selectedSymbol}</span></span>
                    {/* Trend Match Score - 增强版实时分析 */}
                    {currentTrendMatch && (
                      <div className="flex items-center gap-1.5 group/match relative">
                        <span className="text-[#555]">MATCH:</span>
                        <span className={`px-1.5 py-0.5 rounded text-[8px] font-bold cursor-help ${
                          currentTrendMatch.score >= 80 ? 'bg-[#00ff8830] text-[#00ff88] border border-[#00ff8850]' :
                          currentTrendMatch.score >= 60 ? 'bg-[#00aaff30] text-[#00aaff] border border-[#00aaff50]' :
                          currentTrendMatch.score >= 40 ? 'bg-[#ffaa0030] text-[#ffaa00] border border-[#ffaa0050]' :
                          'bg-[#ff444430] text-[#ff4444] border border-[#ff444450]'
                        }`}>
                          {currentTrendMatch.score}/100
                        </span>
                        <span className={`text-[7px] px-1 rounded ${
                          currentTrendMatch.verdict === 'ALIGNED' ? 'bg-[#00ff8820] text-[#00ff88]' :
                          currentTrendMatch.verdict === 'DIVERGENT' ? 'bg-[#ff444420] text-[#ff4444]' :
                          'bg-[#88888820] text-[#888]'
                        }`}>
                          {currentTrendMatch.verdict}
                        </span>
                        {/* Trend Match Tooltip */}
                        <div className="hidden group-hover/match:block absolute left-0 top-full z-50 mt-1 bg-[#1a1a1a] border border-[#333] rounded p-2 shadow-xl min-w-[220px]">
                          <div className="text-[8px] text-[#00aaff] font-bold mb-1 border-b border-[#222] pb-1">📊 走势吻合度分析</div>
                          {currentTrendMatch.details.map((d, i) => (
                            <div key={i} className="text-[8px] py-0.5 flex items-center justify-between">
                              <span className="text-[#888]">{d.platform}</span>
                              <div className="flex items-center gap-2">
                                <span className={d.sentiment > 50 ? 'text-[#00aa66]' : 'text-[#cc4444]'}>{d.sentiment}</span>
                                <span className={d.match ? 'text-[#00ff88]' : 'text-[#ff4444]'}>{d.match ? '✓匹配' : '✗背离'}</span>
                              </div>
                            </div>
                          ))}
                          <div className="text-[7px] text-[#555] mt-1 border-t border-[#222] pt-1">
                            置信度: {currentTrendMatch.confidence}% | 更新: {formatLastUpdate(currentTrendMatch.lastCalculated)}
                          </div>
                        </div>
                      </div>
                    )}
                  </div>
                  {/* Sentiment Impact Weight - INSTITUTIONAL GRADE */}
                  <div className="flex items-center gap-3">
                    <div className="flex items-center gap-1.5 text-[9px]">
                      <span className="text-[#555]">Impact:</span>
                      <span className={`px-1 py-0.5 rounded ${
                        getSentimentImpact().impact === 'SUPPORTIVE' ? 'bg-[#00ff8820] text-[#00ff88]' :
                        getSentimentImpact().impact === 'CONTRARY' ? 'bg-[#ff444420] text-[#ff4444]' :
                        'bg-[#88888820] text-[#888]'
                      }`}>
                        {getSentimentImpact().impact}
                      </span>
                    </div>
                    <div className="flex items-center gap-1.5 text-[9px]">
                      <span className="text-[#555]">Weight:</span>
                      <span className={`px-1 py-0.5 rounded ${
                        getSentimentImpact().weight === 'HIGH' ? 'bg-[#00aaff20] text-[#00aaff]' :
                        getSentimentImpact().weight === 'MEDIUM' ? 'bg-[#ffaa0020] text-[#ffaa00]' :
                        'bg-[#55555520] text-[#666]'
                      }`}>
                        {getSentimentImpact().weight}
                      </span>
                    </div>
                    <span className="text-[#555]">{newsData.filter(n => n.symbol === selectedSymbol).length} NEWS</span>
                  </div>
                </div>
                
                {/* Social Sentiment Bar - 增强版含TikTok */}
                {socialSentiment[selectedSymbol] && (
                  <div className="bg-[#0a0a0a] border-b border-[#1a1a1a] px-2 py-1.5">
                    {/* 更新时间指示器 */}
                    <div className="flex items-center justify-between mb-1">
                      <div className="flex items-center gap-2">
                        <span className="text-[9px] text-[#555]">SOCIAL SIGNALS</span>
                        <span className="text-[8px] text-[#666]">
                          更新: {formatLastUpdate(socialSentiment[selectedSymbol].lastUpdated)}
                        </span>
                      </div>
                      {/* 平台权重指示 */}
                      <div className="flex items-center gap-1.5 text-[8px] text-[#444]">
                        <span>权重: X{(platformWeights.x * 100).toFixed(0)}%</span>
                        <span>YT{(platformWeights.youtube * 100).toFixed(0)}%</span>
                        <span>RD{(platformWeights.reddit * 100).toFixed(0)}%</span>
                        <span>TT{(platformWeights.tiktok * 100).toFixed(0)}%</span>
                      </div>
                    </div>
                    
                    {/* Social Platforms Row */}
                    <div className="flex items-center gap-2.5 mb-1.5 flex-wrap">
                      {/* X (Twitter) */}
                      <div className="flex items-center gap-1.5">
                        <span className="text-[10px] text-[#888]">𝕏</span>
                        <span className={`text-[11px] font-mono font-bold ${
                          socialSentiment[selectedSymbol].xSentiment > 60 ? 'text-[#00ff88]' :
                          socialSentiment[selectedSymbol].xSentiment > 40 ? 'text-[#ffaa00]' :
                          'text-[#ff4444]'
                        }`}>
                          {socialSentiment[selectedSymbol].xSentiment > 0 ? '+' : ''}{socialSentiment[selectedSymbol].xSentiment}
                        </span>
                        <span className="text-[9px] text-[#555]">({(socialSentiment[selectedSymbol].xMentions / 1000).toFixed(0)}K)</span>
                        {/* 速度指示器 */}
                        <span className={`text-[8px] font-mono ${
                          socialSentiment[selectedSymbol].xVelocity > 10 ? 'text-[#00ff88]' :
                          socialSentiment[selectedSymbol].xVelocity > 0 ? 'text-[#888]' :
                          'text-[#ff4444]'
                        }`}>
                          {socialSentiment[selectedSymbol].xVelocity > 0 ? '↑' : '↓'}{Math.abs(socialSentiment[selectedSymbol].xVelocity).toFixed(0)}%/h
                        </span>
                        {socialSentiment[selectedSymbol].xTrending && (
                          <span className="text-[8px] px-1.5 py-0.5 bg-[#ff660030] text-[#ff6600] rounded animate-pulse">🔥</span>
                        )}
                      </div>
                      <span className="text-[#333]">|</span>
                      
                      {/* YouTube */}
                      <div className="flex items-center gap-1.5">
                        <span className="text-[10px] text-[#ff0000]">▶</span>
                        <span className={`text-[11px] font-mono font-bold ${
                          socialSentiment[selectedSymbol].youtubeSentiment > 60 ? 'text-[#00ff88]' :
                          socialSentiment[selectedSymbol].youtubeSentiment > 40 ? 'text-[#ffaa00]' :
                          'text-[#ff4444]'
                        }`}>
                          {socialSentiment[selectedSymbol].youtubeSentiment > 0 ? '+' : ''}{socialSentiment[selectedSymbol].youtubeSentiment}
                        </span>
                        <span className="text-[9px] text-[#555]">({(socialSentiment[selectedSymbol].youtubeViews24h / 1000000).toFixed(1)}M)</span>
                        <span className="text-[8px] text-[#666]">{socialSentiment[selectedSymbol].youtubeNewVideos}新</span>
                      </div>
                      <span className="text-[#333]">|</span>
                      
                      {/* Reddit */}
                      <div className="flex items-center gap-1.5">
                        <span className="text-[10px] text-[#ff4500]">◉</span>
                        <span className={`text-[11px] font-mono font-bold ${
                          socialSentiment[selectedSymbol].redditSentiment > 60 ? 'text-[#00ff88]' :
                          socialSentiment[selectedSymbol].redditSentiment > 40 ? 'text-[#ffaa00]' :
                          'text-[#ff4444]'
                        }`}>
                          {socialSentiment[selectedSymbol].redditSentiment > 0 ? '+' : ''}{socialSentiment[selectedSymbol].redditSentiment}
                        </span>
                        <span className="text-[9px] text-[#555]">({(socialSentiment[selectedSymbol].redditMentions / 1000).toFixed(1)}K)</span>
                        <span className="text-[8px] text-[#666]">↑{(socialSentiment[selectedSymbol].redditUpvoteRatio * 100).toFixed(0)}%</span>
                      </div>
                      <span className="text-[#333]">|</span>
                      
                      {/* TikTok (新增) */}
                      <div className="flex items-center gap-1.5">
                        <span className="text-[10px] text-[#00f2ea]">♪</span>
                        <span className={`text-[11px] font-mono font-bold ${
                          socialSentiment[selectedSymbol].tiktokSentiment > 60 ? 'text-[#00ff88]' :
                          socialSentiment[selectedSymbol].tiktokSentiment > 40 ? 'text-[#ffaa00]' :
                          'text-[#ff4444]'
                        }`}>
                          {socialSentiment[selectedSymbol].tiktokSentiment > 0 ? '+' : ''}{socialSentiment[selectedSymbol].tiktokSentiment}
                        </span>
                        <span className="text-[9px] text-[#555]">({(socialSentiment[selectedSymbol].tiktokViews24h / 1000000).toFixed(1)}M)</span>
                        {socialSentiment[selectedSymbol].tiktokTrending && (
                          <span className="text-[8px] px-1.5 py-0.5 bg-[#00f2ea30] text-[#00f2ea] rounded animate-pulse">🔥</span>
                        )}
                      </div>
                      <span className="text-[#333]">|</span>
                      
                      {/* Overall Social Score */}
                      <div className="flex items-center gap-2 ml-auto">
                        <span className="text-[10px] text-[#888]">SOCIAL:</span>
                        <span className={`text-[13px] font-bold ${
                          socialSentiment[selectedSymbol].overallSocialScore > 60 ? 'text-[#00ff88]' :
                          socialSentiment[selectedSymbol].overallSocialScore > 40 ? 'text-[#ffaa00]' :
                          'text-[#ff4444]'
                        }`}>
                          {socialSentiment[selectedSymbol].overallSocialScore}
                        </span>
                      </div>
                    </div>
                    
                    {/* X Topics + TikTok Hashtags */}
                    <div className="flex items-center gap-2 flex-wrap">
                      {socialSentiment[selectedSymbol].xTopics.map((topic, idx) => (
                        <span key={idx} className={`text-[9px] px-2 py-0.5 rounded ${
                          topic.type === 'FOMO' ? 'bg-[#00ff8820] text-[#00ff88] border border-[#00ff8840]' :
                          topic.type === 'FUD' ? 'bg-[#ff444420] text-[#ff4444] border border-[#ff444440]' :
                          topic.type === 'MEME' ? 'bg-[#ff00ff20] text-[#ff00ff] border border-[#ff00ff40]' :
                          topic.type === 'ANALYSIS' ? 'bg-[#00aaff20] text-[#00aaff] border border-[#00aaff40]' :
                          'bg-[#88888820] text-[#888] border border-[#88888840]'
                        }`}>
                          {topic.tag} <span className="opacity-60">({topic.type})</span>
                        </span>
                      ))}
                      {/* TikTok Hashtags */}
                      {socialSentiment[selectedSymbol].tiktokHashtags.slice(0, 2).map((tag, idx) => (
                        <span key={`tt-${idx}`} className="text-[9px] px-2 py-0.5 rounded bg-[#00f2ea15] text-[#00f2ea] border border-[#00f2ea30]">
                          {tag} <span className="opacity-60">(TikTok)</span>
                        </span>
                      ))}
                    </div>
                  </div>
                )}
                
                {/* Indicators Row */}
                {symbolIndicators[selectedSymbol] && (
                  <div className="bg-[#080808] border-b border-[#1a1a1a] px-2 py-1 flex items-center gap-3 text-[8px]">
                    {/* Momentum */}
                    <div className="flex items-center gap-1">
                      <span className="text-[#555]">MTM:</span>
                      <span className={`font-mono ${
                        symbolIndicators[selectedSymbol].momentum > 50 ? 'text-[#00ff88]' :
                        symbolIndicators[selectedSymbol].momentum > 0 ? 'text-[#00aa66]' :
                        symbolIndicators[selectedSymbol].momentum > -50 ? 'text-[#cc6600]' :
                        'text-[#ff4444]'
                      }`}>
                        {symbolIndicators[selectedSymbol].momentum > 0 ? '+' : ''}{symbolIndicators[selectedSymbol].momentum}
                      </span>
                    </div>
                    <span className="text-[#222]">|</span>
                    {/* Volatility */}
                    <div className="flex items-center gap-1">
                      <span className="text-[#555]">VOL:</span>
                      <span className={`px-1 rounded ${
                        symbolIndicators[selectedSymbol].volatility === 'EXTREME' ? 'bg-[#ff000030] text-[#ff4444]' :
                        symbolIndicators[selectedSymbol].volatility === 'HIGH' ? 'bg-[#ff660030] text-[#ff8800]' :
                        symbolIndicators[selectedSymbol].volatility === 'MEDIUM' ? 'bg-[#ffaa0030] text-[#ffaa00]' :
                        'bg-[#00ff8830] text-[#00ff88]'
                      }`}>
                        {symbolIndicators[selectedSymbol].volatility} ({symbolIndicators[selectedSymbol].volatilityValue})
                      </span>
                    </div>
                    <span className="text-[#222]">|</span>
                    {/* Whale Activity */}
                    <div className="flex items-center gap-1">
                      <span className="text-[#555]">🐋:</span>
                      <span className={`${
                        symbolIndicators[selectedSymbol].whaleActivity === 'ACCUMULATING' ? 'text-[#00ff88]' :
                        symbolIndicators[selectedSymbol].whaleActivity === 'DISTRIBUTING' ? 'text-[#ff4444]' :
                        'text-[#888]'
                      }`}>
                        {symbolIndicators[selectedSymbol].whaleActivity === 'ACCUMULATING' ? '▲' : 
                         symbolIndicators[selectedSymbol].whaleActivity === 'DISTRIBUTING' ? '▼' : '—'}
                        {symbolIndicators[selectedSymbol].whaleNetFlow > 0 ? '+' : ''}{symbolIndicators[selectedSymbol].whaleNetFlow.toFixed(1)}M
                      </span>
                    </div>
                    <span className="text-[#222]">|</span>
                    {/* OI Change */}
                    <div className="flex items-center gap-1">
                      <span className="text-[#555]">OI:</span>
                      <span className={`font-mono ${symbolIndicators[selectedSymbol].openInterestChange >= 0 ? 'text-[#00aa66]' : 'text-[#cc3333]'}`}>
                        {symbolIndicators[selectedSymbol].openInterestChange > 0 ? '+' : ''}{symbolIndicators[selectedSymbol].openInterestChange.toFixed(1)}%
                      </span>
                    </div>
                    {/* Liquidations (only for crypto) */}
                    {symbolIndicators[selectedSymbol].liquidations24h.long > 0 && (
                      <>
                        <span className="text-[#222]">|</span>
                        <div className="flex items-center gap-1">
                          <span className="text-[#555]">LIQ:</span>
                          <span className="text-[#00aa66]">L:{symbolIndicators[selectedSymbol].liquidations24h.long.toFixed(1)}M</span>
                          <span className="text-[#cc3333]">S:{symbolIndicators[selectedSymbol].liquidations24h.short.toFixed(1)}M</span>
                        </div>
                      </>
                    )}
                  </div>
                )}
                
                {/* News List */}
                <div className="flex-1 overflow-y-auto custom-scrollbar">
                  {newsData
                    .filter(n => n.symbol === selectedSymbol)
                    .sort((a, b) => b.time.getTime() - a.time.getTime())
                    .map(news => (
                      <div key={news.id} className="px-2 py-1.5 border-b border-[#151515] hover:bg-[#111] cursor-pointer group">
                        <div className="flex items-start gap-2">
                          <div className="flex-1 min-w-0">
                            <div className="text-[9px] text-[#888] truncate leading-snug group-hover:text-[#aaa]">
                              {news.headline}
                            </div>
                            <div className="flex items-center gap-2 mt-0.5">
                              <span className="text-[7px] text-[#555]">{news.source}</span>
                              <span className="text-[7px] text-[#333]">•</span>
                              <span className="text-[7px] text-[#555]">{formatRelativeTime(news.time)}</span>
                              <span className="text-[7px] text-[#333]">•</span>
                              <span className={`text-[7px] px-1 rounded ${
                                news.category === 'REGULATORY' ? 'bg-[#ffaa0015] text-[#ffaa00]' :
                                news.category === 'MACRO' ? 'bg-[#00aaff15] text-[#00aaff]' :
                                news.category === 'EARNINGS' ? 'bg-[#aa00ff15] text-[#aa00ff]' :
                                news.category === 'TECHNICAL' ? 'bg-[#00ffaa15] text-[#00ffaa]' :
                                news.category === 'ONCHAIN' ? 'bg-[#ff880015] text-[#ff8800]' :
                                news.category === 'INSTITUTIONAL' ? 'bg-[#0088ff15] text-[#0088ff]' :
                                'bg-[#33333330] text-[#888]'
                              }`}>{news.category}</span>
                              <span className="text-[7px] text-[#333]">•</span>
                              <span className={`text-[6px] ${news.reliability >= 90 ? 'text-[#00aa66]' : news.reliability >= 80 ? 'text-[#888]' : 'text-[#aa7700]'}`}>
                                REL:{news.reliability}%
                              </span>
                            </div>
                          </div>
                          <div className="flex flex-col items-end gap-0.5 flex-shrink-0">
                            {/* Sentiment Badge */}
                            <span className={`text-[7px] px-1.5 py-0.5 rounded font-mono ${
                              news.sentiment === 'BULLISH' ? 'bg-[#00aa6620] text-[#00ff88] border border-[#00aa6640]' :
                              news.sentiment === 'BEARISH' ? 'bg-[#cc333320] text-[#ff4444] border border-[#cc333340]' :
                              'bg-[#33333330] text-[#888] border border-[#333]'
                            }`}>
                              {news.sentiment === 'BULLISH' ? '▲ BULL' : news.sentiment === 'BEARISH' ? '▼ BEAR' : '— NEUT'}
                            </span>
                            {/* Risk Impact Badge */}
                            <span className={`text-[7px] px-1.5 py-0.5 rounded font-mono ${
                              news.riskImpact === 'UP' ? 'bg-[#ff440020] text-[#ff6644]' :
                              news.riskImpact === 'DOWN' ? 'bg-[#00ff8820] text-[#00cc66]' :
                              'bg-[#33333320] text-[#666]'
                            }`}>
                              {news.riskImpact === 'UP' ? '⚠ RISK↑' : news.riskImpact === 'DOWN' ? '✓ RISK↓' : '— RISK'}
                            </span>
                          </div>
                        </div>
                      </div>
                    ))}
                  {newsData.filter(n => n.symbol === selectedSymbol).length === 0 && (
                    <div className="flex items-center justify-center h-full text-[10px] text-[#444]">
                      No news for {selectedSymbol}
                    </div>
                  )}
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* 底部状态栏 - Professional Footer */}
      <div className="h-6 bg-[#0a0a0a] border-t border-[#1a1a1a] flex items-center justify-between px-4 text-[9px] font-mono">
        <div className="flex items-center gap-4 text-[#555]">
          <span><span className="text-[#666]">[B]</span> Buy</span>
          <span><span className="text-[#666]">[S]</span> Sell</span>
          <span><span className="text-[#666]">[L]</span> Limit</span>
          <span><span className="text-[#666]">[M]</span> Market</span>
          <span><span className="text-[#666]">[ENTER]</span> Place</span>
          <span><span className="text-[#666]">[ESC]</span> Cancel</span>
        </div>
        <div className="flex items-center gap-4">
          {/* System Status Indicators */}
          <span className={`${marketDataLag < 100 ? 'text-[#00aa66]' : marketDataLag < 500 ? 'text-[#aa7700]' : 'text-[#cc3333]'}`}>
            DATA:{marketDataLag}ms
          </span>
          <span className={`${tradingLag < 100 ? 'text-[#00aa66]' : tradingLag < 500 ? 'text-[#aa7700]' : 'text-[#cc3333]'}`}>
            TRADE:{tradingLag}ms
          </span>
          <span className="text-[#555]">|</span>
          <span className="text-[#aa7700] flex items-center gap-1">
            <AlertTriangle className="h-3 w-3" /> RISK: <span className="text-[#fff]">{riskPercent}%</span>
          </span>
          <span className="text-[#666]">MARGIN: <span className="text-[#fff]">{(((totalPositionValue ?? 0) / (accountBalance || 1)) * 100).toFixed(1)}%</span></span>
          {killSwitchActive && (
            <span className="text-[#ff4444] animate-pulse font-bold">⚠ KILL SWITCH ACTIVE</span>
          )}
        </div>
      </div>
    </div>
  )
}
