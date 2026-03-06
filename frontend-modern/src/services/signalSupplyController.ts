/**
 * Signal Supply Controller
 * 
 * 信号供给可信度控制台
 * 
 * 核心职责：
 * 不是告诉你 API 活着没，而是告诉你：
 * - 哪些来源 可以驱动政策状态
 * - 哪些来源 只能提供早期信号
 * - 哪些来源 当前不应被模型采信
 * 
 * 这是"模型输入闸门"，不是"服务器状态页"
 */

// ============== Layer 1: Source Authority (权威层) ==============

/**
 * 来源权威等级
 * A = Legal / Regulatory Authority (法律/监管权威)
 * B = Official Communication (官方通讯)
 * C = Market / Media Aggregation (市场/媒体聚合)
 * D = Community / Unverified (社区/未验证)
 */
export type SourceAuthority = 'A' | 'B' | 'C' | 'D'

export const AUTHORITY_DEFINITIONS: Record<SourceAuthority, {
  label: string
  labelZh: string
  description: string
  descriptionZh: string
  canDriveState: boolean
  canValidate: boolean
  trustScore: number
}> = {
  A: {
    label: 'Legal/Regulatory Authority',
    labelZh: '法律/监管权威',
    description: 'Official government publications with legal force',
    descriptionZh: '具有法律效力的官方政府出版物',
    canDriveState: true,
    canValidate: true,
    trustScore: 100
  },
  B: {
    label: 'Official Communication',
    labelZh: '官方通讯',
    description: 'Official statements and press releases from authorities',
    descriptionZh: '来自权威机构的官方声明和新闻发布',
    canDriveState: true,
    canValidate: true,
    trustScore: 85
  },
  C: {
    label: 'Market/Media Aggregation',
    labelZh: '市场/媒体聚合',
    description: 'News agencies and market data providers',
    descriptionZh: '新闻机构和市场数据提供商',
    canDriveState: false,
    canValidate: true,
    trustScore: 65
  },
  D: {
    label: 'Community/Unverified',
    labelZh: '社区/未验证',
    description: 'Social media, forums, and unverified sources',
    descriptionZh: '社交媒体、论坛和未验证来源',
    canDriveState: false,
    canValidate: false,
    trustScore: 30
  }
}

// ============== Layer 2: Signal Role (信号角色层) ==============

/**
 * 信号角色
 * - State Driver: 能直接改变政策状态
 * - Validator: 用于确认/否认
 * - Early Indicator: 提前信号，不改变状态
 * - Context Provider: 背景信息
 */
export type SignalRole = 'state-driver' | 'validator' | 'early-indicator' | 'context-provider'

export const SIGNAL_ROLE_DEFINITIONS: Record<SignalRole, {
  label: string
  labelZh: string
  description: string
  descriptionZh: string
  canModifyState: boolean
  canModifyExpectation: boolean
  weight: number
}> = {
  'state-driver': {
    label: 'Policy State Driver',
    labelZh: '政策状态驱动',
    description: 'Can directly change policy state machine',
    descriptionZh: '可直接改变政策状态机',
    canModifyState: true,
    canModifyExpectation: true,
    weight: 1.0
  },
  'validator': {
    label: 'Signal Validator',
    labelZh: '信号验证器',
    description: 'Used to confirm or deny signals from drivers',
    descriptionZh: '用于确认或否认驱动器的信号',
    canModifyState: false,
    canModifyExpectation: true,
    weight: 0.7
  },
  'early-indicator': {
    label: 'Early Indicator',
    labelZh: '早期指标',
    description: 'Provides early signals, cannot change state',
    descriptionZh: '提供早期信号，不能改变状态',
    canModifyState: false,
    canModifyExpectation: true,
    weight: 0.4
  },
  'context-provider': {
    label: 'Context Provider',
    labelZh: '背景信息',
    description: 'Background information only',
    descriptionZh: '仅提供背景信息',
    canModifyState: false,
    canModifyExpectation: false,
    weight: 0.2
  }
}

// ============== Layer 3: Operational Health (运维层 - 降权显示) ==============

export type OperationalStatus = 'online' | 'degraded' | 'offline' | 'unknown'

export interface OperationalHealth {
  status: OperationalStatus
  lastUpdate: string | null
  freshnessMinutes: number | null
  hasError: boolean
  errorMessage?: string
}

// ============== Signal Source Definition ==============

export interface SignalSource {
  id: string
  name: string
  nameZh: string
  
  // Layer 1: Authority
  authority: SourceAuthority
  
  // Layer 2: Role
  role: SignalRole
  
