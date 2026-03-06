/**
 * Daily Market Generator / 每日市场生成器
 * 
 * 定时运行的服务，从新闻中自动生成预测市场
 * 
 * 功能:
 * - 每 6 小时拉取最新新闻
 * - 提取高置信度事件
 * - 去重（同一事件只生成一次）
 * - 生成 L1/L2 短期和中期市场
 * - 缓存到 localStorage
 */

import { 
  NewsToMarketPipeline, 
  GeneratedMarket, 
  MarketTier,
  SAMPLE_NEWS_FOR_TESTING 
} from './newsToMarketPipeline'
import { executivePowerService } from './executivePowerService'

// ==================== 类型定义 ====================

interface DailyGeneratorConfig {
  runIntervalMs: number         // 运行间隔（毫秒）
  minCertainty: number          // 最低置信度阈值
  maxMarketsPerRun: number      // 每次运行最多生成数量
  cacheKey: string              // localStorage 缓存键
  cacheTTL: number              // 缓存有效期（毫秒）
}

interface CachedData {
  markets: GeneratedMarket[]
  lastRun: string               // ISO date string
  stats: {
    L1: number
    L2: number
    L3: number
    total: number
    todayNew: number
  }
}

// ==================== 默认配置 ====================

const DEFAULT_CONFIG: DailyGeneratorConfig = {
  runIntervalMs: 6 * 60 * 60 * 1000,  // 6 小时
  minCertainty: 0.5,
  maxMarketsPerRun: 20,
  cacheKey: 'daily_generated_markets',
  cacheTTL: 24 * 60 * 60 * 1000       // 24 小时
}

// ==================== Daily Market Generator 类 ====================

export class DailyMarketGenerator {
  private pipeline: NewsToMarketPipeline
  private config: DailyGeneratorConfig
  private isRunning: boolean = false
  private intervalId: number | null = null
  private lastRunTime: Date | null = null
  private generatedMarkets: GeneratedMarket[] = []
  private listeners: Array<(markets: GeneratedMarket[]) => void> = []

  constructor(config: Partial<DailyGeneratorConfig> = {}) {
    this.config = { ...DEFAULT_CONFIG, ...config }
    this.pipeline = new NewsToMarketPipeline()
    this.loadFromCache()
  }

  /**
   * 启动自动生成器
   */
  start(): void {
    if (this.isRunning) {
      console.log('⚠️ DailyMarketGenerator already running')
      return
    }

    console.log('🚀 Starting DailyMarketGenerator...')
    this.isRunning = true

    // 立即运行一次
    this.run()

    // 设置定时运行
    this.intervalId = window.setInterval(() => {
      this.run()
    }, this.config.runIntervalMs)
  }

  /**
   * 停止自动生成器
   */
  stop(): void {
    if (!this.isRunning) return

    console.log('⏹️ Stopping DailyMarketGenerator...')
    this.isRunning = false

    if (this.intervalId) {
      window.clearInterval(this.intervalId)
      this.intervalId = null
    }
  }

  /**
   * 手动触发一次运行
   */
  async run(): Promise<GeneratedMarket[]> {
    console.log('🔄 DailyMarketGenerator running...')
    const startTime = Date.now()

    try {
      // 1. 收集新闻源
      const newsList = await this.collectNews()
      console.log(`📰 Collected ${newsList.length} news items`)

      // 2. 处理新闻生成市场
      const newMarkets = this.pipeline.processNewsBatch(newsList)
      console.log(`🏭 Generated ${newMarkets.length} new markets`)

      // 3. 合并到现有市场列表
      this.mergeMarkets(newMarkets)

      // 4. 清理过期市场
      this.cleanExpiredMarkets()

      // 5. 保存到缓存
      this.saveToCache()

      // 6. 通知监听器
      this.notifyListeners()

      this.lastRunTime = new Date()
      const elapsed = Date.now() - startTime
      console.log(`✅ DailyMarketGenerator completed in ${elapsed}ms. Total markets: ${this.generatedMarkets.length}`)

      return newMarkets
    } catch (error) {
      console.error('❌ DailyMarketGenerator error:', error)
      return []
    }
  }

