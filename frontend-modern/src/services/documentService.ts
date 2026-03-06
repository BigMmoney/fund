/**
 * Document Service - 政策文档管理系统
 * 
 * 功能：
 * 1. 文档采集：从官方源自动抓取政策文档
 * 2. 文档解析：提取关键信息、实体、日期、金额
 * 3. 文档索引：全文搜索、标签分类
 * 4. 文档关联：关联相关新闻、实体、政策
 * 5. 版本追踪：追踪政策文档修订历史
 */

// ============== Type Definitions ==============

export type DocumentType = 
  | 'executive_order'      // 行政令
  | 'federal_register'     // 联邦公报
  | 'regulation'           // 法规
  | 'legislation'          // 立法
  | 'guidance'             // 指导文件
  | 'press_release'        // 新闻稿
  | 'speech'               // 演讲
  | 'report'               // 报告
  | 'court_filing'         // 法院文件
  | 'international_agreement' // 国际协议

export type DocumentStatus = 'draft' | 'proposed' | 'final' | 'effective' | 'amended' | 'repealed'

export type DocumentSource = 
  | 'federal_register'
  | 'white_house'
  | 'treasury'
  | 'commerce'
  | 'state_dept'
  | 'ustr'
  | 'congress'
  | 'eu_official_journal'
  | 'boe'
  | 'pboc'

export interface PolicyDocument {
  id: string
  type: DocumentType
  source: DocumentSource
  title: string
  summary: string
  content: string
  url: string
  
  // 元数据
  documentNumber?: string    // 如 EO 14257
  agencyIds?: string[]       // 发布机构
  signatories?: string[]     // 签署人
  
  // 日期
  publishedAt: string
  effectiveDate?: string
  commentDeadline?: string
  
  // 分类
  topics: string[]
  entities: ExtractedEntity[]
  citations: Citation[]
  
  // 状态
  status: DocumentStatus
  amendments?: Amendment[]
  
  // 影响评估
  impactAssessment?: ImpactAssessment
  
  // 系统字段
  indexedAt: string
  updatedAt: string
  viewCount: number
  bookmarked: boolean
}

export interface ExtractedEntity {
  name: string
  type: 'company' | 'person' | 'country' | 'agency' | 'product' | 'regulation'
  mentions: number
  sentiment?: number
  context?: string
}

export interface Citation {
  text: string
  source: string
  type: 'cites' | 'amends' | 'supersedes' | 'implements'
}

export interface Amendment {
  id: string
  date: string
  description: string
  changes: string[]
}

export interface ImpactAssessment {
  affectedIndustries: string[]
  affectedCountries: string[]
  estimatedCost?: string
  complianceDeadline?: string
  riskLevel: 'low' | 'medium' | 'high' | 'critical'
}

export interface DocumentSearchParams {
  query?: string
  types?: DocumentType[]
  sources?: DocumentSource[]
  status?: DocumentStatus[]
  topics?: string[]
  entities?: string[]
  startDate?: string
  endDate?: string
  bookmarked?: boolean
  page?: number
  pageSize?: number
  sortBy?: 'relevance' | 'date' | 'views'
  sortOrder?: 'asc' | 'desc'
}

export interface DocumentSearchResult {
  documents: PolicyDocument[]
  total: number
  page: number
  pageSize: number
  facets: {
    types: Record<string, number>
    sources: Record<string, number>
    topics: Record<string, number>
    status: Record<string, number>
  }
}

// ============== Document Parser ==============

