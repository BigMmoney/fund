/**
 * System Health Service
 * 
 * 管理数据源健康状态、API响应指标、刷新率等系统级监控
 */

// ============== Types ==============

export interface DataSourceHealth {
  id: string
  name: string
  type: 'api' | 'websocket' | 'rss' | 'scraper'
  status: 'healthy' | 'degraded' | 'error' | 'unknown'
  lastChecked: string
  lastSuccessful: string | null
  latencyMs: number
  successRate: number  // 0-1 最近24小时成功率
  errorCount: number
  totalRequests: number
  lastError?: string
  region: 'US' | 'EU' | 'CN' | 'INTL'
}

export interface SystemMetrics {
  totalDataSources: number
  healthyCount: number
  degradedCount: number
  errorCount: number
  avgLatencyMs: number
  overallSuccessRate: number
  lastFullRefresh: string
  refreshIntervalMs: number
  documentsProcessed: number
  alertsGenerated: number
  topicsTracked: number
  memoryUsageMB: number
  activeConnections: number
}

export interface APICallRecord {
  timestamp: string
  source: string
  endpoint: string
  method: 'GET' | 'POST'
  statusCode: number
  latencyMs: number
  responseSize: number
  success: boolean
  error?: string
}

export interface RefreshSchedule {
  sourceId: string
  intervalMs: number
  lastRefresh: string
  nextRefresh: string
  priority: 'critical' | 'high' | 'normal' | 'low'
}

// ============== Service Implementation ==============

class SystemHealthService {
  private dataSources: Map<string, DataSourceHealth> = new Map()
  private apiCallHistory: APICallRecord[] = []
  private refreshSchedules: Map<string, RefreshSchedule> = new Map()
  private listeners: Set<(metrics: SystemMetrics) => void> = new Set()
  private documentsProcessed: number = 0
  private alertsGenerated: number = 0
  private topicsTracked: number = 0
  
  constructor() {
    this.initializeDataSources()
  }
  
  private initializeDataSources() {
    // 初始化已知数据源
    const sources: Omit<DataSourceHealth, 'lastChecked' | 'lastSuccessful' | 'latencyMs' | 'successRate' | 'errorCount' | 'totalRequests'>[] = [
      { id: 'federal-register', name: 'Federal Register API', type: 'api', status: 'unknown', region: 'US' },
      { id: 'ofac-sdn', name: 'OFAC SDN List', type: 'api', status: 'unknown', region: 'US' },
      { id: 'bis-entity', name: 'BIS Entity List', type: 'scraper', status: 'unknown', region: 'US' },
      { id: 'ustr', name: 'USTR Announcements', type: 'rss', status: 'unknown', region: 'US' },
      { id: 'treasury', name: 'Treasury Press', type: 'rss', status: 'unknown', region: 'US' },
      { id: 'white-house', name: 'White House Statements', type: 'rss', status: 'unknown', region: 'US' },
      { id: 'eur-lex', name: 'EUR-Lex', type: 'api', status: 'unknown', region: 'EU' },
      { id: 'ec-trade', name: 'EC Trade DG', type: 'rss', status: 'unknown', region: 'EU' },
      { id: 'boe', name: 'Bank of England', type: 'rss', status: 'unknown', region: 'EU' },
      { id: 'mofcom', name: 'MOFCOM China', type: 'scraper', status: 'unknown', region: 'CN' },
      { id: 'pboc', name: 'PBOC Announcements', type: 'scraper', status: 'unknown', region: 'CN' },
      { id: 'reuters', name: 'Reuters Wire', type: 'api', status: 'unknown', region: 'INTL' },
      { id: 'bloomberg', name: 'Bloomberg Terminal', type: 'api', status: 'unknown', region: 'INTL' },
      { id: 'newsapi', name: 'NewsAPI Aggregator', type: 'api', status: 'unknown', region: 'INTL' }
    ]
    
    sources.forEach(s => {
      this.dataSources.set(s.id, {
        ...s,
        lastChecked: new Date().toISOString(),
        lastSuccessful: null,
        latencyMs: 0,
        successRate: 0,
        errorCount: 0,
        totalRequests: 0
      })
    })
    
    // 初始化刷新计划
    this.refreshSchedules.set('federal-register', {
      sourceId: 'federal-register',
      intervalMs: 5 * 60 * 1000,  // 5分钟
      lastRefresh: new Date().toISOString(),
      nextRefresh: new Date(Date.now() + 5 * 60 * 1000).toISOString(),
      priority: 'critical'
    })
    this.refreshSchedules.set('ofac-sdn', {
      sourceId: 'ofac-sdn',
      intervalMs: 15 * 60 * 1000,  // 15分钟
      lastRefresh: new Date().toISOString(),
      nextRefresh: new Date(Date.now() + 15 * 60 * 1000).toISOString(),
      priority: 'high'
    })
    this.refreshSchedules.set('newsapi', {
      sourceId: 'newsapi',
      intervalMs: 3 * 60 * 1000,  // 3分钟
      lastRefresh: new Date().toISOString(),
      nextRefresh: new Date(Date.now() + 3 * 60 * 1000).toISOString(),
      priority: 'high'
    })
  }
  
