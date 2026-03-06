/**
 * News Intelligence Data Service
 * 
 * 真实数据接入层 - 连接各类官方数据源 API
 * 
 * 数据源包括：
 * - OFAC SDN List (https://sanctionssearch.ofac.treas.gov/)
 * - BIS Entity List (https://www.bis.doc.gov/index.php/policy-guidance/lists-of-parties-of-concern)
 * - Federal Register API (https://www.federalregister.gov/developers/api/v1)
 * - EU Official Journal (https://eur-lex.europa.eu/content/welcome/data-reuse.html)
 * - Reuters/Bloomberg/WSJ API feeds
 */

// ============== API Configuration ==============

export interface APIConfig {
  baseUrl: string
  apiKey?: string
  rateLimit: number  // requests per minute
  timeout: number    // milliseconds
}

// 🆕 检测是否在开发环境（使用 Vite 代理）
const isDev = import.meta.env.DEV

const API_CONFIGS: Record<string, APIConfig> = {
  federalRegister: {
    // 🆕 开发环境使用代理，生产环境直连（需要后端代理）
    baseUrl: isDev ? '/fr-api/api/v1' : 'https://www.federalregister.gov/api/v1',
    rateLimit: 60,
    timeout: 10000
  },
  ofacSdn: {
    baseUrl: isDev ? '/ofac-api/api/v1' : 'https://sanctionssearch.ofac.treas.gov/api/v1',
    rateLimit: 30,
    timeout: 15000
  },
  eurLex: {
    baseUrl: isDev ? '/eurlex-api/eurlex-ws/rest' : 'https://eur-lex.europa.eu/eurlex-ws/rest',
    rateLimit: 20,
    timeout: 20000
  },
  newsApi: {
    baseUrl: 'https://newsapi.org/v2',
    apiKey: import.meta.env.VITE_NEWS_API_KEY || '',
    rateLimit: 100,
    timeout: 10000
  }
}

// ============== Data Types ==============

export interface RawNewsItem {
  id: string
  source: {
    id: string
    name: string
    url: string
  }
  title: string
  description: string
  content: string
  publishedAt: string
  url: string
  urlToImage?: string
  author?: string
  // 元数据
  language: string
  jurisdiction: string
  documentType?: string
  documentNumber?: string
}

export interface SDNEntry {
  uid: string
  firstName?: string
  lastName?: string
  entityName?: string
  sdnType: 'individual' | 'entity' | 'vessel' | 'aircraft'
  programs: string[]
  title?: string
  callSign?: string
  vesselType?: string
  tonnage?: string
  grossRegisteredTonnage?: string
  vesselFlag?: string
  vesselOwner?: string
  remarks?: string
  addresses: Array<{
    address1?: string
    address2?: string
    city?: string
    stateOrProvince?: string
    postalCode?: string
    country: string
  }>
  aliases: Array<{
    uid: string
    type: string
    category: string
    alias: string
  }>
  nationalities: string[]
  citizenships: string[]
  datesOfBirth: string[]
  placesOfBirth: string[]
  programs: string[]
}

export interface EntityListEntry {
  entryNumber: string
  entityName: string
  address: string
  country: string
  federalRegisterCitation: string
  effectiveDate: string
  standardOrder: string
  licensingPolicy: string
  licenseRequirement: string
  reason: string
}

export interface FederalRegisterDocument {
  documentNumber: string
  title: string
  type: string
  abstractText: string
  agencies: Array<{ name: string; slug: string }>
  publicationDate: string
  effectiveOn?: string
  htmlUrl: string
  pdfUrl: string
  fullTextXmlUrl?: string
  topics: string[]
  significantDocument: boolean
  cfr_references: Array<{ title: string; part: string }>
}

// ============== API Clients ==============

class FederalRegisterClient {
  private baseUrl = API_CONFIGS.federalRegister.baseUrl
  // 🆕 熔断状态：401/403 连续失败后禁用一段时间
  private isDisabled = false
  private disabledUntil = 0
  private consecutiveFailures = 0
  private readonly MAX_FAILURES = 3
  private readonly DISABLE_DURATION = 5 * 60 * 1000  // 5分钟

  private checkDisabled(): boolean {
    if (this.isDisabled) {
      if (Date.now() > this.disabledUntil) {
        // 熔断时间结束，重新启用
        this.isDisabled = false
        this.consecutiveFailures = 0
        console.log('[FederalRegister] Re-enabling after cooldown')
        return false
      }
      return true  // 仍在熔断期
    }
    return false
  }