  /**
   * 收集新闻源
   */
  private async collectNews(): Promise<Array<{ title: string; url?: string; date?: string }>> {
    const newsList: Array<{ title: string; url?: string; date?: string }> = []
    const now = new Date()
    const todayStr = now.toISOString().split('T')[0]

    // 1. 从 ExecutivePowerService 获取新闻
    try {
      const epNews = executivePowerService.getAllNews()
      epNews.forEach(news => {
        newsList.push({
          title: news.headline,
          url: news.url,
          date: news.publishedAt || todayStr
        })
      })
    } catch (error) {
      console.warn('⚠️ Failed to get news from ExecutivePowerService:', error)
    }

    // 2. 添加示例新闻（开发/演示用）
    // 在生产环境中应该移除这部分
    if (newsList.length < 5) {
      console.log('📌 Adding sample news for demonstration...')
      SAMPLE_NEWS_FOR_TESTING.forEach(news => {
        newsList.push({
          title: news.title,
          date: news.date || todayStr
        })
      })
    }

    // 3. 生成基于当前日期的动态新闻
    const dynamicNews = this.generateDynamicNews()
    newsList.push(...dynamicNews)

    return newsList
  }

  /**
   * 生成基于当前日期的动态新闻（模拟实时新闻流）
   */
  private generateDynamicNews(): Array<{ title: string; date: string }> {
    const today = new Date()
    const todayStr = today.toISOString().split('T')[0]
    const month = today.getMonth() + 1
    const day = today.getDate()
    // year is available if needed: today.getFullYear()

    // 基于日期生成确定性但看起来随机的新闻
    const seed = day * 31 + month
    const dynamicNews: Array<{ title: string; date: string }> = []

    // 政策新闻
    const policyTopics = [
      `Trump administration announces new trade measures on Chinese imports`,
      `White House signs executive order on AI development guidelines`,
      `US Treasury imposes sanctions on Russian financial institutions`,
      `Commerce Department updates semiconductor export controls`,
      `Biden administration extends tariff exemptions until Q2`
    ]
    dynamicNews.push({ 
      title: policyTopics[seed % policyTopics.length], 
      date: todayStr 
    })

    // 央行新闻
    const rateTopics = [
      `Federal Reserve signals potential rate cut in upcoming FOMC meeting`,
      `ECB President hints at maintaining current interest rate stance`,
      `Fed officials divided on timing of next rate adjustment`,
      `Bank of Japan considers policy shift amid yen weakness`,
      `FOMC minutes reveal debate over inflation trajectory`
    ]
    dynamicNews.push({ 
      title: rateTopics[(seed + 7) % rateTopics.length], 
      date: todayStr 
    })

    // 科技新闻
    const techTopics = [
      `NVIDIA shares surge ahead of quarterly earnings announcement`,
      `OpenAI reveals plans for next-generation AI model`,
      `Apple expected to announce new product line next month`,
      `Microsoft Azure reports record cloud revenue growth`,
      `Google DeepMind achieves breakthrough in protein folding`
    ]
    dynamicNews.push({ 
      title: techTopics[(seed + 13) % techTopics.length], 
      date: todayStr 
    })

    // 加密货币新闻
    const cryptoTopics = [
      `Bitcoin approaches key resistance level amid ETF inflows`,
      `Ethereum developers announce major upgrade timeline`,
      `SEC reviewing multiple spot crypto ETF applications`,
      `Institutional Bitcoin buying reaches new monthly high`,
      `Crypto market cap crosses $3 trillion milestone`
    ]
    dynamicNews.push({ 
      title: cryptoTopics[(seed + 19) % cryptoTopics.length], 
      date: todayStr 
    })

    // 地缘政治新闻
    const geopoliticalTopics = [
      `US-China trade negotiations resume amid tariff tensions`,
      `European Union announces new sanctions package`,
      `Middle East peace talks scheduled for next week`,
      `Taiwan Strait situation draws international concern`,
      `NATO members discuss increased defense spending`
    ]
    dynamicNews.push({ 
      title: geopoliticalTopics[(seed + 23) % geopoliticalTopics.length], 
      date: todayStr 
    })

    // 财报相关
    const earningsTopics = [
      `Tech giants set to report earnings this week`,
      `NVIDIA earnings expectations rise ahead of report`,
      `Amazon revenue forecast beats analyst estimates`,
      `Tesla delivery numbers exceed quarterly guidance`,
      `Apple services revenue hits record high`
    ]
    dynamicNews.push({ 
      title: earningsTopics[(seed + 29) % earningsTopics.length], 
      date: todayStr 
    })

    return dynamicNews
  }

