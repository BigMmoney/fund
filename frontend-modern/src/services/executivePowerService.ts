/**
 * Executive Power Graph Service
 * 
 * 执行权力图谱新闻获取引擎
 * 
 * 核心概念：
 * - L0 (决策层): 最高权威，直接影响市场 (Trump, White House, OFAC, Federal Register)
 * - L0.5 (执行层): 执行机构，政策落地 (Treasury, Fed, Commerce/BIS)
 * - L1 (传播层): 权威媒体，信号放大 (Reuters, Bloomberg, WSJ)
 * - L2 (次级层): 次级来源，噪音验证 (Politico, Prediction Markets)
 */

import { newsDataService, FederalRegisterDocument, RawNewsItem, SDNEntry } from './newsDataService'
import { systemHealthService } from './systemHealthService'

// ============== Types ==============

export type SourceLevel = 'L0' | 'L0.5' | 'L1' | 'L2'
export type SourceRegion = 'US' | 'EU' | 'CN' | 'INTL'
export type PolicyDomain = 'trade' | 'sanction' | 'rate' | 'fiscal' | 'regulation' | 'war' | 'antitrust' | 'export_control'

export interface ExecutiveSource {
  id: string
  name: string
  nameZh: string
  level: SourceLevel
  region: SourceRegion
  domains: PolicyDomain[]
  executionPower: number  // 0-100: 执行权力分数
  authority: number       // 0-100: 权威性分数
  latency: 'realtime' | 'minutes' | 'hours' | 'daily'  // 信息发布延迟
  apiEndpoint?: string
  feedType: 'api' | 'rss' | 'websocket' | 'polling'
  isActive: boolean
  lastFetch?: string
  fetchInterval: number   // 分钟
}

export interface ExecutiveNews {
  id: string
  source: ExecutiveSource
  headline: string
  headlineZh?: string
  summary: string
  summaryZh?: string
  content?: string
  publishedAt: string
  fetchedAt: string
  url: string
  // 权力图谱属性
  level: SourceLevel
  region: SourceRegion
  domains: PolicyDomain[]
  executionPower: number
  // 分析属性
  urgency: 'flash' | 'urgent' | 'breaking' | 'routine'
  sentiment: 'bullish' | 'bearish' | 'neutral' | 'ambiguous'
  impactScore: number       // 0-100: 预估市场影响
  confidenceScore: number   // 0-100: 信息可信度
  // 关联
  relatedTopics: string[]
  affectedAssets: string[]
  affectedEntities: string[]
  // 状态
  isRead: boolean
  isBookmarked: boolean
  isVerified: boolean
  verificationChain: string[]  // 交叉验证来源
}

export interface PowerGraphNode {
  source: ExecutiveSource
  news: ExecutiveNews[]
  lastUpdate: string
  status: 'active' | 'degraded' | 'offline'
  pendingCount: number
}

export interface PowerGraphStats {
  totalSources: number
  activeSources: number
  totalNews: number
  byLevel: Record<SourceLevel, number>
  byRegion: Record<SourceRegion, number>
  byDomain: Record<PolicyDomain, number>
  latestUpdate: string
}

// ============== Source Registry ==============

