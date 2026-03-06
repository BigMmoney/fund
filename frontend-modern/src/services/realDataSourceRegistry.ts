/**
 * Real Data Source Registry
 * 
 * 真实数据源注册表 - 基于实际可用的官方API
 * 
 * 设计原则：
 * 1. 优先接入官方API（最稳定权威）
 * 2. 社交类用第三方（需要降级策略）
 * 3. 融合多源+AI分析
 */

// ============== Types ==============

export type ConnectionStatus = 'connected' | 'pending' | 'error' | 'rate-limited' | 'not-configured'
export type DataSourceTier = 'official' | 'semi-official' | 'third-party' | 'experimental'
export type UpdateFrequency = 'realtime' | 'minutes' | 'hourly' | 'daily' | 'on-demand'

export interface RealDataSource {
  id: string
  name: string
  nameZh: string
  tier: DataSourceTier
  
  // API 信息
  hasOfficialAPI: boolean
  apiEndpoint?: string
  apiKeyRequired: boolean
  isFree: boolean
  
  // 数据特性
  updateFrequency: UpdateFrequency
  supportsStreaming: boolean
  supportsRealtime: boolean
  
  // 数据类型
  dataTypes: ('policy' | 'regulation' | 'social' | 'market' | 'economic' | 'sanction')[]
  jurisdictions: ('US' | 'EU' | 'UK' | 'CN' | 'INTL')[]
  
  // 状态
  status: ConnectionStatus
  lastCheck?: string
  errorMessage?: string
  
  // 优先级
  priority: number  // 1-100, 越高越优先
  reliability: number  // 0-100, 可靠性评分
  
  // 备注
  notes: string
  fallbackSourceId?: string
}

// ============== 官方API数据源 (Tier 1: Official) ==============

export const OFFICIAL_API_SOURCES: RealDataSource[] = [
  {
    id: 'regulations-gov',
    name: 'Regulations.gov',
    nameZh: '美国法规网',
    tier: 'official',
    hasOfficialAPI: true,
    apiEndpoint: 'https://api.regulations.gov/v4',
    apiKeyRequired: true,
    isFree: true,
    updateFrequency: 'realtime',
    supportsStreaming: false,
    supportsRealtime: true,
    dataTypes: ['policy', 'regulation'],
    jurisdictions: ['US'],
    status: 'pending',
    priority: 95,
    reliability: 99,
    notes: '美国联邦法规原文+更新历史，最权威的政策来源'
  },
  {
    id: 'federal-register-api',
    name: 'Federal Register API',
    nameZh: '联邦公报API',
    tier: 'official',
    hasOfficialAPI: true,
    apiEndpoint: 'https://www.federalregister.gov/api/v1',
    apiKeyRequired: false,
    isFree: true,
    updateFrequency: 'hourly',
    supportsStreaming: false,
    supportsRealtime: false,
    dataTypes: ['policy', 'regulation'],
    jurisdictions: ['US'],
    status: 'connected',
    priority: 98,
    reliability: 99,
    notes: '美国官方公报，每日更新'
  },
  {
    id: 'api-data-gov',
    name: 'api.data.gov',
    nameZh: '美国数据开放平台',
    tier: 'official',
    hasOfficialAPI: true,
    apiEndpoint: 'https://api.data.gov',
    apiKeyRequired: true,
    isFree: true,
    updateFrequency: 'daily',
    supportsStreaming: false,
    supportsRealtime: false,
    dataTypes: ['economic', 'policy'],
    jurisdictions: ['US'],
    status: 'pending',
    priority: 75,
    reliability: 95,
    notes: '美国政府开放数据集，可用于趋势/统计分析'
  },
  {
    id: 'data-europa-eu',
    name: 'data.europa.eu',
    nameZh: '欧盟数据开放平台',
    tier: 'official',
    hasOfficialAPI: true,
    apiEndpoint: 'https://data.europa.eu/api/hub/search',
    apiKeyRequired: false,
    isFree: true,
    updateFrequency: 'daily',
    supportsStreaming: false,
    supportsRealtime: false,
    dataTypes: ['policy', 'regulation', 'economic'],
    jurisdictions: ['EU'],
    status: 'pending',
    priority: 85,
    reliability: 95,
    notes: '欧盟官方数据平台'
  },
  {
    id: 'data-gov-uk',
    name: 'data.gov.uk',
    nameZh: '英国数据开放平台',
    tier: 'official',
    hasOfficialAPI: true,
    apiEndpoint: 'https://data.gov.uk/api',
    apiKeyRequired: false,
    isFree: true,
    updateFrequency: 'daily',
    supportsStreaming: false,
    supportsRealtime: false,
    dataTypes: ['policy', 'regulation', 'economic'],
    jurisdictions: ['UK'],
    status: 'pending',
    priority: 80,
    reliability: 95,
    notes: '英国政府开放数据'
  },
  {
    id: 'loc-congress',
    name: 'Library of Congress',
    nameZh: '美国国会图书馆',
    tier: 'official',
    hasOfficialAPI: true,
    apiEndpoint: 'https://www.loc.gov/apis',
    apiKeyRequired: false,
    isFree: true,
    updateFrequency: 'realtime',
    supportsStreaming: false,
    supportsRealtime: true,
    dataTypes: ['policy'],
    jurisdictions: ['US'],
    status: 'pending',
    priority: 85,
    reliability: 99,
    notes: '国会立法动态，法案追踪'
  },
  {
    id: 'treasury-ofac',
    name: 'OFAC Sanctions List',
    nameZh: 'OFAC制裁名单',
    tier: 'official',
    hasOfficialAPI: true,
    apiEndpoint: 'https://www.treasury.gov/ofac/downloads',
    apiKeyRequired: false,
    isFree: true,
    updateFrequency: 'daily',
    supportsStreaming: false,
    supportsRealtime: false,
    dataTypes: ['sanction'],
    jurisdictions: ['US'],
    status: 'connected',
    priority: 95,
    reliability: 99,
    notes: 'SDN名单，制裁实体列表'
  },
  {
    id: 'eur-lex',
    name: 'EUR-Lex',
    nameZh: '欧盟法律数据库',
    tier: 'official',
    hasOfficialAPI: true,
    apiEndpoint: 'https://eur-lex.europa.eu/eurlex-ws',
    apiKeyRequired: false,
    isFree: true,
    updateFrequency: 'daily',
    supportsStreaming: false,
    supportsRealtime: false,
    dataTypes: ['policy', 'regulation'],
    jurisdictions: ['EU'],
    status: 'connected',
    priority: 90,
    reliability: 98,
    notes: '欧盟法律法规数据库'
  }
]