  // Layer 3: Operational (降权)
  operational: OperationalHealth
  
  // 元数据
  jurisdiction: 'US' | 'EU' | 'UK' | 'CN' | 'INTL'
  domains: ('trade' | 'sanction' | 'rate' | 'fiscal' | 'regulation' | 'tech')[]
  
  // 决策权重
  decisionWeight: number  // 计算得出: authority.trustScore * role.weight
}

// ============== Signal Source Registry ==============

export const SIGNAL_SOURCES: SignalSource[] = [
  // ========== A-Level: Legal/Regulatory Authority ==========
  {
    id: 'federal-register',
    name: 'Federal Register',
    nameZh: '联邦公报',
    authority: 'A',
    role: 'state-driver',
    operational: { status: 'online', lastUpdate: new Date().toISOString(), freshnessMinutes: 30, hasError: false },
    jurisdiction: 'US',
    domains: ['trade', 'sanction', 'regulation', 'fiscal'],
    decisionWeight: 100
  },
  {
    id: 'ofac-sdn',
    name: 'OFAC SDN List',
    nameZh: 'OFAC制裁名单',
    authority: 'A',
    role: 'state-driver',
    operational: { status: 'online', lastUpdate: new Date().toISOString(), freshnessMinutes: 60, hasError: false },
    jurisdiction: 'US',
    domains: ['sanction'],
    decisionWeight: 100
  },
  {
    id: 'bis-entity-list',
    name: 'BIS Entity List',
    nameZh: 'BIS实体清单',
    authority: 'A',
    role: 'state-driver',
    operational: { status: 'online', lastUpdate: new Date().toISOString(), freshnessMinutes: 120, hasError: false },
    jurisdiction: 'US',
    domains: ['sanction', 'trade', 'tech'],
    decisionWeight: 100
  },
  {
    id: 'eur-lex',
    name: 'EUR-Lex',
    nameZh: '欧盟法律数据库',
    authority: 'A',
    role: 'state-driver',
    operational: { status: 'online', lastUpdate: new Date().toISOString(), freshnessMinutes: 60, hasError: false },
    jurisdiction: 'EU',
    domains: ['trade', 'sanction', 'regulation'],
    decisionWeight: 100
  },
  {
    id: 'mofcom',
    name: 'MOFCOM China',
    nameZh: '商务部',
    authority: 'A',
    role: 'state-driver',
    operational: { status: 'degraded', lastUpdate: null, freshnessMinutes: null, hasError: false },
    jurisdiction: 'CN',
    domains: ['trade', 'sanction'],
    decisionWeight: 100
  },
  {
    id: 'ec-trade-dg',
    name: 'EC Trade DG',
    nameZh: '欧委会贸易总司',
    authority: 'A',
    role: 'state-driver',
    operational: { status: 'online', lastUpdate: new Date().toISOString(), freshnessMinutes: 90, hasError: false },
    jurisdiction: 'EU',
    domains: ['trade'],
    decisionWeight: 100
  },
  
  // ========== B-Level: Official Communication ==========
  {
    id: 'fed-speeches',
    name: 'Fed Speeches',
    nameZh: '美联储讲话',
    authority: 'B',
    role: 'state-driver',
    operational: { status: 'online', lastUpdate: new Date().toISOString(), freshnessMinutes: 15, hasError: false },
    jurisdiction: 'US',
    domains: ['rate', 'fiscal'],
    decisionWeight: 85
  },
  {
    id: 'ecb-press',
    name: 'ECB Press',
    nameZh: '欧央行新闻',
    authority: 'B',
    role: 'state-driver',
    operational: { status: 'online', lastUpdate: new Date().toISOString(), freshnessMinutes: 30, hasError: false },
    jurisdiction: 'EU',
    domains: ['rate', 'fiscal'],
    decisionWeight: 85
  },
  {
    id: 'whitehouse',
    name: 'White House',
    nameZh: '白宫',
    authority: 'B',
    role: 'state-driver',
    operational: { status: 'online', lastUpdate: new Date().toISOString(), freshnessMinutes: 10, hasError: false },
    jurisdiction: 'US',
    domains: ['trade', 'sanction', 'fiscal'],
    decisionWeight: 85
  },
  {
    id: 'pboc',
    name: 'PBoC',
    nameZh: '中国人民银行',
    authority: 'B',
    role: 'state-driver',
    operational: { status: 'degraded', lastUpdate: null, freshnessMinutes: null, hasError: false },
    jurisdiction: 'CN',
    domains: ['rate', 'fiscal'],
    decisionWeight: 85
  },
  
  // ========== C-Level: Market/Media ==========
  {
    id: 'reuters',
    name: 'Reuters',
    nameZh: '路透社',
    authority: 'C',
    role: 'validator',
    operational: { status: 'online', lastUpdate: new Date().toISOString(), freshnessMinutes: 5, hasError: false },
    jurisdiction: 'INTL',
    domains: ['trade', 'sanction', 'rate', 'fiscal'],
    decisionWeight: 45.5  // 65 * 0.7
  },
  {
    id: 'bloomberg',
    name: 'Bloomberg',
    nameZh: '彭博',
    authority: 'C',
    role: 'validator',
    operational: { status: 'online', lastUpdate: new Date().toISOString(), freshnessMinutes: 5, hasError: false },
    jurisdiction: 'INTL',
    domains: ['trade', 'sanction', 'rate', 'fiscal', 'tech'],
    decisionWeight: 45.5
  },
  {
    id: 'newsapi',
    name: 'NewsAPI',
    nameZh: '新闻聚合',
    authority: 'C',
    role: 'context-provider',
    operational: { status: 'online', lastUpdate: new Date().toISOString(), freshnessMinutes: 10, hasError: false },
    jurisdiction: 'INTL',
    domains: ['trade', 'tech'],
    decisionWeight: 13  // 65 * 0.2
  },
  
  // ========== D-Level: Community/Unverified ==========
  {
    id: 'truth-social',
    name: 'Truth Social',
    nameZh: 'Truth Social',
    authority: 'D',
    role: 'early-indicator',
    operational: { status: 'offline', lastUpdate: null, freshnessMinutes: null, hasError: true, errorMessage: 'No official API' },
    jurisdiction: 'US',
    domains: ['trade', 'sanction'],
    decisionWeight: 12  // 30 * 0.4
  },
  {
    id: 'community-watch',
    name: 'Community Watch',
    nameZh: '社区监控',
    authority: 'D',
    role: 'early-indicator',
    operational: { status: 'offline', lastUpdate: null, freshnessMinutes: null, hasError: false },
    jurisdiction: 'INTL',
    domains: ['trade', 'tech'],
    decisionWeight: 12
  }
]

