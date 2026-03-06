/**
 * 数据持久化服务
 * 使用 localStorage 缓存用户设置和数据
 */

const STORAGE_PREFIX = 'pretrading_'
const STORAGE_VERSION = '1.0'

// 存储键名
export const STORAGE_KEYS = {
  // 用户偏好设置
  THEME: 'theme',
  LANGUAGE: 'language',
  TIME_WINDOW: 'time_window',
  
  // News Intelligence 页面设置
  NEWS_ACTIVE_TAB: 'news_active_tab',
  NEWS_FILTERS: 'news_filters',
  NEWS_REFRESH_INTERVAL: 'news_refresh_interval',
  NEWS_SHOW_SOURCES: 'news_show_sources',
  NEWS_COLLAPSED_PANELS: 'news_collapsed_panels',
  
  // Trading Terminal 设置
  TRADING_SELECTED_MARKET: 'trading_selected_market',
  TRADING_ORDER_TYPE: 'trading_order_type',
  TRADING_CHART_INTERVAL: 'trading_chart_interval',
  
  // 缓存数据
  CACHED_ALERTS: 'cached_alerts',
  CACHED_TOPICS: 'cached_topics',
  CACHED_SOURCES_STATUS: 'cached_sources_status',
  
  // 元数据
  LAST_VISIT: 'last_visit',
  VERSION: 'version',
} as const

type StorageKey = typeof STORAGE_KEYS[keyof typeof STORAGE_KEYS]

interface CacheEntry<T> {
  data: T
  timestamp: number
  ttl: number // Time to live in milliseconds
}

/**
 * 获取完整的存储键名
 */
function getFullKey(key: StorageKey): string {
  return `${STORAGE_PREFIX}${key}`
}

/**
 * 存储数据到 localStorage
 */
export function setItem<T>(key: StorageKey, value: T): boolean {
  try {
    const fullKey = getFullKey(key)
    const serialized = JSON.stringify(value)
    localStorage.setItem(fullKey, serialized)
    return true
  } catch (error) {
    console.error('[PersistenceService] Failed to save:', key, error)
    return false
  }
}

/**
 * 从 localStorage 获取数据
 */
export function getItem<T>(key: StorageKey, defaultValue: T): T {
  try {
    const fullKey = getFullKey(key)
    const serialized = localStorage.getItem(fullKey)
    if (serialized === null) {
      return defaultValue
    }
    return JSON.parse(serialized) as T
  } catch (error) {
    console.error('[PersistenceService] Failed to read:', key, error)
    return defaultValue
  }
}

/**
 * 删除指定键的数据
 */
export function removeItem(key: StorageKey): boolean {
  try {
    const fullKey = getFullKey(key)
    localStorage.removeItem(fullKey)
    return true
  } catch (error) {
    console.error('[PersistenceService] Failed to remove:', key, error)
    return false
  }
}

/**
 * 存储带有过期时间的缓存数据
 * @param ttl Time to live in milliseconds (default: 5 minutes)
 */
export function setCachedItem<T>(key: StorageKey, data: T, ttl: number = 5 * 60 * 1000): boolean {
  const entry: CacheEntry<T> = {
    data,
    timestamp: Date.now(),
    ttl
  }
  return setItem(key, entry)
}

/**
 * 获取缓存数据，如果过期则返回默认值
 */
export function getCachedItem<T>(key: StorageKey, defaultValue: T): T {
  try {
    const entry = getItem<CacheEntry<T> | null>(key, null)
    if (!entry) {
      return defaultValue
    }
    
    // 检查是否过期
    const isExpired = Date.now() - entry.timestamp > entry.ttl
    if (isExpired) {
      removeItem(key)
      return defaultValue
    }
    
    return entry.data
  } catch (error) {
    console.error('[PersistenceService] Failed to read cached item:', key, error)
    return defaultValue
  }
}

/**
 * 检查缓存是否有效（未过期）
 */
export function isCacheValid(key: StorageKey): boolean {
  try {
    const entry = getItem<CacheEntry<unknown> | null>(key, null)
    if (!entry) {
      return false
    }
    return Date.now() - entry.timestamp <= entry.ttl
  } catch {
    return false
  }
}

/**
 * 清除所有预交易相关的存储数据
 */
export function clearAll(): boolean {
  try {
    const keys = Object.keys(localStorage)
    keys.forEach(key => {
      if (key.startsWith(STORAGE_PREFIX)) {
        localStorage.removeItem(key)
      }
    })
    return true
  } catch (error) {
    console.error('[PersistenceService] Failed to clear all:', error)
    return false
  }
}

/**
 * 获取存储使用量信息
 */
export function getStorageInfo(): { used: number; total: number; items: number } {
  let used = 0
  let items = 0
  
  try {
    const keys = Object.keys(localStorage)
    keys.forEach(key => {
      if (key.startsWith(STORAGE_PREFIX)) {
        const value = localStorage.getItem(key)
        if (value) {
          used += value.length * 2 // UTF-16 encoding
          items++
        }
      }
    })
  } catch (error) {
    console.error('[PersistenceService] Failed to get storage info:', error)
  }
  
  return {
    used,
    total: 5 * 1024 * 1024, // 5MB typical localStorage limit
    items
  }
}

