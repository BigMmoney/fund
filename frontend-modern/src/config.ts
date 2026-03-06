export const API_BASE_URL = '/v1'
export const WS_URL = 'ws://localhost:8080/ws'

export const CRYPTO_ASSETS = [
  { id: 'btc', symbol: 'BTC', name: 'Bitcoin', icon: '₿' },
  { id: 'eth', symbol: 'ETH', name: 'Ethereum', icon: 'Ξ' },
  { id: 'sol', symbol: 'SOL', name: 'Solana', icon: '◎' },
  { id: 'bnb', symbol: 'BNB', name: 'Binance Coin', icon: '🔶' },
  { id: 'xrp', symbol: 'XRP', name: 'Ripple', icon: '✕' },
  { id: 'ada', symbol: 'ADA', name: 'Cardano', icon: '₳' },
  { id: 'doge', symbol: 'DOGE', name: 'Dogecoin', icon: 'Ð' },
  { id: 'dot', symbol: 'DOT', name: 'Polkadot', icon: '●' },
  { id: 'matic', symbol: 'MATIC', name: 'Polygon', icon: '⬡' },
  { id: 'avax', symbol: 'AVAX', name: 'Avalanche', icon: '🔺' },
  { id: 'link', symbol: 'LINK', name: 'Chainlink', icon: '🔗' },
  { id: 'uni', symbol: 'UNI', name: 'Uniswap', icon: '🦄' },
]

export const STOCK_ASSETS = [
  { id: 'AAPL', symbol: 'AAPL', name: 'Apple Inc.', sector: 'Technology' },
  { id: 'MSFT', symbol: 'MSFT', name: 'Microsoft', sector: 'Technology' },
  { id: 'GOOGL', symbol: 'GOOGL', name: 'Alphabet', sector: 'Technology' },
  { id: 'AMZN', symbol: 'AMZN', name: 'Amazon', sector: 'Consumer' },
  { id: 'TSLA', symbol: 'TSLA', name: 'Tesla', sector: 'Automotive' },
  { id: 'NVDA', symbol: 'NVDA', name: 'NVIDIA', sector: 'Technology' },
  { id: 'META', symbol: 'META', name: 'Meta Platforms', sector: 'Technology' },
  { id: 'JPM', symbol: 'JPM', name: 'JPMorgan Chase', sector: 'Finance' },
  { id: 'V', symbol: 'V', name: 'Visa', sector: 'Finance' },
  { id: 'WMT', symbol: 'WMT', name: 'Walmart', sector: 'Retail' },
  { id: 'DIS', symbol: 'DIS', name: 'Disney', sector: 'Entertainment' },
  { id: 'BA', symbol: 'BA', name: 'Boeing', sector: 'Aerospace' },
]

// 贵金属资产 - 使用免费API (GoldAPI.io / MetalpriceAPI)
export const COMMODITY_ASSETS = [
  { id: 'XAU', symbol: 'XAU', name: 'Gold', nameZh: '黄金', icon: '🥇', unit: 'oz', sector: 'Precious Metals' },
  { id: 'XAG', symbol: 'XAG', name: 'Silver', nameZh: '白银', icon: '🥈', unit: 'oz', sector: 'Precious Metals' },
  { id: 'XPT', symbol: 'XPT', name: 'Platinum', nameZh: '铂金', icon: '⬜', unit: 'oz', sector: 'Precious Metals' },
  { id: 'XPD', symbol: 'XPD', name: 'Palladium', nameZh: '钯金', icon: '🔘', unit: 'oz', sector: 'Precious Metals' },
]

// 免费贵金属API配置
export const METALS_API_CONFIG = {
  // 免费选项1: MetalpriceAPI (https://metalpriceapi.com) - 100次/月免费
  metalpriceapi: {
    baseUrl: 'https://api.metalpriceapi.com/v1',
    endpoints: {
      latest: '/latest',
      historical: '/historical'
    },
    freeLimit: 100,
    supportedSymbols: ['XAU', 'XAG', 'XPT', 'XPD']
  },
  // 免费选项2: GoldAPI (https://www.goldapi.io) - 免费层
  goldapi: {
    baseUrl: 'https://www.goldapi.io/api',
    endpoints: {
      latest: '/', // /XAU, /XAG
    },
    freeLimit: 50,
    supportedSymbols: ['XAU', 'XAG']
  },
  // 免费选项3: Metals.live (完全免费，无需API key)
  metalslive: {
    baseUrl: 'https://api.metals.live/v1',
    endpoints: {
      spot: '/spot',
      latest: '/latest'
    },
    freeLimit: 'unlimited',
    supportedSymbols: ['gold', 'silver', 'platinum', 'palladium']
  }
}

export const CHART_TYPES = [
  { value: 'line', label: '📈 线图', icon: '📈' },
  { value: 'candle', label: '🕯️ K线', icon: '🕯️' },
  { value: 'depth', label: '📊 深度', icon: '📊' },
  { value: 'volume', label: '📊 量', icon: '📊' },
  { value: 'heatmap', label: '🔥 热力', icon: '🔥' },
]

export const TIMEFRAMES = [
  { value: '1m', label: '1分钟' },
  { value: '5m', label: '5分钟' },
  { value: '15m', label: '15分钟' },
  { value: '1h', label: '1小时' },
  { value: '4h', label: '4小时' },
  { value: '1d', label: '1天' },
]