const EXECUTIVE_SOURCES: ExecutiveSource[] = [
  // ========== US L0 - 最高决策层 ==========
  {
    id: 'federal-register',
    name: 'Federal Register',
    nameZh: '联邦公报',
    level: 'L0',
    region: 'US',
    domains: ['trade', 'sanction', 'regulation', 'fiscal'],
    executionPower: 100,
    authority: 100,
    latency: 'hours',
    apiEndpoint: 'https://www.federalregister.gov/api/v1',
    feedType: 'api',
    isActive: true,
    fetchInterval: 60
  },
  {
    id: 'ofac-sdn',
    name: 'OFAC SDN List',
    nameZh: 'OFAC SDN名单',
    level: 'L0',
    region: 'US',
    domains: ['sanction'],
    executionPower: 100,
    authority: 100,
    latency: 'daily',
    apiEndpoint: 'https://www.treasury.gov/ofac/downloads/sdn.xml',
    feedType: 'polling',
    isActive: true,
    fetchInterval: 30
  },
  {
    id: 'whitehouse',
    name: 'White House',
    nameZh: '白宫',
    level: 'L0',
    region: 'US',
    domains: ['trade', 'sanction', 'fiscal', 'regulation'],
    executionPower: 95,
    authority: 100,
    latency: 'minutes',
    feedType: 'polling',
    isActive: true,
    fetchInterval: 15
  },
  {
    id: 'trump-truth',
    name: 'Trump Truth Social',
    nameZh: 'Trump Truth Social',
    level: 'L0',
    region: 'US',
    domains: ['trade', 'sanction', 'war'],
    executionPower: 100,
    authority: 100,
    latency: 'realtime',
    feedType: 'websocket',
    isActive: true,
    fetchInterval: 1
  },
  
  // ========== US L0.5 - 执行机构 ==========
  {
    id: 'treasury',
    name: 'US Treasury',
    nameZh: '美国财政部',
    level: 'L0.5',
    region: 'US',
    domains: ['sanction', 'fiscal', 'rate'],
    executionPower: 90,
    authority: 95,
    latency: 'hours',
    feedType: 'rss',
    isActive: true,
    fetchInterval: 30
  },
  {
    id: 'fed',
    name: 'Federal Reserve',
    nameZh: '美联储',
    level: 'L0.5',
    region: 'US',
    domains: ['rate', 'fiscal'],
    executionPower: 95,
    authority: 98,
    latency: 'minutes',
    feedType: 'api',
    isActive: true,
    fetchInterval: 15
  },
  {
    id: 'commerce-bis',
    name: 'Commerce/BIS',
    nameZh: '商务部/BIS',
    level: 'L0.5',
    region: 'US',
    domains: ['trade', 'sanction', 'export_control'],
    executionPower: 95,
    authority: 95,
    latency: 'hours',
    feedType: 'polling',
    isActive: true,
    fetchInterval: 60
  },
  {
    id: 'ustr',
    name: 'USTR',
    nameZh: '美国贸易代表',
    level: 'L0.5',
    region: 'US',
    domains: ['trade'],
    executionPower: 88,
    authority: 92,
    latency: 'hours',
    feedType: 'rss',
    isActive: true,
    fetchInterval: 60
  },
  
  // ========== EU L0 - 欧盟决策层 ==========
  {
    id: 'eur-lex',
    name: 'EUR-Lex',
    nameZh: '欧盟法律公报',
    level: 'L0',
    region: 'EU',
    domains: ['trade', 'sanction', 'regulation'],
    executionPower: 100,
    authority: 100,
    latency: 'daily',
    apiEndpoint: 'https://eur-lex.europa.eu/eurlex-ws/rest',
    feedType: 'rss',
    isActive: true,
    fetchInterval: 120
  },
  {
    id: 'eu-sanctions',
    name: 'EU Sanctions List',
    nameZh: 'EU制裁名单',
    level: 'L0',
    region: 'EU',
    domains: ['sanction'],
    executionPower: 100,
    authority: 100,
    latency: 'daily',
    feedType: 'polling',
    isActive: true,
    fetchInterval: 60
  },
  
  // ========== EU L0.5 - 欧盟执行层 ==========
  {
    id: 'ecb',
    name: 'European Central Bank',
    nameZh: '欧洲央行',
    level: 'L0.5',
    region: 'EU',
    domains: ['rate', 'fiscal'],
    executionPower: 92,
    authority: 95,
    latency: 'minutes',
    feedType: 'rss',
    isActive: true,
    fetchInterval: 15
  },
  {
    id: 'dg-trade',
    name: 'DG TRADE',
    nameZh: '欧盟贸易总司',
    level: 'L0.5',
    region: 'EU',
    domains: ['trade', 'sanction'],
    executionPower: 80,
    authority: 88,
    latency: 'hours',
    feedType: 'rss',
    isActive: true,
    fetchInterval: 60
  },
  {
    id: 'dg-comp',
    name: 'DG COMP',
    nameZh: '欧盟竞争总司',
    level: 'L0.5',
    region: 'EU',
    domains: ['regulation', 'antitrust'],
    executionPower: 85,
    authority: 90,
    latency: 'hours',
    feedType: 'rss',
    isActive: true,
    fetchInterval: 60
  },
  
  // ========== CN L0.5 - 中国执行层 ==========
  {
    id: 'pboc',
    name: 'PBoC',
    nameZh: '中国人民银行',
    level: 'L0.5',
    region: 'CN',
    domains: ['rate', 'fiscal', 'trade'],
    executionPower: 92,
    authority: 95,
    latency: 'minutes',
    feedType: 'polling',
    isActive: true,
    fetchInterval: 30
  },
  {
    id: 'mofcom',
    name: 'MOFCOM',
    nameZh: '商务部',
    level: 'L0.5',
    region: 'CN',
    domains: ['trade', 'sanction'],
    executionPower: 88,
    authority: 90,
    latency: 'hours',
    feedType: 'polling',
    isActive: true,
    fetchInterval: 60
  },
  {
    id: 'china-gac',
    name: 'GAC',
    nameZh: '海关总署',
    level: 'L0.5',
    region: 'CN',
    domains: ['trade', 'sanction'],
    executionPower: 92,
    authority: 92,
    latency: 'hours',
    feedType: 'polling',
    isActive: true,
    fetchInterval: 120
  },
  
  // ========== L1 - 权威媒体 ==========
  {
    id: 'reuters',
    name: 'Reuters',
    nameZh: '路透社',
    level: 'L1',
    region: 'INTL',
    domains: ['trade', 'sanction', 'war', 'rate'],
    executionPower: 0,
    authority: 85,
    latency: 'realtime',
    feedType: 'api',
    isActive: true,
    fetchInterval: 5
  },
  {
    id: 'bloomberg',
    name: 'Bloomberg',
    nameZh: '彭博社',
    level: 'L1',
    region: 'INTL',
    domains: ['trade', 'rate', 'fiscal'],
    executionPower: 0,
    authority: 88,
    latency: 'realtime',
    feedType: 'api',
    isActive: true,
    fetchInterval: 5
  },
  {
    id: 'wsj',
    name: 'Wall Street Journal',
    nameZh: '华尔街日报',
    level: 'L1',
    region: 'US',
    domains: ['trade', 'rate', 'regulation'],
    executionPower: 0,
    authority: 85,
    latency: 'minutes',
    feedType: 'rss',
    isActive: true,
    fetchInterval: 10
  },
  {
    id: 'ft',
    name: 'Financial Times',
    nameZh: '金融时报',
    level: 'L1',
    region: 'EU',
    domains: ['trade', 'rate', 'fiscal'],
    executionPower: 0,
    authority: 86,
    latency: 'minutes',
    feedType: 'rss',
    isActive: true,
    fetchInterval: 10
  },
  
  // ========== L2 - 次级来源 ==========
  {
    id: 'politico',
    name: 'Politico',
    nameZh: 'Politico',
    level: 'L2',
    region: 'US',
    domains: ['trade', 'regulation'],
    executionPower: 0,
    authority: 70,
    latency: 'minutes',
    feedType: 'rss',
    isActive: true,
    fetchInterval: 15
  },
  {
    id: 'polymarket',
    name: 'Polymarket',
    nameZh: '预测市场',
    level: 'L2',
    region: 'INTL',
    domains: ['trade', 'war', 'regulation'],
    executionPower: 0,
    authority: 65,
    latency: 'realtime',
    feedType: 'websocket',
    isActive: true,
    fetchInterval: 1
  }
]

