/**
 * useNewsIntelligence Hook
 * 
 * 整合所有数据服务的自定义 Hook
 * - 真实 API 数据获取
 * - 决策评分计算
 * - 警报处理
 * - 文档管理
 */

import { useState, useEffect, useCallback, useRef } from 'react'
import { newsDataService, type RawNewsItem, type FederalRegisterDocument, type SDNEntry } from './newsDataService'
import { alertService, type Alert as AlertServiceAlert, type AlertStats } from './alertService'
import { documentService, type PolicyDocument, type DocumentSearchResult } from './documentService'
import { 
  calculateDecisionScore, 
  type DecisionResult, 
  type DecisionContext, 
  type Evidence,
  type Domain,
  type PolicyState,
  aggregateSentiment
} from './decisionScoringEngine'

// ============== Types ==============

export interface NewsIntelligenceState {
  // 数据状态
  isLoading: boolean
  isRefreshing: boolean
  error: string | null
  lastUpdate: Date | null
  
  // 新闻数据
  headlines: ProcessedHeadline[]
  federalRegisterDocs: ProcessedFederalDoc[]
  sdnUpdates: ProcessedSDNEntry[]
  
  // 警报
  alerts: AlertServiceAlert[]
  alertStats: AlertStats
  
  // 文档
  documents: PolicyDocument[]
  documentSearchResult: DocumentSearchResult | null
  
  // 统计
  stats: {
    totalHeadlines: number
    totalDocuments: number
    totalAlerts: number
    byDomain: Record<string, number>
    bySource: Record<string, number>
  }
}

export interface ProcessedHeadline {
  id: string
  title: string
  source: string
  sourceLevel: 'L0' | 'L0.5' | 'L1' | 'L2'
  publishedAt: string
  url: string
  summary: string
  domain: Domain
  sentiment: number
  confidence: number
  decisionScore?: DecisionResult
  entities: string[]
  isRead: boolean
}

export interface ProcessedFederalDoc {
  id: string
  documentNumber: string
  title: string
  type: string
  agencies: string[]
  publishedAt: string
  effectiveDate?: string
  commentDeadline?: string
  abstractText: string
  fullTextUrl: string
  htmlUrl: string
  topics: string[]
  decisionScore?: DecisionResult
  impactLevel: 'low' | 'medium' | 'high' | 'critical'
}

export interface ProcessedSDNEntry {
  id: string
  name: string
  type: string
  programs: string[]
  addedDate: string
  country?: string
  remarks?: string
  changeType: 'add' | 'modify' | 'remove'
  impactLevel: 'low' | 'medium' | 'high' | 'critical'
}

// ============== Utility Functions ==============

// 从标题中提取域
function extractDomain(text: string): Domain {
  const lowerText = text.toLowerCase()
  
  if (/tariff|trade|import|export|customs|duty/i.test(lowerText)) return 'trade'
  if (/sanction|ofac|sdn|blocked|designated/i.test(lowerText)) return 'sanction'
  if (/war|conflict|military|defense|invasion/i.test(lowerText)) return 'war'
  if (/rate|fed|fomc|monetary|interest/i.test(lowerText)) return 'rate'
  if (/fiscal|budget|spending|deficit|debt/i.test(lowerText)) return 'fiscal'
  if (/regulat|rule|compliance|sec|ftc/i.test(lowerText)) return 'regulation'
  if (/export control|bis|entity list|semiconductor/i.test(lowerText)) return 'export_control'
  if (/antitrust|merger|acquisition|competition/i.test(lowerText)) return 'antitrust'
  
  return 'regulation' // 默认
}

// 从来源判断层级
function determineSourceLevel(source: string): 'L0' | 'L0.5' | 'L1' | 'L2' {
  const l0Sources = ['white house', 'treasury', 'ofac', 'bis', 'federal register', 'president']
  const l05Sources = ['fed', 'federal reserve', 'ecb', 'pboc', 'congress', 'senate', 'house']
  const l1Sources = ['reuters', 'bloomberg', 'associated press', 'afp', 'ft', 'wsj', 'financial times']
  
  const lowerSource = source.toLowerCase()
  
  if (l0Sources.some(s => lowerSource.includes(s))) return 'L0'
  if (l05Sources.some(s => lowerSource.includes(s))) return 'L0.5'
  if (l1Sources.some(s => lowerSource.includes(s))) return 'L1'
  return 'L2'
}