export class DocumentParser {
  // 提取实体
  extractEntities(text: string): ExtractedEntity[] {
    const entities: ExtractedEntity[] = []
    
    // 公司名称模式
    const companyPatterns = [
      /(?:Inc\.|Corp\.|LLC|Ltd\.|Company|Corporation|Group|Holdings)/gi,
      /(?:Huawei|ZTE|SMIC|TikTok|ByteDance|Tencent|Alibaba|DJI|Hikvision)/gi
    ]
    
    // 国家/地区模式
    const countryPatterns = [
      /(?:China|Chinese|PRC|Taiwan|Russia|Russian|Iran|North Korea|Cuba|Venezuela|Hong Kong|Macau)/gi,
      /(?:中国|中华人民共和国|台湾|俄罗斯|伊朗|朝鲜|古巴|委内瑞拉)/gi
    ]
    
    // 政府机构模式
    const agencyPatterns = [
      /(?:OFAC|BIS|Treasury|Commerce|State Department|USTR|DOJ|FBI|SEC|FTC)/gi,
      /(?:European Commission|ECB|Bank of England|PBoC|MOFCOM)/gi
    ]
    
    // 法规/政策模式
    const regulationPatterns = [
      /(?:Executive Order \d+|EO \d+|Section \d+|Regulation [A-Z]|Directive \d+)/gi,
      /(?:ITAR|EAR|OFAC Sanctions|Entity List|SDN List)/gi
    ]

    // 处理每种模式
    const processPattern = (
      patterns: RegExp[], 
      type: ExtractedEntity['type']
    ) => {
      patterns.forEach(pattern => {
        let match
        while ((match = pattern.exec(text)) !== null) {
          const name = match[0]
          const existing = entities.find(e => e.name.toLowerCase() === name.toLowerCase())
          if (existing) {
            existing.mentions++
          } else {
            entities.push({
              name,
              type,
              mentions: 1,
              context: text.substring(
                Math.max(0, match.index - 50),
                Math.min(text.length, match.index + name.length + 50)
              )
            })
          }
        }
      })
    }

    processPattern(companyPatterns, 'company')
    processPattern(countryPatterns, 'country')
    processPattern(agencyPatterns, 'agency')
    processPattern(regulationPatterns, 'regulation')

    return entities.sort((a, b) => b.mentions - a.mentions)
  }

  // 提取日期
  extractDates(text: string): { type: string; date: string }[] {
    const dates: { type: string; date: string }[] = []
    
    const patterns = [
      { regex: /effective (?:on |as of )?(\w+ \d+, \d{4})/gi, type: 'effective' },
      { regex: /comments (?:by|due|deadline:?) ?(\w+ \d+, \d{4})/gi, type: 'comment_deadline' },
      { regex: /published (?:on )?(\w+ \d+, \d{4})/gi, type: 'published' },
      { regex: /signed (?:on )?(\w+ \d+, \d{4})/gi, type: 'signed' }
    ]

    patterns.forEach(({ regex, type }) => {
      let match
      while ((match = regex.exec(text)) !== null) {
        try {
          const dateStr = match[1]
          const parsed = new Date(dateStr)
          if (!isNaN(parsed.getTime())) {
            dates.push({ type, date: parsed.toISOString() })
          }
        } catch {
          // Ignore parse errors
        }
      }
    })

    return dates
  }

  // 提取引用
  extractCitations(text: string): Citation[] {
    const citations: Citation[] = []
    
    const patterns = [
      { regex: /pursuant to ((?:Executive Order|EO) \d+)/gi, type: 'implements' as const },
      { regex: /amends ((?:Executive Order|EO) \d+)/gi, type: 'amends' as const },
      { regex: /supersedes ((?:Executive Order|EO) \d+)/gi, type: 'supersedes' as const },
      { regex: /(?:under|by) authority of ((?:\d+ U\.S\.C\. § \d+)|(?:Section \d+ of .+?))/gi, type: 'cites' as const }
    ]

    patterns.forEach(({ regex, type }) => {
      let match
      while ((match = regex.exec(text)) !== null) {
        citations.push({
          text: match[1],
          source: 'extracted',
          type
        })
      }
    })

    return citations
  }