// ============== Analysis Functions ==============

/**
 * 计算新闻影响分数
 * 基于来源执行权力、权威性和内容分析
 */
function calculateImpactScore(source: ExecutiveSource, headline: string, content: string): number {
  let score = 0
  
  // 基础分：来源执行权力 (40%)
  score += source.executionPower * 0.4
  
  // 权威性分数 (30%)
  score += source.authority * 0.3
  
  // 关键词加分 (30%)
  const urgentKeywords = [
    'immediate', 'effective immediately', 'emergency', 'breaking',
    '立即生效', '紧急', '突发', 'flash'
  ]
  const impactKeywords = [
    'tariff', 'sanction', 'ban', 'restriction', 'entity list',
    'rate hike', 'rate cut', 'intervention',
    '关税', '制裁', '禁令', '实体清单', '加息', '降息'
  ]
  
  const text = `${headline} ${content}`.toLowerCase()
  
  urgentKeywords.forEach(kw => {
    if (text.includes(kw.toLowerCase())) score += 5
  })
  
  impactKeywords.forEach(kw => {
    if (text.includes(kw.toLowerCase())) score += 3
  })
  
  return Math.min(100, Math.round(score))
}

/**
 * 分析新闻情绪
 */
function analyzeSentiment(headline: string, content: string): 'bullish' | 'bearish' | 'neutral' | 'ambiguous' {
  const text = `${headline} ${content}`.toLowerCase()
  
  const bullishKeywords = [
    'lift', 'remove', 'ease', 'reduce tariff', 'cut rate', 'stimulus',
    'relief', 'agreement', 'deal', 'resolution', 'de-escalation',
    '取消', '减免', '刺激', '达成协议', '缓和'
  ]
  
  const bearishKeywords = [
    'tariff', 'sanction', 'ban', 'restrict', 'hike', 'escalation',
    'entity list', 'blacklist', 'retaliation', 'war', 'conflict',
    '关税', '制裁', '禁令', '升级', '实体清单', '报复'
  ]
  
  let bullishScore = 0
  let bearishScore = 0
  
  bullishKeywords.forEach(kw => {
    if (text.includes(kw.toLowerCase())) bullishScore++
  })
  
  bearishKeywords.forEach(kw => {
    if (text.includes(kw.toLowerCase())) bearishScore++
  })
  
  if (bullishScore > bearishScore + 2) return 'bullish'
  if (bearishScore > bullishScore + 2) return 'bearish'
  if (bullishScore === 0 && bearishScore === 0) return 'neutral'
  return 'ambiguous'
}