// 简单情绪分析
function analyzeSentiment(text: string): number {
  const positiveWords = ['growth', 'positive', 'increase', 'improve', 'boost', 'relief', 'ease', 'support']
  const negativeWords = ['decline', 'negative', 'decrease', 'worsen', 'sanction', 'ban', 'restrict', 'penalty', 'fine']
  
  const lowerText = text.toLowerCase()
  let score = 0
  
  positiveWords.forEach(word => {
    if (lowerText.includes(word)) score += 0.15
  })
  
  negativeWords.forEach(word => {
    if (lowerText.includes(word)) score -= 0.15
  })
  
  return Math.max(-1, Math.min(1, score))
}

// 提取实体
function extractEntities(text: string): string[] {
  const entities: string[] = []
  
  // 公司名称
  const companyPattern = /\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)*(?:\s+(?:Inc|Corp|Ltd|LLC|Co)\.?))\b/g
  let match
  while ((match = companyPattern.exec(text)) !== null) {
    entities.push(match[1])
  }
  
  // 国家
  const countries = ['China', 'Russia', 'Iran', 'Taiwan', 'EU', 'Japan', 'Korea', 'India']
  countries.forEach(country => {
    if (text.includes(country)) entities.push(country)
  })
  
  return [...new Set(entities)].slice(0, 10)
}

// 评估影响级别
function assessImpactLevel(doc: { title?: string; agencies?: string[]; type?: string }): 'low' | 'medium' | 'high' | 'critical' {
  const text = (doc.title || '').toLowerCase()
  const agencies = doc.agencies || []
  const type = doc.type || ''
  
  // 高影响机构
  const criticalAgencies = ['OFAC', 'BIS', 'White House', 'Treasury']
  const highAgencies = ['Federal Reserve', 'SEC', 'DOJ', 'Commerce']
  
  if (criticalAgencies.some(a => agencies.some(ag => ag.includes(a)))) return 'critical'
  if (highAgencies.some(a => agencies.some(ag => ag.includes(a)))) return 'high'
  
  // 关键词
  if (/emergency|immediate|national security/i.test(text)) return 'critical'
  if (/sanction|ban|prohibit|block/i.test(text)) return 'high'
  if (/rule|regulation|guidance/i.test(text)) return 'medium'
  
  // 文档类型
  if (type.toLowerCase().includes('executive order')) return 'critical'
  if (type.toLowerCase().includes('final rule')) return 'high'
  
  return 'low'
}

// ============== Main Hook ==============