// ============== 半官方数据源 (Tier 2: Semi-Official) ==============

export const SEMI_OFFICIAL_SOURCES: RealDataSource[] = [
  {
    id: 'fed-speech',
    name: 'Fed Speeches & Statements',
    nameZh: '美联储讲话',
    tier: 'semi-official',
    hasOfficialAPI: false,
    apiEndpoint: 'https://www.federalreserve.gov/feeds',
    apiKeyRequired: false,
    isFree: true,
    updateFrequency: 'hourly',
    supportsStreaming: false,
    supportsRealtime: false,
    dataTypes: ['policy'],
    jurisdictions: ['US'],
    status: 'connected',
    priority: 92,
    reliability: 95,
    notes: 'RSS Feed，需要解析'
  },
  {
    id: 'ecb-press',
    name: 'ECB Press Releases',
    nameZh: '欧央行新闻',
    tier: 'semi-official',
    hasOfficialAPI: false,
    apiEndpoint: 'https://www.ecb.europa.eu/rss',
    apiKeyRequired: false,
    isFree: true,
    updateFrequency: 'hourly',
    supportsStreaming: false,
    supportsRealtime: false,
    dataTypes: ['policy'],
    jurisdictions: ['EU'],
    status: 'connected',
    priority: 90,
    reliability: 95,
    notes: 'RSS Feed'
  },
  {
    id: 'boe-press',
    name: 'Bank of England',
    nameZh: '英格兰银行',
    tier: 'semi-official',
    hasOfficialAPI: false,
    isFree: true,
    updateFrequency: 'hourly',
    supportsStreaming: false,
    supportsRealtime: false,
    dataTypes: ['policy'],
    jurisdictions: ['UK'],
    status: 'connected',
    priority: 88,
    reliability: 95,
    notes: 'RSS Feed'
  },
  {
    id: 'pboc',
    name: 'PBoC Announcements',
    nameZh: '中国人民银行',
    tier: 'semi-official',
    hasOfficialAPI: false,
    isFree: true,
    updateFrequency: 'daily',
    supportsStreaming: false,
    supportsRealtime: false,
    dataTypes: ['policy'],
    jurisdictions: ['CN'],
    status: 'pending',
    priority: 88,
    reliability: 90,
    notes: '需要页面解析，无官方API'
  },
  {
    id: 'mofcom',
    name: 'MOFCOM China',
    nameZh: '商务部',
    tier: 'semi-official',
    hasOfficialAPI: false,
    isFree: true,
    updateFrequency: 'daily',
    supportsStreaming: false,
    supportsRealtime: false,
    dataTypes: ['policy', 'regulation'],
    jurisdictions: ['CN'],
    status: 'pending',
    priority: 85,
    reliability: 88,
    notes: '需要页面解析'
  }
]

// ============== 第三方数据源 (Tier 3: Third-Party) ==============