/**
 * 确定新闻紧急程度
 */
function determineUrgency(source: ExecutiveSource, headline: string): 'flash' | 'urgent' | 'breaking' | 'routine' {
  const text = headline.toLowerCase()
  
  // Flash: 最高紧急（L0来源的重大政策）
  if (source.level === 'L0') {
    if (text.includes('immediate') || text.includes('emergency') || text.includes('flash')) {
      return 'flash'
    }
    if (text.includes('breaking') || text.includes('announces')) {
      return 'urgent'
    }
    return 'breaking'
  }
  
  // L0.5 最高到 urgent
  if (source.level === 'L0.5') {
    if (text.includes('breaking') || text.includes('announces') || text.includes('decision')) {
      return 'urgent'
    }
    return 'breaking'
  }
  
  // L1/L2 一般为 breaking 或 routine
  if (text.includes('breaking') || text.includes('exclusive')) {
    return 'breaking'
  }
  
  return 'routine'
}

/**
 * 提取受影响资产
 */
function extractAffectedAssets(headline: string, content: string, domains: PolicyDomain[]): string[] {
  const assets: string[] = []
  const text = `${headline} ${content}`.toLowerCase()
  
  // 货币对
  if (text.includes('dollar') || text.includes('usd') || text.includes('美元')) assets.push('USD')
  if (text.includes('euro') || text.includes('eur') || text.includes('欧元')) assets.push('EUR')
  if (text.includes('yuan') || text.includes('cny') || text.includes('rmb') || text.includes('人民币')) assets.push('CNY')
  if (text.includes('yen') || text.includes('jpy') || text.includes('日元')) assets.push('JPY')
  
  // 商品
  if (text.includes('oil') || text.includes('crude') || text.includes('原油')) assets.push('OIL')
  if (text.includes('gold') || text.includes('黄金')) assets.push('GOLD')
  if (text.includes('copper') || text.includes('铜')) assets.push('COPPER')
  
  // 指数
  if (domains.includes('rate') || domains.includes('fiscal')) {
    assets.push('BONDS', 'SPY')
  }
  if (domains.includes('trade')) {
    assets.push('SPY', 'FXI', 'EWG')
  }
  
  // 行业 ETF
  if (text.includes('semiconductor') || text.includes('chip') || text.includes('半导体')) {
    assets.push('SOXX', 'SMH')
  }
  if (text.includes('auto') || text.includes('car') || text.includes('汽车')) {
    assets.push('CARZ')
  }
  if (text.includes('tech') || text.includes('technology') || text.includes('科技')) {
    assets.push('QQQ', 'XLK')
  }
  
  return [...new Set(assets)]
}

/**
 * 提取相关主题
 */
function extractRelatedTopics(headline: string, content: string, domains: PolicyDomain[]): string[] {
  const topics: string[] = []
  const text = `${headline} ${content}`.toLowerCase()
  
  // 政策主题
  if (text.includes('tariff') || text.includes('关税')) topics.push('US-China Tariffs')
  if (text.includes('entity list') || text.includes('实体清单')) topics.push('Entity List')
  if (text.includes('rate') && (text.includes('fed') || text.includes('央行'))) topics.push('Interest Rates')
  if (text.includes('sanction') || text.includes('制裁')) topics.push('Sanctions')
  if (text.includes('export control') || text.includes('出口管制')) topics.push('Export Controls')
  
  // 地区主题
  if (text.includes('china') || text.includes('中国')) topics.push('China Policy')
  if (text.includes('taiwan') || text.includes('台湾')) topics.push('Taiwan Strait')
  if (text.includes('russia') || text.includes('俄罗斯')) topics.push('Russia Sanctions')
  if (text.includes('eu') || text.includes('europe') || text.includes('欧盟')) topics.push('EU Trade')
  
  return [...new Set(topics)]
}

// ============== Main Service Class ==============

export class ExecutivePowerService {
  private sources: Map<string, ExecutiveSource> = new Map()
  private newsCache: Map<string, ExecutiveNews[]> = new Map()
  private listeners: Set<(news: ExecutiveNews[]) => void> = new Set()
  private fetchIntervals: Map<string, NodeJS.Timeout> = new Map()
  private isRunning = false
  