/**
 * 版本迁移检查
 */
export function checkAndMigrateVersion(): void {
  const storedVersion = getItem(STORAGE_KEYS.VERSION, null)
  
  if (storedVersion !== STORAGE_VERSION) {
    console.log(`[PersistenceService] Migrating from ${storedVersion} to ${STORAGE_VERSION}`)
    
    // 在这里添加版本迁移逻辑
    // 例如：清除旧格式的缓存数据
    if (!storedVersion) {
      // 首次使用，不需要迁移
    }
    
    setItem(STORAGE_KEYS.VERSION, STORAGE_VERSION)
  }
}

/**
 * 记录最后访问时间
 */
export function recordVisit(): void {
  setItem(STORAGE_KEYS.LAST_VISIT, Date.now())
}

/**
 * 获取最后访问时间
 */
export function getLastVisit(): number | null {
  return getItem(STORAGE_KEYS.LAST_VISIT, null)
}

// ============================================
// News Intelligence 特定的持久化函数
// ============================================

export interface NewsFilters {
  level?: string
  source?: string
  dateRange?: { start: string; end: string }
  showOnlyUnread?: boolean
}

export interface NewsSettings {
  activeTab: string
  filters: NewsFilters
  refreshInterval: number // in seconds
  showSources: boolean
  collapsedPanels: string[]
}

const DEFAULT_NEWS_SETTINGS: NewsSettings = {
  activeTab: 'overview',
  filters: {},
  refreshInterval: 300, // 5 minutes
  showSources: true,
  collapsedPanels: []
}

/**
 * 保存 News Intelligence 页面设置
 */
export function saveNewsSettings(settings: Partial<NewsSettings>): boolean {
  const current = getNewsSettings()
  const updated = { ...current, ...settings }
  
  setItem(STORAGE_KEYS.NEWS_ACTIVE_TAB, updated.activeTab)
  setItem(STORAGE_KEYS.NEWS_FILTERS, updated.filters)
  setItem(STORAGE_KEYS.NEWS_REFRESH_INTERVAL, updated.refreshInterval)
  setItem(STORAGE_KEYS.NEWS_SHOW_SOURCES, updated.showSources)
  setItem(STORAGE_KEYS.NEWS_COLLAPSED_PANELS, updated.collapsedPanels)
  
  return true
}

/**
 * 获取 News Intelligence 页面设置
 */
export function getNewsSettings(): NewsSettings {
  return {
    activeTab: getItem(STORAGE_KEYS.NEWS_ACTIVE_TAB, DEFAULT_NEWS_SETTINGS.activeTab),
    filters: getItem(STORAGE_KEYS.NEWS_FILTERS, DEFAULT_NEWS_SETTINGS.filters),
    refreshInterval: getItem(STORAGE_KEYS.NEWS_REFRESH_INTERVAL, DEFAULT_NEWS_SETTINGS.refreshInterval),
    showSources: getItem(STORAGE_KEYS.NEWS_SHOW_SOURCES, DEFAULT_NEWS_SETTINGS.showSources),
    collapsedPanels: getItem(STORAGE_KEYS.NEWS_COLLAPSED_PANELS, DEFAULT_NEWS_SETTINGS.collapsedPanels),
  }
}

// ============================================
// Trading Terminal 特定的持久化函数
// ============================================

export interface TradingSettings {
  selectedMarket: string
  orderType: 'market' | 'limit'
  chartInterval: string
}

const DEFAULT_TRADING_SETTINGS: TradingSettings = {
  selectedMarket: 'BTC-USD',
  orderType: 'limit',
  chartInterval: '1h'
}

/**
 * 保存 Trading Terminal 设置
 */
export function saveTradingSettings(settings: Partial<TradingSettings>): boolean {
  const current = getTradingSettings()
  const updated = { ...current, ...settings }
  
  setItem(STORAGE_KEYS.TRADING_SELECTED_MARKET, updated.selectedMarket)
  setItem(STORAGE_KEYS.TRADING_ORDER_TYPE, updated.orderType)
  setItem(STORAGE_KEYS.TRADING_CHART_INTERVAL, updated.chartInterval)
  
  return true
}

/**
 * 获取 Trading Terminal 设置
 */
export function getTradingSettings(): TradingSettings {
  return {
    selectedMarket: getItem(STORAGE_KEYS.TRADING_SELECTED_MARKET, DEFAULT_TRADING_SETTINGS.selectedMarket),
    orderType: getItem(STORAGE_KEYS.TRADING_ORDER_TYPE, DEFAULT_TRADING_SETTINGS.orderType),
    chartInterval: getItem(STORAGE_KEYS.TRADING_CHART_INTERVAL, DEFAULT_TRADING_SETTINGS.chartInterval),
  }
}

// 初始化：检查版本并记录访问
checkAndMigrateVersion()
recordVisit()

export default {
  setItem,
  getItem,
  removeItem,
  setCachedItem,
  getCachedItem,
  isCacheValid,
  clearAll,
  getStorageInfo,
  saveNewsSettings,
  getNewsSettings,
  saveTradingSettings,
  getTradingSettings,
  STORAGE_KEYS
}
