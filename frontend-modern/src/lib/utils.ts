import { type ClassValue, clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function formatCurrency(amount: number, decimals: number = 2): string {
  // Handle NaN, undefined, null
  if (amount === undefined || amount === null || isNaN(amount)) {
    return '$0.00'
  }
  if (amount >= 1000) {
    return `$${amount.toLocaleString('en-US', { minimumFractionDigits: 0, maximumFractionDigits: 0 })}`
  }
  return `$${amount.toFixed(decimals)}`
}

export function formatNumber(num: number): string {
  return num.toLocaleString('en-US')
}

export function formatPercentage(value: number): string {
  const formatted = value.toFixed(2)
  return value >= 0 ? `+${formatted}%` : `${formatted}%`
}

export function formatTime(timestamp: number | string): string {
  return new Date(timestamp).toLocaleTimeString('en-US', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

export function formatRelativeTime(date: Date): string {
  const now = Date.now()
  const diff = now - date.getTime()
  const seconds = Math.floor(diff / 1000)
  const minutes = Math.floor(seconds / 60)
  const hours = Math.floor(minutes / 60)
  
  if (seconds < 60) return `${seconds}s ago`
  if (minutes < 60) return `${minutes}m ago`
  if (hours < 24) return `${hours}h ago`
  return `${Math.floor(hours / 24)}d ago`
}

export function generateMockPriceHistory(basePrice: number, points: number = 100) {
  const history = []
  let price = basePrice
  const now = Date.now()
  
  for (let i = 0; i < points; i++) {
    const change = (Math.random() - 0.48) * basePrice * 0.02
    price = Math.max(price + change, basePrice * 0.8)
    const volume = Math.random() * 1000000 + 500000
    
    history.push({
      time: now - (points - i) * 60000,
      price: parseFloat(price.toFixed(2)),
      volume: Math.floor(volume),
      open: parseFloat((price - Math.random() * 10).toFixed(2)),
      high: parseFloat((price + Math.random() * 15).toFixed(2)),
      low: parseFloat((price - Math.random() * 15).toFixed(2)),
      close: parseFloat(price.toFixed(2)),
    })
  }
  
  return history
}

export function generateOrderBook(basePrice: number) {
  const bids = []
  const asks = []
  
  for (let i = 0; i < 15; i++) {
    const bidPrice = basePrice - (i + 1) * basePrice * 0.001
    const askPrice = basePrice + (i + 1) * basePrice * 0.001
    const bidSize = Math.random() * 5 + 0.1
    const askSize = Math.random() * 5 + 0.1
    
    bids.push({
      price: parseFloat(bidPrice.toFixed(2)),
      size: parseFloat(bidSize.toFixed(4)),
      total: parseFloat((bidPrice * bidSize).toFixed(2)),
    })
    
    asks.push({
      price: parseFloat(askPrice.toFixed(2)),
      size: parseFloat(askSize.toFixed(4)),
      total: parseFloat((askPrice * askSize).toFixed(2)),
    })
  }
  
  return { bids, asks: asks.reverse() }
}