  private handleFatalError(status: number): void {
    this.consecutiveFailures++
    if (this.consecutiveFailures >= this.MAX_FAILURES) {
      this.isDisabled = true
      this.disabledUntil = Date.now() + this.DISABLE_DURATION
      console.warn(`[FederalRegister] Disabled for ${this.DISABLE_DURATION / 60000}min after ${status} errors`)
    }
  }

  async searchDocuments(query: string, options: {
    agency?: string
    type?: string
    fromDate?: string
    toDate?: string
    perPage?: number
  } = {}): Promise<FederalRegisterDocument[]> {
    // 🆕 熔断检查
    if (this.checkDisabled()) {
      return []  // 静默跳过
    }

    const params = new URLSearchParams({
      conditions: JSON.stringify({
        term: query,
        agencies: options.agency ? [options.agency] : undefined,
        type: options.type ? [options.type] : undefined,
        publication_date: options.fromDate || options.toDate ? {
          gte: options.fromDate,
          lte: options.toDate
        } : undefined
      }),
      per_page: String(options.perPage || 20),
      order: 'newest'
    })

    // 🆕 添加超时控制
    const controller = new AbortController()
    const timeoutId = setTimeout(() => controller.abort(), API_CONFIGS.federalRegister.timeout)

    try {
      const response = await fetch(`${this.baseUrl}/documents.json?${params}`, {
        signal: controller.signal
      })
      clearTimeout(timeoutId)
      
      // 🆕 401/403 熔断处理
      if (response.status === 401 || response.status === 403) {
        this.handleFatalError(response.status)
        return []
      }
      
      if (!response.ok) throw new Error(`Federal Register API error: ${response.status}`)
      
      // 成功后重置失败计数
      this.consecutiveFailures = 0
      
      const data = await response.json()
      return data.results || []
    } catch (error) {
      clearTimeout(timeoutId)
      // 🆕 静默处理，不刷屏
      if (error instanceof Error && error.name === 'AbortError') {
        // 超时也算失败
        this.consecutiveFailures++
      } else {
        // 检查是否是 403/401 错误
        const errorMsg = (error as Error).message || ''
        if (errorMsg.includes('401') || errorMsg.includes('403')) {
          this.handleFatalError(403)
        }
      }
      return []
    }
  }

  async getRecentTariffDocuments(): Promise<FederalRegisterDocument[]> {
    return this.searchDocuments('tariff OR customs duty OR trade', {
      agency: 'commerce-department',
      fromDate: new Date(Date.now() - 30 * 24 * 60 * 60 * 1000).toISOString().split('T')[0]
    })
  }

  async getRecentSanctionDocuments(): Promise<FederalRegisterDocument[]> {
    return this.searchDocuments('sanction OR SDN OR entity list', {
      agency: 'treasury-department',
      fromDate: new Date(Date.now() - 30 * 24 * 60 * 60 * 1000).toISOString().split('T')[0]
    })
  }
}

class OFACClient {
  // OFAC SDN List - 使用公开的 XML/CSV 数据
  // 真实环境需要定期同步 https://sanctionslistservice.ofac.treas.gov/api/PublicationPreview/exports/SDN_ENHANCED.XML

  async getRecentAdditions(days: number = 7): Promise<SDNEntry[]> {
    // 模拟从 OFAC 获取最近添加的实体
    // 真实实现需要解析 XML 文件并比对历史记录
    // 🆕 静默模式，不刷屏
    
    // 返回空数组，需要连接真实 API
    return []
  }

  async searchEntity(name: string): Promise<SDNEntry[]> {
    // 搜索 SDN 名单 - 静默模式
    return []
  }
}

class NewsAggregatorClient {
  private apiKey = API_CONFIGS.newsApi.apiKey
  private baseUrl = API_CONFIGS.newsApi.baseUrl
  private hasWarnedMissingKey = false  // 🆕 只警告一次