  // 提取话题标签
  extractTopics(text: string): string[] {
    const topicKeywords: Record<string, string[]> = {
      'Sanctions': ['sanction', 'OFAC', 'SDN', 'designated', 'blocked'],
      'Trade': ['tariff', 'import', 'export', 'trade', 'customs', 'duty'],
      'Export Control': ['export control', 'Entity List', 'BIS', 'license', 'deemed export'],
      'Technology': ['semiconductor', 'AI', 'quantum', 'technology', '5G', 'chip'],
      'Finance': ['banking', 'financial', 'SWIFT', 'correspondent', 'transaction'],
      'Energy': ['oil', 'gas', 'petroleum', 'LNG', 'energy', 'pipeline'],
      'Defense': ['defense', 'military', 'arms', 'ITAR', 'munitions'],
      'Cybersecurity': ['cyber', 'hacking', 'malware', 'ransomware', 'data breach'],
      'Human Rights': ['human rights', 'Uyghur', 'genocide', 'forced labor', 'repression'],
      'Investment': ['CFIUS', 'investment', 'acquisition', 'national security']
    }

    const foundTopics: string[] = []
    const lowerText = text.toLowerCase()

    Object.entries(topicKeywords).forEach(([topic, keywords]) => {
      if (keywords.some(kw => lowerText.includes(kw.toLowerCase()))) {
        foundTopics.push(topic)
      }
    })

    return foundTopics
  }

  // 评估影响
  assessImpact(doc: Partial<PolicyDocument>): ImpactAssessment {
    const text = `${doc.title || ''} ${doc.summary || ''} ${doc.content || ''}`.toLowerCase()
    
    // 评估影响行业
    const industryKeywords: Record<string, string[]> = {
      'Semiconductors': ['semiconductor', 'chip', 'wafer', 'fab', 'EDA'],
      'Technology': ['software', 'cloud', 'AI', 'data center', 'tech'],
      'Finance': ['bank', 'financial', 'insurance', 'investment'],
      'Energy': ['oil', 'gas', 'solar', 'wind', 'nuclear'],
      'Defense': ['defense', 'aerospace', 'military', 'weapons'],
      'Healthcare': ['pharma', 'medical', 'biotech', 'healthcare'],
      'Telecommunications': ['telecom', '5G', 'wireless', 'network']
    }

    const affectedIndustries: string[] = []
    Object.entries(industryKeywords).forEach(([industry, keywords]) => {
      if (keywords.some(kw => text.includes(kw))) {
        affectedIndustries.push(industry)
      }
    })

    // 评估影响国家
    const countryKeywords: Record<string, string[]> = {
      'China': ['china', 'chinese', 'prc', 'beijing'],
      'Russia': ['russia', 'russian', 'moscow', 'kremlin'],
      'Iran': ['iran', 'iranian', 'tehran'],
      'North Korea': ['north korea', 'dprk', 'pyongyang'],
      'Venezuela': ['venezuela', 'caracas', 'maduro']
    }

    const affectedCountries: string[] = []
    Object.entries(countryKeywords).forEach(([country, keywords]) => {
      if (keywords.some(kw => text.includes(kw))) {
        affectedCountries.push(country)
      }
    })

    // 评估风险等级
    let riskLevel: ImpactAssessment['riskLevel'] = 'low'
    
    const criticalTerms = ['immediate', 'effective immediately', 'emergency', 'national security']
    const highTerms = ['prohibition', 'ban', 'block', 'sanction']
    const mediumTerms = ['restriction', 'license required', 'review']
    
    if (criticalTerms.some(t => text.includes(t))) riskLevel = 'critical'
    else if (highTerms.some(t => text.includes(t))) riskLevel = 'high'
    else if (mediumTerms.some(t => text.includes(t))) riskLevel = 'medium'

    return {
      affectedIndustries,
      affectedCountries,
      riskLevel
    }
  }