  // 🆕 指数退避控制
  private retryCount: Map<string, number> = new Map()
  private readonly MAX_RETRY = 5
  private readonly BASE_DELAY = 2000  // 2秒起始延迟
  private readonly MAX_DELAY = 60000  // 最大60秒延迟
  
  // 🆕 AbortController 用于取消正在进行的请求
  private abortController: AbortController | null = null
  
  constructor() {
    // 初始化来源注册表
    EXECUTIVE_SOURCES.forEach(source => {
      this.sources.set(source.id, source)
      this.newsCache.set(source.id, [])
      this.retryCount.set(source.id, 0)
    })
  }
  
  // ========== Public API ==========
  
  /**
   * 启动新闻获取引擎
   */
  start() {
    if (this.isRunning) return
    this.isRunning = true
    
    console.log('[ExecutivePower] Starting news fetch engine...')
    
    // 🆕 创建新的 AbortController
    this.abortController = new AbortController()
    
    // 🆕 重置所有重试计数
    this.sources.forEach((_, id) => this.retryCount.set(id, 0))
    
    // 立即执行一次全量获取（使用退避策略）
    this.fetchAllSourcesWithBackoff()
    
    // 设置定时获取 - 使用更长的间隔避免过度请求
    this.sources.forEach((source, id) => {
      if (!source.isActive) return
      
      // 🆕 最小间隔 5 分钟，避免过于频繁
      const minInterval = Math.max(source.fetchInterval, 5) * 60 * 1000
      
      const interval = setInterval(() => {
        if (this.isRunning) {
          this.fetchSourceNewsWithBackoff(id)
        }
      }, minInterval)
      
      this.fetchIntervals.set(id, interval)
    })
    
    console.log(`[ExecutivePower] Engine started with ${this.sources.size} sources`)
  }
  
  /**
   * 停止新闻获取引擎
   */
  stop() {
    this.isRunning = false
    
    // 🆕 取消所有正在进行的请求
    if (this.abortController) {
      this.abortController.abort()
      this.abortController = null
    }
    
    // 清理所有定时器
    this.fetchIntervals.forEach(interval => clearInterval(interval))
    this.fetchIntervals.clear()
    
    // 🆕 重置重试计数
    this.sources.forEach((_, id) => this.retryCount.set(id, 0))
    
    console.log('[ExecutivePower] Engine stopped')
  }
  
  /**
   * 订阅新闻更新
   */
  subscribe(callback: (news: ExecutiveNews[]) => void): () => void {
    this.listeners.add(callback)
    return () => this.listeners.delete(callback)
  }
  
  /**
   * 获取所有来源
   */
  getSources(): ExecutiveSource[] {
    return Array.from(this.sources.values())
  }
  
  /**
   * 按层级获取来源
   */
  getSourcesByLevel(level: SourceLevel): ExecutiveSource[] {
    return Array.from(this.sources.values()).filter(s => s.level === level)
  }
  
  /**
   * 按地区获取来源
   */
  getSourcesByRegion(region: SourceRegion): ExecutiveSource[] {
    return Array.from(this.sources.values()).filter(s => s.region === region)
  }
  
  /**
   * 获取所有缓存新闻
   */
  getAllNews(): ExecutiveNews[] {
    const allNews: ExecutiveNews[] = []
    this.newsCache.forEach(news => allNews.push(...news))
    return allNews.sort((a, b) => 
      new Date(b.publishedAt).getTime() - new Date(a.publishedAt).getTime()
    )
  }
  
  /**
   * 按层级获取新闻
   */
  getNewsByLevel(level: SourceLevel): ExecutiveNews[] {
    return this.getAllNews().filter(n => n.level === level)
  }
  
  /**
   * 按地区获取新闻
   */
  getNewsByRegion(region: SourceRegion): ExecutiveNews[] {
    return this.getAllNews().filter(n => n.region === region)
  }
  
  /**
   * 按领域获取新闻
   */
  getNewsByDomain(domain: PolicyDomain): ExecutiveNews[] {
    return this.getAllNews().filter(n => n.domains.includes(domain))
  }
  
  /**
   * 获取权力图谱统计
   */
  getStats(): PowerGraphStats {
    const allNews = this.getAllNews()
    const activeSources = Array.from(this.sources.values()).filter(s => s.isActive)
    
    const byLevel: Record<SourceLevel, number> = { 'L0': 0, 'L0.5': 0, 'L1': 0, 'L2': 0 }
    const byRegion: Record<SourceRegion, number> = { 'US': 0, 'EU': 0, 'CN': 0, 'INTL': 0 }
    const byDomain: Record<PolicyDomain, number> = {
      trade: 0, sanction: 0, rate: 0, fiscal: 0,
      regulation: 0, war: 0, antitrust: 0, export_control: 0
    }
    
    allNews.forEach(news => {
      byLevel[news.level]++
      byRegion[news.region]++
      news.domains.forEach(d => byDomain[d]++)
    })
    
    return {
      totalSources: this.sources.size,
      activeSources: activeSources.length,
      totalNews: allNews.length,
      byLevel,
      byRegion,
      byDomain,
      latestUpdate: allNews[0]?.fetchedAt || new Date().toISOString()
    }
  }
  