  /**
   * 合并新生成的市场
   */
  private mergeMarkets(newMarkets: GeneratedMarket[]): void {
    for (const market of newMarkets) {
      // 检查是否已存在（基于问题相似度）
      const exists = this.generatedMarkets.some(m => 
        m.question.toLowerCase() === market.question.toLowerCase() ||
        m.sourceEvent.id === market.sourceEvent.id
      )
      
      if (!exists) {
        this.generatedMarkets.push(market)
      }
    }

    // 限制总数量
    if (this.generatedMarkets.length > 100) {
      // 保留最新的 100 个
      this.generatedMarkets = this.generatedMarkets
        .sort((a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime())
        .slice(0, 100)
    }
  }

  /**
   * 清理过期市场
   */
  private cleanExpiredMarkets(): void {
    const now = new Date()
    this.generatedMarkets = this.generatedMarkets.filter(m => {
      const endDate = new Date(m.endDate)
      return endDate > now
    })
  }

  /**
   * 从缓存加载
   */
  private loadFromCache(): void {
    try {
      const cached = localStorage.getItem(this.config.cacheKey)
      if (!cached) return

      const data: CachedData = JSON.parse(cached)
      const cacheTime = new Date(data.lastRun).getTime()
      
      // 检查缓存是否过期
      if (Date.now() - cacheTime < this.config.cacheTTL) {
        this.generatedMarkets = data.markets.map(m => ({
          ...m,
          createdAt: new Date(m.createdAt),
          sourceEvent: {
            ...m.sourceEvent,
            extractedAt: new Date(m.sourceEvent.extractedAt)
          }
        }))
        this.lastRunTime = new Date(data.lastRun)
        console.log(`📦 Loaded ${this.generatedMarkets.length} markets from cache`)
      }
    } catch (error) {
      console.warn('⚠️ Failed to load from cache:', error)
    }
  }

  /**
   * 保存到缓存
   */
  private saveToCache(): void {
    try {
      const today = new Date()
      today.setHours(0, 0, 0, 0)

      const data: CachedData = {
        markets: this.generatedMarkets,
        lastRun: new Date().toISOString(),
        stats: {
          L1: this.generatedMarkets.filter(m => m.tier === 'L1').length,
          L2: this.generatedMarkets.filter(m => m.tier === 'L2').length,
          L3: this.generatedMarkets.filter(m => m.tier === 'L3').length,
          total: this.generatedMarkets.length,
          todayNew: this.generatedMarkets.filter(m => new Date(m.createdAt) >= today).length
        }
      }

      localStorage.setItem(this.config.cacheKey, JSON.stringify(data))
    } catch (error) {
      console.warn('⚠️ Failed to save to cache:', error)
    }
  }

  /**
   * 注册监听器
   */
  onMarketsUpdated(callback: (markets: GeneratedMarket[]) => void): () => void {
    this.listeners.push(callback)
    return () => {
      this.listeners = this.listeners.filter(l => l !== callback)
    }
  }

  /**
   * 通知所有监听器
   */
  private notifyListeners(): void {
    for (const listener of this.listeners) {
      try {
        listener(this.generatedMarkets)
      } catch (error) {
        console.error('❌ Listener error:', error)
      }
    }
  }

  // ==================== 公共 API ====================

  /**
   * 获取所有生成的市场
   */
  getAllMarkets(): GeneratedMarket[] {
    return [...this.generatedMarkets]
  }

  /**
   * 获取按层级分组的市场
   */
  getMarketsByTier(): Record<MarketTier, GeneratedMarket[]> {
    return {
      L1: this.generatedMarkets.filter(m => m.tier === 'L1'),
      L2: this.generatedMarkets.filter(m => m.tier === 'L2'),
      L3: this.generatedMarkets.filter(m => m.tier === 'L3')
    }
  }

  /**
   * 获取今日新生成的市场
   */
  getTodayMarkets(): GeneratedMarket[] {
    const today = new Date()
    today.setHours(0, 0, 0, 0)
    return this.generatedMarkets.filter(m => new Date(m.createdAt) >= today)
  }

  /**
   * 获取 L1 短期市场（最需要每日更新的）
   */
  getShortTermMarkets(): GeneratedMarket[] {
    return this.generatedMarkets
      .filter(m => m.tier === 'L1')
      .sort((a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime())
  }

  /**
   * 获取统计信息
   */
  getStats(): CachedData['stats'] {
    const today = new Date()
    today.setHours(0, 0, 0, 0)

    return {
      L1: this.generatedMarkets.filter(m => m.tier === 'L1').length,
      L2: this.generatedMarkets.filter(m => m.tier === 'L2').length,
      L3: this.generatedMarkets.filter(m => m.tier === 'L3').length,
      total: this.generatedMarkets.length,
      todayNew: this.generatedMarkets.filter(m => new Date(m.createdAt) >= today).length
    }
  }

  /**
   * 获取运行状态
   */
  getStatus(): { isRunning: boolean; lastRun: Date | null; nextRun: Date | null } {
    const nextRun = this.lastRunTime && this.isRunning
      ? new Date(this.lastRunTime.getTime() + this.config.runIntervalMs)
      : null

    return {
      isRunning: this.isRunning,
      lastRun: this.lastRunTime,
      nextRun
    }
  }

  /**
   * 转换为 PredictionMarket 组件使用的 Market 格式
   */
  toMarketFormat(): Array<{
    ID: string
    Name: string
    NameZh: string
    Description: string
    DescriptionZh: string
    Outcomes: string[]
    State: string
    CreatedAt: string
    Volume: number
    Participants: number
    EndDate: string
    Category: string
    YesPrice: number
    NoPrice: number
    isRealData: boolean
    dataSource: string
    externalPlatform: string
    tier: MarketTier
  }> {
    return this.generatedMarkets.map(m => ({
      ID: m.id,
      Name: m.question,
      NameZh: m.questionZh,
      Description: m.description,
      DescriptionZh: m.descriptionZh,
      Outcomes: ['YES', 'NO'],
      State: 'open',
      CreatedAt: new Date(m.createdAt).toISOString().split('T')[0],
      Volume: m.volume,
      Participants: m.participants,
      EndDate: m.endDate,
      Category: m.category,
      YesPrice: m.yesPrice,
      NoPrice: m.noPrice,
      isRealData: false,
      dataSource: this.getTierEmoji(m.tier) + ' Auto-Generated',
      externalPlatform: `auto_${m.tier.toLowerCase()}`,
      tier: m.tier
    }))
  }

  private getTierEmoji(tier: MarketTier): string {
    switch (tier) {
      case 'L1': return '🟢'
      case 'L2': return '🟡'
      case 'L3': return '🔵'
    }
  }
}

// ==================== 单例实例 ====================

export const dailyMarketGenerator = new DailyMarketGenerator()

// ==================== 便捷 Hook ====================

import { useState, useEffect } from 'react'

export function useDailyGeneratedMarkets() {
  const [markets, setMarkets] = useState(dailyMarketGenerator.getAllMarkets())
  const [stats, setStats] = useState(dailyMarketGenerator.getStats())
  const [status, setStatus] = useState(dailyMarketGenerator.getStatus())

  useEffect(() => {
    // 确保生成器在运行
    if (!status.isRunning) {
      dailyMarketGenerator.start()
    }

    // 订阅更新
    const unsubscribe = dailyMarketGenerator.onMarketsUpdated((newMarkets) => {
      setMarkets([...newMarkets])
      setStats(dailyMarketGenerator.getStats())
      setStatus(dailyMarketGenerator.getStatus())
    })

    // 初始加载
    setMarkets(dailyMarketGenerator.getAllMarkets())
    setStats(dailyMarketGenerator.getStats())

    return () => {
      unsubscribe()
    }
  }, [])

  return {
    markets,
    marketsByTier: dailyMarketGenerator.getMarketsByTier(),
    todayMarkets: dailyMarketGenerator.getTodayMarkets(),
    shortTermMarkets: dailyMarketGenerator.getShortTermMarkets(),
    stats,
    status,
    refresh: () => dailyMarketGenerator.run(),
    toMarketFormat: () => dailyMarketGenerator.toMarketFormat()
  }
}