  // 完整解析文档
  parseDocument(raw: {
    title: string
    content: string
    url: string
    source: DocumentSource
    type?: DocumentType
    publishedAt?: string
  }): Partial<PolicyDocument> {
    const fullText = `${raw.title} ${raw.content}`
    
    const entities = this.extractEntities(fullText)
    const dates = this.extractDates(fullText)
    const citations = this.extractCitations(fullText)
    const topics = this.extractTopics(fullText)
    
    const effectiveDateEntry = dates.find(d => d.type === 'effective')
    const commentDeadlineEntry = dates.find(d => d.type === 'comment_deadline')

    const doc: Partial<PolicyDocument> = {
      title: raw.title,
      content: raw.content,
      url: raw.url,
      source: raw.source,
      type: raw.type || 'regulation',
      publishedAt: raw.publishedAt || new Date().toISOString(),
      effectiveDate: effectiveDateEntry?.date,
      commentDeadline: commentDeadlineEntry?.date,
      entities,
      citations,
      topics,
      summary: raw.content.substring(0, 500) + '...',
      status: 'final',
      indexedAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      viewCount: 0,
      bookmarked: false
    }

    doc.impactAssessment = this.assessImpact(doc)

    return doc
  }
}

// ============== Document Storage ==============

class DocumentStorage {
  private readonly STORAGE_KEY = 'intel_documents'
  private readonly BOOKMARKS_KEY = 'intel_doc_bookmarks'

  getDocuments(): PolicyDocument[] {
    try {
      const stored = localStorage.getItem(this.STORAGE_KEY)
      return stored ? JSON.parse(stored) : []
    } catch {
      return []
    }
  }

  saveDocument(doc: PolicyDocument): void {
    const docs = this.getDocuments()
    const index = docs.findIndex(d => d.id === doc.id)
    
    if (index >= 0) {
      docs[index] = { ...doc, updatedAt: new Date().toISOString() }
    } else {
      docs.unshift(doc)
    }
    
    // 保留最近 1000 个文档
    const trimmed = docs.slice(0, 1000)
    localStorage.setItem(this.STORAGE_KEY, JSON.stringify(trimmed))
  }

  deleteDocument(docId: string): void {
    const docs = this.getDocuments().filter(d => d.id !== docId)
    localStorage.setItem(this.STORAGE_KEY, JSON.stringify(docs))
  }

  incrementViewCount(docId: string): void {
    const docs = this.getDocuments()
    const doc = docs.find(d => d.id === docId)
    if (doc) {
      doc.viewCount++
      doc.updatedAt = new Date().toISOString()
      localStorage.setItem(this.STORAGE_KEY, JSON.stringify(docs))
    }
  }

  toggleBookmark(docId: string): boolean {
    const docs = this.getDocuments()
    const doc = docs.find(d => d.id === docId)
    if (doc) {
      doc.bookmarked = !doc.bookmarked
      doc.updatedAt = new Date().toISOString()
      localStorage.setItem(this.STORAGE_KEY, JSON.stringify(docs))
      return doc.bookmarked
    }
    return false
  }
}

// ============== Document Search Engine ==============