  /**
   * 获取权力图谱节点（用于可视化）
   */
  getPowerGraphNodes(): PowerGraphNode[] {
    return Array.from(this.sources.values()).map(source => ({
      source,
      news: this.newsCache.get(source.id) || [],
      lastUpdate: source.lastFetch || '',
      status: source.isActive ? 'active' : 'offline',
      pendingCount: 0
    }))
  }
  
  /**
   * 手动刷新特定来源
   */
  async refreshSource(sourceId: string): Promise<ExecutiveNews[]> {
    return this.fetchSourceNewsWithBackoff(sourceId)
  }
  
  /**
   * 刷新所有来源
   */
  async refreshAll(): Promise<ExecutiveNews[]> {
    return this.fetchAllSourcesWithBackoff()
  }
  
  // ========== Private Methods ==========
  
  /**
   * 🆕 计算指数退避延迟
   */
  private calculateBackoffDelay(retryCount: number): number {
    const delay = this.BASE_DELAY * Math.pow(2, retryCount)
    return Math.min(delay, this.MAX_DELAY)
  }
  
  /**
   * 🆕 带退避策略的全量获取
   */
  private async fetchAllSourcesWithBackoff(): Promise<ExecutiveNews[]> {
    if (!this.isRunning) return []
    
    const allNews: ExecutiveNews[] = []
    
    // 串行获取（避免并发风暴），每个请求之间间隔 500ms
    const activeSourceIds = Array.from(this.sources.entries())
      .filter(([_, source]) => source.isActive)
      .map(([id]) => id)
    
    for (const id of activeSourceIds) {
      if (!this.isRunning) break  // 检查是否已停止
      
      try {
        const news = await this.fetchSourceNewsWithBackoff(id)
        allNews.push(...news)
        
        // 请求间隔 500ms，避免过载
        await new Promise(resolve => setTimeout(resolve, 500))
      } catch (error) {
        // 静默处理，继续下一个
        console.warn(`[ExecutivePower] Skipping ${id} due to error`)
      }
    }
    
    return allNews
  }
  
  /**
   * 🆕 带退避策略的单源获取
   */
  private async fetchSourceNewsWithBackoff(sourceId: string): Promise<ExecutiveNews[]> {
    if (!this.isRunning) return []
    
    const currentRetry = this.retryCount.get(sourceId) || 0
    
    // 超过最大重试次数，静默跳过
    if (currentRetry >= this.MAX_RETRY) {
      console.log(`[ExecutivePower] ${sourceId} exceeded max retries, skipping`)
      return this.newsCache.get(sourceId) || []
    }
    
    try {
      const news = await this.fetchSourceNews(sourceId)
      // 成功后重置重试计数
      this.retryCount.set(sourceId, 0)
      return news
    } catch (error) {
      // 递增重试计数
      this.retryCount.set(sourceId, currentRetry + 1)
      
      const delay = this.calculateBackoffDelay(currentRetry)
      console.log(`[ExecutivePower] ${sourceId} failed, retry ${currentRetry + 1}/${this.MAX_RETRY} in ${delay}ms`)
      
      // 不立即重试，返回缓存数据
      // 下次定时器触发时会自动重试
      return this.newsCache.get(sourceId) || []
    }
  }
  
  private async fetchAllSources(): Promise<ExecutiveNews[]> {
    const allNews: ExecutiveNews[] = []
    
    // 并行获取所有活跃来源
    const promises = Array.from(this.sources.entries())
      .filter(([_, source]) => source.isActive)
      .map(([id]) => this.fetchSourceNews(id))
    
    const results = await Promise.allSettled(promises)
    
    results.forEach(result => {
      if (result.status === 'fulfilled') {
        allNews.push(...result.value)
      }
    })
    
    return allNews
  }
  