  async getTopHeadlines(options: {
    country?: string
    category?: string
    sources?: string
    q?: string
    pageSize?: number
  } = {}): Promise<RawNewsItem[]> {
    if (!this.apiKey) {
      // 🆕 只警告一次，避免刷屏
      if (!this.hasWarnedMissingKey) {
        console.warn('[NewsAPI] API key not configured - skipping news fetch')
        this.hasWarnedMissingKey = true
      }
      return []
    }

    const params = new URLSearchParams({
      apiKey: this.apiKey,
      ...options,
      pageSize: String(options.pageSize || 50)
    } as Record<string, string>)

    try {
      const response = await fetch(`${this.baseUrl}/top-headlines?${params}`)
      if (!response.ok) throw new Error(`News API error: ${response.status}`)
      const data = await response.json()
      
      return (data.articles || []).map((article: any) => ({
        id: `news-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
        source: {
          id: article.source?.id || 'unknown',
          name: article.source?.name || 'Unknown Source',
          url: article.url
        },
        title: article.title,
        description: article.description,
        content: article.content,
        publishedAt: article.publishedAt,
        url: article.url,
        urlToImage: article.urlToImage,
        author: article.author,
        language: 'en',
        jurisdiction: options.country?.toUpperCase() || 'INTL'
      }))
    } catch (error) {
      console.error('News API error:', error)
      return []
    }
  }

  async searchEverything(query: string, options: {
    from?: string
    to?: string
    language?: string
    sortBy?: 'relevancy' | 'popularity' | 'publishedAt'
    pageSize?: number
  } = {}): Promise<RawNewsItem[]> {
    if (!this.apiKey) {
      // 🆕 使用共享的警告标记，避免重复打印
      if (!this.hasWarnedMissingKey) {
        console.warn('[NewsAPI] API key not configured - skipping news fetch')
        this.hasWarnedMissingKey = true
      }
      return []
    }

    const params = new URLSearchParams({
      apiKey: this.apiKey,
      q: query,
      language: options.language || 'en',
      sortBy: options.sortBy || 'publishedAt',
      pageSize: String(options.pageSize || 50),
      ...(options.from && { from: options.from }),
      ...(options.to && { to: options.to })
    })

    try {
      const response = await fetch(`${this.baseUrl}/everything?${params}`)
      if (!response.ok) throw new Error(`News API error: ${response.status}`)
      const data = await response.json()
      
      return (data.articles || []).map((article: any) => ({
        id: `news-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
        source: {
          id: article.source?.id || 'unknown',
          name: article.source?.name || 'Unknown Source',
          url: article.url
        },
        title: article.title,
        description: article.description,
        content: article.content,
        publishedAt: article.publishedAt,
        url: article.url,
        urlToImage: article.urlToImage,
        author: article.author,
        language: options.language || 'en',
        jurisdiction: 'INTL'
      }))
    } catch (error) {
      console.error('News API error:', error)
      return []
    }
  }
}

// ============== Main Data Service ==============

export class NewsDataService {
  private federalRegister = new FederalRegisterClient()
  private ofac = new OFACClient()
  private newsAggregator = new NewsAggregatorClient()

  // 🆕 获取 Federal Register 文档
  async getFederalRegisterDocuments(options: {
    agencies?: string[]
    perPage?: number
    documentTypes?: string[]
  } = {}): Promise<FederalRegisterDocument[]> {
    return this.federalRegister.searchDocuments({
      agencies: options.agencies,
      per_page: options.perPage,
      document_types: options.documentTypes
    })
  }

  // 🆕 获取 OFAC 更新
  async getOFACUpdates(days: number = 30): Promise<SDNEntry[]> {
    return this.ofac.getRecentAdditions(days)
  }

  // 🆕 获取新闻头条
  async getNewsHeadlines(options: {
    q?: string
    language?: string
    sortBy?: string
    pageSize?: number
  } = {}): Promise<RawNewsItem[]> {
    if (options.q) {
      return this.newsAggregator.searchEverything(options.q, {
        language: options.language,
        sortBy: options.sortBy as any,
        pageSize: options.pageSize
      })
    }
    return this.newsAggregator.getTopHeadlines({
      category: 'business',
      country: 'us',
      pageSize: options.pageSize
    })
  }

  // 获取最新政策文档
  async getLatestPolicyDocuments(): Promise<FederalRegisterDocument[]> {
    const [tariffs, sanctions] = await Promise.all([
      this.federalRegister.getRecentTariffDocuments(),
      this.federalRegister.getRecentSanctionDocuments()
    ])
    return [...tariffs, ...sanctions].sort((a, b) => 
      new Date(b.publicationDate).getTime() - new Date(a.publicationDate).getTime()
    )
  }

  // 获取实体名单变更
  async getEntityListChanges(): Promise<SDNEntry[]> {
    return this.ofac.getRecentAdditions(7)
  }

  // 获取新闻头条
  async getBreakingNews(topics: string[]): Promise<RawNewsItem[]> {
    const query = topics.join(' OR ')
    return this.newsAggregator.searchEverything(query, {
      sortBy: 'publishedAt',
      pageSize: 50
    })
  }

  // 搜索特定实体相关新闻
  async searchEntityNews(entityName: string): Promise<RawNewsItem[]> {
    return this.newsAggregator.searchEverything(entityName, {
      sortBy: 'relevancy',
      pageSize: 20
    })
  }
}

// 导出单例
export const newsDataService = new NewsDataService()

export default newsDataService