export function useNewsIntelligence() {
  // 状态
  const [state, setState] = useState<NewsIntelligenceState>({
    isLoading: true,
    isRefreshing: false,
    error: null,
    lastUpdate: null,
    headlines: [],
    federalRegisterDocs: [],
    sdnUpdates: [],
    alerts: [],
    alertStats: {
      total: 0,
      byPriority: { critical: 0, high: 0, medium: 0, low: 0 },
      byStatus: { active: 0, acknowledged: 0, dismissed: 0, expired: 0 },
      last24Hours: 0,
      last7Days: 0
    },
    documents: [],
    documentSearchResult: null,
    stats: {
      totalHeadlines: 0,
      totalDocuments: 0,
      totalAlerts: 0,
      byDomain: {},
      bySource: {}
    }
  })

  // Refs for intervals
  const refreshIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  // 加载初始数据
  const loadInitialData = useCallback(async () => {
    setState(prev => ({ ...prev, isLoading: true, error: null }))

    try {
      // 并行加载数据
      const [fedDocs, sdnData, newsData] = await Promise.allSettled([
        newsDataService.getFederalRegisterDocuments({
          agencies: ['ofac', 'bis', 'treasury', 'commerce'],
          perPage: 50
        }),
        newsDataService.getOFACUpdates(30),
        newsDataService.getNewsHeadlines({
          q: 'tariff OR sanction OR trade OR semiconductor',
          language: 'en',
          sortBy: 'publishedAt',
          pageSize: 50
        })
      ])

      // 处理 Federal Register 数据
      let processedFedDocs: ProcessedFederalDoc[] = []
      if (fedDocs.status === 'fulfilled' && fedDocs.value) {
        processedFedDocs = fedDocs.value.map(doc => ({
          id: doc.document_number,
          documentNumber: doc.document_number,
          title: doc.title,
          type: doc.type || 'Notice',
          agencies: doc.agencies.map(a => a.name),
          publishedAt: doc.publication_date,
          effectiveDate: doc.effective_on,
          commentDeadline: doc.comments_close_on,
          abstractText: doc.abstract || '',
          fullTextUrl: doc.pdf_url,
          htmlUrl: doc.html_url,
          topics: doc.topics || [],
          impactLevel: assessImpactLevel({
            title: doc.title,
            agencies: doc.agencies.map(a => a.name),
            type: doc.type
          })
        }))
      }

      // 处理 SDN 数据
      let processedSDN: ProcessedSDNEntry[] = []
      if (sdnData.status === 'fulfilled' && sdnData.value) {
        processedSDN = sdnData.value.map(entry => ({
          id: entry.uid || `sdn-${Date.now()}-${Math.random()}`,
          name: entry.name,
          type: entry.type,
          programs: entry.programs,
          addedDate: entry.addedDate,
          country: entry.country,
          remarks: entry.remarks,
          changeType: 'add' as const,
          impactLevel: 'high' as const
        }))
      }

      // 处理新闻数据
      let processedHeadlines: ProcessedHeadline[] = []
      if (newsData.status === 'fulfilled' && newsData.value) {
        processedHeadlines = newsData.value.map((item, index) => {
          const sourceLevel = determineSourceLevel(item.source)
          const domain = extractDomain(item.title + ' ' + item.description)
          const sentiment = analyzeSentiment(item.title + ' ' + item.description)
          
          return {
            id: `news-${index}-${Date.now()}`,
            title: item.title,
            source: item.source,
            sourceLevel,
            publishedAt: item.publishedAt,
            url: item.url,
            summary: item.description,
            domain,
            sentiment,
            confidence: sourceLevel === 'L0' ? 0.95 : sourceLevel === 'L0.5' ? 0.85 : sourceLevel === 'L1' ? 0.7 : 0.5,
            entities: extractEntities(item.title + ' ' + item.description),
            isRead: false
          }
        })
      }

      // 加载警报和文档
      const alerts = alertService.getAlerts()
      const alertStats = alertService.getStats()
      const documents = documentService.getRecentDocuments(50)

      // 计算统计
      const stats = {
        totalHeadlines: processedHeadlines.length,
        totalDocuments: documents.length + processedFedDocs.length,
        totalAlerts: alerts.length,
        byDomain: processedHeadlines.reduce((acc, h) => {
          acc[h.domain] = (acc[h.domain] || 0) + 1
          return acc
        }, {} as Record<string, number>),
        bySource: processedHeadlines.reduce((acc, h) => {
          acc[h.sourceLevel] = (acc[h.sourceLevel] || 0) + 1
          return acc
        }, {} as Record<string, number>)
      }

      setState({
        isLoading: false,
        isRefreshing: false,
        error: null,
        lastUpdate: new Date(),
        headlines: processedHeadlines,
        federalRegisterDocs: processedFedDocs,
        sdnUpdates: processedSDN,
        alerts,
        alertStats,
        documents,
        documentSearchResult: null,
        stats
      })

    } catch (error) {
      setState(prev => ({
        ...prev,
        isLoading: false,
        isRefreshing: false,
        error: error instanceof Error ? error.message : 'Unknown error'
      }))
    }
  }, [])

  // 刷新数据
  const refresh = useCallback(async () => {
    setState(prev => ({ ...prev, isRefreshing: true }))
    await loadInitialData()
  }, [loadInitialData])

  // 计算决策分数
  const calculateScore = useCallback((
    domain: Domain,
    policyState: PolicyState,
    items: Array<{ sourceLevel: string; publishedAt: string; text: string; sentiment: number; confidence: number }>
  ): DecisionResult => {
    const evidences: Evidence[] = items.map(item => ({
      sourceId: `source-${Math.random()}`,
      sourceLevel: item.sourceLevel as 'L0' | 'L0.5' | 'L1' | 'L2',
      publishedAt: item.publishedAt,
      text: item.text,
      sentiment: item.sentiment,
      confidence: item.confidence
    }))

    const context: DecisionContext = {
      domain,
      state: policyState,
      evidences,
      priorProbability: 0.5
    }

    return calculateDecisionScore(context)
  }, [])

  // 处理新闻项并检查警报
  const processNewsItem = useCallback(async (headline: ProcessedHeadline) => {
    const alerts = await alertService.processNewsItem({
      id: headline.id,
      title: headline.title,
      content: headline.summary,
      sourceLevel: headline.sourceLevel,
      domain: headline.domain,
      sentiment: headline.sentiment,
      score: headline.decisionScore?.score,
      publishedAt: headline.publishedAt,
      entities: headline.entities
    })

    if (alerts.length > 0) {
      setState(prev => ({
        ...prev,
        alerts: [...alerts, ...prev.alerts],
        alertStats: alertService.getStats()
      }))
    }

    return alerts
  }, [])

  // 搜索文档
  const searchDocuments = useCallback((query: string, filters?: {
    types?: string[]
    sources?: string[]
    startDate?: string
    endDate?: string
  }) => {
    const result = documentService.search({
      query,
      types: filters?.types as any,
      sources: filters?.sources as any,
      startDate: filters?.startDate,
      endDate: filters?.endDate,
      pageSize: 20
    })

    setState(prev => ({
      ...prev,
      documentSearchResult: result
    }))

    return result
  }, [])

  // 确认警报
  const acknowledgeAlert = useCallback((alertId: string) => {
    alertService.acknowledgeAlert(alertId)
    setState(prev => ({
      ...prev,
      alerts: prev.alerts.map(a => 
        a.id === alertId ? { ...a, status: 'acknowledged' as const } : a
      ),
      alertStats: alertService.getStats()
    }))
  }, [])

  // 忽略警报
  const dismissAlert = useCallback((alertId: string) => {
    alertService.dismissAlert(alertId)
    setState(prev => ({
      ...prev,
      alerts: prev.alerts.filter(a => a.id !== alertId),
      alertStats: alertService.getStats()
    }))
  }, [])

  // 切换书签
  const toggleDocumentBookmark = useCallback((docId: string) => {
    const isBookmarked = documentService.toggleBookmark(docId)
    setState(prev => ({
      ...prev,
      documents: prev.documents.map(d =>
        d.id === docId ? { ...d, bookmarked: isBookmarked } : d
      )
    }))
    return isBookmarked
  }, [])

  // 请求通知权限
  const requestNotificationPermission = useCallback(async () => {
    return alertService.requestNotificationPermission()
  }, [])

  // 初始化加载
  useEffect(() => {
    loadInitialData()

    // 订阅警报更新
    const unsubscribe = alertService.subscribe((alert) => {
      setState(prev => ({
        ...prev,
        alerts: [alert, ...prev.alerts],
        alertStats: alertService.getStats()
      }))
    })

    // 设置自动刷新 (每5分钟)
    refreshIntervalRef.current = setInterval(() => {
      refresh()
    }, 5 * 60 * 1000)

    return () => {
      unsubscribe()
      if (refreshIntervalRef.current) {
        clearInterval(refreshIntervalRef.current)
      }
    }
  }, [loadInitialData, refresh])

  return {
    // 状态
    ...state,
    
    // 方法
    refresh,
    calculateScore,
    processNewsItem,
    searchDocuments,
    acknowledgeAlert,
    dismissAlert,
    toggleDocumentBookmark,
    requestNotificationPermission,
    
    // 服务访问
    alertService,
    documentService,
    newsDataService
  }
}

export default useNewsIntelligence