  private async fetchSourceNews(sourceId: string): Promise<ExecutiveNews[]> {
    const source = this.sources.get(sourceId)
    if (!source) return []
    
    // 🆕 检查源是否已被禁用（超过最大重试次数）
    const retryCount = this.retryCount.get(sourceId) || 0
    if (retryCount >= this.MAX_RETRY) {
      // 静默跳过，返回缓存
      return this.newsCache.get(sourceId) || []
    }
    
    const startTime = Date.now()
    let news: ExecutiveNews[] = []
    
    try {
      switch (sourceId) {
        case 'federal-register':
          news = await this.fetchFederalRegister(source)
          break
        case 'ofac-sdn':
          news = await this.fetchOFAC(source)
          break
        case 'reuters':
        case 'bloomberg':
        case 'wsj':
        case 'ft':
        case 'politico':
          // 🆕 NewsAPI 需要 API key，如果没有则跳过
          if (!import.meta.env.VITE_NEWS_API_KEY) {
            // 静默跳过，不打日志刷屏
            return this.newsCache.get(sourceId) || []
          }
          news = await this.fetchNewsAPI(source)
          break
        default:
          // 其他来源使用通用新闻 API（也需要 API key）
          if (!import.meta.env.VITE_NEWS_API_KEY) {
            return this.newsCache.get(sourceId) || []
          }
          news = await this.fetchGenericNews(source)
      }
      
      // 更新缓存
      this.newsCache.set(sourceId, news)
      
      // 更新来源状态
      source.lastFetch = new Date().toISOString()
      
      // 🆕 成功后重置重试计数
      this.retryCount.set(sourceId, 0)
      
      // 记录 API 调用
      systemHealthService.recordAPICall(sourceId, true, Date.now() - startTime)
      
      // 通知订阅者
      this.notifyListeners()
      
    } catch (error) {
      // 🆕 递增重试计数（不重启引擎！）
      const currentRetry = this.retryCount.get(sourceId) || 0
      this.retryCount.set(sourceId, currentRetry + 1)
      
      // 🆕 只在前几次失败时打印日志，避免刷屏
      if (currentRetry < 3) {
        console.warn(`[ExecutivePower] ${sourceId} failed (${currentRetry + 1}/${this.MAX_RETRY})`)
      }
      
      systemHealthService.recordAPICall(sourceId, false, Date.now() - startTime)
      
      // 🆕 返回缓存数据，不抛出错误
      return this.newsCache.get(sourceId) || []
    }
    
    return news
  }
  
  private async fetchFederalRegister(source: ExecutiveSource): Promise<ExecutiveNews[]> {
    const docs = await newsDataService.getFederalRegisterDocuments({
      perPage: 20
    })
    
    return docs.map(doc => this.convertFederalRegisterDoc(source, doc))
  }
  
  private async fetchOFAC(source: ExecutiveSource): Promise<ExecutiveNews[]> {
    const entries = await newsDataService.getOFACUpdates(30)
    return entries.map(entry => this.convertSDNEntry(source, entry))
  }
  
  private async fetchNewsAPI(source: ExecutiveSource): Promise<ExecutiveNews[]> {
    const keywords = source.domains.map(d => {
      switch (d) {
        case 'trade': return 'trade tariff'
        case 'sanction': return 'sanctions'
        case 'rate': return 'interest rate central bank'
        case 'fiscal': return 'fiscal policy'
        case 'regulation': return 'regulation policy'
        case 'war': return 'geopolitical conflict'
        default: return d
      }
    }).join(' OR ')
    
    const articles = await newsDataService.getNewsHeadlines({
      q: keywords,
      pageSize: 20
    })
    
    return articles.map(article => this.convertNewsArticle(source, article))
  }
  
  private async fetchGenericNews(source: ExecutiveSource): Promise<ExecutiveNews[]> {
    // 为没有专用 API 的来源生成模拟数据
    return this.generateMockNews(source, 5)
  }
  
  // ========== Converters ==========
  
  private convertFederalRegisterDoc(source: ExecutiveSource, doc: FederalRegisterDocument): ExecutiveNews {
    return {
      id: `fr-${doc.documentNumber}`,
      source,
      headline: doc.title,
      summary: doc.abstractText || doc.title,
      publishedAt: doc.publicationDate,
      fetchedAt: new Date().toISOString(),
      url: doc.htmlUrl,
      level: source.level,
      region: source.region,
      domains: source.domains,
      executionPower: source.executionPower,
      urgency: determineUrgency(source, doc.title),
      sentiment: analyzeSentiment(doc.title, doc.abstractText || ''),
      impactScore: calculateImpactScore(source, doc.title, doc.abstractText || ''),
      confidenceScore: source.authority,
      relatedTopics: extractRelatedTopics(doc.title, doc.abstractText || '', source.domains),
      affectedAssets: extractAffectedAssets(doc.title, doc.abstractText || '', source.domains),
      affectedEntities: doc.agencies?.map(a => a.name) || [],
      isRead: false,
      isBookmarked: false,
      isVerified: true,
      verificationChain: ['Federal Register (Official)']
    }
  }
  