  /**
   * 记录API调用结果
   */
  recordAPICall(record: APICallRecord) {
    this.apiCallHistory.push(record)
    
    // 保留最近1000条记录
    if (this.apiCallHistory.length > 1000) {
      this.apiCallHistory = this.apiCallHistory.slice(-1000)
    }
    
    // 更新数据源状态
    const source = this.dataSources.get(record.source)
    if (source) {
      source.lastChecked = record.timestamp
      source.totalRequests++
      
      if (record.success) {
        source.lastSuccessful = record.timestamp
        // 保留整数毫秒值
        source.latencyMs = Math.round(record.latencyMs)
        source.status = record.latencyMs > 5000 ? 'degraded' : 'healthy'
      } else {
        source.errorCount++
        source.lastError = record.error
        source.status = 'error'
      }
      
      // 计算成功率（基于最近100次请求）
      const recentCalls = this.apiCallHistory
        .filter(c => c.source === record.source)
        .slice(-100)
      source.successRate = recentCalls.filter(c => c.success).length / recentCalls.length
      
      this.dataSources.set(record.source, source)
    }
    
    // 使用防抖来避免过于频繁的更新
    this.debouncedNotify()
  }
  
  private notifyTimeout: ReturnType<typeof setTimeout> | null = null
  private lastNotifyHash: string = ''
  
  private debouncedNotify() {
    if (this.notifyTimeout) {
      clearTimeout(this.notifyTimeout)
    }
    this.notifyTimeout = setTimeout(() => {
      // 计算哈希避免无变化通知
      const metrics = this.getMetrics()
      const hash = `${metrics.healthyCount}-${metrics.avgLatencyMs}-${metrics.documentsProcessed}`
      if (hash !== this.lastNotifyHash) {
        this.lastNotifyHash = hash
        this.notifyListeners()
      }
    }, 2000) // 2秒防抖 (从500ms增加)
  }
  
  /**
   * 更新计数器 - 使用防抖避免频繁通知
   */
  updateCounters(docs: number = 0, alerts: number = 0, topics: number = 0) {
    this.documentsProcessed += docs
    this.alertsGenerated += alerts
    this.topicsTracked = topics
    // 使用防抖而不是直接通知
    this.debouncedNotify()
  }
  
  /**
   * 获取所有数据源状态
   */
  getDataSources(): DataSourceHealth[] {
    return Array.from(this.dataSources.values())
  }
  
  /**
   * 获取特定区域的数据源
   */
  getDataSourcesByRegion(region: 'US' | 'EU' | 'CN' | 'INTL'): DataSourceHealth[] {
    return this.getDataSources().filter(s => s.region === region)
  }
  
  /**
   * 获取系统总体指标
   */
  getMetrics(): SystemMetrics {
    const sources = this.getDataSources()
    const healthy = sources.filter(s => s.status === 'healthy')
    const degraded = sources.filter(s => s.status === 'degraded')
    const error = sources.filter(s => s.status === 'error')
    
    // 安全计算平均延迟，避免 NaN
    const validLatencies = sources.filter(s => s.latencyMs > 0)
    const avgLatency = validLatencies.length > 0
      ? validLatencies.reduce((sum, s) => sum + s.latencyMs, 0) / validLatencies.length
      : 0
    
    const validSuccessRates = sources.filter(s => s.successRate > 0)
    const overallSuccessRate = validSuccessRates.length > 0
      ? validSuccessRates.reduce((sum, s) => sum + s.successRate, 0) / validSuccessRates.length
      : 0
    
    return {
      totalDataSources: sources.length,
      healthyCount: healthy.length,
      degradedCount: degraded.length,
      errorCount: error.length,
      avgLatencyMs: Math.round(avgLatency),
      overallSuccessRate: Math.round(overallSuccessRate * 100) / 100,
      lastFullRefresh: new Date().toISOString(),
      refreshIntervalMs: 5 * 60 * 1000,
      documentsProcessed: this.documentsProcessed,
      alertsGenerated: this.alertsGenerated,
      topicsTracked: this.topicsTracked,
      memoryUsageMB: typeof performance !== 'undefined' && 'memory' in performance 
        ? Math.round((performance as any).memory?.usedJSHeapSize / (1024 * 1024)) 
        : 0,
      activeConnections: healthy.length + degraded.length
    }
  }
  