// ============== Signal Supply Status ==============

export interface SignalSupplyStatus {
  // 顶部总览
  overallCredibility: 'high' | 'medium' | 'degraded' | 'critical'
  
  // 按角色统计
  stateDrivers: { online: number; total: number }
  validators: { online: number; total: number }
  earlyIndicators: { online: number; total: number }
  contextProviders: { online: number; total: number }
  
  // 模型输入状态
  modelInputStatus: {
    stateDrivingInputs: 'ok' | 'partial' | 'failed'
    validationInputs: 'ok' | 'partial' | 'failed'
    earlySignalNoiseLevel: 'low' | 'medium' | 'high'
  }
  
  // 决策系统建议
  decisionGuidance: {
    canMakeDecision: boolean
    confidenceLevel: number  // 0-100
    missingCriticalSources: string[]
    warnings: string[]
  }
}

// ============== Controller Class ==============

class SignalSupplyController {
  private sources: Map<string, SignalSource> = new Map()
  
  constructor() {
    SIGNAL_SOURCES.forEach(s => this.sources.set(s.id, s))
  }
  
  // 获取所有来源
  getSources(): SignalSource[] {
    return Array.from(this.sources.values())
  }
  
  // 按角色获取
  getSourcesByRole(role: SignalRole): SignalSource[] {
    return this.getSources().filter(s => s.role === role)
  }
  
  // 按权威等级获取
  getSourcesByAuthority(authority: SourceAuthority): SignalSource[] {
    return this.getSources().filter(s => s.authority === authority)
  }
  
  // 获取在线的状态驱动源
  getOnlineStateDrivers(): SignalSource[] {
    return this.getSources().filter(s => 
      s.role === 'state-driver' && 
      s.operational.status === 'online'
    )
  }
  
  // 获取在线的验证源
  getOnlineValidators(): SignalSource[] {
    return this.getSources().filter(s => 
      s.role === 'validator' && 
      s.operational.status === 'online'
    )
  }
  