  private convertSDNEntry(source: ExecutiveSource, entry: SDNEntry): ExecutiveNews {
    const entityName = entry.entityName || `${entry.firstName || ''} ${entry.lastName || ''}`.trim()
    
    return {
      id: `sdn-${entry.uid}`,
      source,
      headline: `OFAC SDN Update: ${entityName}`,
      headlineZh: `OFAC SDN更新: ${entityName}`,
      summary: `${entry.sdnType === 'entity' ? 'Entity' : 'Individual'} added to SDN List. Programs: ${entry.programs?.join(', ') || 'N/A'}`,
      publishedAt: new Date().toISOString(),
      fetchedAt: new Date().toISOString(),
      url: 'https://sanctionssearch.ofac.treas.gov/',
      level: source.level,
      region: source.region,
      domains: ['sanction'],
      executionPower: 100,
      urgency: 'flash',
      sentiment: 'bearish',
      impactScore: 95,
      confidenceScore: 100,
      relatedTopics: ['Sanctions', 'OFAC SDN'],
      affectedAssets: [],
      affectedEntities: [entityName],
      isRead: false,
      isBookmarked: false,
      isVerified: true,
      verificationChain: ['OFAC Official List']
    }
  }
  
  private convertNewsArticle(source: ExecutiveSource, article: RawNewsItem): ExecutiveNews {
    return {
      id: article.id,
      source,
      headline: article.title,
      summary: article.description || article.title,
      content: article.content,
      publishedAt: article.publishedAt,
      fetchedAt: new Date().toISOString(),
      url: article.url,
      level: source.level,
      region: source.region,
      domains: source.domains,
      executionPower: source.executionPower,
      urgency: determineUrgency(source, article.title),
      sentiment: analyzeSentiment(article.title, article.description || ''),
      impactScore: calculateImpactScore(source, article.title, article.description || ''),
      confidenceScore: source.authority,
      relatedTopics: extractRelatedTopics(article.title, article.description || '', source.domains),
      affectedAssets: extractAffectedAssets(article.title, article.description || '', source.domains),
      affectedEntities: [],
      isRead: false,
      isBookmarked: false,
      isVerified: false,
      verificationChain: []
    }
  }
  
  private generateMockNews(source: ExecutiveSource, count: number): ExecutiveNews[] {
    const headlines = [
      `${source.name} announces new policy measures`,
      `${source.name} releases statement on economic outlook`,
      `${source.name} official speaks on trade relations`,
      `${source.name} confirms regulatory review in progress`,
      `${source.name} to hold press conference on upcoming decisions`
    ]
    
    return headlines.slice(0, count).map((headline, i) => ({
      id: `mock-${source.id}-${Date.now()}-${i}`,
      source,
      headline,
      summary: `Summary from ${source.name}: ${headline}`,
      publishedAt: new Date(Date.now() - i * 3600000).toISOString(),
      fetchedAt: new Date().toISOString(),
      url: '#',
      level: source.level,
      region: source.region,
      domains: source.domains,
      executionPower: source.executionPower,
      urgency: 'routine',
      sentiment: 'neutral',
      impactScore: Math.round(source.executionPower * 0.5 + source.authority * 0.3),
      confidenceScore: source.authority,
      relatedTopics: source.domains.slice(0, 2).map(d => d.charAt(0).toUpperCase() + d.slice(1)),
      affectedAssets: [],
      affectedEntities: [],
      isRead: false,
      isBookmarked: false,
      isVerified: false,
      verificationChain: []
    }))
  }
  
  // 🆕 防抖机制 - 避免频繁通知
  private notifyTimeout: ReturnType<typeof setTimeout> | null = null
  private lastNotifyHash: string = ''
  private pendingNotify: boolean = false
  
  private notifyListeners() {
    // 计算当前数据的哈希值
    const allNews = this.getAllNews()
    const currentHash = `${allNews.length}-${allNews[0]?.id || 'empty'}-${allNews[0]?.publishedAt || ''}`
    
    // 如果数据没有变化，不通知
    if (currentHash === this.lastNotifyHash) {
      return
    }
    
    this.lastNotifyHash = currentHash
    this.pendingNotify = true
    
    // 防抖：等待500ms后统一通知，避免频繁更新
    if (this.notifyTimeout) {
      clearTimeout(this.notifyTimeout)
    }
    
    this.notifyTimeout = setTimeout(() => {
      if (this.pendingNotify) {
        this.pendingNotify = false
        this.listeners.forEach(listener => listener(allNews))
      }
    }, 500)
  }
}

// ============== Singleton Export ==============

export const executivePowerService = new ExecutivePowerService()

export default executivePowerService