export const THIRD_PARTY_SOURCES: RealDataSource[] = [
  {
    id: 'truth-social',
    name: 'Truth Social (via 3rd party)',
    nameZh: 'Truth Social（第三方）',
    tier: 'third-party',
    hasOfficialAPI: false,
    apiKeyRequired: true,
    isFree: false,
    updateFrequency: 'realtime',
    supportsStreaming: true,
    supportsRealtime: true,
    dataTypes: ['social'],
    jurisdictions: ['US'],
    status: 'not-configured',
    priority: 75,
    reliability: 70,
    notes: '⚠️ 无官方API，需使用第三方服务（如 ScrapeCreators），可能有访问限制',
    fallbackSourceId: 'whitehouse-press'
  },
  {
    id: 'newsapi',
    name: 'NewsAPI',
    nameZh: '新闻聚合API',
    tier: 'third-party',
    hasOfficialAPI: true,
    apiEndpoint: 'https://newsapi.org/v2',
    apiKeyRequired: true,
    isFree: false,
    updateFrequency: 'minutes',
    supportsStreaming: false,
    supportsRealtime: true,
    dataTypes: ['market'],
    jurisdictions: ['US', 'EU', 'UK', 'INTL'],
    status: 'connected',
    priority: 70,
    reliability: 85,
    notes: '新闻聚合，需付费订阅获得更高限额'
  },
  {
    id: 'reuters-api',
    name: 'Reuters Wire',
    nameZh: '路透社',
    tier: 'third-party',
    hasOfficialAPI: true,
    apiKeyRequired: true,
    isFree: false,
    updateFrequency: 'realtime',
    supportsStreaming: true,
    supportsRealtime: true,
    dataTypes: ['market'],
    jurisdictions: ['INTL'],
    status: 'not-configured',
    priority: 88,
    reliability: 95,
    notes: '需要企业订阅'
  },
  {
    id: 'bloomberg-api',
    name: 'Bloomberg Terminal API',
    nameZh: '彭博终端API',
    tier: 'third-party',
    hasOfficialAPI: true,
    apiKeyRequired: true,
    isFree: false,
    updateFrequency: 'realtime',
    supportsStreaming: true,
    supportsRealtime: true,
    dataTypes: ['market', 'economic'],
    jurisdictions: ['INTL'],
    status: 'not-configured',
    priority: 90,
    reliability: 98,
    notes: '需要Bloomberg Terminal订阅'
  }
]

// ============== 全部数据源 ==============

export const ALL_DATA_SOURCES: RealDataSource[] = [
  ...OFFICIAL_API_SOURCES,
  ...SEMI_OFFICIAL_SOURCES,
  ...THIRD_PARTY_SOURCES
]

// ============== 辅助函数 ==============

export const getSourcesByStatus = (status: ConnectionStatus): RealDataSource[] => {
  return ALL_DATA_SOURCES.filter(s => s.status === status)
}

export const getSourcesByTier = (tier: DataSourceTier): RealDataSource[] => {
  return ALL_DATA_SOURCES.filter(s => s.tier === tier)
}

export const getSourcesByJurisdiction = (jurisdiction: string): RealDataSource[] => {
  return ALL_DATA_SOURCES.filter(s => s.jurisdictions.includes(jurisdiction as any))
}

export const getConnectedSources = (): RealDataSource[] => {
  return ALL_DATA_SOURCES.filter(s => s.status === 'connected')
}

export const getPendingSources = (): RealDataSource[] => {
  return ALL_DATA_SOURCES.filter(s => s.status === 'pending' || s.status === 'not-configured')
}

export const getSourceStats = () => {
  const connected = getConnectedSources().length
  const pending = getPendingSources().length
  const total = ALL_DATA_SOURCES.length
  
  const byTier = {
    official: OFFICIAL_API_SOURCES.filter(s => s.status === 'connected').length,
    semiOfficial: SEMI_OFFICIAL_SOURCES.filter(s => s.status === 'connected').length,
    thirdParty: THIRD_PARTY_SOURCES.filter(s => s.status === 'connected').length
  }
  
  const byJurisdiction = {
    US: getSourcesByJurisdiction('US').filter(s => s.status === 'connected').length,
    EU: getSourcesByJurisdiction('EU').filter(s => s.status === 'connected').length,
    UK: getSourcesByJurisdiction('UK').filter(s => s.status === 'connected').length,
    CN: getSourcesByJurisdiction('CN').filter(s => s.status === 'connected').length,
    INTL: getSourcesByJurisdiction('INTL').filter(s => s.status === 'connected').length
  }
  
  return { connected, pending, total, byTier, byJurisdiction }
}

// ============== 数据源状态颜色 ==============

export const getStatusColor = (status: ConnectionStatus): string => {
  switch (status) {
    case 'connected': return 'bg-emerald-500'
    case 'pending': return 'bg-amber-500'
    case 'error': return 'bg-red-500'
    case 'rate-limited': return 'bg-orange-500'
    case 'not-configured': return 'bg-gray-400'
  }
}

export const getStatusLabel = (status: ConnectionStatus): string => {
  switch (status) {
    case 'connected': return '已连接'
    case 'pending': return '待接入'
    case 'error': return '错误'
    case 'rate-limited': return '限流中'
    case 'not-configured': return '未配置'
  }
}

export const getTierLabel = (tier: DataSourceTier): string => {
  switch (tier) {
    case 'official': return '官方API'
    case 'semi-official': return '半官方'
    case 'third-party': return '第三方'
    case 'experimental': return '实验性'
  }
}

export const getTierColor = (tier: DataSourceTier): string => {
  switch (tier) {
    case 'official': return 'bg-blue-100 text-blue-700'
    case 'semi-official': return 'bg-violet-100 text-violet-700'
    case 'third-party': return 'bg-amber-100 text-amber-700'
    case 'experimental': return 'bg-gray-100 text-gray-600'
  }
}