  // 计算信号供给状态
  getSupplyStatus(): SignalSupplyStatus {
    const sources = this.getSources()
    
    const stateDrivers = sources.filter(s => s.role === 'state-driver')
    const validators = sources.filter(s => s.role === 'validator')
    const earlyIndicators = sources.filter(s => s.role === 'early-indicator')
    const contextProviders = sources.filter(s => s.role === 'context-provider')
    
    const onlineStateDrivers = stateDrivers.filter(s => s.operational.status === 'online')
    const onlineValidators = validators.filter(s => s.operational.status === 'online')
    
    // 计算整体可信度
    const stateDriverRatio = onlineStateDrivers.length / stateDrivers.length
    const validatorRatio = onlineValidators.length / validators.length
    
    let overallCredibility: SignalSupplyStatus['overallCredibility']
    if (stateDriverRatio >= 0.8 && validatorRatio >= 0.5) {
      overallCredibility = 'high'
    } else if (stateDriverRatio >= 0.5 && validatorRatio >= 0.3) {
      overallCredibility = 'medium'
    } else if (stateDriverRatio >= 0.3) {
      overallCredibility = 'degraded'
    } else {
      overallCredibility = 'critical'
    }
    
    // 模型输入状态
    const stateDrivingInputs = stateDriverRatio >= 0.8 ? 'ok' : stateDriverRatio >= 0.5 ? 'partial' : 'failed'
    const validationInputs = validatorRatio >= 0.8 ? 'ok' : validatorRatio >= 0.3 ? 'partial' : 'failed'
    
    // 早期信号噪音水平
    const dLevelOnline = sources.filter(s => s.authority === 'D' && s.operational.status === 'online').length
    const earlySignalNoiseLevel = dLevelOnline >= 3 ? 'high' : dLevelOnline >= 1 ? 'medium' : 'low'
    
    // 决策建议
    const missingCriticalSources = stateDrivers
      .filter(s => s.operational.status !== 'online' && s.authority === 'A')
      .map(s => s.name)
    
    const warnings: string[] = []
    if (stateDrivingInputs === 'partial') {
      warnings.push('部分状态驱动源离线，决策评分可能不完整')
    }
    if (validationInputs === 'failed') {
      warnings.push('验证源不足，无法交叉验证信号')
    }
    if (earlySignalNoiseLevel === 'high') {
      warnings.push('早期信号噪音较高，注意区分')
    }
    
    const canMakeDecision = stateDrivingInputs !== 'failed'
    const confidenceLevel = Math.round(
      (stateDriverRatio * 60 + validatorRatio * 30 + (earlySignalNoiseLevel === 'low' ? 10 : 5)) 
    )
    
    return {
      overallCredibility,
      stateDrivers: { online: onlineStateDrivers.length, total: stateDrivers.length },
      validators: { online: onlineValidators.length, total: validators.length },
      earlyIndicators: { 
        online: earlyIndicators.filter(s => s.operational.status === 'online').length, 
        total: earlyIndicators.length 
      },
      contextProviders: { 
        online: contextProviders.filter(s => s.operational.status === 'online').length, 
        total: contextProviders.length 
      },
      modelInputStatus: {
        stateDrivingInputs,
        validationInputs,
        earlySignalNoiseLevel
      },
      decisionGuidance: {
        canMakeDecision,
        confidenceLevel,
        missingCriticalSources,
        warnings
      }
    }
  }
  
  // 判断是否可以进行决策评分
  canCalculateDecisionScore(): { allowed: boolean; reason: string } {
    const status = this.getSupplyStatus()
    
    if (status.modelInputStatus.stateDrivingInputs === 'failed') {
      return { 
        allowed: false, 
        reason: '没有足够的A/B级来源在线，无法计算决策评分' 
      }
    }
    
    if (status.stateDrivers.online < 3) {
      return { 
        allowed: false, 
        reason: `仅有 ${status.stateDrivers.online} 个状态驱动源在线，需要至少 3 个` 
      }
    }
    
    return { allowed: true, reason: '信号供给充足' }
  }
  
  // 判断信号是否可以驱动状态变化
  canDriveStateChange(sourceId: string): boolean {
    const source = this.sources.get(sourceId)
    if (!source) return false
    
    const authorityDef = AUTHORITY_DEFINITIONS[source.authority]
    const roleDef = SIGNAL_ROLE_DEFINITIONS[source.role]
    
    return authorityDef.canDriveState && roleDef.canModifyState
  }
  
  // 获取信号的决策权重
  getDecisionWeight(sourceId: string): number {
    const source = this.sources.get(sourceId)
    if (!source) return 0
    
    // 如果离线，权重为0
    if (source.operational.status === 'offline') return 0
    
    // 如果降级，权重减半
    if (source.operational.status === 'degraded') return source.decisionWeight * 0.5
    
    return source.decisionWeight
  }
}

// ============== Singleton Export ==============

export const signalSupplyController = new SignalSupplyController()
export default signalSupplyController