class DocumentSearchEngine {
  // 简单的 TF-IDF 搜索
  search(
    documents: PolicyDocument[], 
    params: DocumentSearchParams
  ): DocumentSearchResult {
    let results = [...documents]

    // 全文搜索
    if (params.query) {
      const queryLower = params.query.toLowerCase()
      const queryTerms = queryLower.split(/\s+/).filter(t => t.length > 2)
      
      results = results
        .map(doc => {
          const text = `${doc.title} ${doc.summary} ${doc.content}`.toLowerCase()
          const entityNames = doc.entities.map(e => e.name.toLowerCase()).join(' ')
          const topicText = doc.topics.join(' ').toLowerCase()
          const fullText = `${text} ${entityNames} ${topicText}`
          
          // 计算匹配分数
          let score = 0
          queryTerms.forEach(term => {
            if (doc.title.toLowerCase().includes(term)) score += 10
            if (doc.summary?.toLowerCase().includes(term)) score += 5
            if (fullText.includes(term)) score += 1
          })
          
          return { doc, score }
        })
        .filter(item => item.score > 0)
        .sort((a, b) => b.score - a.score)
        .map(item => item.doc)
    }

    // 类型过滤
    if (params.types && params.types.length > 0) {
      results = results.filter(d => params.types!.includes(d.type))
    }

    // 来源过滤
    if (params.sources && params.sources.length > 0) {
      results = results.filter(d => params.sources!.includes(d.source))
    }

    // 状态过滤
    if (params.status && params.status.length > 0) {
      results = results.filter(d => params.status!.includes(d.status))
    }

    // 话题过滤
    if (params.topics && params.topics.length > 0) {
      results = results.filter(d => 
        params.topics!.some(t => d.topics.includes(t))
      )
    }

    // 实体过滤
    if (params.entities && params.entities.length > 0) {
      results = results.filter(d =>
        params.entities!.some(e => 
          d.entities.some(de => de.name.toLowerCase().includes(e.toLowerCase()))
        )
      )
    }

    // 日期过滤
    if (params.startDate) {
      const start = new Date(params.startDate)
      results = results.filter(d => new Date(d.publishedAt) >= start)
    }
    if (params.endDate) {
      const end = new Date(params.endDate)
      results = results.filter(d => new Date(d.publishedAt) <= end)
    }

    // 书签过滤
    if (params.bookmarked !== undefined) {
      results = results.filter(d => d.bookmarked === params.bookmarked)
    }

    // 计算 facets
    const facets = {
      types: this.countFacet(results, 'type'),
      sources: this.countFacet(results, 'source'),
      topics: this.countTopics(results),
      status: this.countFacet(results, 'status')
    }

    // 排序
    if (params.sortBy === 'date') {
      results.sort((a, b) => {
        const dateA = new Date(a.publishedAt).getTime()
        const dateB = new Date(b.publishedAt).getTime()
        return params.sortOrder === 'asc' ? dateA - dateB : dateB - dateA
      })
    } else if (params.sortBy === 'views') {
      results.sort((a, b) => {
        return params.sortOrder === 'asc' 
          ? a.viewCount - b.viewCount 
          : b.viewCount - a.viewCount
      })
    }

    // 分页
    const page = params.page || 1
    const pageSize = params.pageSize || 20
    const start = (page - 1) * pageSize
    const paginatedResults = results.slice(start, start + pageSize)

    return {
      documents: paginatedResults,
      total: results.length,
      page,
      pageSize,
      facets
    }
  }

  private countFacet(
    docs: PolicyDocument[], 
    field: keyof PolicyDocument
  ): Record<string, number> {
    const counts: Record<string, number> = {}
    docs.forEach(doc => {
      const value = String(doc[field] || 'unknown')
      counts[value] = (counts[value] || 0) + 1
    })
    return counts
  }

  private countTopics(docs: PolicyDocument[]): Record<string, number> {
    const counts: Record<string, number> = {}
    docs.forEach(doc => {
      doc.topics.forEach(topic => {
        counts[topic] = (counts[topic] || 0) + 1
      })
    })
    return counts
  }
}

// ============== Main Document Service ==============

export class DocumentService {
  private storage: DocumentStorage
  private parser: DocumentParser
  private searchEngine: DocumentSearchEngine

  constructor() {
    this.storage = new DocumentStorage()
    this.parser = new DocumentParser()
    this.searchEngine = new DocumentSearchEngine()
  }