  /**
   * 获取API调用历史
   */
  getAPICallHistory(limit: number = 50): APICallRecord[] {
    return this.apiCallHistory.slice(-limit).reverse()
  }
  
  /**
   * 获取刷新计划
   */
  getRefreshSchedules(): RefreshSchedule[] {
    return Array.from(this.refreshSchedules.values())
  }
  
  /**
   * 更新刷新时间
   */
  updateRefreshSchedule(sourceId: string) {
    const schedule = this.refreshSchedules.get(sourceId)
    if (schedule) {
      schedule.lastRefresh = new Date().toISOString()
      schedule.nextRefresh = new Date(Date.now() + schedule.intervalMs).toISOString()
      this.refreshSchedules.set(sourceId, schedule)
    }
  }
  
  /**
   * 手动设置数据源状态（用于模拟/测试）
   */
  setDataSourceStatus(sourceId: string, status: DataSourceHealth['status'], latencyMs?: number) {
    const source = this.dataSources.get(sourceId)
    if (source) {
      source.status = status
      source.lastChecked = new Date().toISOString()
      if (latencyMs !== undefined) source.latencyMs = latencyMs
      if (status === 'healthy' || status === 'degraded') {
        source.lastSuccessful = new Date().toISOString()
      }
      this.dataSources.set(sourceId, source)
      this.notifyListeners()
    }
  }
  
  /**
   * 批量更新数据源状态（用于真实数据加载后的状态同步）
   */
  updateFromRealDataLoad(results: Array<{ sourceId: string; success: boolean; latencyMs: number; error?: string }>) {
    results.forEach(r => {
      this.recordAPICall({
        timestamp: new Date().toISOString(),
        source: r.sourceId,
        endpoint: '/data',
        method: 'GET',
        statusCode: r.success ? 200 : 500,
        latencyMs: r.latencyMs,
        responseSize: 0,
        success: r.success,
        error: r.error
      })
    })
  }
  
  /**
   * 订阅指标更新
   */
  subscribe(callback: (metrics: SystemMetrics) => void): () => void {
    this.listeners.add(callback)
    return () => this.listeners.delete(callback)
  }
  
  private notifyListeners() {
    const metrics = this.getMetrics()
    this.listeners.forEach(cb => cb(metrics))
  }
  
  /**
   * 模拟健康检查（用于演示）- 使用稳定的延迟值
   * 每个数据源保持相对稳定的延迟，仅有小幅波动
   */
  private stableLatencies: Map<string, number> = new Map()
  
  simulateHealthCheck() {
    const sources = this.getDataSources()
    sources.forEach(s => {
      const rand = Math.random()
      
      // 获取或初始化该源的基础延迟
      let baseLatency = this.stableLatencies.get(s.id)
      if (baseLatency === undefined) {
        // 首次初始化：根据源类型设置合理的基础延迟
        if (s.id.includes('federal') || s.id.includes('ofac')) {
          baseLatency = 200 + Math.floor(Math.random() * 150) // 200-350ms
        } else if (s.id.includes('bis') || s.id.includes('ustr')) {
          baseLatency = 150 + Math.floor(Math.random() * 100) // 150-250ms
        } else if (s.id.includes('treasury') || s.id.includes('white-house')) {
          baseLatency = 250 + Math.floor(Math.random() * 200) // 250-450ms
        } else {
          baseLatency = 180 + Math.floor(Math.random() * 120) // 180-300ms
        }
        this.stableLatencies.set(s.id, baseLatency)
      }
      
      // 仅在基础延迟上做 ±5% 的微小波动，保持稳定
      const jitter = Math.round(baseLatency * 0.05 * (Math.random() - 0.5))
      const latency = baseLatency + jitter
      
      this.recordAPICall({
        timestamp: new Date().toISOString(),
        source: s.id,
        endpoint: '/health',
        method: 'GET',
        statusCode: rand > 0.1 ? 200 : rand > 0.05 ? 503 : 500,
        latencyMs: latency,
        responseSize: 128,
        success: rand > 0.1,
        error: rand <= 0.1 ? 'Connection timeout' : undefined
      })
    })
  }
}

// 导出单例
export const systemHealthService = new SystemHealthService()
export default systemHealthService