  // 导入并解析文档
  importDocument(raw: {
    title: string
    content: string
    url: string
    source: DocumentSource
    type?: DocumentType
    publishedAt?: string
  }): PolicyDocument {
    const parsed = this.parser.parseDocument(raw)
    
    const doc: PolicyDocument = {
      id: `doc-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      type: parsed.type || 'regulation',
      source: parsed.source || 'federal_register',
      title: parsed.title || 'Untitled',
      summary: parsed.summary || '',
      content: parsed.content || '',
      url: parsed.url || '',
      publishedAt: parsed.publishedAt || new Date().toISOString(),
      effectiveDate: parsed.effectiveDate,
      commentDeadline: parsed.commentDeadline,
      topics: parsed.topics || [],
      entities: parsed.entities || [],
      citations: parsed.citations || [],
      status: 'final',
      impactAssessment: parsed.impactAssessment,
      indexedAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      viewCount: 0,
      bookmarked: false
    }

    this.storage.saveDocument(doc)
    return doc
  }

  // 搜索文档
  search(params: DocumentSearchParams): DocumentSearchResult {
    const allDocs = this.storage.getDocuments()
    return this.searchEngine.search(allDocs, params)
  }

  // 获取单个文档
  getDocument(docId: string): PolicyDocument | null {
    const docs = this.storage.getDocuments()
    const doc = docs.find(d => d.id === docId)
    if (doc) {
      this.storage.incrementViewCount(docId)
    }
    return doc || null
  }

  // 获取最近文档
  getRecentDocuments(limit: number = 10): PolicyDocument[] {
    return this.storage.getDocuments()
      .sort((a, b) => new Date(b.publishedAt).getTime() - new Date(a.publishedAt).getTime())
      .slice(0, limit)
  }

  // 获取热门文档
  getPopularDocuments(limit: number = 10): PolicyDocument[] {
    return this.storage.getDocuments()
      .sort((a, b) => b.viewCount - a.viewCount)
      .slice(0, limit)
  }

  // 获取书签文档
  getBookmarkedDocuments(): PolicyDocument[] {
    return this.storage.getDocuments().filter(d => d.bookmarked)
  }

  // 切换书签
  toggleBookmark(docId: string): boolean {
    return this.storage.toggleBookmark(docId)
  }

  // 删除文档
  deleteDocument(docId: string): void {
    this.storage.deleteDocument(docId)
  }

  // 按实体查找相关文档
  findRelatedByEntity(entityName: string, limit: number = 10): PolicyDocument[] {
    return this.storage.getDocuments()
      .filter(d => d.entities.some(e => 
        e.name.toLowerCase().includes(entityName.toLowerCase())
      ))
      .slice(0, limit)
  }

  // 按话题查找相关文档
  findRelatedByTopic(topic: string, limit: number = 10): PolicyDocument[] {
    return this.storage.getDocuments()
      .filter(d => d.topics.includes(topic))
      .sort((a, b) => new Date(b.publishedAt).getTime() - new Date(a.publishedAt).getTime())
      .slice(0, limit)
  }

  // 获取统计数据
  getStats(): {
    total: number
    byType: Record<string, number>
    bySource: Record<string, number>
    byStatus: Record<string, number>
    topTopics: { topic: string; count: number }[]
    topEntities: { entity: string; count: number }[]
  } {
    const docs = this.storage.getDocuments()
    
    const byType: Record<string, number> = {}
    const bySource: Record<string, number> = {}
    const byStatus: Record<string, number> = {}
    const topicCounts: Record<string, number> = {}
    const entityCounts: Record<string, number> = {}

    docs.forEach(doc => {
      byType[doc.type] = (byType[doc.type] || 0) + 1
      bySource[doc.source] = (bySource[doc.source] || 0) + 1
      byStatus[doc.status] = (byStatus[doc.status] || 0) + 1
      
      doc.topics.forEach(t => {
        topicCounts[t] = (topicCounts[t] || 0) + 1
      })
      
      doc.entities.forEach(e => {
        entityCounts[e.name] = (entityCounts[e.name] || 0) + e.mentions
      })
    })

    const topTopics = Object.entries(topicCounts)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 10)
      .map(([topic, count]) => ({ topic, count }))

    const topEntities = Object.entries(entityCounts)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 10)
      .map(([entity, count]) => ({ entity, count }))

    return {
      total: docs.length,
      byType,
      bySource,
      byStatus,
      topTopics,
      topEntities
    }
  }

  // 导出解析器供外部使用
  getParser(): DocumentParser {
    return this.parser
  }
}

// 导出单例
export const documentService = new DocumentService()

export default documentService
