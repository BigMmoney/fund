import { useState, useEffect, useMemo, useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { GlobalNavbar } from '@/components/GlobalNavbar'
import LanguageSelector from '@/components/LanguageSelector'
import { 
  Globe, AlertTriangle, TrendingUp, TrendingDown, Clock, 
  Shield, Target, FileText, Search, RefreshCw,
  ChevronDown, ExternalLink, Zap, Activity,
  Building2, Landmark, Scale, Swords, DollarSign, Newspaper,
  GitBranch, List, AlertCircle, CheckCircle,
  Database, GitCompare, Layers,
  Plus, Minus, Edit3, Hash, XCircle, Circle,
  Bell, BellOff, BookmarkPlus, Bookmark, Settings, Download, Upload,
  HelpCircle
} from 'lucide-react'

// 🆕 Import Real Data Services
import { 
  newsDataService,
  alertService as realAlertService,
  documentService as realDocumentService,
  calculateDecisionScore as calculateBayesianScore,
  systemHealthService,
  executivePowerService,
  type DecisionResult,
  type Evidence as ScoringEvidence,
  type AlertRule,
  type Alert as ServiceAlert,
  type PolicyDocument,
  type DataSourceHealth,
  type SystemMetrics,
  type ExecutiveNews,
  type ExecutiveSource,
  type PowerGraphStats
} from '@/services'

// 🆕 Import Signal Supply Controller - 信号供给可信度控制台
import signalSupplyController, {
  type SignalSupplyStatus,
  type SignalSource,
  type SourceAuthority,
  type SignalRole,
  AUTHORITY_DEFINITIONS,
  SIGNAL_ROLE_DEFINITIONS
} from '@/services/signalSupplyController'

// 🆕 Import Keyboard Shortcuts and Persistence
import { useKeyboardShortcuts, createTabShortcuts, type KeyboardShortcut } from '@/hooks/useKeyboardShortcuts'
import { getNewsSettings, saveNewsSettings } from '@/services/persistenceService'

// 🆕 Import Safe Display Components and Math Utils
import { SafeNumber, SafePercentChange, ScoreDisplay, PolicyStateDisplay, SignalValue } from '@/components/SafeDisplay'
import { safeDivide, safePercentChange, clamp, formatPercentChange, sanitizeOutput } from '@/lib/safemath'
import { StableSignalLine, type SignalLineResult } from '@/services/stableSignalLine'
import { PolicyStateMachine, type StateContext } from '@/services/policyStateMachine'
import { useTimeWindow, TimeWindowSelector, type TimeWindow } from '@/contexts/TimeWindowContext'

// ============== 类型定义 ==============
type SourceLevel = 'L0' | 'L0.5' | 'L1' | 'L2'
type SourceTier = 'A' | 'B' | 'C'
type ImpactDirection = 'bullish' | 'bearish' | 'ambiguous'
type Domain = 'trade' | 'sanction' | 'war' | 'rate' | 'fiscal' | 'regulation' | 'export_control' | 'antitrust'

// ============== 🆕 SOURCE REGISTRY SYSTEM ==============

// 组织类型
type OrgType = 'government' | 'central_bank' | 'regulator' | 'multilateral' | 'news_agency' | 'think_tank' | 'industry_body' | 'market_data'

// 访问类型
type AccessType = 'RSS' | 'API' | 'HTML_SCRAPE' | 'PDF_PARSE' | 'MANUAL'

// Feed 类型 (按优先级排序)
type FeedType = 'lists_databases' | 'regulations_notices' | 'press_releases' | 'calendar_events' | 'speeches'

// 去重策略
type DedupStrategy = 'url_hash' | 'content_hash' | 'title_similarity' | 'entity_match'

// Source Feed 定义
interface SourceFeed {
  feedId: string
  feedType: FeedType
  feedName: string
  feedUrl: string
  accessType: AccessType
  updateFrequency: number        // 分钟
  priority: number               // 1-5, 1最�?
  parser: string                 // 解析器名�?
  isActive: boolean
  lastSuccessfulFetch?: string
  lastError?: string
}

// Source Health 健康度指�?
interface SourceHealth {
  fetchSuccessRate: number       // 抓取成功�?(0-1)
  avgLatencyMs: number           // 平均延迟 (毫秒)
  dedupRate: number              // 去重比例 (0-1)
  extractionSuccessRate: number  // 正文抽取成功�?(0-1)
  fieldCoverage: number          // 字段覆盖�?(0-1)
  falsePositiveRate: number      // 误报�?(0-1)
  lastHealthCheck: string
  healthScore: number            // 综合健康�?(0-100)
  status: 'healthy' | 'degraded' | 'down' | 'unknown'
}

// Source Registry 完整定义
interface SourceRegistryEntry {
  sourceId: string
  nameCn: string
  nameEn: string
  jurisdiction: Jurisdiction
  orgType: OrgType
  level: SourceLevel
  tier?: SourceTier
  sourceWeight: number           // 0-100
  
  // 访问配置
  canonicalUrl: string
  feeds: SourceFeed[]
  accessType: AccessType
  updateFrequency: number        // 主更新频�?(分钟)
  
  // 质量指标
  reliabilityScore: number       // 可靠性分�?(0-100)
  dedupStrategy: DedupStrategy
  
  // 域覆�?
  domains: Domain[]
  executionPower: number         // 执行权力 (0-100)
  authority: number              // 权威�?(0-100)
  
  // 健康�?
  health: SourceHealth
  
  // 元数�?
  region?: SourceRegion
  description?: string
  lastUpdate: string
  isActive: boolean
}

// ============== 🆕 NOISE GATE SYSTEM ==============

// 噪声等级
type NoiseLevel = 'signal' | 'noise' | 'archive_only'

// 触发条件类型
type TriggerType = 
  | 'effective_date'          // 生效日期变更
  | 'scope_change'            // 范围变更
  | 'list_change'             // 名单变动
  | 'stance_reversal'         // 口径反转
  | 'channel_upgrade'         // 渠道升级 (L1→L0.5→L0)
  | 'execution_confirmation'  // 执行确认
  | 'deadline_imminent'       // 截止日临�?

// Noise Gate 规则
interface NoiseGateRule {
  ruleId: string
  ruleName: string
  description: string
  triggerTypes: TriggerType[]
  minSourceLevel: SourceLevel
  minExecutionPower: number
  minReliabilityScore: number
  isActive: boolean
}

// Noise Gate 评估结果
interface NoiseGateResult {
  newsId: string
  noiseLevel: NoiseLevel
  triggeredRules: string[]
  passedGate: boolean
  shouldAlert: boolean
  shouldUpdateTopic: boolean
  archiveOnly: boolean
  reasoning: string
  evaluatedAt: string
}

// Noise Budget 状�?
interface NoiseGateBudget {
  dailyAlertLimit: number
  currentAlertCount: number
  remainingBudget: number
  
  // 按类型分�?
  budgetByType: {
    type: TriggerType
    limit: number
    used: number
  }[]
  
  // 抑制统计
  suppressedToday: number
  suppressedByRule: {
    ruleId: string
    count: number
  }[]
  
  // 重置时间
  resetAt: string
}

// ============== 🆕 EXECUTION TRACKING SYSTEM ==============

// 政策执行时间轴节点状�?
type TimelineNodeStatus = 'pending' | 'active' | 'completed' | 'current' | 'delayed' | 'cancelled' | 'modified' | 'skipped'

// 政策时间轴节点类�?
type TimelineNodeType = 'signal' | 'draft' | 'approval' | 'publication' | 'effective' | 'amendment' | 'extension' | 'reversal' | 'committee_review' | 'debate' | 'vote'

// 司法辖区
type Jurisdiction = 'US' | 'EU' | 'CN' | 'UK' | 'JP' | 'INTL'

// 官方发布渠道
type PublicationChannel = 'federal_register' | 'official_journal' | 'gazette' | 'gov_cn' | 'cabinet_order' | 'central_bank'

// ============== TIMELINE ENGINE ==============

interface TimelineNode {
  id: string
  type: TimelineNodeType
  status: TimelineNodeStatus
  jurisdiction: Jurisdiction
  
  // 时间信息
  expectedDate?: string           // 预期日期
  actualDate?: string             // 实际日期
  completedAt?: string            // 完成时间
  delayDays?: number              // 延迟天数
  
  // 来源证据
  sourceUrl: string
  sourceLevel: SourceLevel
  sourceName: string
  publicationChannel?: PublicationChannel
  documentNumber?: string         // �?Federal Register Doc No.
  
  // 内容
  title: string
  summary: string
  originalText: string            // 原文（可审计�?
  translatedText?: string         // 翻译
  
  // 变更追踪
  previousNodeId?: string         // 前一版本节点
  changeType?: 'scope_expanded' | 'scope_narrowed' | 'date_changed' | 'content_modified'
  changeSummary?: string
}

interface PolicyTimeline {
  policyId: string
  policyName: string
  jurisdiction: Jurisdiction
  domain: Domain
  hasExecutionAuthority?: boolean  // 是否有执行权限
  
  // 时间轴节点
  nodes: TimelineNode[]
  currentNode: TimelineNodeType
  nextExpectedNode?: TimelineNodeType
  
  // 状态
  isActive: boolean
  effectiveDate?: string
  expirationDate?: string
  
  // 风险指标
  delayRisk: number               // 0-1 概率
  reversalRisk: number            // 0-1 概率
  scopeChangeCount: number
  
  // 关联
  relatedPolicies: string[]
  affectedEntities: string[]
  affectedAssets: string[]
}

// ============== LIST DIFF ENGINE ==============

// 名单类型
type OfficialListType = 'sdn' | 'entity_list' | 'bis_unverified' | 'denied_persons' | 'tariff_schedule' | 'antitrust_target' | 'export_control' | 'aml_watchlist'

// 名单变动类型
type ListChangeType = 'ADD' | 'REMOVE' | 'MODIFY' | 'DELIST' | 'UPGRADE' | 'DOWNGRADE'

// 名单条目
interface ListEntry {
  id: string
  listType: OfficialListType
  jurisdiction: Jurisdiction
  
  // 实体信息
  entityName: string
  entityAliases: string[]
  entityType: 'individual' | 'company' | 'organization' | 'vessel' | 'aircraft' | 'other'
  country?: string
  
  // 名单详情
  addedDate: string
  lastModified: string
  reason: string
  legalBasis: string              // 法律依据
  sourceUrl: string
  
  // 影响评估
  industryImpact: string[]
  estimatedMarketImpact: 'high' | 'medium' | 'low'
}

// 名单变动事件
interface ListChangeEvent {
  id: string
  timestamp: string
  listType: OfficialListType
  jurisdiction: Jurisdiction
  
  // 变动详情
  changeType: ListChangeType
  entity: ListEntry
  previousState?: Partial<ListEntry>  // 修改前状�?
  
  // 来源
  sourceUrl: string
  sourceLevel: SourceLevel
  effectiveDate: string
  announcementDate: string
  
  // 影响分析
  affectedTickers: string[]
  suggestedExposure: {
    ticker: string
    direction: ImpactDirection
    confidence: number
    reasoning: string
  }[]
  
  // 双语
  originalText: string
  translatedText?: string
}

// 名单差异报告
interface ListDiffReport {
  reportId: string
  generatedAt: string
  listType: OfficialListType
  jurisdiction: Jurisdiction
  
  // 变动统计
  totalChanges: number
  additions: number
  removals: number
  modifications: number
  
  // 变动事件
  changes: ListChangeEvent[]
  
  // 市场影响
  highImpactChanges: ListChangeEvent[]
  suggestedBasket: {
    ticker: string
    weight: number
    direction: ImpactDirection
    reasoning: string
  }[]
}

// ============== EXECUTION POWER MAP ==============

// 执行权力等级
type ExecutionPowerLevel = 'full' | 'partial' | 'advisory' | 'none'

// 执行机构类型
type ExecutionAgencyType = 'treasury' | 'commerce' | 'central_bank' | 'regulator' | 'customs' | 'judiciary' | 'legislature' | 'executive'

// 执行权力节点
interface ExecutionPowerNode {
  agencyId: string
  agencyName: string
  agencyType: ExecutionAgencyType
  jurisdiction: Jurisdiction
  
  // 权力评估
  executionPower: ExecutionPowerLevel
  canIssueList: boolean           // 能否发布名单
  canEnforceRules: boolean        // 能否执行规则
  canImposePenalties: boolean     // 能否实施惩罚
  hasLegalAuthority: boolean      // 是否有法律授�?
  
  // 历史表现
  historicalEnforcementRate: number  // 历史执行�?(0-1)
  avgTimeToEnforce: number           // 平均执行时间（天�?
  
  // 当前状�?
  hasActiveStatement: boolean
  lastStatementDate?: string
  statementSummary?: string
}

// 执行权力图谱
interface ExecutionPowerMap {
  policyId: string
  policyName: string
  
  // 权力�?
  powerNodes: ExecutionPowerNode[]
  
  // 权力评估
  overallExecutionPower: number      // 0-100
  hasLegalBasis: boolean
  hasEffectiveDate: boolean
  hasListAuthority: boolean
  
  // 关键节点
  primaryEnforcer?: ExecutionPowerNode
  secondaryEnforcers: ExecutionPowerNode[]
  
  // 风险
  executionGaps: string[]
  potentialBlocks: string[]
}

// ============== POLICY VERSIONING ==============

// 政策版本
interface PolicyVersion {
  versionId: string
  versionNumber: string           // v1, v2, v3...
  policyId: string
  
  // 时间
  publishedAt: string
  effectiveFrom?: string
  effectiveUntil?: string
  
  // 内容
  title: string
  summary: string
  fullTextUrl: string
  originalText: string
  translatedText?: string
  
  // 来源
  sourceLevel: SourceLevel
  sourceName: string
  publicationChannel: PublicationChannel
  documentNumber?: string
  
  // 变更
  isInitial: boolean
  previousVersionId?: string
  changeType?: 'amendment' | 'extension' | 'narrowing' | 'expansion' | 'clarification' | 'reversal'
}

// 政策版本差异
interface PolicyVersionDiff {
  fromVersion: string
  toVersion: string
  
  // 变更摘要
  changesSummary: string
  changeType: 'amendment' | 'extension' | 'narrowing' | 'expansion' | 'clarification' | 'reversal'
  
  // 详细变更
  addedSections: string[]
  removedSections: string[]
  modifiedSections: {
    section: string
    before: string
    after: string
    significance: 'high' | 'medium' | 'low'
  }[]
  
  // 影响评估
  marketImpact: 'bullish' | 'bearish' | 'neutral'
  impactMagnitude: 'high' | 'medium' | 'low'
  affectedEntities: string[]
  affectedAssets: string[]
}

// 政策版本历史
interface PolicyVersionHistory {
  policyId: string
  policyName: string
  jurisdiction: Jurisdiction
  domain: Domain
  
  versions: PolicyVersion[]
  diffs: PolicyVersionDiff[]
  
  // 当前状�?
  currentVersion: string
  isActive: boolean
  nextExpectedChange?: string
}

// ============== JURISDICTION DIVERGENCE ==============

// 辖区状�?
interface JurisdictionState {
  jurisdiction: Jurisdiction
  policyState: PolicyState
  effectiveDate?: string
  enforcementLevel: 'full' | 'partial' | 'suspended' | 'none'
  lastUpdate: string
  sourceUrl: string
}

// 辖区冲突
interface JurisdictionDivergence {
  policyTopic: string
  divergenceId: string
  
  // 各辖区状�?
  states: JurisdictionState[]
  
  // 冲突分析
  divergenceType: 'timing' | 'scope' | 'enforcement' | 'reversal' | 'none'
  divergenceSeverity: 'high' | 'medium' | 'low'
  
  // 领先/落后分析
  leadingJurisdiction?: Jurisdiction
  laggingJurisdictions: Jurisdiction[]
  catchUpProbability: number       // 0-1
  estimatedCatchUpDays?: number
  
  // 历史相似案例
  historicalPrecedents: {
    caseId: string
    description: string
    outcome: string
    duration: number              // 天数
  }[]
  
  // 套利机会
  arbitrageWindow?: {
    startDate: string
    estimatedEndDate: string
    suggestedStrategy: string
    riskLevel: 'high' | 'medium' | 'low'
  }
}

// ============== IMMEDIATE ACTION SYSTEM ==============

// 紧急行动类�?
type ImmediateActionType = 'list_change' | 'effective_date' | 'stance_reversal' | 'timeline_change' | 'divergence_update' | 'version_change'

// 紧急行动项
interface ImmediateAction {
  id: string
  type: ImmediateActionType
  priority: 'critical' | 'high' | 'medium'
  
  // 内容
  title: string
  summary: string
  
  // 行动建议
  actionRequired: string
  deadline?: string
  
  // 影响
  affectedAssets: string[]
  suggestedDirection?: ImpactDirection
  
  // 来源
  sourceUrl: string
  sourceLevel: SourceLevel
  originalText: string
  translatedText?: string
  
  // 状�?
  createdAt: string
  acknowledgedAt?: string
  resolvedAt?: string
}

// ============== NOISE BUDGET ==============

interface NoiseBudget {
  dailyLimit: number
  currentUsed: number
  remaining: number
  
  // 按类型分�?
  byType: {
    type: ImmediateActionType
    limit: number
    used: number
  }[]
  
  // 降噪规则
  suppressionRules: {
    rule: string
    triggeredCount: number
  }[]
}

// ============== BILINGUAL EVIDENCE ==============

interface BilingualEvidence {
  id: string
  originalLanguage: 'en' | 'zh' | 'de' | 'fr' | 'other'
  
  // 原文
  originalText: string
  originalSpan: [number, number]
  
  // 翻译
  translatedText: string
  translatedSpan: [number, number]
  
  // 元数�?
  sourceUrl: string
  sourceLevel: SourceLevel
  isLegalText: boolean            // 法条/名单原文优先显示
  translationConfidence: number   // 翻译置信�?(0-1)
}

// ============== 🆕 PHASE 1: 决策完整性类�?==============

// 仓位制度 (Position Regime) - 基于DecisionScore
type PositionRegime = 'FULL' | 'STANDARD' | 'STARTER' | 'NO_TRADE'

// 时机评估 (Timing Assessment)
type TimingAssessment = 'EARLY' | 'OPTIMAL' | 'LATE'

// NO_TRADE 原因
type NoTradeReason = 'priced_in_high' | 'drift_high' | 'conflict_high' | 'weak_loop' | 'low_execution_power'

// System Stance 标签
type StanceTag = 'priced_in' | 'high_drift' | 'conflict' | 'execution_confirmed' | 'early_signal' | 'late_signal'

// 仓位制度接口
interface PositionRegimeInfo {
  regime: PositionRegime
  positionCap: number          // 0-1, 仓位上限
  label: string                // 中文标签
  color: string                // 显示颜色
  description: string          // 描述
}

// 时机评估接口
interface TimingInfo {
  timing: TimingAssessment
  pricingInIndex: number       // 0-100
  label: string
  color: string
  recommendation: string
}

// System Stance - 一句话结论
interface SystemStance {
  stanceText: string           // 完整的一句话结论
  directionalBias: 'LONG' | 'SHORT' | 'NEUTRAL' | 'AVOID'
  conviction: 'HIGH' | 'MEDIUM' | 'LOW' | 'NONE'
  mainRisk: string             // 主要风险
  stanceTags: StanceTag[]      // 标签列表
}

// NO_TRADE 可行动化
interface NoTradeInfo {
  reasons: NoTradeReason[]     // 最�?个原�?
  reasonLabels: string[]       // 原因中文说明
  nextAction: string           // 下一步建�?
  watchTriggers: string[]      // 观察触发条件
}

// ============== 🆕 PHASE 2: 组合现实性类�?==============

// 冲突解决�?
interface ConflictResolver {
  conflictDetected: boolean
  conflictItems: {
    topicId: string
    topicName: string
    direction: ImpactDirection
    score: number
  }[]
  netBias: number              // -1 to +1
  netConfidence: number        // 0-1
  topDrivers: {
    topicId: string
    topicName: string
    contribution: number       // 贡献百分�?
    direction: ImpactDirection
  }[]
  resolution: 'clear_long' | 'clear_short' | 'conflicted' | 'neutral'
}

// ============== 🆕 PHASE 3: 长期稳健性类�?==============

// 信号半衰�?
interface SignalHalfLife {
  halfLifeHours: number        // 信号半衰�?(小时)
  persistenceScore: number     // 持续性分�?(0-1.5, 作为DecisionScore乘数)
  decayRate: number            // 衰减�?
  estimatedRemainingLife: number // 预估剩余有效时间 (小时)
  persistenceLabel: string     // 持续性标�?
}

// 拥挤风险
interface CrowdingRisk {
  riskLevel: 'low' | 'medium' | 'high' | 'extreme'
  sameDirectionCount: number   // 同方向资产数�?
  fullRegimeCount: number      // FULL制度资产数量
  positionCapOverlay: number   // 风险覆盖后的仓位上限 (0-1)
  warningMessage: string
}

// 假信号审�?
interface FalseSignalAudit {
  // 高分无反应分�?
  highScoreNoReaction: {
    count: number
    rootCauses: ('priced_in' | 'execution_failed' | 'conflict' | 'coverage_gap')[]
  }
  // 大波动未捕捉分类
  bigMoveMissed: {
    count: number
    rootCauses: ('taxonomy_gap' | 'entity_mapping_gap' | 'source_gap')[]
  }
  // 审计指标
  totalSignals: number
  falsePositiveRate: number
  falseNegativeRate: number
  auditSummary: string
}

// ============== 🆕 周报/晨会输出 ==============

interface ReportOutput {
  type: 'morning' | 'weekly'
  generatedAt: string
  
  // 摘要
  executiveSummary: string
  
  // 持仓建议
  positionRecommendations: {
    ticker: string
    regime: PositionRegime
    direction: 'LONG' | 'SHORT' | 'NEUTRAL'
    conviction: string
    keyReason: string
  }[]
  
  // 风险提示
  riskAlerts: {
    type: string
    severity: 'high' | 'medium' | 'low'
    description: string
  }[]
  
  // 观察列表
  watchList: {
    ticker: string
    trigger: string
    expectedAction: string
  }[]
  
  // 市场验证回顾
  validationReview: {
    hitRate: number
    avgMove: number
    bestCall: string
    worstCall: string
  }
}

// ============== RESEARCH DELIVERY TYPES ==============

// Daily Decision Brief (1-page)
interface DailyBrief {
  date: string
  generatedAt: string
  
  // Section 1: Top Trades (max 5)
  topTrades: {
    rank: number
    ticker: string
    direction: 'LONG' | 'SHORT'
    regime: PositionRegime
    decisionScore: number
    stance: string
    timing: TimingAssessment
    pricingIn: number
    keyRisk: string
    topicId: string
  }[]
  
  // Section 2: Watchlist (signals approaching actionable)
  watchlist: {
    ticker: string
    currentScore: number
    triggerCondition: string
    expectedDirection: 'LONG' | 'SHORT'
    estimatedTimeToAction: string
  }[]
  
  // Section 3: Do Not Trade (with reasons)
  doNotTrade: {
    ticker: string
    reasons: string[]
    avoidUntil: string
  }[]
  
  // Section 4: Key Risks
  keyRisks: {
    risk: string
    severity: 'HIGH' | 'MEDIUM' | 'LOW'
    affectedAssets: string[]
    mitigationAction: string
  }[]
  
  // Section 5: What Changed (vs yesterday)
  whatChanged: {
    type: 'NEW_SIGNAL' | 'REGIME_CHANGE' | 'STATE_TRANSITION' | 'RISK_ESCALATION' | 'SIGNAL_EXPIRED'
    description: string
    impact: string
  }[]
}

// Policy Playbook mapped to State Engine
interface PolicyPlaybook {
  state: PolicyState
  playbook: {
    recommendedAction: string
    positionSizing: string
    entryTiming: string
    exitTriggers: string[]
    riskManagement: string
    historicalWinRate: number
    avgHoldingPeriod: string
    typicalPnL: string
  }
}

// IC Memo (Investment Committee)
interface ICMemo {
  id: string
  generatedAt: string
  topic: string
  
  // Header
  recommendation: 'INITIATE' | 'ADD' | 'REDUCE' | 'CLOSE' | 'NO_ACTION'
  conviction: 'HIGH' | 'MEDIUM' | 'LOW'
  targetAssets: string[]
  
  // Decision Metrics
  decisionScore: number
  positionRegime: PositionRegime
  timing: TimingAssessment
  pricingIn: number
  
  // Thesis (max 3 points)
  thesis: string[]
  
  // Evidence Chain
  evidenceChain: {
    level: SourceLevel
    source: string
    quote: string
    date: string
  }[]
  
  // Risks (max 3)
  risks: {
    risk: string
    probability: 'HIGH' | 'MEDIUM' | 'LOW'
    mitigation: string
  }[]
  
  // Invalidation Triggers
  invalidationTriggers: string[]
  
  // Position Sizing
  suggestedSize: string
  maxLoss: string
}

// ============== PERFORMANCE & EVOLUTION TYPES ==============

// Historical Replay Entry
interface HistoricalReplayEntry {
  timestamp: string
  topicId: string
  topicName: string
  
  // Snapshot at time T
  decisionScoreAtT: number
  regimeAtT: PositionRegime
  stateAtT: PolicyState
  timingAtT: TimingAssessment
  
  // Actual outcome
  actualMove24h: number
  actualMove7d: number
  directionCorrect: boolean
  
  // Configuration used
  configSnapshot: {
    domainSensitivity: Record<Domain, number>
    credibilityThresholds: Record<CredibilityGrade, number>
    actionTierThresholds: Record<ActionTier, number>
  }
}

// PnL Attribution
interface PnLAttribution {
  period: string
  totalPnL: number
  
  // By Domain
  byDomain: Record<Domain, {
    pnl: number
    contribution: number
    winRate: number
    avgMove: number
    signalCount: number
  }>
  
  // By State
  byState: Record<PolicyState, {
    pnl: number
    contribution: number
    winRate: number
    avgHoldingPeriod: number
  }>
  
  // By Timing
  byTiming: Record<TimingAssessment, {
    pnl: number
    contribution: number
    avgEntry: number
    avgExit: number
  }>
  
  // Best/Worst
  bestTrade: { ticker: string; pnl: number; reason: string }
  worstTrade: { ticker: string; pnl: number; reason: string }
}

// Weekly Self-Audit
interface WeeklyAudit {
  weekEnding: string
  
  // Performance Summary
  performance: {
    totalSignals: number
    actionableSignals: number
    executedSignals: number
    hitRate: number
    avgReturn: number
  }
  
  // Root Cause Classification
  falsePositives: {
    count: number
    causes: {
      cause: 'priced_in' | 'execution_failed' | 'conflict' | 'coverage_gap' | 'timing_late'
      count: number
      examples: string[]
    }[]
  }
  
  falseNegatives: {
    count: number
    causes: {
      cause: 'taxonomy_gap' | 'entity_mapping_gap' | 'source_gap' | 'threshold_too_high'
      count: number
      examples: string[]
    }[]
  }
  
  // Parameter Suggestions (human-approved)
  parameterSuggestions: {
    parameter: string
    currentValue: number
    suggestedValue: number
    rationale: string
    expectedImpact: string
    status: 'pending' | 'approved' | 'rejected'
  }[]
  
  // Action Items
  actionItems: {
    item: string
    priority: 'HIGH' | 'MEDIUM' | 'LOW'
    owner: string
    dueDate: string
  }[]
}

// ============== TRADING COLLABORATION TYPES ==============

// Execution-Ready Signal Packet (JSON export)
interface ExecutionPacket {
  packetId: string
  generatedAt: string
  expiresAt: string
  
  // Signal
  signal: {
    ticker: string
    direction: 'LONG' | 'SHORT'
    regime: PositionRegime
    decisionScore: number
    conviction: 'HIGH' | 'MEDIUM' | 'LOW'
  }
  
  // Sizing
  sizing: {
    positionCap: number
    suggestedNotional: string
    maxLossPercent: number
  }
  
  // Timing
  timing: {
    assessment: TimingAssessment
    pricingIn: number
    urgency: 'IMMEDIATE' | 'TODAY' | 'THIS_WEEK' | 'MONITOR'
  }
  
  // Validation
  validation: {
    credibilityGrade: CredibilityGrade
    directionHitRate: number
    avgMove: number
  }
  
  // Risks
  risks: string[]
  
  // Invalidation
  invalidation: {
    triggers: string[]
    stopLoss: string
  }
  
  // Source Reference
  sourceTopicId: string
  evidenceCount: number
}

// Portfolio Risk Overlay
interface PortfolioRiskOverlay {
  timestamp: string
  
  // Crowding Risk
  crowding: {
    level: 'LOW' | 'MEDIUM' | 'HIGH' | 'EXTREME'
    longCount: number
    shortCount: number
    concentrationRisk: number
    warning: string
  }
  
  // Correlation Risk
  correlation: {
    avgPairwiseCorrelation: number
    highlyCorrelatedPairs: { asset1: string; asset2: string; correlation: number }[]
    diversificationScore: number
  }
  
  // Domain Concentration
  domainConcentration: Record<Domain, {
    exposure: number
    riskContribution: number
    warning?: string
  }>
  
  // Overall Risk Score
  overallRiskScore: number
  maxRecommendedExposure: number
  actionRequired: string[]
}

// Cross-Validation (Options IV / Prediction Markets)
interface CrossValidation {
  ticker: string
  timestamp: string
  
  // Our Signal
  systemSignal: {
    direction: 'LONG' | 'SHORT'
    decisionScore: number
    expectedMove: number
  }
  
  // Options Market
  optionsData?: {
    impliedVolatility: number
    ivPercentile: number
    putCallRatio: number
    unusualActivity: boolean
    marketExpectation: 'HIGH_VOL' | 'LOW_VOL' | 'NEUTRAL'
  }
  
  // Prediction Markets
  predictionMarkets?: {
    platform: string
    eventDescription: string
    probability: number
    volume: number
  }[]
  
  // Validation Result
  validationResult: {
    aligned: boolean
    confidence: number
    discrepancy?: string
    recommendation: string
  }
}

// ============== Left Sidebar Alert Types ==============

interface RealTimeAlert {
  id: string
  timestamp: string
  type: 'REGIME_CHANGE' | 'NEW_L0' | 'STATE_TRANSITION' | 'RISK_ESCALATION' | 'SIGNAL_EXPIRED' | 'CONFLICT_DETECTED' | 'CROWDING_WARNING'
  severity: 'CRITICAL' | 'HIGH' | 'MEDIUM' | 'LOW'
  title: string
  message: string
  affectedAssets: string[]
  actionRequired: string
  read: boolean
}

// ============== 原有决策引擎核心类型 ==============

// 状态引�?- 7阶段状态机 (含EU Negotiating)
type PolicyState = 'emerging' | 'negotiating' | 'contested' | 'implementing' | 'digesting' | 'exhausted' | 'reversed'

// 来源区域 (Source Region)
type SourceRegion = 'US' | 'EU' | 'CN' | 'INTL'

// EU机构类型
type EUInstitution = 'commission' | 'council' | 'parliament' | 'ecb' | 'dg' | 'member_state' | 'media'

// EU信号类型
type EUSignalType = 'proposal' | 'draft' | 'implementing_act' | 'delegated_act' | 'guideline' | 'enforcement' | 'statement' | 'procedural'

// ============== EU SOURCE CAPSULE ARCHITECTURE ==============

interface SourceCapsule {
  sourceId: string
  institution: string
  unit?: string                    // e.g., DG TRADE, DG COMP
  region: SourceRegion
  role: 'agenda_setter' | 'rule_maker' | 'amplifier' | 'pressure_noise'
  signalTypes: EUSignalType[]
  defaultState: PolicyState        // 默认触发的政策状�?
  sourceWeight: number             // 65-90
  executionPower: number           // 0-1 (LOW=0.3, MED=0.6, HIGH=0.9)
  noiseLevel: number               // 0-1 (高噪音来源需要更多确�?
  canTriggerTrade: boolean         // 是否可直接触发交易信�?
  maxPositionRegime: PositionRegime  // 最高可触发的仓位制�?
}

// EU来源胶囊注册�?
const EU_SOURCE_CAPSULES: Record<string, SourceCapsule> = {
  // ========== EU-L0: Agenda Setters (低执行力) ==========
  'eu-commission-president': {
    sourceId: 'eu-commission-president',
    institution: 'European Commission',
    unit: 'President Office',
    region: 'EU',
    role: 'agenda_setter',
    signalTypes: ['statement', 'proposal'],
    defaultState: 'emerging',
    sourceWeight: 70,
    executionPower: 0.35,
    noiseLevel: 0.4,
    canTriggerTrade: false,
    maxPositionRegime: 'STARTER'
  },
  'eu-high-representative': {
    sourceId: 'eu-high-representative',
    institution: 'European Commission',
    unit: 'High Representative',
    region: 'EU',
    role: 'agenda_setter',
    signalTypes: ['statement', 'proposal'],
    defaultState: 'emerging',
    sourceWeight: 68,
    executionPower: 0.3,
    noiseLevel: 0.45,
    canTriggerTrade: false,
    maxPositionRegime: 'STARTER'
  },
  
  // ========== EU-L0.5: Rule Makers (核心Alpha来源) ==========
  'eu-dg-trade': {
    sourceId: 'eu-dg-trade',
    institution: 'European Commission',
    unit: 'DG TRADE',
    region: 'EU',
    role: 'rule_maker',
    signalTypes: ['implementing_act', 'delegated_act', 'guideline', 'enforcement'],
    defaultState: 'negotiating',
    sourceWeight: 88,
    executionPower: 0.8,
    noiseLevel: 0.15,
    canTriggerTrade: true,
    maxPositionRegime: 'FULL'
  },
  'eu-dg-comp': {
    sourceId: 'eu-dg-comp',
    institution: 'European Commission',
    unit: 'DG COMP',
    region: 'EU',
    role: 'rule_maker',
    signalTypes: ['implementing_act', 'enforcement', 'guideline'],
    defaultState: 'negotiating',
    sourceWeight: 90,
    executionPower: 0.85,
    noiseLevel: 0.12,
    canTriggerTrade: true,
    maxPositionRegime: 'FULL'
  },
  'eu-dg-fisma': {
    sourceId: 'eu-dg-fisma',
    institution: 'European Commission',
    unit: 'DG FISMA',
    region: 'EU',
    role: 'rule_maker',
    signalTypes: ['implementing_act', 'delegated_act', 'guideline'],
    defaultState: 'negotiating',
    sourceWeight: 86,
    executionPower: 0.75,
    noiseLevel: 0.18,
    canTriggerTrade: true,
    maxPositionRegime: 'FULL'
  },
  'eu-dg-connect': {
    sourceId: 'eu-dg-connect',
    institution: 'European Commission',
    unit: 'DG CONNECT',
    region: 'EU',
    role: 'rule_maker',
    signalTypes: ['implementing_act', 'guideline', 'enforcement'],
    defaultState: 'negotiating',
    sourceWeight: 84,
    executionPower: 0.7,
    noiseLevel: 0.2,
    canTriggerTrade: true,
    maxPositionRegime: 'STANDARD'
  },
  'eu-council-coreper': {
    sourceId: 'eu-council-coreper',
    institution: 'EU Council',
    unit: 'COREPER',
    region: 'EU',
    role: 'rule_maker',
    signalTypes: ['procedural', 'draft'],
    defaultState: 'negotiating',
    sourceWeight: 82,
    executionPower: 0.65,
    noiseLevel: 0.25,
    canTriggerTrade: true,
    maxPositionRegime: 'STANDARD'
  },
  
  // ========== EU-L1: Market Amplifiers ==========
  'reuters-eu': {
    sourceId: 'reuters-eu',
    institution: 'Reuters',
    unit: 'EU Bureau',
    region: 'EU',
    role: 'amplifier',
    signalTypes: ['statement'],
    defaultState: 'emerging',
    sourceWeight: 75,
    executionPower: 0,
    noiseLevel: 0.3,
    canTriggerTrade: false,
    maxPositionRegime: 'STARTER'
  },
  'bloomberg-eu': {
    sourceId: 'bloomberg-eu',
    institution: 'Bloomberg',
    unit: 'EU Bureau',
    region: 'EU',
    role: 'amplifier',
    signalTypes: ['statement'],
    defaultState: 'emerging',
    sourceWeight: 78,
    executionPower: 0,
    noiseLevel: 0.28,
    canTriggerTrade: false,
    maxPositionRegime: 'STARTER'
  },
  'politico-eu': {
    sourceId: 'politico-eu',
    institution: 'Politico',
    unit: 'Europe',
    region: 'EU',
    role: 'amplifier',
    signalTypes: ['statement'],
    defaultState: 'emerging',
    sourceWeight: 72,
    executionPower: 0,
    noiseLevel: 0.35,
    canTriggerTrade: false,
    maxPositionRegime: 'NO_TRADE'
  },
  'ft-eu': {
    sourceId: 'ft-eu',
    institution: 'Financial Times',
    unit: 'Europe',
    region: 'EU',
    role: 'amplifier',
    signalTypes: ['statement'],
    defaultState: 'emerging',
    sourceWeight: 76,
    executionPower: 0,
    noiseLevel: 0.3,
    canTriggerTrade: false,
    maxPositionRegime: 'STARTER'
  },
  
  // ========== EU-L2: Process Noise & Pressure ==========
  'germany-finance': {
    sourceId: 'germany-finance',
    institution: 'German Government',
    unit: 'Finance Ministry',
    region: 'EU',
    role: 'pressure_noise',
    signalTypes: ['statement'],
    defaultState: 'emerging',
    sourceWeight: 55,
    executionPower: 0.2,
    noiseLevel: 0.6,
    canTriggerTrade: false,
    maxPositionRegime: 'NO_TRADE'
  },
  'france-economy': {
    sourceId: 'france-economy',
    institution: 'French Government',
    unit: 'Economy Ministry',
    region: 'EU',
    role: 'pressure_noise',
    signalTypes: ['statement'],
    defaultState: 'emerging',
    sourceWeight: 52,
    executionPower: 0.18,
    noiseLevel: 0.65,
    canTriggerTrade: false,
    maxPositionRegime: 'NO_TRADE'
  },
  'italy-industry': {
    sourceId: 'italy-industry',
    institution: 'Italian Government',
    unit: 'Industry Ministry',
    region: 'EU',
    role: 'pressure_noise',
    signalTypes: ['statement'],
    defaultState: 'emerging',
    sourceWeight: 48,
    executionPower: 0.15,
    noiseLevel: 0.7,
    canTriggerTrade: false,
    maxPositionRegime: 'NO_TRADE'
  },
  'mep-individual': {
    sourceId: 'mep-individual',
    institution: 'European Parliament',
    unit: 'Individual MEP',
    region: 'EU',
    role: 'pressure_noise',
    signalTypes: ['statement'],
    defaultState: 'emerging',
    sourceWeight: 35,
    executionPower: 0.1,
    noiseLevel: 0.8,
    canTriggerTrade: false,
    maxPositionRegime: 'NO_TRADE'
  },
  'digital-europe': {
    sourceId: 'digital-europe',
    institution: 'DigitalEurope',
    unit: 'Industry Group',
    region: 'EU',
    role: 'pressure_noise',
    signalTypes: ['statement'],
    defaultState: 'emerging',
    sourceWeight: 30,
    executionPower: 0.05,
    noiseLevel: 0.85,
    canTriggerTrade: false,
    maxPositionRegime: 'NO_TRADE'
  },
  'acea': {
    sourceId: 'acea',
    institution: 'ACEA',
    unit: 'Auto Industry Group',
    region: 'EU',
    role: 'pressure_noise',
    signalTypes: ['statement'],
    defaultState: 'emerging',
    sourceWeight: 32,
    executionPower: 0.05,
    noiseLevel: 0.82,
    canTriggerTrade: false,
    maxPositionRegime: 'NO_TRADE'
  }
}

// 来源胶囊归一化函�?- 将任意来源信号归一化到标准政策行动
interface NormalizedSignal {
  originalSource: SourceCapsule
  normalizedAction: 'proposal' | 'negotiation' | 'implementation' | 'enforcement' | 'delay' | 'reversal'
  adjustedWeight: number
  adjustedExecutionPower: number
  proceduralRiskMultiplier: number
  maxAllowedRegime: PositionRegime
}

const normalizeEUSignal = (capsule: SourceCapsule, signalType: EUSignalType): NormalizedSignal => {
  // EU程序风险乘数
  const proceduralRisk = capsule.defaultState === 'negotiating' ? 0.75 : 
                         capsule.defaultState === 'implementing' ? 1.0 : 0.6
  
  // 根据角色调整权重
  const roleMultiplier = capsule.role === 'rule_maker' ? 1.0 :
                         capsule.role === 'agenda_setter' ? 0.8 :
                         capsule.role === 'amplifier' ? 0.6 : 0.3
  
  // 噪音惩罚
  const noisePenalty = 1 - (capsule.noiseLevel * 0.5)
  
  // 归一化行动类�?
  let normalizedAction: NormalizedSignal['normalizedAction']
  switch (signalType) {
    case 'proposal':
    case 'draft':
      normalizedAction = 'proposal'
      break
    case 'implementing_act':
    case 'delegated_act':
      normalizedAction = 'implementation'
      break
    case 'enforcement':
    case 'guideline':
      normalizedAction = 'enforcement'
      break
    case 'procedural':
      normalizedAction = 'negotiation'
      break
    default:
      normalizedAction = 'proposal'
  }
  
  return {
    originalSource: capsule,
    normalizedAction,
    adjustedWeight: capsule.sourceWeight * roleMultiplier * noisePenalty,
    adjustedExecutionPower: capsule.executionPower * proceduralRisk,
    proceduralRiskMultiplier: proceduralRisk,
    maxAllowedRegime: capsule.maxPositionRegime
  }
}

// 交易行动层级
type ActionTier = 'no-trade' | 'watch' | 'trade' | 'high-conviction'

// 信号可信度评�?
type CredibilityGrade = 'A' | 'B' | 'C' | 'D' | 'F'

// 状态引擎定�?- 每个状态的行为特征
interface PolicyStateDefinition {
  state: PolicyState
  label: string
  description: string
  expectedMarketBehavior: string
  recommendedStyle: string
  primaryRisks: string[]
  positionSizing: 'full' | 'reduced' | 'minimal' | 'none'
  holdPeriod: string
}

// 🆕 DecisionScore - 唯一暴露给交易的分数
interface DecisionScoreBreakdown {
  docScore: number               // 原始DocScore (0-100)
  netDirectionalBias: number     // 净方向偏差 (-1 to 1, 绝对值用于计�?
  domainSensitivity: number      // 领域敏感�?(0-1.5)
  pricingInIndex: number         // 已定价指�?(0-1, �?已被市场消化)
  signalPersistence: number      // 信号持续�?(0-1.5)
  narrativeDriftPenalty: number  // 漂移惩罚 (0.5-1.0)
  credibilityCap: number         // 可信度上�?(0-100)
  rawScore: number               // 原始计算分数
  finalScore: number             // 最终分�?(0-100, 受credibilityCap限制)
  actionTier: ActionTier         // 行动层级
  actionLabel: string            // 行动标签 (中文)
}

// 🆕 信号可信度雷�?
interface SignalCredibility {
  directionHitRatio: number      // 方向命中�?(0-1)
  scoreMovCorrelation: number    // 评分-波动相关�?(-1 to 1)
  timingValidity: number         // 时间有效�?(0-1)
  falsePositiveRate: number      // 假阳性率 (0-1)
  sampleSize: number             // 样本�?
  grade: CredibilityGrade        // A-F评级
  capMultiplier: number          // 对DecisionScore的上限乘�?(0.3-1.0)
}

// 🆕 净敞口矩阵 - 聚合冲突信号
interface NetExposureEntry {
  entityId: string
  entityName: string
  ticker?: string
  type: 'stock' | 'etf' | 'futures' | 'forex' | 'crypto'
  
  // 信号聚合
  bullishSignals: number
  bearishSignals: number
  ambiguousSignals: number
  
  // 净结果
  netBias: ImpactDirection
  netScore: number               // -100 to +100
  convictionLevel: 'high' | 'medium' | 'low' | 'conflicted'
  
  // 主导驱动因素
  dominantDrivers: {
    topicId: string
    topicName: string
    contribution: number         // 贡献�?%
    direction: ImpactDirection
  }[]
  
  // 行动建议
  suggestedAction: 'long' | 'short' | 'avoid' | 'hedge'
  suggestedSize: 'full' | 'half' | 'quarter' | 'none'
}

// 🆕 信息层级结构 (What/How/Why/Risk)
interface InformationHierarchy {
  whatToDo: string               // 应该做什�?
  howStrong: string              // 有多�?
  whyThisConclusion: string[]    // 为什么得出这个结�?
  whatCouldGoWrong: string[]     // 什么可能出�?
  knownFacts: string[]           // 已知事实
  uncertainties: string[]        // 不确定�?
  invalidationTriggers: string[] // 信号失效条件
}

interface Source {
  id: string
  name: string
  level: SourceLevel
  tier?: SourceTier
  domain: Domain[]
  executionPower: number // 0-100
  authority: number // 0-100
  logo?: string
  region?: SourceRegion // EU/US/CN/INTL
}

// ============== EU程序风险乘数计算 ==============

interface EUProceduralRisk {
  multiplier: number           // 0.6-1.0
  riskFactors: string[]
  timelineRisk: 'high' | 'medium' | 'low'
  memberStateRisk: 'high' | 'medium' | 'low'
  dilutionRisk: 'high' | 'medium' | 'low'
}

const calculateEUProceduralRisk = (
  state: PolicyState,
  region: SourceRegion | undefined,
  l2Count: number,  // 成员�?NGO噪音来源数量
  hasCouncilSignal: boolean
): EUProceduralRisk => {
  // 仅EU来源需要程序风险折�?
  if (region !== 'EU') {
    return {
      multiplier: 1.0,
      riskFactors: [],
      timelineRisk: 'low',
      memberStateRisk: 'low',
      dilutionRisk: 'low'
    }
  }
  
  const riskFactors: string[] = []
  let multiplier = 1.0
  
  // 状态风�?
  if (state === 'negotiating') {
    multiplier = 0.75  // Negotiating阶段折扣25%
    riskFactors.push('EU程序处于Negotiating阶段，执行不确定')
  } else if (state === 'emerging') {
    multiplier = 0.6   // Emerging阶段折扣40%
    riskFactors.push('仅议程设定，无执行力')
  } else if (state === 'implementing') {
    multiplier = 1.0   // 执行阶段无折�?
  }
  
  // 成员国噪音风�?
  const memberStateRisk = l2Count >= 5 ? 'high' : l2Count >= 2 ? 'medium' : 'low'
  if (memberStateRisk === 'high') {
    multiplier *= 0.9
    riskFactors.push('多个成员国表态，存在否决/延迟风险')
  }
  
  // 理事会确认增益
  if (hasCouncilSignal && state === 'negotiating') {
    multiplier = Math.min(1.0, multiplier * 1.15)
    riskFactors.push('COREPER程序信号确认，执行概率提升')
  }
  
  // 时间线风险
  const timelineRisk = state === 'negotiating' ? 'high' : 
                       state === 'emerging' ? 'high' : 'low'
  
  // 稀释风�?
  const dilutionRisk = l2Count >= 3 ? 'high' : l2Count >= 1 ? 'medium' : 'low'
  
  return {
    multiplier: Math.max(0.5, Math.min(1.0, multiplier)),
    riskFactors,
    timelineRisk,
    memberStateRisk,
    dilutionRisk
  }
}

interface Entity {
  id: string
  name: string
  type: 'company' | 'country' | 'department' | 'legislation' | 'person' | 'government' | 'organization'
  ticker?: string
  exposure: ImpactDirection
  confidence: number
}

interface Evidence {
  id: string
  text: string
  span: [number, number]
  sourceId: string
  sourceName: string
  level: SourceLevel
  url: string
  publishedAt: string
}

// DocScore分项拆解 - 可见化每个乘�?
interface DocScoreBreakdown {
  sourceWeight: number      // 来源权重 0-100
  actionStrength: number    // 动作强度 0.5-2.0 (EO=1.5, announce=1.0, consider=0.7)
  attributionMultiplier: number  // 归因乘数 (direct_quote=1.25, mention=1.0, inference=0.8)
  freshness: number         // 新鲜�?0-1 (按小时衰�?
  executionPower: number    // 执行力乘�?0.5-1.5
  uncertaintyPenalty: number // 不确定性惩�?0.5-1.0 (could/might/if = 0.7)
  finalScore: number        // 最终分�?
}

// 政策闭环验证结构
interface PolicyLoopVerification {
  confirmed: boolean
  windowHours: number       // 闭环窗口时长
  l0Evidence: Evidence | null
  l05Evidence: Evidence | null
  l1Evidences: Evidence[]   // 至少2�?
  l2Evidence: Evidence | null
  completedAt?: string
}

// 叙事漂移量化
interface NarrativeDriftMetrics {
  l0_l1_drift: number       // L0与L1之间的语义漂�?
  l1_l2_drift: number       // L1与L2之间的语义漂�?
  overall: number           // 整体漂移
  riskLevel: 'low' | 'medium' | 'high' | 'critical'
  trend: 'increasing' | 'stable' | 'decreasing'
}

// 可交易资产映�?
interface TradeableAsset {
  ticker: string
  name: string
  type: 'stock' | 'etf' | 'futures' | 'forex' | 'crypto'
  exposure: ImpactDirection
  confidence: number
  reasoning: string
}

interface Document {
  id: string
  title: string
  source: Source
  publishedAt: string
  url: string
  summary: string
  entities: Entity[]
  topics: string[]
  actions: string[]
  sentiment: ImpactDirection
  uncertainty: number // 0-1, higher = more uncertain
  docScore: number
  scoreBreakdown: DocScoreBreakdown  // 新增：分项拆�?
  evidences: Evidence[]
  quotePrimary?: string
  mentionPrimary?: string
}

interface Topic {
  id: string
  name: string
  state: PolicyState                    // 🆕 升级�?阶段状态机
  stateDefinition?: PolicyStateDefinition  // 🆕 状态详细定�?
  score6h: number
  score24h: number
  score7d: number
  velocity: number // change rate
  narrativeDrift: number
  driftMetrics: NarrativeDriftMetrics  // 新增：量化漂�?
  documents: Document[]
  entities: Entity[]
  tradeableAssets: TradeableAsset[]    // 新增：可交易映射
  l0Count: number
  l05Count: number
  l1Count: number
  l2Count: number
  inPolicyLoop: boolean
  policyLoop?: PolicyLoopVerification   // 新增：闭环验�?
  lastUpdated: string
  validation?: MarketValidation         // 新增：市场验�?
  
  // 🆕 决策引擎核心
  decisionScore?: DecisionScoreBreakdown  // 决策分数
  credibility?: SignalCredibility         // 信号可信�?
  netExposure?: NetExposureEntry[]        // 净敞口矩阵
  infoHierarchy?: InformationHierarchy    // 信息层级
  domain: Domain                          // 主要领域
  
  // 🆕 PHASE 1: 决策完整�?
  positionRegime?: PositionRegimeInfo     // 仓位制度
  timing?: TimingInfo                     // 时机评估
  stance?: SystemStance                   // 系统立场 (一句话结论)
  noTradeInfo?: NoTradeInfo               // NO_TRADE 可行动化
  
  // 🆕 PHASE 2: 组合现实�?
  conflictResolver?: ConflictResolver     // 冲突解决�?
  
  // 🆕 PHASE 3: 长期稳健�?
  signalHalfLife?: SignalHalfLife         // 信号半衰�?
  crowdingRisk?: CrowdingRisk             // 拥挤风险
  falseSignalAudit?: FalseSignalAudit     // 假信号审�?
}

// ============== 市场验证系统 ==============

// 资产价格数据
interface AssetPriceData {
  ticker: string
  priceT0: number           // 信号触发时价�?
  priceT1_1h: number        // T0 + 1h
  priceT1_6h: number        // T0 + 6h
  priceT1_24h: number       // T0 + 24h
  priceT2_3d: number        // T0 + 3d
  pricePreMove: number      // T0 - 24h (用于时间一致�?
}

// 单个资产的验证结�?
interface AssetValidation {
  ticker: string
  name: string
  type: 'stock' | 'etf' | 'futures' | 'forex' | 'crypto'
  systemDirection: ImpactDirection     // 系统预测方向
  actualDirection: 'up' | 'down' | 'flat'  // 市场实际方向
  directionMatch: boolean              // 方向是否一�?
  confidence: number                   // 系统置信�?
  returns: {
    ret1h: number    // 1h 收益�?%
    ret6h: number    // 6h 收益�?%
    ret24h: number   // 24h 收益�?%
    ret3d: number    // 3d 收益�?%
  }
  preMove: number      // T0�?4h波动
  postMove: number     // T0�?4h波动
  isForwardLooking: boolean  // PostMove > PreMove = 前瞻�?
}

// 主题级别的市场验�?
interface MarketValidation {
  signalTime: string          // T0 信号触发时间
  validationTime: string      // 验证计算时间
  
  // 方向一致�?
  directionHitRatio: number   // 方向命中�?(0-1)
  directionHits: number       // 命中�?
  directionTotal: number      // 总数
  
  // 强度相关�?
  avgPostMove24h: number      // 平均24h波动 %
  scoreMovCorrelation: number // DocScore与波动的相关系数
  
  // 时间一致�?
  forwardLookingRatio: number // 前瞻性比�?
  avgPreMove: number          // 平均事前波动
  avgPostMove: number         // 平均事后波动
  
  // 资产分层验证
  byAssetType: {
    stock: { hitRatio: number; avgMove: number; count: number }
    etf: { hitRatio: number; avgMove: number; count: number }
    futures: { hitRatio: number; avgMove: number; count: number }
    forex: { hitRatio: number; avgMove: number; count: number }
  }
  
  // 详细验证
  assetValidations: AssetValidation[]
  
  // 质量等级
  qualityGrade: 'A' | 'B' | 'C' | 'D' | 'F'
  qualityScore: number        // 0-100
}

// 闭环 vs 非闭环对�?
interface LoopComparison {
  looped: {
    count: number
    avgHitRatio: number
    avgMove: number
    avgDuration: number       // 波动持续时间 (hours)
  }
  nonLooped: {
    count: number
    avgHitRatio: number
    avgMove: number
    avgDuration: number
  }
  loopAdvantage: number       // 闭环优势 = looped.avgHitRatio - nonLooped.avgHitRatio
}

interface Alert {
  id: string
  type: 'policy_loop' | 'narrative_reversal' | 'missing_signal' | 'high_impact' | 'state_change'
  topic: Topic
  title: string
  summary: string
  severity: 'critical' | 'high' | 'medium' | 'low'
  evidences: Evidence[]
  createdAt: string
  read: boolean
}

// ============== 状态引擎定�?(7阶段 - 含EU Negotiating) ==============

const POLICY_STATE_DEFINITIONS: Record<PolicyState, PolicyStateDefinition> = {
  emerging: {
    state: 'emerging',
    label: 'EMERGING',
    description: '信号首次出现，尚未被市场充分认知',
    expectedMarketBehavior: '低波动，价格未反应',
    recommendedStyle: '建仓观察，小仓位试探',
    primaryRisks: ['信号可能是噪音', '缺乏确认', 'L0尚未表态'],
    positionSizing: 'minimal',
    holdPeriod: '1-3天观察'
  },
  negotiating: {
    state: 'negotiating',
    label: 'NEGOTIATING',
    description: 'EU特有: 委员会提案已发布，理事会/议会辩论中',
    expectedMarketBehavior: '缓慢定价，预期形成中但执行不确定',
    recommendedStyle: '折扣定价，等待DG执行',
    primaryRisks: ['程序延迟', '成员国否决', '稀释修改', '时间风险'],
    positionSizing: 'reduced',
    holdPeriod: '观察至Implementing'
  },
  contested: {
    state: 'contested',
    label: 'CONTESTED',
    description: '多方势力博弈，方向不明确',
    expectedMarketBehavior: '高波动，双向震荡',
    recommendedStyle: '期权策略，跨式/宽跨式',
    primaryRisks: ['方向反转', '政策反复', '叙事分裂'],
    positionSizing: 'reduced',
    holdPeriod: '日内或隔夜'
  },
  implementing: {
    state: 'implementing',
    label: 'IMPLEMENTING',
    description: 'L0确认，政策进入执行阶段',
    expectedMarketBehavior: '单边趋势，高动量',
    recommendedStyle: '趋势跟随，加仓突破',
    primaryRisks: ['执行力度不及预期', '时间延迟', '市场已定价'],
    positionSizing: 'full',
    holdPeriod: '持有至信号衰减'
  },
  digesting: {
    state: 'digesting',
    label: 'DIGESTING',
    description: '市场正在消化政策影响',
    expectedMarketBehavior: '波动收敛，区间震荡',
    recommendedStyle: '获利了结，区间交易',
    primaryRisks: ['二次冲击', '延伸影响', '预期差'],
    positionSizing: 'reduced',
    holdPeriod: '逐步减仓'
  },
  exhausted: {
    state: 'exhausted',
    label: 'EXHAUSTED',
    description: '信号影响已充分定价',
    expectedMarketBehavior: '低波动，均值回归',
    recommendedStyle: '平仓离场，等待新信号',
    primaryRisks: ['错过反转', '过度交易'],
    positionSizing: 'none',
    holdPeriod: '不持仓'
  },
  reversed: {
    state: 'reversed',
    label: 'REVERSED',
    description: '政策方向发生根本性逆转',
    expectedMarketBehavior: '急剧反向波动',
    recommendedStyle: '反向建仓，止损严格',
    primaryRisks: ['反转不彻底', '假突破', '情绪过度'],
    positionSizing: 'reduced',
    holdPeriod: '短期博弈'
  }
}

// ============== Decision Scoring Engine ==============

// 领域敏感度映�?
const DOMAIN_SENSITIVITY: Record<Domain, number> = {
  trade: 1.3,           // 贸易政策高敏�?
  sanction: 1.4,        // 制裁最高敏�?
  war: 1.5,             // 战争最高敏�?
  rate: 1.2,            // 利率中高敏感
  fiscal: 1.0,          // 财政中等敏感
  regulation: 0.9,      // 监管相对低敏�?
  export_control: 1.35, // 出口管制高敏�?
  antitrust: 0.95       // 反垄断中等敏�?
}

// 可信度评级阈�?
const CREDIBILITY_THRESHOLDS = {
  A: { minHitRatio: 0.8, minCorrelation: 0.6, maxFalsePositive: 0.1, cap: 1.0 },
  B: { minHitRatio: 0.7, minCorrelation: 0.5, maxFalsePositive: 0.15, cap: 0.85 },
  C: { minHitRatio: 0.6, minCorrelation: 0.4, maxFalsePositive: 0.2, cap: 0.7 },
  D: { minHitRatio: 0.5, minCorrelation: 0.3, maxFalsePositive: 0.3, cap: 0.5 },
  F: { minHitRatio: 0, minCorrelation: 0, maxFalsePositive: 1, cap: 0.3 }
}

// 行动层级阈�?- 机构级命�?
const ACTION_TIER_THRESHOLDS = {
  'high-conviction': { min: 75, label: 'HIGH CONVICTION' },
  'trade': { min: 50, label: 'ACTIONABLE' },
  'watch': { min: 25, label: 'MONITOR' },
  'no-trade': { min: 0, label: 'NO ACTION' }
}

// 计算DecisionScore (含EU程序风险乘数) - 本地版本
const calculateLocalDecisionScore = (
  docScore: number,
  netBias: number,           // -1 to 1
  domain: Domain,
  pricingIn: number,         // 0-1 已定价程度
  persistence: number,       // 0-1.5 信号持续性
  driftPenalty: number,      // 0.5-1.0 漂移惩罚
  credibilityCap: number,    // 0-100 可信度上限
  euProceduralMultiplier: number = 1.0  // EU程序风险乘数 0.5-1.0
): DecisionScoreBreakdown => {
  const domainSensitivity = DOMAIN_SENSITIVITY[domain]
  const absNetBias = Math.abs(netBias)
  
  // 原始计算: DocScore × |NetBias| × DomainSensitivity × (1 - PricingIn) × Persistence × DriftPenalty × EU_Multiplier
  const rawScore = docScore * absNetBias * domainSensitivity * (1 - pricingIn) * persistence * driftPenalty * euProceduralMultiplier
  
  // 归一化到 0-100
  const normalizedScore = Math.min(100, Math.max(0, rawScore))
  
  // 应用可信度上�?
  const finalScore = Math.min(normalizedScore, credibilityCap)
  
  // 确定行动层级
  let actionTier: ActionTier = 'no-trade'
  let actionLabel = ACTION_TIER_THRESHOLDS['no-trade'].label
  
  if (finalScore >= ACTION_TIER_THRESHOLDS['high-conviction'].min) {
    actionTier = 'high-conviction'
    actionLabel = ACTION_TIER_THRESHOLDS['high-conviction'].label
  } else if (finalScore >= ACTION_TIER_THRESHOLDS['trade'].min) {
    actionTier = 'trade'
    actionLabel = ACTION_TIER_THRESHOLDS['trade'].label
  } else if (finalScore >= ACTION_TIER_THRESHOLDS['watch'].min) {
    actionTier = 'watch'
    actionLabel = ACTION_TIER_THRESHOLDS['watch'].label
  }
  
  return {
    docScore,
    netDirectionalBias: netBias,
    domainSensitivity,
    pricingInIndex: pricingIn,
    signalPersistence: persistence,
    narrativeDriftPenalty: driftPenalty,
    credibilityCap,
    rawScore: normalizedScore,
    finalScore,
    actionTier,
    actionLabel
  }
}

// 计算信号可信�?
const calculateCredibility = (validation?: MarketValidation): SignalCredibility => {
  if (!validation) {
    return {
      directionHitRatio: 0,
      scoreMovCorrelation: 0,
      timingValidity: 0,
      falsePositiveRate: 1,
      sampleSize: 0,
      grade: 'F',
      capMultiplier: 0.3
    }
  }
  
  const hitRatio = validation.directionHitRatio
  const correlation = validation.scoreMovCorrelation
  const timingValidity = validation.forwardLookingRatio
  const falsePositiveRate = 1 - hitRatio
  
  // 确定评级
  let grade: CredibilityGrade = 'F'
  let capMultiplier = CREDIBILITY_THRESHOLDS.F.cap
  
  if (hitRatio >= CREDIBILITY_THRESHOLDS.A.minHitRatio && 
      correlation >= CREDIBILITY_THRESHOLDS.A.minCorrelation) {
    grade = 'A'
    capMultiplier = CREDIBILITY_THRESHOLDS.A.cap
  } else if (hitRatio >= CREDIBILITY_THRESHOLDS.B.minHitRatio && 
             correlation >= CREDIBILITY_THRESHOLDS.B.minCorrelation) {
    grade = 'B'
    capMultiplier = CREDIBILITY_THRESHOLDS.B.cap
  } else if (hitRatio >= CREDIBILITY_THRESHOLDS.C.minHitRatio && 
             correlation >= CREDIBILITY_THRESHOLDS.C.minCorrelation) {
    grade = 'C'
    capMultiplier = CREDIBILITY_THRESHOLDS.C.cap
  } else if (hitRatio >= CREDIBILITY_THRESHOLDS.D.minHitRatio) {
    grade = 'D'
    capMultiplier = CREDIBILITY_THRESHOLDS.D.cap
  }
  
  return {
    directionHitRatio: hitRatio,
    scoreMovCorrelation: correlation,
    timingValidity,
    falsePositiveRate,
    sampleSize: validation.directionTotal,
    grade,
    capMultiplier
  }
}

// 计算净敞口
const calculateNetExposure = (entities: Entity[], tradeableAssets: TradeableAsset[], topicId: string, topicName: string): NetExposureEntry[] => {
  const exposureMap = new Map<string, NetExposureEntry>()
  
  tradeableAssets.forEach(asset => {
    const existing = exposureMap.get(asset.ticker)
    
    if (!existing) {
      exposureMap.set(asset.ticker, {
        entityId: asset.ticker,
        entityName: asset.name,
        ticker: asset.ticker,
        type: asset.type,
        bullishSignals: asset.exposure === 'bullish' ? 1 : 0,
        bearishSignals: asset.exposure === 'bearish' ? 1 : 0,
        ambiguousSignals: asset.exposure === 'ambiguous' ? 1 : 0,
        netBias: asset.exposure,
        netScore: asset.exposure === 'bullish' ? asset.confidence : 
                  asset.exposure === 'bearish' ? -asset.confidence : 0,
        convictionLevel: asset.confidence >= 80 ? 'high' : 
                        asset.confidence >= 60 ? 'medium' : 'low',
        dominantDrivers: [{
          topicId,
          topicName,
          contribution: 100,
          direction: asset.exposure
        }],
        suggestedAction: asset.exposure === 'bullish' ? 'long' : 
                        asset.exposure === 'bearish' ? 'short' : 'avoid',
        suggestedSize: asset.confidence >= 80 ? 'full' : 
                      asset.confidence >= 60 ? 'half' : 'quarter'
      })
    } else {
      // 聚合冲突信号
      if (asset.exposure === 'bullish') existing.bullishSignals++
      else if (asset.exposure === 'bearish') existing.bearishSignals++
      else existing.ambiguousSignals++
      
      // 更新净分数
      const delta = asset.exposure === 'bullish' ? asset.confidence : 
                   asset.exposure === 'bearish' ? -asset.confidence : 0
      existing.netScore += delta
      
      // 重新计算净偏向
      if (existing.netScore > 20) existing.netBias = 'bullish'
      else if (existing.netScore < -20) existing.netBias = 'bearish'
      else existing.netBias = 'ambiguous'
      
      // 检查冲�?
      if (existing.bullishSignals > 0 && existing.bearishSignals > 0) {
        existing.convictionLevel = 'conflicted'
        existing.suggestedAction = 'hedge'
        existing.suggestedSize = 'quarter'
      }
    }
  })
  
  return Array.from(exposureMap.values())
}

// 生成信息层级
const generateInfoHierarchy = (
  topic: Topic,
  decisionScore: DecisionScoreBreakdown,
  credibility: SignalCredibility
): InformationHierarchy => {
  const stateInfo = POLICY_STATE_DEFINITIONS[topic.state]
  
  return {
    whatToDo: decisionScore.actionLabel + (
      decisionScore.actionTier === 'high-conviction' ? ` - ${topic.tradeableAssets[0]?.ticker || '相关资产'}` :
      decisionScore.actionTier === 'trade' ? ` - 考虑${topic.tradeableAssets[0]?.ticker || '相关资产'}` :
      ''
    ),
    howStrong: `DecisionScore: ${decisionScore.finalScore.toFixed(0)}/100 | 可信�? ${credibility.grade}级`,
    whyThisConclusion: [
      `信号来源: L0=${topic.l0Count}, L0.5=${topic.l05Count}`,
      `方向命中�? ${(credibility.directionHitRatio * 100).toFixed(0)}%`,
      `政策状�? ${stateInfo.label}`,
      `领域敏感�? ${topic.domain} (×${DOMAIN_SENSITIVITY[topic.domain]})`
    ],
    whatCouldGoWrong: [
      ...stateInfo.primaryRisks,
      topic.driftMetrics.riskLevel !== 'low' ? `叙事漂移风险: ${topic.driftMetrics.riskLevel}` : '',
      credibility.falsePositiveRate > 0.2 ? `假阳性率较高: ${(credibility.falsePositiveRate * 100).toFixed(0)}%` : ''
    ].filter(Boolean),
    knownFacts: [
      topic.policyLoop?.l0Evidence ? `L0确认: ${topic.policyLoop.l0Evidence.text.slice(0, 50)}...` : '无L0确认',
      `最新更新: ${topic.lastUpdated}`
    ],
    uncertainties: [
      topic.driftMetrics.riskLevel !== 'low' ? '叙事可能继续漂移' : '',
      !topic.inPolicyLoop ? '政策闭环未完成' : '',
      topic.state === 'contested' ? '多方博弈中，方向不明' : ''
    ].filter(Boolean),
    invalidationTriggers: [
      'L0发布相反信号',
      '关键支撑/阻力突破',
      '新的高权重来源反驳'
    ]
  }
}

// ============== PHASE 1: Position Regime Calculation ==============

const calculatePositionRegime = (decisionScore: number): PositionRegimeInfo => {
  if (decisionScore >= 75) {
    return {
      regime: 'FULL',
      positionCap: 1.0,
      label: 'FULL POSITION',
      color: 'text-green-400 bg-green-500/20 border-green-500/50',
      description: 'Highest signal strength, full position allowed'
    }
  } else if (decisionScore >= 60) {
    return {
      regime: 'STANDARD',
      positionCap: 0.66,
      label: 'STANDARD',
      color: 'text-blue-400 bg-blue-500/20 border-blue-500/50',
      description: 'Solid signal, standard position allowed'
    }
  } else if (decisionScore >= 45) {
    return {
      regime: 'STARTER',
      positionCap: 0.33,
      label: 'STARTER',
      color: 'text-yellow-400 bg-yellow-500/20 border-yellow-500/50',
      description: 'Early signal, small position only'
    }
  } else {
    return {
      regime: 'NO_TRADE',
      positionCap: 0,
      label: 'NO TRADE',
      color: 'text-red-400 bg-red-500/20 border-red-500/50',
      description: 'Insufficient signal, no trade allowed'
    }
  }
}

// ============== PHASE 1: Timing Assessment (EU Sensitivity) ==============

const calculateTiming = (validation?: MarketValidation, isEU: boolean = false): TimingInfo => {
  if (!validation) {
    return {
      timing: 'EARLY',
      pricingInIndex: 0,
      label: 'EARLY',
      color: 'text-green-400 bg-green-500/20',
      recommendation: isEU 
        ? 'EU signal is early, procedural risks exist but expectations not yet formed'
        : 'Fresh signal, not yet priced by market, optimal entry timing'
    }
  }
  
  // pricing_in_index = PreMove / (PreMove + PostMove) * 100
  const preMove = Math.abs(validation.avgPreMove)
  const postMove = Math.abs(validation.avgPostMove)
  let pricingInIndex = (preMove + postMove) > 0 
    ? (preMove / (preMove + postMove)) * 100 
    : 0
  
  // EU signals price slower, higher sensitivity - adjust thresholds
  const earlyThreshold = isEU ? 25 : 30
  const lateThreshold = isEU ? 55 : 60
  if (pricingInIndex < earlyThreshold) {
    return {
      timing: 'EARLY',
      pricingInIndex,
      label: 'EARLY',
      color: 'text-green-400 bg-green-500/20',
      recommendation: isEU 
        ? 'EU signal is early, procedural risks exist but expectations not yet formed'
        : 'Fresh signal, not yet priced by market, optimal entry timing'
    }
  } else if (pricingInIndex <= lateThreshold) {
    return {
      timing: 'OPTIMAL',
      pricingInIndex,
      label: 'OPTIMAL',
      color: 'text-cyan-400 bg-cyan-500/20',
      recommendation: isEU
        ? 'EU rulemaking confirmed, partially priced but execution certainty increasing'
        : 'Partially priced but room remains, consider entry'
    }
  } else {
    return {
      timing: 'LATE',
      pricingInIndex,
      label: 'LATE',
      color: 'text-orange-400 bg-orange-500/20',
      recommendation: isEU
        ? 'EU execution mostly complete, market has fully reacted'
        : 'Mostly priced in, missed optimal timing, for profit-taking only'
    }
  }
}

// ============== PHASE 1: System Stance Calculation (EU Templates) ==============

const generateSystemStance = (
  topic: Topic,
  decisionScore: DecisionScoreBreakdown,
  timing: TimingInfo,
  credibility: SignalCredibility,
  euProceduralRisk?: EUProceduralRisk
): SystemStance => {
  const stateInfo = POLICY_STATE_DEFINITIONS[topic.state]
  const isEU = topic.documents.some(d => d.source.region === 'EU')
  
  // 确定方向偏向
  let directionalBias: 'LONG' | 'SHORT' | 'NEUTRAL' | 'AVOID' = 'NEUTRAL'
  if (decisionScore.actionTier === 'no-trade') {
    directionalBias = 'AVOID'
  } else if (decisionScore.netDirectionalBias > 0.3) {
    directionalBias = 'LONG'
  } else if (decisionScore.netDirectionalBias < -0.3) {
    directionalBias = 'SHORT'
  }
  
  // 确定信心
  let conviction: 'HIGH' | 'MEDIUM' | 'LOW' | 'NONE' = 'NONE'
  if (decisionScore.finalScore >= 75) conviction = 'HIGH'
  else if (decisionScore.finalScore >= 50) conviction = 'MEDIUM'
  else if (decisionScore.finalScore >= 25) conviction = 'LOW'
  
  // Main risk (including EU procedural risk)
  let mainRisk = stateInfo.primaryRisks[0] || 'Unknown risk'
  if (isEU && euProceduralRisk && euProceduralRisk.riskFactors.length > 0) {
    mainRisk = euProceduralRisk.riskFactors[0]
  }
  
  // Generate tags
  const stanceTags: StanceTag[] = []
  if (timing.pricingInIndex > 65) stanceTags.push('priced_in')
  if (topic.driftMetrics.riskLevel === 'high' || topic.driftMetrics.riskLevel === 'critical') {
    stanceTags.push('high_drift')
  }
  if (topic.state === 'contested') stanceTags.push('conflict')
  if (topic.inPolicyLoop && topic.policyLoop?.confirmed) stanceTags.push('execution_confirmed')
  if (timing.timing === 'EARLY') stanceTags.push('early_signal')
  if (timing.timing === 'LATE') stanceTags.push('late_signal')
  
  // Generate one-line conclusion (EU templates)
  const directionText = directionalBias === 'LONG' ? 'LONG' : 
                        directionalBias === 'SHORT' ? 'SHORT' : 
                        directionalBias === 'AVOID' ? 'AVOID' : 'WAIT'
  const convictionText = conviction === 'HIGH' ? 'High Conviction' :
                        conviction === 'MEDIUM' ? 'Medium Conviction' :
                        conviction === 'LOW' ? 'Low Conviction' : 'No Conviction'
  const asset = topic.tradeableAssets[0]?.ticker || 'Related Assets'
  
  // EU-specific Stance templates
  let stanceText: string
  if (isEU && topic.state === 'negotiating') {
    stanceText = `EU policy direction clear, but timing depends on Council approval. ${directionText} ${asset}, ${convictionText}; Procedural risk: ${mainRisk}`
  } else if (isEU && topic.state === 'emerging') {
    stanceText = `High regulatory intent; execution affected by member state negotiations. Watch ${asset}, await DG implementation confirmation`
  } else if (isEU && euProceduralRisk && euProceduralRisk.multiplier < 0.8) {
    stanceText = `${directionText} ${asset}, ${convictionText}; EU procedural discount ${((1 - euProceduralRisk.multiplier) * 100).toFixed(0)}%; ${mainRisk}`
  } else {
    stanceText = `${directionText} ${asset}, ${convictionText}; Main risk: ${mainRisk}`
  }
  
  return {
    stanceText,
    directionalBias,
    conviction,
    mainRisk,
    stanceTags
  }
}

// ============== PHASE 1: NO_TRADE 可行动化 ==============

const generateNoTradeInfo = (
  topic: Topic,
  decisionScore: DecisionScoreBreakdown,
  timing: TimingInfo,
  credibility: SignalCredibility
): NoTradeInfo | undefined => {
  if (decisionScore.actionTier !== 'no-trade') return undefined
  
  const reasons: NoTradeReason[] = []
  const reasonLabels: string[] = []
  
  // 检查各种原�?
  if (timing.pricingInIndex > 70) {
    reasons.push('priced_in_high')
    reasonLabels.push(`已定价过�?(${timing.pricingInIndex.toFixed(0)}%)`)
  }
  
  if (topic.driftMetrics.riskLevel === 'high' || topic.driftMetrics.riskLevel === 'critical') {
    reasons.push('drift_high')
    reasonLabels.push(`叙事漂移严重 (${topic.driftMetrics.riskLevel})`)
  }
  
  if (topic.state === 'contested' || 
      (topic.tradeableAssets.some(a => a.exposure === 'bullish') && 
       topic.tradeableAssets.some(a => a.exposure === 'bearish'))) {
    reasons.push('conflict_high')
    reasonLabels.push('信号方向冲突')
  }
  
  if (!topic.inPolicyLoop || !topic.policyLoop?.confirmed) {
    reasons.push('weak_loop')
    reasonLabels.push('Policy loop not confirmed')
  }
  
  if (topic.documents.every(d => d.source.executionPower < 70)) {
    reasons.push('low_execution_power')
    reasonLabels.push('Low source execution power')
  }
  
  // Take top 3 reasons only
  const topReasons = reasons.slice(0, 3)
  const topLabels = reasonLabels.slice(0, 3)
  
  // Generate watch triggers
  const watchTriggers: string[] = []
  if (reasons.includes('weak_loop')) {
    watchTriggers.push('Wait for L0/L0.5 confirmation')
  }
  if (reasons.includes('conflict_high')) {
    watchTriggers.push('等待方向明确')
  }
  if (reasons.includes('priced_in_high')) {
    watchTriggers.push('等待回调至支撑位')
  }
  if (reasons.includes('drift_high')) {
    watchTriggers.push('等待叙事稳定')
  }
  
  return {
    reasons: topReasons,
    reasonLabels: topLabels,
    nextAction: watchTriggers.length > 0 ? watchTriggers[0] : '持续观察',
    watchTriggers
  }
}

// ============== 🆕 PHASE 2: 冲突解决�?==============

const calculateConflictResolver = (
  topics: Topic[],
  entityTicker: string
): ConflictResolver => {
  // 找出所有涉及该实体的主�?
  const relevantTopics = topics.filter(t => 
    t.tradeableAssets?.some(a => a.ticker === entityTicker)
  )
  
  const conflictItems = relevantTopics.map(t => {
    const asset = t.tradeableAssets?.find(a => a.ticker === entityTicker)
    return {
      topicId: t.id,
      topicName: t.name,
      direction: asset?.exposure || 'ambiguous' as ImpactDirection,
      score: t.decisionScore?.finalScore || t.score24h
    }
  })
  
  // 计算净偏差
  let netBiasSum = 0
  let totalWeight = 0
  
  conflictItems.forEach(item => {
    const direction = item.direction === 'bullish' ? 1 : 
                     item.direction === 'bearish' ? -1 : 0
    const weight = item.score * DOMAIN_SENSITIVITY[topics.find(t => t.id === item.topicId)?.domain || 'regulation']
    netBiasSum += direction * weight
    totalWeight += Math.abs(weight)
  })
  
  const netBias = totalWeight > 0 ? netBiasSum / totalWeight : 0
  
  // 检测冲�?
  const bullishItems = conflictItems.filter(i => i.direction === 'bullish')
  const bearishItems = conflictItems.filter(i => i.direction === 'bearish')
  const conflictDetected = bullishItems.length > 0 && bearishItems.length > 0
  
  // 计算置信�?
  const netConfidence = conflictDetected 
    ? 0.5 - (Math.min(bullishItems.length, bearishItems.length) / Math.max(bullishItems.length, bearishItems.length)) * 0.3
    : Math.abs(netBias)
  
  // 确定解决方案
  let resolution: 'clear_long' | 'clear_short' | 'conflicted' | 'neutral' = 'neutral'
  if (conflictDetected) {
    resolution = 'conflicted'
  } else if (netBias > 0.3) {
    resolution = 'clear_long'
  } else if (netBias < -0.3) {
    resolution = 'clear_short'
  }
  
  // 顶级驱动因素
  const topDrivers = conflictItems
    .sort((a, b) => b.score - a.score)
    .slice(0, 3)
    .map(item => ({
      topicId: item.topicId,
      topicName: item.topicName,
      contribution: totalWeight > 0 ? (item.score / totalWeight) * 100 : 0,
      direction: item.direction
    }))
  
  return {
    conflictDetected,
    conflictItems,
    netBias,
    netConfidence,
    topDrivers,
    resolution
  }
}

// ============== 🆕 PHASE 3: 信号半衰期计�?==============

const calculateSignalHalfLife = (topic: Topic): SignalHalfLife => {
  // 基于文档时间戳估算半衰期
  const now = new Date()
  const docTimes = topic.documents.map(d => new Date(d.publishedAt).getTime())
  const avgDocAge = docTimes.length > 0 
    ? (now.getTime() - docTimes.reduce((a, b) => a + b, 0) / docTimes.length) / (1000 * 60 * 60)
    : 24
  
  // 根据领域估算基础半衰�?
  const domainHalfLife: Record<Domain, number> = {
    war: 6,             // 战争信号衰减�?
    sanction: 24,       // 制裁信号中等衰减
    trade: 48,          // 贸易信号较持�?
    rate: 72,           // 利率信号持久
    fiscal: 96,         // 财政信号最持久
    regulation: 120,    // 监管信号最持久
    export_control: 36, // 出口管制中等衰减
    antitrust: 168      // 反垄断信号非常持�?
  }
  
  const baseHalfLife = domainHalfLife[topic.domain]
  
  // 根据闭环状态调�?
  const loopMultiplier = topic.inPolicyLoop && topic.policyLoop?.confirmed ? 1.5 : 1.0
  
  const halfLifeHours = baseHalfLife * loopMultiplier
  
  // 计算持续性分�?(用于DecisionScore乘数)
  const decayFactor = Math.exp(-avgDocAge / halfLifeHours * Math.LN2)
  const persistenceScore = 0.5 + decayFactor // 0.5 - 1.5 范围
  
  // Estimate remaining life
  const estimatedRemainingLife = Math.max(0, halfLifeHours - avgDocAge)
  
  // Persistence label
  let persistenceLabel = ''
  if (persistenceScore >= 1.2) persistenceLabel = 'High Persistence'
  else if (persistenceScore >= 0.9) persistenceLabel = 'Medium Persistence'
  else persistenceLabel = 'Low Persistence'
  
  return {
    halfLifeHours,
    persistenceScore,
    decayRate: Math.LN2 / halfLifeHours,
    estimatedRemainingLife,
    persistenceLabel
  }
}

// ============== 🆕 PHASE 3: 拥挤风险计算 ==============

const calculateCrowdingRisk = (
  topics: Topic[],
  currentDirection: ImpactDirection
): CrowdingRisk => {
  // 统计同方向的高仓位制度资�?
  let sameDirectionCount = 0
  let fullRegimeCount = 0
  
  topics.forEach(t => {
    t.tradeableAssets?.forEach(asset => {
      if (asset.exposure === currentDirection) {
        sameDirectionCount++
        const regime = calculatePositionRegime(t.decisionScore?.finalScore || t.score24h)
        if (regime.regime === 'FULL' || regime.regime === 'STANDARD') {
          fullRegimeCount++
        }
      }
    })
  })
  
  // 确定拥挤风险等级
  let riskLevel: 'low' | 'medium' | 'high' | 'extreme' = 'low'
  let positionCapOverlay = 1.0
  let warningMessage = ''
  
  if (fullRegimeCount >= 6) {
    riskLevel = 'extreme'
    positionCapOverlay = 0.5
    warningMessage = `EXTREME CROWDING: ${fullRegimeCount} assets same-direction FULL, reduce 50%`
  } else if (fullRegimeCount >= 4) {
    riskLevel = 'high'
    positionCapOverlay = 0.7
    warningMessage = `HIGH CROWDING: ${fullRegimeCount} assets same-direction, reduce 30%`
  } else if (fullRegimeCount >= 2) {
    riskLevel = 'medium'
    positionCapOverlay = 0.85
    warningMessage = `MODERATE CROWDING: ${fullRegimeCount} assets same-direction, diversify`
  } else {
    warningMessage = 'CROWDING RISK: LOW'
  }
  
  return {
    riskLevel,
    sameDirectionCount,
    fullRegimeCount,
    positionCapOverlay,
    warningMessage
  }
}

// ============== 🆕 PHASE 3: 假信号审�?==============

const generateFalseSignalAudit = (topics: Topic[]): FalseSignalAudit => {
  let highScoreNoReactionCount = 0
  let bigMoveMissedCount = 0
  const highScoreRootCauses: ('priced_in' | 'execution_failed' | 'conflict' | 'coverage_gap')[] = []
  const bigMoveRootCauses: ('taxonomy_gap' | 'entity_mapping_gap' | 'source_gap')[] = []
  
  topics.forEach(t => {
    if (!t.validation) return
    
    // 检查高分无反应
    if (t.score24h >= 70 && t.validation.avgPostMove24h < 1) {
      highScoreNoReactionCount++
      // 分类根因
      if (t.validation.avgPreMove > t.validation.avgPostMove) {
        if (!highScoreRootCauses.includes('priced_in')) highScoreRootCauses.push('priced_in')
      } else if (t.state === 'contested') {
        if (!highScoreRootCauses.includes('conflict')) highScoreRootCauses.push('conflict')
      } else if (!t.inPolicyLoop) {
        if (!highScoreRootCauses.includes('execution_failed')) highScoreRootCauses.push('execution_failed')
      } else {
        if (!highScoreRootCauses.includes('coverage_gap')) highScoreRootCauses.push('coverage_gap')
      }
    }
    
    // 检查大波动未捕�?(假设有此数据)
    if (t.validation.avgPostMove24h > 3 && t.score24h < 50) {
      bigMoveMissedCount++
      // 分类根因
      if (t.entities.length < 2) {
        if (!bigMoveRootCauses.includes('entity_mapping_gap')) bigMoveRootCauses.push('entity_mapping_gap')
      } else if (t.l0Count === 0 && t.l05Count === 0) {
        if (!bigMoveRootCauses.includes('source_gap')) bigMoveRootCauses.push('source_gap')
      } else {
        if (!bigMoveRootCauses.includes('taxonomy_gap')) bigMoveRootCauses.push('taxonomy_gap')
      }
    }
  })
  
  const totalSignals = topics.length
  const falsePositiveRate = totalSignals > 0 ? highScoreNoReactionCount / totalSignals : 0
  const falseNegativeRate = totalSignals > 0 ? bigMoveMissedCount / totalSignals : 0
  
  // 审计摘要
  let auditSummary = ''
  if (falsePositiveRate > 0.2) {
    auditSummary = `⚠️ 假阳性率偏高 (${(falsePositiveRate * 100).toFixed(0)}%)，主�? ${highScoreRootCauses.join(', ')}`
  } else if (falseNegativeRate > 0.1) {
    auditSummary = `⚠️ 漏报率偏�?(${(falseNegativeRate * 100).toFixed(0)}%)，主�? ${bigMoveRootCauses.join(', ')}`
  } else {
    auditSummary = `�?信号质量良好，假阳�?{(falsePositiveRate * 100).toFixed(0)}%，漏�?{(falseNegativeRate * 100).toFixed(0)}%`
  }
  
  return {
    highScoreNoReaction: {
      count: highScoreNoReactionCount,
      rootCauses: highScoreRootCauses
    },
    bigMoveMissed: {
      count: bigMoveMissedCount,
      rootCauses: bigMoveRootCauses
    },
    totalSignals,
    falsePositiveRate,
    falseNegativeRate,
    auditSummary
  }
}

// ============== 🆕 周报/晨会报告生成 ==============

const generateReport = (
  topics: Topic[],
  type: 'morning' | 'weekly'
): ReportOutput => {
  const now = new Date()
  
  // 过滤可交易主�?
  const tradeableTopics = topics.filter(t => 
    t.decisionScore && t.decisionScore.actionTier !== 'no-trade'
  ).sort((a, b) => (b.decisionScore?.finalScore || 0) - (a.decisionScore?.finalScore || 0))
  
  // 生成持仓建议
  const positionRecommendations = tradeableTopics.slice(0, 5).flatMap(t => 
    (t.tradeableAssets || []).slice(0, 2).map(asset => ({
      ticker: asset.ticker,
      regime: t.positionRegime?.regime || 'NO_TRADE',
      direction: asset.exposure === 'bullish' ? 'LONG' as const : 
                 asset.exposure === 'bearish' ? 'SHORT' as const : 'NEUTRAL' as const,
      conviction: t.stance?.conviction || 'NONE',
      keyReason: t.stance?.stanceText || t.name
    }))
  )
  
  // 生成风险提示
  const riskAlerts = topics
    .filter(t => t.crowdingRisk?.riskLevel === 'high' || t.crowdingRisk?.riskLevel === 'extreme')
    .map(t => ({
      type: '拥挤风险',
      severity: t.crowdingRisk?.riskLevel === 'extreme' ? 'high' as const : 'medium' as const,
      description: t.crowdingRisk?.warningMessage || ''
    }))
  
  // 添加漂移风险
  topics.filter(t => t.driftMetrics.riskLevel === 'high' || t.driftMetrics.riskLevel === 'critical')
    .forEach(t => {
      riskAlerts.push({
        type: '叙事漂移',
        severity: t.driftMetrics.riskLevel === 'critical' ? 'high' as const : 'medium' as const,
        description: `${t.name} 叙事漂移${t.driftMetrics.riskLevel}级别`
      })
    })
  
  // Generate watch list
  const watchList = topics
    .filter(t => t.decisionScore?.actionTier === 'watch')
    .slice(0, 5)
    .map(t => ({
      ticker: t.tradeableAssets?.[0]?.ticker || t.name,
      trigger: t.noTradeInfo?.watchTriggers?.[0] || 'Await signal strengthening',
      expectedAction: t.stance?.directionalBias === 'LONG' ? 'Long on breakout' :
                      t.stance?.directionalBias === 'SHORT' ? 'Short on breakdown' : 'Pending'
    }))
  
  // Validation review
  const topicsWithValidation = topics.filter(t => t.validation)
  const avgHitRate = topicsWithValidation.length > 0
    ? topicsWithValidation.reduce((sum, t) => sum + (t.validation?.directionHitRatio || 0), 0) / topicsWithValidation.length
    : 0
  const avgMove = topicsWithValidation.length > 0
    ? topicsWithValidation.reduce((sum, t) => sum + (t.validation?.avgPostMove24h || 0), 0) / topicsWithValidation.length
    : 0
  
  // Executive summary
  const highConvictionCount = topics.filter(t => t.positionRegime?.regime === 'FULL').length
  const executiveSummary = type === 'morning'
    ? `Today: ${highConvictionCount} high conviction signals, ${riskAlerts.filter(r => r.severity === 'high').length} major risks to watch`
    : `This week: ${positionRecommendations.length} position recommendations, avg hit rate ${(avgHitRate * 100).toFixed(0)}%`
  
  return {
    type,
    generatedAt: now.toISOString(),
    executiveSummary,
    positionRecommendations,
    riskAlerts,
    watchList,
    validationReview: {
      hitRate: avgHitRate,
      avgMove,
      bestCall: tradeableTopics[0]?.name || 'N/A',
      worstCall: 'Pending verification'
    }
  }
}

// ============== DAILY BRIEF GENERATION ==============

const generateDailyBrief = (topics: Topic[]): DailyBrief => {
  const now = new Date()
  
  // Sort by DecisionScore
  const sorted = [...topics].sort((a, b) => 
    (b.decisionScore?.finalScore || 0) - (a.decisionScore?.finalScore || 0)
  )
  
  // Top Trades: FULL or STANDARD regime
  const topTrades = sorted
    .filter(t => t.positionRegime?.regime === 'FULL' || t.positionRegime?.regime === 'STANDARD')
    .slice(0, 5)
    .map((t, i) => ({
      rank: i + 1,
      ticker: t.tradeableAssets?.[0]?.ticker || t.name.slice(0, 8),
      direction: (t.stance?.directionalBias === 'LONG' ? 'LONG' : 'SHORT') as 'LONG' | 'SHORT',
      regime: t.positionRegime?.regime || 'NO_TRADE' as PositionRegime,
      decisionScore: t.decisionScore?.finalScore || 0,
      stance: t.stance?.stanceText || '',
      timing: t.timing?.timing || 'OPTIMAL' as TimingAssessment,
      pricingIn: t.timing?.pricingInIndex || 0,
      keyRisk: t.stance?.mainRisk || '',
      topicId: t.id
    }))
  
  // Watchlist: watch tier approaching actionable
  const watchlist = sorted
    .filter(t => t.decisionScore?.actionTier === 'watch' && (t.decisionScore?.finalScore || 0) >= 35)
    .slice(0, 5)
    .map(t => ({
      ticker: t.tradeableAssets?.[0]?.ticker || t.name.slice(0, 8),
      currentScore: t.decisionScore?.finalScore || 0,
      triggerCondition: t.noTradeInfo?.watchTriggers?.[0] || 'L0 confirmation',
      expectedDirection: (t.stance?.directionalBias === 'LONG' ? 'LONG' : 'SHORT') as 'LONG' | 'SHORT',
      estimatedTimeToAction: t.signalHalfLife?.estimatedRemainingLife 
        ? `${t.signalHalfLife.estimatedRemainingLife.toFixed(0)}h` 
        : '24-48h'
    }))
  
  // Do Not Trade
  const doNotTrade = sorted
    .filter(t => t.positionRegime?.regime === 'NO_TRADE')
    .slice(0, 5)
    .map(t => ({
      ticker: t.tradeableAssets?.[0]?.ticker || t.name.slice(0, 8),
      reasons: t.noTradeInfo?.reasonLabels || ['Insufficient signal'],
      avoidUntil: t.noTradeInfo?.watchTriggers?.[0] || 'Signal strengthens'
    }))
  
  // Key Risks
  const keyRisks: DailyBrief['keyRisks'] = []
  
  // Crowding risks
  const crowdedTopics = topics.filter(t => 
    t.crowdingRisk?.riskLevel === 'high' || t.crowdingRisk?.riskLevel === 'extreme'
  )
  if (crowdedTopics.length > 0) {
    keyRisks.push({
      risk: 'Portfolio Crowding',
      severity: 'HIGH',
      affectedAssets: crowdedTopics.flatMap(t => t.tradeableAssets?.map(a => a.ticker) || []).slice(0, 5),
      mitigationAction: 'Reduce position sizes by 30%'
    })
  }
  
  // Drift risks
  const driftTopics = topics.filter(t => 
    t.driftMetrics.riskLevel === 'high' || t.driftMetrics.riskLevel === 'critical'
  )
  if (driftTopics.length > 0) {
    keyRisks.push({
      risk: 'Narrative Drift',
      severity: driftTopics.some(t => t.driftMetrics.riskLevel === 'critical') ? 'HIGH' : 'MEDIUM',
      affectedAssets: driftTopics.flatMap(t => t.tradeableAssets?.map(a => a.ticker) || []).slice(0, 5),
      mitigationAction: 'Monitor for reversal signals'
    })
  }
  
  // Timing risks
  const lateTopics = topics.filter(t => t.timing?.timing === 'LATE')
  if (lateTopics.length > 2) {
    keyRisks.push({
      risk: 'Late Entry Risk',
      severity: 'MEDIUM',
      affectedAssets: lateTopics.flatMap(t => t.tradeableAssets?.map(a => a.ticker) || []).slice(0, 5),
      mitigationAction: 'Consider reduced sizing or wait for pullback'
    })
  }
  
  // What Changed (simulated)
  const whatChanged: DailyBrief['whatChanged'] = []
  const newHighConviction = topics.filter(t => 
    t.decisionScore?.actionTier === 'high-conviction' && t.state === 'implementing'
  )
  if (newHighConviction.length > 0) {
    whatChanged.push({
      type: 'REGIME_CHANGE',
      description: `${newHighConviction.length} topic(s) upgraded to FULL regime`,
      impact: newHighConviction.map(t => t.tradeableAssets?.[0]?.ticker || t.name).join(', ')
    })
  }
  
  return {
    date: now.toISOString().split('T')[0],
    generatedAt: now.toISOString(),
    topTrades,
    watchlist,
    doNotTrade,
    keyRisks,
    whatChanged
  }
}

// ============== IC MEMO GENERATION ==============

const generateICMemo = (topic: Topic): ICMemo => {
  const now = new Date()
  
  return {
    id: `ic-${topic.id}-${Date.now()}`,
    generatedAt: now.toISOString(),
    topic: topic.name,
    
    recommendation: topic.positionRegime?.regime === 'FULL' ? 'INITIATE' :
                    topic.positionRegime?.regime === 'STANDARD' ? 'ADD' :
                    topic.positionRegime?.regime === 'NO_TRADE' ? 'NO_ACTION' : 'NO_ACTION',
    conviction: topic.stance?.conviction === 'NONE' ? 'LOW' : (topic.stance?.conviction || 'LOW'),
    targetAssets: topic.tradeableAssets?.map(a => a.ticker) || [],
    
    decisionScore: topic.decisionScore?.finalScore || 0,
    positionRegime: topic.positionRegime?.regime || 'NO_TRADE',
    timing: topic.timing?.timing || 'OPTIMAL',
    pricingIn: topic.timing?.pricingInIndex || 0,
    
    thesis: [
      topic.stance?.stanceText || '',
      `Policy state: ${topic.state} - ${POLICY_STATE_DEFINITIONS[topic.state]?.recommendedStyle || ''}`,
      `Signal persistence: ${topic.signalHalfLife?.persistenceLabel || 'Unknown'}`
    ].filter(Boolean),
    
    evidenceChain: topic.documents.slice(0, 4).map(doc => ({
      level: doc.source?.level || 'L2',
      source: doc.source?.name || 'Unknown',
      quote: doc.quotePrimary || doc.summary.slice(0, 100),
      date: doc.publishedAt
    })),
    
    risks: [
      { risk: topic.stance?.mainRisk || 'Execution risk', probability: 'MEDIUM', mitigation: 'Stop-loss at key level' },
      ...(topic.driftMetrics.riskLevel !== 'low' 
        ? [{ risk: 'Narrative drift', probability: 'MEDIUM' as const, mitigation: 'Monitor L0/L1 sources' }] 
        : []),
      ...(topic.crowdingRisk?.riskLevel !== 'low'
        ? [{ risk: 'Crowding risk', probability: topic.crowdingRisk?.riskLevel === 'high' ? 'HIGH' as const : 'MEDIUM' as const, mitigation: 'Reduce size' }]
        : [])
    ].slice(0, 3),
    
    invalidationTriggers: topic.infoHierarchy?.invalidationTriggers || ['L0 reversal', 'Key support break'],
    
    suggestedSize: topic.positionRegime?.regime === 'FULL' ? '100% of allocation' :
                   topic.positionRegime?.regime === 'STANDARD' ? '66% of allocation' :
                   topic.positionRegime?.regime === 'STARTER' ? '33% of allocation' : '0%',
    maxLoss: '2% of NAV'
  }
}

// ============== EXECUTION PACKET GENERATION ==============

const generateExecutionPacket = (topic: Topic, asset: TradeableAsset): ExecutionPacket => {
  const now = new Date()
  const expiresAt = new Date(now.getTime() + 24 * 60 * 60 * 1000) // 24h expiry
  
  return {
    packetId: `exec-${topic.id}-${asset.ticker}-${Date.now()}`,
    generatedAt: now.toISOString(),
    expiresAt: expiresAt.toISOString(),
    
    signal: {
      ticker: asset.ticker,
      direction: asset.exposure === 'bullish' ? 'LONG' : 'SHORT',
      regime: topic.positionRegime?.regime || 'NO_TRADE',
      decisionScore: topic.decisionScore?.finalScore || 0,
      conviction: topic.stance?.conviction === 'NONE' ? 'LOW' : (topic.stance?.conviction || 'LOW')
    },
    
    sizing: {
      positionCap: topic.positionRegime?.positionCap || 0,
      suggestedNotional: topic.positionRegime?.regime === 'FULL' ? '$1M' :
                         topic.positionRegime?.regime === 'STANDARD' ? '$660K' :
                         topic.positionRegime?.regime === 'STARTER' ? '$330K' : '$0',
      maxLossPercent: 2
    },
    
    timing: {
      assessment: topic.timing?.timing || 'OPTIMAL',
      pricingIn: topic.timing?.pricingInIndex || 0,
      urgency: topic.timing?.timing === 'EARLY' ? 'IMMEDIATE' :
               topic.timing?.timing === 'OPTIMAL' ? 'TODAY' : 'MONITOR'
    },
    
    validation: {
      credibilityGrade: topic.credibility?.grade || 'C',
      directionHitRate: topic.credibility?.directionHitRatio || 0,
      avgMove: topic.validation?.avgPostMove24h || 0
    },
    
    risks: [
      topic.stance?.mainRisk || '',
      topic.crowdingRisk?.warningMessage || '',
      topic.driftMetrics.riskLevel !== 'low' ? `Drift: ${topic.driftMetrics.riskLevel}` : ''
    ].filter(Boolean),
    
    invalidation: {
      triggers: topic.infoHierarchy?.invalidationTriggers || [],
      stopLoss: '-2%'
    },
    
    sourceTopicId: topic.id,
    evidenceCount: topic.documents.length
  }
}

// ============== PORTFOLIO RISK OVERLAY ==============

const calculatePortfolioRiskOverlay = (topics: Topic[]): PortfolioRiskOverlay => {
  const now = new Date()
  
  // Count positions by direction
  let longCount = 0
  let shortCount = 0
  topics.forEach(t => {
    if (t.positionRegime?.regime !== 'NO_TRADE') {
      t.tradeableAssets?.forEach(a => {
        if (a.exposure === 'bullish') longCount++
        else if (a.exposure === 'bearish') shortCount++
      })
    }
  })
  
  // Crowding
  const totalPositions = longCount + shortCount
  const crowdingLevel = totalPositions >= 15 ? 'EXTREME' :
                        totalPositions >= 10 ? 'HIGH' :
                        totalPositions >= 5 ? 'MEDIUM' : 'LOW'
  
  // Domain concentration
  const domainExposure: Record<Domain, number> = {
    trade: 0, sanction: 0, war: 0, rate: 0, fiscal: 0, regulation: 0, export_control: 0, antitrust: 0
  }
  topics.forEach(t => {
    if (t.positionRegime?.regime !== 'NO_TRADE') {
      domainExposure[t.domain] += t.positionRegime?.positionCap || 0
    }
  })
  
  const domainConcentration: PortfolioRiskOverlay['domainConcentration'] = {} as any
  Object.entries(domainExposure).forEach(([domain, exposure]) => {
    const totalExposure = Object.values(domainExposure).reduce((a, b) => a + b, 0)
    const riskContribution = totalExposure > 0 ? exposure / totalExposure : 0
    domainConcentration[domain as Domain] = {
      exposure,
      riskContribution,
      warning: riskContribution > 0.4 ? `High ${domain} concentration` : undefined
    }
  })
  
  // Overall risk
  const overallRiskScore = 
    (crowdingLevel === 'EXTREME' ? 40 : crowdingLevel === 'HIGH' ? 25 : crowdingLevel === 'MEDIUM' ? 10 : 0) +
    Math.max(...Object.values(domainConcentration).map(d => d.riskContribution)) * 40 +
    topics.filter(t => t.crowdingRisk?.riskLevel === 'high').length * 5
  
  return {
    timestamp: now.toISOString(),
    crowding: {
      level: crowdingLevel,
      longCount,
      shortCount,
      concentrationRisk: Math.abs(longCount - shortCount) / Math.max(totalPositions, 1),
      warning: crowdingLevel === 'EXTREME' ? 'Reduce overall exposure immediately' :
               crowdingLevel === 'HIGH' ? 'Consider reducing positions' : ''
    },
    correlation: {
      avgPairwiseCorrelation: 0.45, // Simulated
      highlyCorrelatedPairs: [],
      diversificationScore: 0.6
    },
    domainConcentration,
    overallRiskScore: Math.min(100, overallRiskScore),
    maxRecommendedExposure: overallRiskScore > 60 ? 0.5 : overallRiskScore > 40 ? 0.7 : 1.0,
    actionRequired: overallRiskScore > 60 
      ? ['Reduce position sizes', 'Increase hedges', 'Avoid new positions']
      : overallRiskScore > 40
        ? ['Monitor closely', 'Consider hedges']
        : []
  }
}

// ============== REAL-TIME ALERT GENERATION ==============

const generateRealTimeAlerts = (topics: Topic[]): RealTimeAlert[] => {
  const alerts: RealTimeAlert[] = []
  const now = new Date()
  
  topics.forEach(t => {
    // High conviction new signals
    if (t.decisionScore?.actionTier === 'high-conviction' && t.l0Count > 0) {
      alerts.push({
        id: `alert-${t.id}-l0-${Date.now()}`,
        timestamp: now.toISOString(),
        type: 'NEW_L0',
        severity: 'CRITICAL',
        title: `L0 Signal: ${t.name}`,
        message: `New L0 source confirmed. DecisionScore: ${t.decisionScore.finalScore.toFixed(0)}`,
        affectedAssets: t.tradeableAssets?.map(a => a.ticker) || [],
        actionRequired: `${t.positionRegime?.regime} position recommended`,
        read: false
      })
    }
    
    // State transitions to implementing
    if (t.state === 'implementing' && t.inPolicyLoop) {
      alerts.push({
        id: `alert-${t.id}-state-${Date.now()}`,
        timestamp: now.toISOString(),
        type: 'STATE_TRANSITION',
        severity: 'HIGH',
        title: `Implementing: ${t.name}`,
        message: 'Policy entered execution phase with loop confirmation',
        affectedAssets: t.tradeableAssets?.map(a => a.ticker) || [],
        actionRequired: 'Consider position initiation',
        read: false
      })
    }
    
    // Crowding warnings
    if (t.crowdingRisk?.riskLevel === 'extreme') {
      alerts.push({
        id: `alert-${t.id}-crowd-${Date.now()}`,
        timestamp: now.toISOString(),
        type: 'CROWDING_WARNING',
        severity: 'HIGH',
        title: 'Crowding Risk',
        message: t.crowdingRisk.warningMessage,
        affectedAssets: t.tradeableAssets?.map(a => a.ticker) || [],
        actionRequired: 'Reduce position sizes',
        read: false
      })
    }
    
    // Conflict detection
    if (t.state === 'contested') {
      alerts.push({
        id: `alert-${t.id}-conflict-${Date.now()}`,
        timestamp: now.toISOString(),
        type: 'CONFLICT_DETECTED',
        severity: 'MEDIUM',
        title: `Conflict: ${t.name}`,
        message: 'Multiple opposing signals detected',
        affectedAssets: t.tradeableAssets?.map(a => a.ticker) || [],
        actionRequired: 'Avoid new positions until resolved',
        read: false
      })
    }
  })
  
  return alerts.sort((a, b) => {
    const severityOrder = { CRITICAL: 0, HIGH: 1, MEDIUM: 2, LOW: 3 }
    return severityOrder[a.severity] - severityOrder[b.severity]
  })
}

// ============== POLICY PLAYBOOKS ==============

const POLICY_PLAYBOOKS: PolicyPlaybook[] = [
  {
    state: 'emerging',
    playbook: {
      recommendedAction: 'Scout position only',
      positionSizing: '10-20% of target',
      entryTiming: 'Wait for L0.5 confirmation',
      exitTriggers: ['No follow-through in 48h', 'Contradicting L0 signal'],
      riskManagement: 'Tight stops, small size',
      historicalWinRate: 0.45,
      avgHoldingPeriod: '1-3 days',
      typicalPnL: '+/- 1%'
    }
  },
  {
    state: 'contested',
    playbook: {
      recommendedAction: 'No directional bias - options strategies',
      positionSizing: 'Straddles/Strangles only',
      entryTiming: 'On volatility compression',
      exitTriggers: ['Resolution signal', 'Volatility spike'],
      riskManagement: 'Defined risk via options',
      historicalWinRate: 0.55,
      avgHoldingPeriod: '1-7 days',
      typicalPnL: '+/- 3%'
    }
  },
  {
    state: 'implementing',
    playbook: {
      recommendedAction: 'Full conviction directional',
      positionSizing: '100% of target allocation',
      entryTiming: 'Immediate on confirmation',
      exitTriggers: ['Target reached', 'State change to digesting', 'Invalidation trigger'],
      riskManagement: 'Trail stops as profit develops',
      historicalWinRate: 0.72,
      avgHoldingPeriod: '3-14 days',
      typicalPnL: '+5-15%'
    }
  },
  {
    state: 'digesting',
    playbook: {
      recommendedAction: 'Take profits, reduce size',
      positionSizing: '50% of original',
      entryTiming: 'No new entries',
      exitTriggers: ['Mean reversion signal', 'New catalyst'],
      riskManagement: 'Lock in profits, tighten stops',
      historicalWinRate: 0.50,
      avgHoldingPeriod: '1-5 days',
      typicalPnL: '+/- 2%'
    }
  },
  {
    state: 'exhausted',
    playbook: {
      recommendedAction: 'Exit all positions',
      positionSizing: '0%',
      entryTiming: 'Do not enter',
      exitTriggers: ['Any remaining position'],
      riskManagement: 'Full exit',
      historicalWinRate: 0.30,
      avgHoldingPeriod: '0 days',
      typicalPnL: 'N/A'
    }
  },
  {
    state: 'reversed',
    playbook: {
      recommendedAction: 'Fade original direction',
      positionSizing: '50% in opposite direction',
      entryTiming: 'On confirmation of reversal',
      exitTriggers: ['Original trend resumes', 'New L0 signal'],
      riskManagement: 'Aggressive stops',
      historicalWinRate: 0.58,
      avgHoldingPeriod: '2-7 days',
      typicalPnL: '+3-8%'
    }
  }
]

// ============== 模拟数据 ==============

// 全球权威机构来源 - 扩展版(含EU Source Hierarchy)
const SOURCES: Record<string, Source> = {
  // ========== 美国 L0 最高权威 ==========
  'trump-truth': { id: 'trump-truth', name: 'Trump Truth Social', level: 'L0', domain: ['trade', 'sanction', 'war'], executionPower: 100, authority: 100, region: 'US' as SourceRegion },
  'whitehouse': { id: 'whitehouse', name: 'White House Official', level: 'L0', domain: ['trade', 'sanction', 'fiscal', 'regulation'], executionPower: 95, authority: 100, region: 'US' as SourceRegion },
  
  // ========== 美国 L0.5 执行机构 ==========
  'treasury': { id: 'treasury', name: 'US Treasury', level: 'L0.5', domain: ['sanction', 'fiscal', 'rate'], executionPower: 90, authority: 95, region: 'US' as SourceRegion },
  'fed': { id: 'fed', name: 'Federal Reserve', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 95, authority: 98, region: 'US' as SourceRegion },
  'commerce': { id: 'commerce', name: 'Commerce (BIS)', level: 'L0.5', domain: ['trade', 'sanction'], executionPower: 85, authority: 90, region: 'US' as SourceRegion },
  'ustr': { id: 'ustr', name: 'USTR', level: 'L0.5', domain: ['trade'], executionPower: 88, authority: 92, region: 'US' as SourceRegion },
  'dod': { id: 'dod', name: 'DoD', level: 'L0.5', domain: ['war', 'sanction'], executionPower: 92, authority: 95, region: 'US' as SourceRegion },
  'sec': { id: 'sec', name: 'SEC', level: 'L0.5', domain: ['regulation', 'fiscal'], executionPower: 85, authority: 88, region: 'US' as SourceRegion },
  'cftc': { id: 'cftc', name: 'CFTC', level: 'L0.5', domain: ['regulation', 'fiscal'], executionPower: 82, authority: 85, region: 'US' as SourceRegion },
  
  // ========== EU-L0 Agenda Setters (低执行力, 仅触发Emerging) ==========
  'eu-commission-president': { id: 'eu-commission-president', name: 'EU Commission President', level: 'L0', domain: ['trade', 'regulation', 'sanction'], executionPower: 35, authority: 70, region: 'EU' as SourceRegion },
  'eu-high-representative': { id: 'eu-high-representative', name: 'EU High Representative', level: 'L0', domain: ['sanction', 'war'], executionPower: 30, authority: 68, region: 'EU' as SourceRegion },
  
  // ========== EU-L0.5 Rule Makers (核心EU Alpha来源) ==========
  'eu-dg-trade': { id: 'eu-dg-trade', name: 'DG TRADE', level: 'L0.5', domain: ['trade', 'sanction'], executionPower: 80, authority: 88, region: 'EU' as SourceRegion },
  'eu-dg-comp': { id: 'eu-dg-comp', name: 'DG COMP (Competition)', level: 'L0.5', domain: ['regulation'], executionPower: 85, authority: 90, region: 'EU' as SourceRegion },
  'eu-dg-fisma': { id: 'eu-dg-fisma', name: 'DG FISMA (Financial)', level: 'L0.5', domain: ['regulation', 'fiscal'], executionPower: 75, authority: 86, region: 'EU' as SourceRegion },
  'eu-dg-connect': { id: 'eu-dg-connect', name: 'DG CONNECT (Digital)', level: 'L0.5', domain: ['regulation'], executionPower: 70, authority: 84, region: 'EU' as SourceRegion },
  'eu-council': { id: 'eu-council', name: 'EU Council / COREPER', level: 'L0.5', domain: ['trade', 'regulation', 'sanction'], executionPower: 65, authority: 82, region: 'EU' as SourceRegion },
  
  // ========== 欧洲央行 (独立L0.5) ==========
  'ecb': { id: 'ecb', name: 'European Central Bank', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 92, authority: 95, region: 'EU' as SourceRegion },
  'bundesbank': { id: 'bundesbank', name: 'Bundesbank', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 78, authority: 82, region: 'EU' as SourceRegion },
  'boe': { id: 'boe', name: 'Bank of England', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 88, authority: 90 },
  'snb': { id: 'snb', name: 'Swiss National Bank', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 85, authority: 88 },
  
  // ========== EU-L1 Market Amplifiers (确认/漂移/验证) ==========
  'reuters-eu': { id: 'reuters-eu', name: 'Reuters EU', level: 'L1', tier: 'A', domain: ['trade', 'regulation', 'sanction'], executionPower: 0, authority: 75, region: 'EU' as SourceRegion },
  'bloomberg-eu': { id: 'bloomberg-eu', name: 'Bloomberg EU', level: 'L1', tier: 'A', domain: ['trade', 'regulation', 'fiscal'], executionPower: 0, authority: 78, region: 'EU' as SourceRegion },
  'politico-eu': { id: 'politico-eu', name: 'Politico Europe', level: 'L1', tier: 'B', domain: ['trade', 'regulation'], executionPower: 0, authority: 72, region: 'EU' as SourceRegion },
  'ft-eu': { id: 'ft-eu', name: 'Financial Times Europe', level: 'L1', tier: 'A', domain: ['trade', 'regulation', 'fiscal'], executionPower: 0, authority: 76, region: 'EU' as SourceRegion },
  
  // ========== EU-L2 Process Noise (仅风险检�? 不影响DecisionScore) ==========
  'germany-finance': { id: 'germany-finance', name: 'German Finance Ministry', level: 'L2', domain: ['fiscal', 'regulation'], executionPower: 20, authority: 55, region: 'EU' as SourceRegion },
  'france-economy': { id: 'france-economy', name: 'French Economy Ministry', level: 'L2', domain: ['fiscal', 'trade'], executionPower: 18, authority: 52, region: 'EU' as SourceRegion },
  'italy-industry': { id: 'italy-industry', name: 'Italian Industry Ministry', level: 'L2', domain: ['trade', 'regulation'], executionPower: 15, authority: 48, region: 'EU' as SourceRegion },
  'mep-individual': { id: 'mep-individual', name: 'Individual MEP', level: 'L2', domain: ['regulation'], executionPower: 10, authority: 35, region: 'EU' as SourceRegion },
  'digital-europe': { id: 'digital-europe', name: 'DigitalEurope (Industry)', level: 'L2', domain: ['regulation'], executionPower: 5, authority: 30, region: 'EU' as SourceRegion },
  'acea': { id: 'acea', name: 'ACEA (Auto Industry)', level: 'L2', domain: ['trade', 'regulation'], executionPower: 5, authority: 32, region: 'EU' as SourceRegion },
  
  // ========== 亚洲权威机构 ==========
  'pboc': { id: 'pboc', name: 'PBoC (中国央行)', level: 'L0.5', domain: ['rate', 'fiscal', 'trade'], executionPower: 92, authority: 95, region: 'CN' as SourceRegion },
  'mofcom': { id: 'mofcom', name: 'MOFCOM (中国商务部)', level: 'L0.5', domain: ['trade', 'sanction'], executionPower: 88, authority: 90, region: 'CN' as SourceRegion },
  'ndrc': { id: 'ndrc', name: 'NDRC (发改委)', level: 'L0.5', domain: ['regulation', 'fiscal'], executionPower: 90, authority: 92, region: 'CN' as SourceRegion },
  'boj': { id: 'boj', name: 'Bank of Japan', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 90, authority: 92, region: 'INTL' as SourceRegion },
  'meti': { id: 'meti', name: 'METI (日本经产省)', level: 'L0.5', domain: ['trade', 'regulation'], executionPower: 82, authority: 85, region: 'INTL' as SourceRegion },
  'rbi': { id: 'rbi', name: 'Reserve Bank of India', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 85, authority: 88, region: 'INTL' as SourceRegion },
  'mas': { id: 'mas', name: 'MAS (新加坡金管局)', level: 'L0.5', domain: ['rate', 'regulation'], executionPower: 88, authority: 90, region: 'INTL' as SourceRegion },
  
  // ========== 国际组织 ==========
  'imf': { id: 'imf', name: 'IMF', level: 'L0.5', domain: ['fiscal', 'rate'], executionPower: 60, authority: 85, region: 'INTL' as SourceRegion },
  'worldbank': { id: 'worldbank', name: 'World Bank', level: 'L0.5', domain: ['fiscal'], executionPower: 55, authority: 82, region: 'INTL' as SourceRegion },
  'wto': { id: 'wto', name: 'WTO', level: 'L0.5', domain: ['trade'], executionPower: 50, authority: 78, region: 'INTL' as SourceRegion },
  'bis': { id: 'bis', name: 'BIS (国际清算银行)', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 45, authority: 80, region: 'INTL' as SourceRegion },
  
  // ========== L1 权威媒体 ==========
  'reuters': { id: 'reuters', name: 'Reuters', level: 'L1', tier: 'A', domain: ['trade', 'sanction', 'war', 'rate'], executionPower: 0, authority: 85 },
  'bloomberg': { id: 'bloomberg', name: 'Bloomberg', level: 'L1', tier: 'A', domain: ['trade', 'rate', 'fiscal'], executionPower: 0, authority: 88 },
  'wsj': { id: 'wsj', name: 'Wall Street Journal', level: 'L1', tier: 'A', domain: ['trade', 'rate', 'regulation'], executionPower: 0, authority: 85 },
  'ft': { id: 'ft', name: 'Financial Times', level: 'L1', tier: 'A', domain: ['trade', 'rate', 'fiscal'], executionPower: 0, authority: 86 },
  'nyt': { id: 'nyt', name: 'New York Times', level: 'L1', tier: 'B', domain: ['trade', 'war', 'regulation'], executionPower: 0, authority: 80 },
  'scmp': { id: 'scmp', name: 'South China Morning Post', level: 'L1', tier: 'B', domain: ['trade', 'war'], executionPower: 0, authority: 75 },
  'nikkei': { id: 'nikkei', name: 'Nikkei Asia', level: 'L1', tier: 'B', domain: ['trade', 'rate'], executionPower: 0, authority: 78 },
  
  // ========== 英国/英联邦官方机�?==========
  'uk-treasury': { id: 'uk-treasury', name: 'HM Treasury', level: 'L0.5', domain: ['fiscal', 'regulation'], executionPower: 85, authority: 88 },
  'uk-fca': { id: 'uk-fca', name: 'FCA (UK Financial)', level: 'L0.5', domain: ['regulation'], executionPower: 82, authority: 86 },
  'uk-trade': { id: 'uk-trade', name: 'UK Trade Dept', level: 'L0.5', domain: ['trade', 'sanction'], executionPower: 78, authority: 82 },
  'rba': { id: 'rba', name: 'Reserve Bank of Australia', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 85, authority: 88 },
  'rbnz': { id: 'rbnz', name: 'Reserve Bank of NZ', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 82, authority: 85 },
  'bok': { id: 'bok', name: 'Bank of Korea', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 85, authority: 88 },
  
  // ========== 中东/新兴市场官方机构 ==========
  'sama': { id: 'sama', name: 'SAMA (Saudi Central Bank)', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 82, authority: 85 },
  'cbuae': { id: 'cbuae', name: 'UAE Central Bank', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 80, authority: 82 },
  'cbrt': { id: 'cbrt', name: 'CBRT (Turkey Central)', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 75, authority: 78 },
  'sarb': { id: 'sarb', name: 'SARB (South Africa)', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 78, authority: 80 },
  'bcb': { id: 'bcb', name: 'BCB (Brazil Central)', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 82, authority: 85 },
  'banxico': { id: 'banxico', name: 'Banxico (Mexico)', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 80, authority: 82 },
  
  // ========== 能源/商品相关官方机构 ==========
  'opec': { id: 'opec', name: 'OPEC Secretariat', level: 'L0', domain: ['trade', 'fiscal'], executionPower: 75, authority: 85 },
  'iea': { id: 'iea', name: 'IEA (Intl Energy)', level: 'L0.5', domain: ['trade', 'fiscal'], executionPower: 55, authority: 82 },
  'eia': { id: 'eia', name: 'EIA (US Energy Info)', level: 'L0.5', domain: ['trade', 'fiscal'], executionPower: 65, authority: 85 },
  'russia-cb': { id: 'russia-cb', name: 'Bank of Russia', level: 'L0.5', domain: ['rate', 'fiscal', 'sanction'], executionPower: 85, authority: 88 },
  'russia-minec': { id: 'russia-minec', name: 'Russia MinEcon', level: 'L0.5', domain: ['trade', 'sanction'], executionPower: 75, authority: 78 },
  
  // ========== 其他亚太官方机构 ==========
  'hkma': { id: 'hkma', name: 'HKMA (Hong Kong)', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 82, authority: 85 },
  'cbc-taiwan': { id: 'cbc-taiwan', name: 'CBC (Taiwan Central)', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 80, authority: 82 },
  'china-miit': { id: 'china-miit', name: 'MIIT (工信部)', level: 'L0.5', domain: ['trade', 'regulation'], executionPower: 88, authority: 90 },
  'china-safe': { id: 'china-safe', name: 'SAFE (外汇局)', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 85, authority: 88 },
  'china-csrc': { id: 'china-csrc', name: 'CSRC (证监会)', level: 'L0.5', domain: ['regulation'], executionPower: 85, authority: 88 },
  'japan-mof': { id: 'japan-mof', name: 'MOF (日本财务省)', level: 'L0.5', domain: ['rate', 'fiscal'], executionPower: 88, authority: 90 },
  'japan-fsa': { id: 'japan-fsa', name: 'FSA (日本金融厅)', level: 'L0.5', domain: ['regulation'], executionPower: 82, authority: 85 },
  'india-sebi': { id: 'india-sebi', name: 'SEBI (印度证监)', level: 'L0.5', domain: ['regulation'], executionPower: 80, authority: 82 },
  
  // ========== L2 次级来源 + 假信号审计增强来�?==========
  'politico': { id: 'politico', name: 'Politico', level: 'L2', domain: ['trade', 'regulation'], executionPower: 0, authority: 70 },
  'axios': { id: 'axios', name: 'Axios', level: 'L2', domain: ['trade', 'fiscal'], executionPower: 0, authority: 68 },
  'zerohedge': { id: 'zerohedge', name: 'ZeroHedge', level: 'L2', domain: ['rate', 'fiscal'], executionPower: 0, authority: 55 },
  'xinhua': { id: 'xinhua', name: 'Xinhua', level: 'L1', tier: 'B', domain: ['trade', 'war', 'sanction'], executionPower: 0, authority: 72 },
  // Enhanced audit sources
  'cme-fedwatch': { id: 'cme-fedwatch', name: 'CME FedWatch', level: 'L2', domain: ['rate'], executionPower: 0, authority: 82 },
  'polymarket': { id: 'polymarket', name: 'Polymarket', level: 'L2', domain: ['trade', 'war', 'regulation'], executionPower: 0, authority: 65 },
  'kalshi': { id: 'kalshi', name: 'Kalshi', level: 'L2', domain: ['trade', 'regulation'], executionPower: 0, authority: 62 },
  'predictit': { id: 'predictit', name: 'PredictIt', level: 'L2', domain: ['trade', 'fiscal'], executionPower: 0, authority: 58 },
  'cboe-vix': { id: 'cboe-vix', name: 'CBOE VIX', level: 'L2', domain: ['rate', 'fiscal', 'war'], executionPower: 0, authority: 85 },
  'options-iv': { id: 'options-iv', name: 'Options IV Surface', level: 'L2', domain: ['trade', 'rate', 'regulation'], executionPower: 0, authority: 80 },
  'cot-report': { id: 'cot-report', name: 'CFTC COT Report', level: 'L2', domain: ['trade', 'rate'], executionPower: 0, authority: 78 },
  
  // ========== 🆕 EXECUTION AGENCIES (L0.5 - 高执行权) ==========
  // US Execution Agencies
  'ofac': { id: 'ofac', name: 'OFAC (Sanctions Office)', level: 'L0.5', domain: ['sanction'], executionPower: 98, authority: 98 },
  'bis-commerce': { id: 'bis-commerce', name: 'BIS (Export Control)', level: 'L0.5', domain: ['sanction', 'export_control'], executionPower: 95, authority: 95 },
  'cfius': { id: 'cfius', name: 'CFIUS (Foreign Investment)', level: 'L0.5', domain: ['regulation', 'sanction'], executionPower: 92, authority: 94 },
  'ita-commerce': { id: 'ita-commerce', name: 'ITA (Int\'l Trade Admin)', level: 'L0.5', domain: ['trade', 'antitrust'], executionPower: 88, authority: 88 },
  'cbp': { id: 'cbp', name: 'CBP (Customs & Border)', level: 'L0.5', domain: ['trade', 'sanction'], executionPower: 90, authority: 90 },
  'federal-register': { id: 'federal-register', name: 'Federal Register', level: 'L0', domain: ['trade', 'sanction', 'regulation', 'fiscal'], executionPower: 100, authority: 100 },
  'fincen': { id: 'fincen', name: 'FinCEN (Financial Crimes)', level: 'L0.5', domain: ['sanction', 'regulation'], executionPower: 92, authority: 92 },
  'doj-antitrust': { id: 'doj-antitrust', name: 'DOJ Antitrust Division', level: 'L0.5', domain: ['antitrust', 'regulation'], executionPower: 95, authority: 95 },
  'ftc': { id: 'ftc', name: 'FTC (Federal Trade)', level: 'L0.5', domain: ['antitrust', 'regulation'], executionPower: 90, authority: 90 },
  
  // EU Execution Agencies (L0.5)
  'eu-official-journal': { id: 'eu-official-journal', name: 'EU Official Journal', level: 'L0', domain: ['trade', 'sanction', 'regulation'], executionPower: 100, authority: 100, region: 'EU' as SourceRegion },
  'eu-dg-taxud': { id: 'eu-dg-taxud', name: 'DG TAXUD (Customs)', level: 'L0.5', domain: ['trade', 'fiscal'], executionPower: 85, authority: 88, region: 'EU' as SourceRegion },
  'eu-dg-just': { id: 'eu-dg-just', name: 'DG JUST (Justice)', level: 'L0.5', domain: ['sanction', 'regulation'], executionPower: 82, authority: 85, region: 'EU' as SourceRegion },
  'eu-esma': { id: 'eu-esma', name: 'ESMA (Securities)', level: 'L0.5', domain: ['regulation'], executionPower: 82, authority: 85, region: 'EU' as SourceRegion },
  'eu-eba': { id: 'eu-eba', name: 'EBA (Banking)', level: 'L0.5', domain: ['regulation', 'fiscal'], executionPower: 80, authority: 82, region: 'EU' as SourceRegion },
  
  // China Execution Agencies
  'china-gac': { id: 'china-gac', name: 'GAC (海关总署)', level: 'L0.5', domain: ['trade', 'sanction'], executionPower: 92, authority: 92, region: 'CN' as SourceRegion },
  'china-samr': { id: 'china-samr', name: 'SAMR (市场监管总局)', level: 'L0.5', domain: ['antitrust', 'regulation'], executionPower: 90, authority: 90, region: 'CN' as SourceRegion },
  'china-cac': { id: 'china-cac', name: 'CAC (网信办)', level: 'L0.5', domain: ['regulation'], executionPower: 88, authority: 88, region: 'CN' as SourceRegion },
  'china-mof': { id: 'china-mof', name: 'MOF (财政部)', level: 'L0.5', domain: ['fiscal', 'trade'], executionPower: 90, authority: 92, region: 'CN' as SourceRegion },
  
  // International Execution Bodies
  'fatf': { id: 'fatf', name: 'FATF (Anti-Money Laundering)', level: 'L0.5', domain: ['sanction', 'regulation'], executionPower: 75, authority: 85, region: 'INTL' as SourceRegion },
  'bcbs': { id: 'bcbs', name: 'BCBS (Basel Committee)', level: 'L0.5', domain: ['regulation', 'fiscal'], executionPower: 70, authority: 85, region: 'INTL' as SourceRegion },
  'fsb': { id: 'fsb', name: 'FSB (Financial Stability)', level: 'L0.5', domain: ['regulation', 'fiscal'], executionPower: 68, authority: 82, region: 'INTL' as SourceRegion },
  
  // Official List Databases (L0 - Highest Authority)
  'sdn-list': { id: 'sdn-list', name: 'OFAC SDN List', level: 'L0', domain: ['sanction'], executionPower: 100, authority: 100 },
  'entity-list': { id: 'entity-list', name: 'BIS Entity List', level: 'L0', domain: ['sanction', 'export_control'], executionPower: 100, authority: 100 },
  'denied-persons': { id: 'denied-persons', name: 'BIS Denied Persons', level: 'L0', domain: ['sanction'], executionPower: 100, authority: 100 },
  'eu-sanctions-list': { id: 'eu-sanctions-list', name: 'EU Sanctions List', level: 'L0', domain: ['sanction'], executionPower: 100, authority: 100, region: 'EU' as SourceRegion },
  'hts-tariff': { id: 'hts-tariff', name: 'HTS Tariff Schedule', level: 'L0', domain: ['trade'], executionPower: 100, authority: 100 },
  'china-unreliable': { id: 'china-unreliable', name: 'China Unreliable Entity List', level: 'L0', domain: ['sanction'], executionPower: 100, authority: 100, region: 'CN' as SourceRegion },
  
  // ========== 🆕 EUR-Lex & Official Journal (EU L0 核心) ==========
  'eur-lex': { id: 'eur-lex', name: 'EUR-Lex', level: 'L0', domain: ['trade', 'sanction', 'regulation', 'fiscal'], executionPower: 100, authority: 100, region: 'EU' as SourceRegion },
  'eu-oj-l': { id: 'eu-oj-l', name: 'EU OJ L-Series (Legislation)', level: 'L0', domain: ['trade', 'sanction', 'regulation'], executionPower: 100, authority: 100, region: 'EU' as SourceRegion },
  'eu-oj-c': { id: 'eu-oj-c', name: 'EU OJ C-Series (Information)', level: 'L0.5', domain: ['regulation'], executionPower: 85, authority: 90, region: 'EU' as SourceRegion },
  
  // ========== 🆕 欧洲成员国财政部/经济部 ==========
  'de-bmwk': { id: 'de-bmwk', name: 'BMWK (德国经济部)', level: 'L1', domain: ['trade', 'regulation'], executionPower: 45, authority: 65, region: 'EU' as SourceRegion },
  'de-bmf': { id: 'de-bmf', name: 'BMF (德国财政部)', level: 'L1', domain: ['fiscal'], executionPower: 50, authority: 68, region: 'EU' as SourceRegion },
  'fr-tresor': { id: 'fr-tresor', name: 'DG Trésor (法国财政总署)', level: 'L1', domain: ['fiscal', 'sanction'], executionPower: 48, authority: 66, region: 'EU' as SourceRegion },
  'fr-dgccrf': { id: 'fr-dgccrf', name: 'DGCCRF (法国竞争消费)', level: 'L1', domain: ['regulation', 'antitrust'], executionPower: 42, authority: 60, region: 'EU' as SourceRegion },
  'it-mef': { id: 'it-mef', name: 'MEF (意大利经财部)', level: 'L1', domain: ['fiscal'], executionPower: 40, authority: 58, region: 'EU' as SourceRegion },
  'nl-minfin': { id: 'nl-minfin', name: 'MinFin (荷兰财政部)', level: 'L1', domain: ['fiscal', 'regulation'], executionPower: 38, authority: 55, region: 'EU' as SourceRegion },
  'es-mineco': { id: 'es-mineco', name: 'MINECO (西班牙经济部)', level: 'L1', domain: ['fiscal', 'trade'], executionPower: 35, authority: 52, region: 'EU' as SourceRegion },
  
  // ========== 🆕 欧洲监管机构 ==========
  'eu-eiopa': { id: 'eu-eiopa', name: 'EIOPA (保险年金)', level: 'L0.5', domain: ['regulation'], executionPower: 75, authority: 80, region: 'EU' as SourceRegion },
  'de-bafin': { id: 'de-bafin', name: 'BaFin (德国金融监管)', level: 'L0.5', domain: ['regulation'], executionPower: 78, authority: 82, region: 'EU' as SourceRegion },
  'fr-amf': { id: 'fr-amf', name: 'AMF (法国金融市场)', level: 'L0.5', domain: ['regulation'], executionPower: 75, authority: 80, region: 'EU' as SourceRegion },
  'fr-acpr': { id: 'fr-acpr', name: 'ACPR (法国审慎监管)', level: 'L0.5', domain: ['regulation', 'fiscal'], executionPower: 72, authority: 78, region: 'EU' as SourceRegion },
  'nl-afm': { id: 'nl-afm', name: 'AFM (荷兰金融市场)', level: 'L0.5', domain: ['regulation'], executionPower: 70, authority: 75, region: 'EU' as SourceRegion },
  
  // ========== 🆕 公司/研究机构 (L1/L2 - 企业声明) ==========
  'microsoft': { id: 'microsoft', name: 'Microsoft Corp', level: 'L1', domain: ['regulation'], executionPower: 0, authority: 65 },
  'apple': { id: 'apple', name: 'Apple Inc', level: 'L1', domain: ['regulation', 'trade'], executionPower: 0, authority: 65 },
  'tesla': { id: 'tesla', name: 'Tesla Inc', level: 'L1', domain: ['trade', 'regulation'], executionPower: 0, authority: 62 },
  'tsmc': { id: 'tsmc', name: 'TSMC', level: 'L1', domain: ['trade', 'sanction'], executionPower: 0, authority: 68 },
  'berkshire': { id: 'berkshire', name: 'Berkshire Hathaway', level: 'L1', domain: ['fiscal'], executionPower: 0, authority: 70 },
  'gazprom': { id: 'gazprom', name: 'Gazprom', level: 'L1', domain: ['trade', 'sanction'], executionPower: 0, authority: 55, region: 'INTL' as SourceRegion },
  'ifo': { id: 'ifo', name: 'ifo Institute', level: 'L1', domain: ['fiscal'], executionPower: 0, authority: 72, region: 'EU' as SourceRegion },
  'eu-commission': { id: 'eu-commission', name: 'EU Commission', level: 'L0.5', domain: ['trade', 'regulation', 'sanction'], executionPower: 85, authority: 90, region: 'EU' as SourceRegion },
}

// ============== 默认源 (用于缺失源的回退) ==============
const DEFAULT_SOURCE: Source = { id: 'unknown', name: 'Unknown Source', level: 'L2', domain: [], executionPower: 0, authority: 0 }

// 安全获取源
const getSource = (key: string): Source => SOURCES[key] || DEFAULT_SOURCE

// ============== 🆕 SOURCE REGISTRY 完整数据 ==============

const SOURCE_REGISTRY: Record<string, SourceRegistryEntry> = {
  'federal-register': {
    sourceId: 'federal-register',
    nameCn: '联邦公报',
    nameEn: 'Federal Register',
    jurisdiction: 'US',
    orgType: 'government',
    level: 'L0',
    sourceWeight: 100,
    canonicalUrl: 'https://www.federalregister.gov',
    feeds: [
      { feedId: 'fr-rules', feedType: 'regulations_notices', feedName: 'Final Rules', feedUrl: 'https://www.federalregister.gov/api/v1/documents.rss?conditions[type][]=RULE', accessType: 'RSS', updateFrequency: 60, priority: 1, parser: 'fr-parser', isActive: true },
      { feedId: 'fr-notices', feedType: 'regulations_notices', feedName: 'Notices', feedUrl: 'https://www.federalregister.gov/api/v1/documents.rss?conditions[type][]=NOTICE', accessType: 'RSS', updateFrequency: 60, priority: 2, parser: 'fr-parser', isActive: true },
      { feedId: 'fr-proposed', feedType: 'regulations_notices', feedName: 'Proposed Rules', feedUrl: 'https://www.federalregister.gov/api/v1/documents.rss?conditions[type][]=PRORULE', accessType: 'RSS', updateFrequency: 120, priority: 3, parser: 'fr-parser', isActive: true },
    ],
    accessType: 'API',
    updateFrequency: 60,
    reliabilityScore: 100,
    dedupStrategy: 'url_hash',
    domains: ['trade', 'sanction', 'regulation', 'fiscal'],
    executionPower: 100,
    authority: 100,
    health: { fetchSuccessRate: 0.99, avgLatencyMs: 250, dedupRate: 0.02, extractionSuccessRate: 0.98, fieldCoverage: 0.95, falsePositiveRate: 0.01, lastHealthCheck: new Date().toISOString(), healthScore: 98, status: 'healthy' },
    lastUpdate: new Date().toISOString(),
    isActive: true
  },
  'eu-official-journal': {
    sourceId: 'eu-official-journal',
    nameCn: '欧盟官方公报',
    nameEn: 'EU Official Journal',
    jurisdiction: 'EU',
    orgType: 'government',
    level: 'L0',
    sourceWeight: 100,
    canonicalUrl: 'https://eur-lex.europa.eu/oj/direct-access.html',
    feeds: [
      { feedId: 'oj-l', feedType: 'regulations_notices', feedName: 'OJ L-Series (Legislation)', feedUrl: 'https://eur-lex.europa.eu/rss/serie-l-latest.xml', accessType: 'RSS', updateFrequency: 60, priority: 1, parser: 'eurlex-parser', isActive: true },
      { feedId: 'oj-c', feedType: 'regulations_notices', feedName: 'OJ C-Series (Information)', feedUrl: 'https://eur-lex.europa.eu/rss/serie-c-latest.xml', accessType: 'RSS', updateFrequency: 120, priority: 2, parser: 'eurlex-parser', isActive: true },
      { feedId: 'oj-sanctions', feedType: 'lists_databases', feedName: 'CFSP Sanctions', feedUrl: 'https://eur-lex.europa.eu/rss/sanctions-latest.xml', accessType: 'RSS', updateFrequency: 30, priority: 1, parser: 'eurlex-sanctions', isActive: true },
    ],
    accessType: 'RSS',
    updateFrequency: 60,
    reliabilityScore: 100,
    dedupStrategy: 'url_hash',
    domains: ['trade', 'sanction', 'regulation'],
    executionPower: 100,
    authority: 100,
    region: 'EU' as SourceRegion,
    health: { fetchSuccessRate: 0.98, avgLatencyMs: 380, dedupRate: 0.03, extractionSuccessRate: 0.96, fieldCoverage: 0.92, falsePositiveRate: 0.02, lastHealthCheck: new Date().toISOString(), healthScore: 95, status: 'healthy' },
    lastUpdate: new Date().toISOString(),
    isActive: true
  },
  'ofac-sdn': {
    sourceId: 'ofac-sdn',
    nameCn: 'OFAC SDN名单',
    nameEn: 'OFAC SDN List',
    jurisdiction: 'US',
    orgType: 'government',
    level: 'L0',
    sourceWeight: 100,
    canonicalUrl: 'https://www.treasury.gov/ofac/downloads/sdnlist.txt',
    feeds: [
      { feedId: 'sdn-xml', feedType: 'lists_databases', feedName: 'SDN XML', feedUrl: 'https://www.treasury.gov/ofac/downloads/sdn.xml', accessType: 'API', updateFrequency: 30, priority: 1, parser: 'ofac-xml', isActive: true },
      { feedId: 'sdn-changes', feedType: 'lists_databases', feedName: 'SDN Changes', feedUrl: 'https://www.treasury.gov/ofac/downloads/sdnlist.txt', accessType: 'API', updateFrequency: 30, priority: 1, parser: 'ofac-txt', isActive: true },
    ],
    accessType: 'API',
    updateFrequency: 30,
    reliabilityScore: 100,
    dedupStrategy: 'entity_match',
    domains: ['sanction'],
    executionPower: 100,
    authority: 100,
    health: { fetchSuccessRate: 0.995, avgLatencyMs: 180, dedupRate: 0.01, extractionSuccessRate: 0.99, fieldCoverage: 0.98, falsePositiveRate: 0.005, lastHealthCheck: new Date().toISOString(), healthScore: 99, status: 'healthy' },
    lastUpdate: new Date().toISOString(),
    isActive: true
  },
  'dg-trade': {
    sourceId: 'dg-trade',
    nameCn: 'DG TRADE (欧盟贸易总司)',
    nameEn: 'DG TRADE',
    jurisdiction: 'EU',
    orgType: 'government',
    level: 'L0.5',
    sourceWeight: 88,
    canonicalUrl: 'https://policy.trade.ec.europa.eu',
    feeds: [
      { feedId: 'dgt-press', feedType: 'press_releases', feedName: 'Press Releases', feedUrl: 'https://policy.trade.ec.europa.eu/rss/news_en', accessType: 'RSS', updateFrequency: 60, priority: 3, parser: 'ec-press', isActive: true },
      { feedId: 'dgt-measures', feedType: 'regulations_notices', feedName: 'Trade Defence', feedUrl: 'https://policy.trade.ec.europa.eu/enforcement-and-protection/trade-defence_en', accessType: 'HTML_SCRAPE', updateFrequency: 120, priority: 1, parser: 'ec-measures', isActive: true },
      { feedId: 'dgt-calendar', feedType: 'calendar_events', feedName: 'Events Calendar', feedUrl: 'https://policy.trade.ec.europa.eu/events_en', accessType: 'HTML_SCRAPE', updateFrequency: 720, priority: 4, parser: 'ec-calendar', isActive: true },
    ],
    accessType: 'RSS',
    updateFrequency: 60,
    reliabilityScore: 88,
    dedupStrategy: 'url_hash',
    domains: ['trade', 'sanction'],
    executionPower: 80,
    authority: 88,
    region: 'EU' as SourceRegion,
    health: { fetchSuccessRate: 0.95, avgLatencyMs: 420, dedupRate: 0.08, extractionSuccessRate: 0.92, fieldCoverage: 0.88, falsePositiveRate: 0.05, lastHealthCheck: new Date().toISOString(), healthScore: 90, status: 'healthy' },
    lastUpdate: new Date().toISOString(),
    isActive: true
  },
  'dg-comp': {
    sourceId: 'dg-comp',
    nameCn: 'DG COMP (欧盟竞争总司)',
    nameEn: 'DG COMP',
    jurisdiction: 'EU',
    orgType: 'regulator',
    level: 'L0.5',
    sourceWeight: 90,
    canonicalUrl: 'https://ec.europa.eu/competition',
    feeds: [
      { feedId: 'dgc-decisions', feedType: 'regulations_notices', feedName: 'Decisions', feedUrl: 'https://ec.europa.eu/competition/elojade/isef/index.cfm', accessType: 'HTML_SCRAPE', updateFrequency: 60, priority: 1, parser: 'ec-decisions', isActive: true },
      { feedId: 'dgc-press', feedType: 'press_releases', feedName: 'Press Releases', feedUrl: 'https://ec.europa.eu/competition/rss/news_en.xml', accessType: 'RSS', updateFrequency: 30, priority: 2, parser: 'ec-press', isActive: true },
      { feedId: 'dgc-mergers', feedType: 'lists_databases', feedName: 'Merger Cases', feedUrl: 'https://ec.europa.eu/competition/mergers/cases/', accessType: 'HTML_SCRAPE', updateFrequency: 120, priority: 1, parser: 'ec-mergers', isActive: true },
    ],
    accessType: 'RSS',
    updateFrequency: 30,
    reliabilityScore: 90,
    dedupStrategy: 'url_hash',
    domains: ['regulation', 'antitrust'],
    executionPower: 85,
    authority: 90,
    region: 'EU' as SourceRegion,
    health: { fetchSuccessRate: 0.94, avgLatencyMs: 350, dedupRate: 0.06, extractionSuccessRate: 0.93, fieldCoverage: 0.90, falsePositiveRate: 0.04, lastHealthCheck: new Date().toISOString(), healthScore: 91, status: 'healthy' },
    lastUpdate: new Date().toISOString(),
    isActive: true
  }
}

// ============== 🆕 NOISE GATE 规则配置 ==============

const NOISE_GATE_RULES: NoiseGateRule[] = [
  {
    ruleId: 'ng-effective-date',
    ruleName: 'Effective Date Change',
    description: '生效日期变更 - 必须触发警报',
    triggerTypes: ['effective_date'],
    minSourceLevel: 'L1',
    minExecutionPower: 0,
    minReliabilityScore: 60,
    isActive: true
  },
  {
    ruleId: 'ng-list-change',
    ruleName: 'List Change Detection',
    description: '名单变动 (ADD/REMOVE/MODIFY) - 高优先级',
    triggerTypes: ['list_change'],
    minSourceLevel: 'L0.5',
    minExecutionPower: 80,
    minReliabilityScore: 90,
    isActive: true
  },
  {
    ruleId: 'ng-stance-reversal',
    ruleName: 'Stance Reversal',
    description: '口径反转 - 必须触发主题状态变更',
    triggerTypes: ['stance_reversal'],
    minSourceLevel: 'L0.5',
    minExecutionPower: 50,
    minReliabilityScore: 80,
    isActive: true
  },
  {
    ruleId: 'ng-channel-upgrade',
    ruleName: 'Channel Upgrade',
    description: '渠道升级 (L1→L0.5→L0) - 确认信号',
    triggerTypes: ['channel_upgrade'],
    minSourceLevel: 'L0',
    minExecutionPower: 70,
    minReliabilityScore: 85,
    isActive: true
  },
  {
    ruleId: 'ng-scope-change',
    ruleName: 'Scope Change',
    description: '范围变更 (扩大/缩小) - 影响评估',
    triggerTypes: ['scope_change'],
    minSourceLevel: 'L0.5',
    minExecutionPower: 60,
    minReliabilityScore: 75,
    isActive: true
  },
  {
    ruleId: 'ng-deadline',
    ruleName: 'Deadline Imminent',
    description: '截止日临近(7天内) - 提醒',
    triggerTypes: ['deadline_imminent'],
    minSourceLevel: 'L2',
    minExecutionPower: 0,
    minReliabilityScore: 50,
    isActive: true
  }
]

// Noise Gate 评估函数
const evaluateNoiseGate = (
  newsItem: { sourceLevel: SourceLevel; executionPower: number; triggerTypes: TriggerType[]; reliabilityScore: number },
  rules: NoiseGateRule[]
): NoiseGateResult => {
  const levelOrder: Record<SourceLevel, number> = { 'L0': 4, 'L0.5': 3, 'L1': 2, 'L2': 1 }
  const triggeredRules: string[] = []
  let passedGate = false
  
  for (const rule of rules) {
    if (!rule.isActive) continue
    
    // 检查是否有匹配的触发类�?
    const hasTrigger = rule.triggerTypes.some(t => newsItem.triggerTypes.includes(t))
    if (!hasTrigger) continue
    
    // 检查来源等�?
    const meetsLevel = levelOrder[newsItem.sourceLevel] >= levelOrder[rule.minSourceLevel]
    if (!meetsLevel) continue
    
    // 检查执行权�?
    const meetsExecution = newsItem.executionPower >= rule.minExecutionPower
    if (!meetsExecution) continue
    
    // 检查可靠�?
    const meetsReliability = newsItem.reliabilityScore >= rule.minReliabilityScore
    if (!meetsReliability) continue
    
    triggeredRules.push(rule.ruleId)
    passedGate = true
  }
  
  return {
    newsId: Math.random().toString(36).substr(2, 9),
    noiseLevel: passedGate ? 'signal' : 'archive_only',
    triggeredRules,
    passedGate,
    shouldAlert: triggeredRules.some(r => ['ng-list-change', 'ng-stance-reversal', 'ng-effective-date'].includes(r)),
    shouldUpdateTopic: triggeredRules.some(r => ['ng-stance-reversal', 'ng-scope-change', 'ng-channel-upgrade'].includes(r)),
    archiveOnly: !passedGate,
    reasoning: passedGate ? `Triggered ${triggeredRules.length} rules` : 'No rules matched - archive only',
    evaluatedAt: new Date().toISOString()
  }
}

// 行业影响映射
interface IndustryImpact {
  industry: string
  icon: string
  confidence: number
  direction: ImpactDirection
  reasoning: string
  relatedETFs: string[]
}

// 辖区模型 - 精确到执行机构
interface JurisdictionInfo {
  region: string
  authorityLevel: 'supranational' | 'federal' | 'ministry' | 'agency' | 'state'
  executingBody: string
  enforcementPower: 'full' | 'partial' | 'signaling'
  executionAuthority: boolean
}

// 突发新闻类型 - 扩展版
interface BreakingNews {
  id: string
  headline: string
  source: Source
  publishedAt: string
  urgency: 'flash' | 'urgent' | 'breaking' | 'developing'
  topics: string[]
  industries: IndustryImpact[]
  sentiment: 'bullish' | 'bearish' | 'ambiguous'
  isRead: boolean
  // 🆕 扩展字段
  summary?: string                    // 摘要
  originalText?: string               // 原文
  translatedText?: string             // 翻译
  jurisdiction?: JurisdictionInfo     // 辖区信息
  decisionScore?: number              // 决策评分
  confidence?: number                 // 置信度
  tradeability?: 'tradeable' | 'monitor' | 'ignore'
  keyRisk?: string                    // 关键风险
  nextTrigger?: string                // 下一验证触发点
  relatedPolicies?: string[]          // 关联政策
  affectedAssets?: Array<{
    ticker: string
    direction: 'bullish' | 'bearish' | 'neutral'
    confidence: number
  }>
}

// 生成模拟突发新闻 - 带完整决策信息
const generateBreakingNews = (): BreakingNews[] => {
  return [
    {
      id: 'bn-1',
      headline: 'FLASH: Trump announces immediate 50% tariff on all EU auto imports',
      source: SOURCES['trump-truth'],
      publishedAt: new Date(Date.now() - 120000).toISOString(),
      urgency: 'flash',
      topics: ['EU Tariffs', 'Auto Industry'],
      industries: [
        { industry: 'Auto Manufacturing', icon: '🚗', confidence: 95, direction: 'bearish', reasoning: 'EU automakers directly impacted', relatedETFs: ['CARZ', 'VW', 'BMW'] },
        { industry: 'Auto Parts', icon: '⚙️', confidence: 82, direction: 'bearish', reasoning: 'Supply chain cost increase', relatedETFs: ['XLI'] },
        { industry: 'US Automakers', icon: '🇺🇸', confidence: 72, direction: 'bullish', reasoning: 'Competitive advantage boost', relatedETFs: ['F', 'GM'] },
      ],
      sentiment: 'bearish',
      isRead: false,
      // 🆕 详情字段
      summary: 'President Trump announced via Truth Social that a 50% tariff on all EU automobile imports will take effect immediately. This marks a significant escalation in US-EU trade tensions.',
      originalText: '"We are imposing MASSIVE 50% TARIFFS on all European cars starting TODAY. America First! They have been ripping us off for years. No more!"',
      jurisdiction: { region: 'US', authorityLevel: 'federal', executingBody: 'White House / USTR', enforcementPower: 'full', executionAuthority: true },
      decisionScore: 72,
      confidence: 0.85,
      tradeability: 'tradeable',
      keyRisk: 'EU retaliation risk; Congressional pushback possible',
      nextTrigger: 'USTR formal notice in Federal Register (expected within 48h)',
      relatedPolicies: ['us-eu-trade-2026', 'auto-tariff-section-232'],
      affectedAssets: [
        { ticker: 'VW', direction: 'bearish', confidence: 95 },
        { ticker: 'BMW', direction: 'bearish', confidence: 92 },
        { ticker: 'F', direction: 'bullish', confidence: 72 },
        { ticker: 'GM', direction: 'bullish', confidence: 70 },
      ]
    },
    {
      id: 'bn-2',
      headline: 'ECB Emergency Meeting: Considering 50bp rate hike to combat inflation',
      source: SOURCES['ecb'],
      publishedAt: new Date(Date.now() - 900000).toISOString(),
      urgency: 'urgent',
      topics: ['ECB Rate', 'EU Inflation'],
      industries: [
        { industry: 'EU Banks', icon: '🏦', confidence: 88, direction: 'bullish', reasoning: 'Wider spreads benefit banks', relatedETFs: ['EUFN', 'DB'] },
        { industry: 'Real Estate', icon: '🏠', confidence: 78, direction: 'bearish', reasoning: 'Higher financing costs', relatedETFs: ['VNQ', 'IYR'] },
        { industry: 'Tech Growth', icon: '💻', confidence: 75, direction: 'bearish', reasoning: 'High rates pressure valuations', relatedETFs: ['QQQ', 'XLK'] },
      ],
      sentiment: 'ambiguous',
      isRead: false,
      summary: 'ECB Governing Council convened emergency session to discuss accelerated rate increases. Sources indicate 50bp hike is base case.',
      originalText: 'The Governing Council will convene on an extraordinary basis to assess the inflation trajectory and monetary policy stance.',
      jurisdiction: { region: 'EU', authorityLevel: 'supranational', executingBody: 'ECB Governing Council', enforcementPower: 'full', executionAuthority: true },
      decisionScore: 45,
      confidence: 0.72,
      tradeability: 'tradeable',
      keyRisk: 'Recession concerns may limit hawkishness',
      nextTrigger: 'ECB press conference (scheduled in 6 hours)',
      relatedPolicies: ['ecb-rate-2026-q1'],
      affectedAssets: [
        { ticker: 'EUFN', direction: 'bullish', confidence: 88 },
        { ticker: 'VNQ', direction: 'bearish', confidence: 78 },
        { ticker: 'TLT', direction: 'bearish', confidence: 65 },
      ]
    },
    {
      id: 'bn-3',
      headline: 'PBoC announces 100bp RRR cut, releasing 1.5T liquidity',
      source: SOURCES['pboc'],
      publishedAt: new Date(Date.now() - 1800000).toISOString(),
      urgency: 'breaking',
      topics: ['China Monetary Policy', 'Liquidity Release'],
      industries: [
        { industry: 'China Property', icon: '🏗️', confidence: 85, direction: 'bullish', reasoning: 'Liquidity eases debt pressure', relatedETFs: ['FXI', 'KWEB'] },
        { industry: 'China Banks', icon: '🏦', confidence: 78, direction: 'ambiguous', reasoning: 'Ample liquidity but tighter spreads', relatedETFs: ['FXI'] },
        { industry: 'Commodities', icon: '🛢️', confidence: 70, direction: 'bullish', reasoning: 'Demand expectations rise', relatedETFs: ['DBC', 'GSG'] },
      ],
      sentiment: 'bullish',
      isRead: true,
      summary: '中国人民银行宣布下调存款准备金率100个基点，释放约1.5万亿元流动性，为2020年以来最大规模降准。',
      originalText: '中国人民银行决定于2026年1月25日下调金融机构存款准备金率1个百分点。',
      translatedText: 'The People\'s Bank of China decided to cut the reserve requirement ratio for financial institutions by 1 percentage point on January 25, 2026.',
      jurisdiction: { region: 'CN', authorityLevel: 'ministry', executingBody: 'PBoC', enforcementPower: 'full', executionAuthority: true },
      decisionScore: 58,
      confidence: 0.92,
      tradeability: 'tradeable',
      keyRisk: 'Capital outflow pressure; RMB depreciation',
      nextTrigger: 'MLF rate decision (expected next week)',
      relatedPolicies: ['cn-monetary-easing-2026'],
      affectedAssets: [
        { ticker: 'FXI', direction: 'bullish', confidence: 85 },
        { ticker: 'KWEB', direction: 'bullish', confidence: 80 },
        { ticker: 'USD/CNH', direction: 'bullish', confidence: 72 },
      ]
    },
    {
      id: 'bn-4',
      headline: 'DoD confirms: US military had unsafe encounter with Chinese vessels in South China Sea',
      source: SOURCES['dod'],
      publishedAt: new Date(Date.now() - 3600000).toISOString(),
      urgency: 'developing',
      topics: ['Taiwan Strait', 'Geopolitical Risk'],
      industries: [
        { industry: 'Defense', icon: '🛡️', confidence: 88, direction: 'bullish', reasoning: 'Defense budget expected to increase', relatedETFs: ['ITA', 'XAR', 'LMT'] },
        { industry: 'Safe Haven', icon: '🥇', confidence: 82, direction: 'bullish', reasoning: 'Geopolitical risk rising', relatedETFs: ['GLD', 'TLT'] },
        { industry: 'Semiconductors', icon: '🔌', confidence: 78, direction: 'bearish', reasoning: 'Taiwan supply chain risk', relatedETFs: ['SOXX', 'SMH'] },
      ],
      sentiment: 'bearish',
      isRead: true,
      summary: 'Pentagon confirms Chinese naval vessels conducted "unsafe maneuvers" near US destroyer in South China Sea. State Department summoned Chinese ambassador.',
      originalText: 'A PLA Navy vessel conducted an unsafe interaction with USS Nimitz in international waters of the South China Sea.',
      jurisdiction: { region: 'US', authorityLevel: 'federal', executingBody: 'DoD / State', enforcementPower: 'signaling', executionAuthority: false },
      decisionScore: 28,
      confidence: 0.65,
      tradeability: 'monitor',
      keyRisk: 'Escalation to military confrontation; Taiwan invasion scenario',
      nextTrigger: 'State Department briefing; China MFA response',
      relatedPolicies: ['us-cn-taiwan-2026'],
      affectedAssets: [
        { ticker: 'LMT', direction: 'bullish', confidence: 88 },
        { ticker: 'GLD', direction: 'bullish', confidence: 82 },
        { ticker: 'SOXX', direction: 'bearish', confidence: 78 },
      ]
    },
    // === 扩展新闻: 额外16条历史快讯 ===
    {
      id: 'bn-5',
      headline: 'BREAKING: Japan announces ¥10T fiscal stimulus package',
      source: SOURCES['boj'],
      publishedAt: new Date(Date.now() - 4 * 3600000).toISOString(),
      urgency: 'breaking',
      topics: ['Japan Fiscal', 'Yen'],
      industries: [
        { industry: 'Japan Equities', icon: '', confidence: 88, direction: 'bullish', reasoning: 'Fiscal stimulus boosts economy', relatedETFs: ['EWJ', 'DXJ'] },
      ],
      sentiment: 'bullish',
      isRead: true
    },
    {
      id: 'bn-6',
      headline: 'Fed Chair Powell: "Inflation fight not over, higher for longer"',
      source: SOURCES['fed'],
      publishedAt: new Date(Date.now() - 5 * 3600000).toISOString(),
      urgency: 'urgent',
      topics: ['Fed Policy', 'Inflation'],
      industries: [
        { industry: 'Tech Growth', icon: '', confidence: 82, direction: 'bearish', reasoning: 'Higher rates pressure valuations', relatedETFs: ['QQQ', 'ARKK'] },
        { industry: 'Banks', icon: '', confidence: 75, direction: 'bullish', reasoning: 'Net interest margin expansion', relatedETFs: ['XLF', 'KBE'] },
      ],
      sentiment: 'bearish',
      isRead: true
    },
    {
      id: 'bn-7',
      headline: 'SEC approves spot Ethereum ETF applications from BlackRock, Fidelity',
      source: SOURCES['sec'],
      publishedAt: new Date(Date.now() - 6 * 3600000).toISOString(),
      urgency: 'breaking',
      topics: ['Crypto Regulation', 'ETF'],
      industries: [
        { industry: 'Crypto', icon: '', confidence: 95, direction: 'bullish', reasoning: 'Institutional access opens', relatedETFs: ['COIN', 'MSTR'] },
      ],
      sentiment: 'bullish',
      isRead: true
    },
    {
      id: 'bn-8',
      headline: 'OPEC+ agrees emergency 2M bpd production cut starting next month',
      source: SOURCES['opec'],
      publishedAt: new Date(Date.now() - 7 * 3600000).toISOString(),
      urgency: 'flash',
      topics: ['Oil Supply', 'OPEC'],
      industries: [
        { industry: 'Energy', icon: '', confidence: 92, direction: 'bullish', reasoning: 'Supply reduction raises prices', relatedETFs: ['XLE', 'USO', 'OXY'] },
        { industry: 'Airlines', icon: '', confidence: 78, direction: 'bearish', reasoning: 'Higher fuel costs', relatedETFs: ['JETS'] },
      ],
      sentiment: 'bullish',
      isRead: true
    },
    {
      id: 'bn-9',
      headline: 'Microsoft announces $100B AI infrastructure investment over 3 years',
      source: SOURCES['microsoft'],
      publishedAt: new Date(Date.now() - 8 * 3600000).toISOString(),
      urgency: 'breaking',
      topics: ['AI Investment', 'Tech CapEx'],
      industries: [
        { industry: 'AI Infrastructure', icon: '', confidence: 90, direction: 'bullish', reasoning: 'Massive demand signal', relatedETFs: ['NVDA', 'AMD', 'SMCI'] },
      ],
      sentiment: 'bullish',
      isRead: true
    },
    {
      id: 'bn-10',
      headline: 'Germany manufacturing PMI hits 18-month low at 42.3',
      source: SOURCES['ifo'],
      publishedAt: new Date(Date.now() - 10 * 3600000).toISOString(),
      urgency: 'developing',
      topics: ['EU Economy', 'Manufacturing'],
      industries: [
        { industry: 'EU Industrials', icon: '', confidence: 75, direction: 'bearish', reasoning: 'Contraction deepens', relatedETFs: ['EWG', 'FEZ'] },
      ],
      sentiment: 'bearish',
      isRead: true
    },
    {
      id: 'bn-11',
      headline: 'India GDP growth revised up to 7.8% for Q3, beating estimates',
      source: SOURCES['rbi'],
      publishedAt: new Date(Date.now() - 12 * 3600000).toISOString(),
      urgency: 'developing',
      topics: ['India Growth', 'Emerging Markets'],
      industries: [
        { industry: 'India Equities', icon: '', confidence: 85, direction: 'bullish', reasoning: 'Strong growth momentum', relatedETFs: ['INDA', 'EPI', 'INDY'] },
      ],
      sentiment: 'bullish',
      isRead: true
    },
    {
      id: 'bn-12',
      headline: 'Apple confirms 15% iPhone demand decline in China',
      source: SOURCES['apple'],
      publishedAt: new Date(Date.now() - 14 * 3600000).toISOString(),
      urgency: 'urgent',
      topics: ['Apple China', 'Consumer Tech'],
      industries: [
        { industry: 'Consumer Tech', icon: '', confidence: 88, direction: 'bearish', reasoning: 'China demand weakness', relatedETFs: ['AAPL', 'XLK'] },
      ],
      sentiment: 'bearish',
      isRead: true
    },
    {
      id: 'bn-13',
      headline: 'UK inflation falls to 3.2%, BoE rate cut expectations rise',
      source: SOURCES['boe'],
      publishedAt: new Date(Date.now() - 16 * 3600000).toISOString(),
      urgency: 'developing',
      topics: ['UK Inflation', 'BoE Policy'],
      industries: [
        { industry: 'UK Gilts', icon: '', confidence: 80, direction: 'bullish', reasoning: 'Rate cut expectations', relatedETFs: ['EWU', 'FXB'] },
      ],
      sentiment: 'bullish',
      isRead: true
    },
    {
      id: 'bn-14',
      headline: 'TSMC announces Arizona fab construction accelerated, 2nm production 2026',
      source: SOURCES['tsmc'],
      publishedAt: new Date(Date.now() - 18 * 3600000).toISOString(),
      urgency: 'breaking',
      topics: ['Semiconductors', 'Supply Chain'],
      industries: [
        { industry: 'Semiconductors', icon: '', confidence: 85, direction: 'bullish', reasoning: 'Capacity expansion', relatedETFs: ['TSM', 'SOXX', 'SMH'] },
      ],
      sentiment: 'bullish',
      isRead: true
    },
    {
      id: 'bn-15',
      headline: 'Brazil central bank unexpectedly raises rates 100bp to combat inflation',
      source: SOURCES['bcb'],
      publishedAt: new Date(Date.now() - 20 * 3600000).toISOString(),
      urgency: 'urgent',
      topics: ['Brazil Rates', 'LatAm'],
      industries: [
        { industry: 'Brazil Equities', icon: '', confidence: 72, direction: 'bearish', reasoning: 'Tightening cycle extends', relatedETFs: ['EWZ', 'BRF'] },
      ],
      sentiment: 'bearish',
      isRead: true
    },
    {
      id: 'bn-16',
      headline: 'EU antitrust: €2.3B fine proposed for Google advertising practices',
      source: SOURCES['eu-commission'],
      publishedAt: new Date(Date.now() - 22 * 3600000).toISOString(),
      urgency: 'developing',
      topics: ['EU Antitrust', 'Big Tech'],
      industries: [
        { industry: 'Digital Advertising', icon: '', confidence: 78, direction: 'bearish', reasoning: 'Regulatory pressure', relatedETFs: ['GOOGL', 'META'] },
      ],
      sentiment: 'bearish',
      isRead: true
    },
    {
      id: 'bn-17',
      headline: 'Tesla announces new Gigafactory in Saudi Arabia for $8B investment',
      source: SOURCES['tesla'],
      publishedAt: new Date(Date.now() - 24 * 3600000).toISOString(),
      urgency: 'breaking',
      topics: ['Tesla Expansion', 'Middle East'],
      industries: [
        { industry: 'EV Manufacturing', icon: '', confidence: 85, direction: 'bullish', reasoning: 'Capacity growth', relatedETFs: ['TSLA', 'DRIV'] },
      ],
      sentiment: 'bullish',
      isRead: true
    },
    {
      id: 'bn-18',
      headline: 'Swiss National Bank unexpectedly holds rates, signals dovish outlook',
      source: SOURCES['snb'],
      publishedAt: new Date(Date.now() - 26 * 3600000).toISOString(),
      urgency: 'developing',
      topics: ['SNB Policy', 'CHF'],
      industries: [
        { industry: 'Swiss Franc', icon: '', confidence: 72, direction: 'bearish', reasoning: 'Dovish hold weakens CHF', relatedETFs: ['FXF', 'EWL'] },
      ],
      sentiment: 'ambiguous',
      isRead: true
    },
    {
      id: 'bn-19',
      headline: 'Russia halts natural gas flow through Ukraine pipeline',
      source: SOURCES['gazprom'],
      publishedAt: new Date(Date.now() - 28 * 3600000).toISOString(),
      urgency: 'flash',
      topics: ['Energy Supply', 'Geopolitics'],
      industries: [
        { industry: 'European Utilities', icon: '', confidence: 88, direction: 'bearish', reasoning: 'Supply disruption', relatedETFs: ['VGK', 'EUFN'] },
        { industry: 'US LNG', icon: '', confidence: 82, direction: 'bullish', reasoning: 'Alternative supply', relatedETFs: ['LNG', 'TELL'] },
      ],
      sentiment: 'bearish',
      isRead: true
    },
    {
      id: 'bn-20',
      headline: 'Warren Buffett: Berkshire sells $10B Apple stake, raises cash position',
      source: SOURCES['berkshire'],
      publishedAt: new Date(Date.now() - 30 * 3600000).toISOString(),
      urgency: 'urgent',
      topics: ['Buffett Moves', 'Apple'],
      industries: [
        { industry: 'Consumer Tech', icon: '', confidence: 75, direction: 'bearish', reasoning: 'Smart money selling', relatedETFs: ['AAPL', 'BRK.B'] },
      ],
      sentiment: 'bearish',
      isRead: true
    }
  ]
}

// ============== MOCK DATA GENERATORS FOR NEW ENGINES ==============

// Generate Mock Policy Timelines
const generateMockTimelines = (): PolicyTimeline[] => {
  const now = new Date()
  
  return [
    {
      policyId: 'us-china-tariff-2026',
      policyName: 'US-China Section 301 Tariff Escalation',
      jurisdiction: 'US',
      domain: 'trade',
      hasExecutionAuthority: true,
      nodes: [
        {
          id: 'tl-1-signal',
          type: 'signal',
          status: 'completed',
          completedAt: new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000).toISOString(),
          jurisdiction: 'US',
          actualDate: new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://truthsocial.com/@realDonaldTrump/123456',
          sourceLevel: 'L0',
          sourceName: 'Trump Truth Social',
          title: 'Trump announces intention to raise tariffs on China',
          summary: 'President Trump signals 50% tariff increase on Chinese goods',
          originalText: 'We will impose MASSIVE TARIFFS on China. 50% minimum!',
        },
        {
          id: 'tl-1-draft',
          type: 'draft',
          status: 'completed',
          completedAt: new Date(now.getTime() - 20 * 24 * 60 * 60 * 1000).toISOString(),
          jurisdiction: 'US',
          actualDate: new Date(now.getTime() - 20 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://ustr.gov/draft/tariff-2026',
          sourceLevel: 'L0.5',
          sourceName: 'USTR',
          title: 'USTR publishes draft tariff schedule',
          summary: 'Draft includes 50% tariff on $300B of Chinese imports',
          originalText: 'The United States Trade Representative hereby proposes...',
        },
        {
          id: 'tl-1-approval',
          type: 'approval',
          status: 'completed',
          completedAt: new Date(now.getTime() - 10 * 24 * 60 * 60 * 1000).toISOString(),
          jurisdiction: 'US',
          actualDate: new Date(now.getTime() - 10 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://whitehouse.gov/briefing/tariff-approval',
          sourceLevel: 'L0',
          sourceName: 'White House',
          title: 'President signs tariff order',
          summary: 'Executive Order 14XXX signed',
          originalText: 'By the authority vested in me as President...',
        },
        {
          id: 'tl-1-publication',
          type: 'publication',
          status: 'current',
          jurisdiction: 'US',
          actualDate: new Date(now.getTime() - 5 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://federalregister.gov/doc/2026-01234',
          sourceLevel: 'L0',
          sourceName: 'Federal Register',
          publicationChannel: 'federal_register',
          documentNumber: '2026-01234',
          title: 'Federal Register: Additional Tariffs on Goods of China',
          summary: 'Published in Federal Register, effective Feb 1, 2026',
          originalText: 'DEPARTMENT OF COMMERCE... effective date: February 1, 2026',
        },
        {
          id: 'tl-1-effective',
          type: 'effective',
          status: 'pending',
          jurisdiction: 'US',
          expectedDate: new Date(now.getTime() + 10 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://federalregister.gov/doc/2026-01234',
          sourceLevel: 'L0',
          sourceName: 'Federal Register',
          title: 'Tariffs Take Effect',
          summary: 'All covered goods subject to 50% duty',
          originalText: 'Effective February 1, 2026',
        }
      ],
      currentNode: 'publication',
      nextExpectedNode: 'effective',
      isActive: true,
      effectiveDate: new Date(now.getTime() + 10 * 24 * 60 * 60 * 1000).toISOString(),
      delayRisk: 0.15,
      reversalRisk: 0.08,
      scopeChangeCount: 0,
      relatedPolicies: ['china-retaliation-2026'],
      affectedEntities: ['China', 'NVIDIA', 'Apple', 'Tesla'],
      affectedAssets: ['NVDA', 'AAPL', 'TSLA', 'FXI', 'KWEB']
    },
    {
      policyId: 'eu-dma-compliance',
      policyName: 'EU Digital Markets Act Enforcement',
      jurisdiction: 'EU',
      domain: 'regulation',
      hasExecutionAuthority: true,
      nodes: [
        {
          id: 'tl-2-signal',
          type: 'signal',
          status: 'completed',
          completedAt: new Date(now.getTime() - 90 * 24 * 60 * 60 * 1000).toISOString(),
          jurisdiction: 'EU',
          actualDate: new Date(now.getTime() - 90 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://ec.europa.eu/commission/presscorner/detail/en/ip_26_xxx',
          sourceLevel: 'L0',
          sourceName: 'EU Commission President',
          title: 'Commission announces DMA enforcement priority',
          summary: 'Gatekeepers will face strict compliance deadlines',
          originalText: 'The European Commission will ensure full compliance...',
        },
        {
          id: 'tl-2-draft',
          type: 'draft',
          status: 'completed',
          completedAt: new Date(now.getTime() - 60 * 24 * 60 * 60 * 1000).toISOString(),
          jurisdiction: 'EU',
          actualDate: new Date(now.getTime() - 60 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://ec.europa.eu/competition/dma/draft',
          sourceLevel: 'L0.5',
          sourceName: 'DG COMP',
          title: 'Draft compliance guidelines published',
          summary: 'Detailed requirements for designated gatekeepers',
          originalText: 'Guidelines on Article 6(5) interoperability requirements...',
        },
        {
          id: 'tl-2-review',
          type: 'approval',
          status: 'current',
          jurisdiction: 'EU',
          expectedDate: new Date(now.getTime() + 5 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://eur-lex.europa.eu/oj',
          sourceLevel: 'L0.5',
          sourceName: 'EU Council',
          title: 'Council review pending',
          summary: 'Expected Council approval',
          originalText: '',
        },
        {
          id: 'tl-2-publication',
          type: 'publication',
          status: 'pending',
          jurisdiction: 'EU',
          expectedDate: new Date(now.getTime() + 15 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://eur-lex.europa.eu/oj',
          sourceLevel: 'L0',
          sourceName: 'EU Official Journal',
          publicationChannel: 'official_journal',
          title: 'Official Journal publication pending',
          summary: 'Expected publication in OJ L series',
          originalText: '',
        }
      ],
      currentNode: 'approval',
      nextExpectedNode: 'publication',
      isActive: true,
      delayRisk: 0.35,
      reversalRisk: 0.12,
      scopeChangeCount: 2,
      relatedPolicies: ['eu-dsa-enforcement'],
      affectedEntities: ['Apple', 'Google', 'Meta', 'Amazon', 'Microsoft'],
      affectedAssets: ['AAPL', 'GOOGL', 'META', 'AMZN', 'MSFT']
    },
    {
      policyId: 'china-export-control',
      policyName: 'China Rare Earth Export Controls',
      jurisdiction: 'CN',
      domain: 'export_control',
      hasExecutionAuthority: true,
      nodes: [
        {
          id: 'tl-3-signal',
          type: 'signal',
          status: 'completed',
          completedAt: new Date(now.getTime() - 15 * 24 * 60 * 60 * 1000).toISOString(),
          jurisdiction: 'CN',
          actualDate: new Date(now.getTime() - 15 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://mofcom.gov.cn',
          sourceLevel: 'L0',
          sourceName: 'MOFCOM',
          title: 'MOFCOM signals rare earth export review',
          summary: 'Review of rare earth export quotas announced',
          originalText: '商务部宣布将对稀土出口配额进行审查...',
        },
        {
          id: 'tl-3-draft',
          type: 'draft',
          status: 'current',
          jurisdiction: 'CN',
          expectedDate: new Date(now.getTime() + 7 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://mofcom.gov.cn/draft',
          sourceLevel: 'L0.5',
          sourceName: 'MOFCOM',
          title: 'Draft export control measures',
          summary: 'Pending draft publication',
          originalText: '',
        }
      ],
      currentNode: 'signal',
      nextExpectedNode: 'draft',
      isActive: true,
      delayRisk: 0.25,
      reversalRisk: 0.20,
      scopeChangeCount: 0,
      relatedPolicies: [],
      affectedEntities: ['US Tech', 'EU Auto', 'Defense'],
      affectedAssets: ['MP', 'TSLA', 'F', 'GM', 'LMT', 'RTX']
    },
    // === 扩展时间轴: 额外3条政策追踪 ===
    {
      policyId: 'fed-rate-hold-2026',
      policyName: 'Fed Rate Decision Q1 2026',
      jurisdiction: 'US',
      domain: 'rate',
      hasExecutionAuthority: true,
      nodes: [
        {
          id: 'tl-4-signal',
          type: 'signal',
          status: 'completed',
          completedAt: new Date(now.getTime() - 45 * 24 * 60 * 60 * 1000).toISOString(),
          jurisdiction: 'US',
          actualDate: new Date(now.getTime() - 45 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://federalreserve.gov/newsevents',
          sourceLevel: 'L0',
          sourceName: 'Fed Chair Powell',
          title: 'Powell signals data-dependent approach',
          summary: 'Fed will assess incoming data before rate decisions',
          originalText: 'We will continue to make decisions meeting by meeting...',
        },
        {
          id: 'tl-4-meeting',
          type: 'committee_review',
          status: 'current',
          jurisdiction: 'US',
          expectedDate: new Date(now.getTime() + 3 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://federalreserve.gov/fomc',
          sourceLevel: 'L0',
          sourceName: 'FOMC',
          title: 'FOMC Meeting Jan 28-29',
          summary: 'Markets pricing 85% probability of hold',
          originalText: '',
        },
        {
          id: 'tl-4-decision',
          type: 'vote',
          status: 'pending',
          jurisdiction: 'US',
          expectedDate: new Date(now.getTime() + 4 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://federalreserve.gov/fomc/statement',
          sourceLevel: 'L0',
          sourceName: 'FOMC Statement',
          title: 'Rate Decision Announcement',
          summary: 'Decision to be announced 2:00 PM ET',
          originalText: '',
        }
      ],
      currentNode: 'committee_review',
      nextExpectedNode: 'vote',
      isActive: true,
      delayRisk: 0.05,
      reversalRisk: 0.15,
      scopeChangeCount: 0,
      relatedPolicies: ['ecb-rate-decision'],
      affectedEntities: ['Banks', 'Real Estate', 'Tech'],
      affectedAssets: ['SPY', 'QQQ', 'TLT', 'XLF', 'VNQ']
    },
    {
      policyId: 'japan-boj-exit-2026',
      policyName: 'BoJ Yield Curve Control Exit',
      jurisdiction: 'JP',
      domain: 'rate',
      hasExecutionAuthority: true,
      nodes: [
        {
          id: 'tl-5-signal',
          type: 'signal',
          status: 'completed',
          completedAt: new Date(now.getTime() - 60 * 24 * 60 * 60 * 1000).toISOString(),
          jurisdiction: 'JP',
          actualDate: new Date(now.getTime() - 60 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://boj.or.jp/en/announcements',
          sourceLevel: 'L0',
          sourceName: 'BoJ Governor Ueda',
          title: 'BoJ signals potential YCC adjustment',
          summary: 'Governor hints at policy normalization path',
          originalText: '日本銀行は今後の物価動向を注視...',
        },
        {
          id: 'tl-5-debate',
          type: 'debate',
          status: 'current',
          jurisdiction: 'JP',
          expectedDate: new Date(now.getTime() + 14 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://boj.or.jp/en/mopo',
          sourceLevel: 'L0.5',
          sourceName: 'BoJ Policy Board',
          title: 'Policy Board deliberation ongoing',
          summary: 'Board members divided on timing',
          originalText: '',
        },
        {
          id: 'tl-5-decision',
          type: 'vote',
          status: 'pending',
          jurisdiction: 'JP',
          expectedDate: new Date(now.getTime() + 21 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://boj.or.jp/en/mopo',
          sourceLevel: 'L0',
          sourceName: 'BoJ',
          title: 'BoJ Policy Decision',
          summary: 'Potential YCC band widening or removal',
          originalText: '',
        }
      ],
      currentNode: 'debate',
      nextExpectedNode: 'vote',
      isActive: true,
      delayRisk: 0.40,
      reversalRisk: 0.30,
      scopeChangeCount: 1,
      relatedPolicies: ['fed-rate-hold-2026'],
      affectedEntities: ['Japan Banks', 'Yen Carry Trade', 'JGBs'],
      affectedAssets: ['EWJ', 'DXJ', 'FXY', 'USDJPY', 'TLT']
    },
    {
      policyId: 'uk-ai-regulation-2026',
      policyName: 'UK AI Safety Bill',
      jurisdiction: 'UK',
      domain: 'regulation',
      hasExecutionAuthority: false,
      nodes: [
        {
          id: 'tl-6-signal',
          type: 'signal',
          status: 'completed',
          completedAt: new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000).toISOString(),
          jurisdiction: 'UK',
          actualDate: new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://gov.uk/government/speeches',
          sourceLevel: 'L0',
          sourceName: 'UK PM',
          title: 'PM announces AI Safety legislation',
          summary: 'New framework for frontier AI models',
          originalText: 'We will introduce the most comprehensive AI safety...',
        },
        {
          id: 'tl-6-draft',
          type: 'draft',
          status: 'completed',
          completedAt: new Date(now.getTime() - 15 * 24 * 60 * 60 * 1000).toISOString(),
          jurisdiction: 'UK',
          actualDate: new Date(now.getTime() - 15 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://bills.parliament.uk',
          sourceLevel: 'L0.5',
          sourceName: 'DSIT',
          title: 'Draft AI Safety Bill published',
          summary: 'Requirements for AI labs operating in UK',
          originalText: '',
        },
        {
          id: 'tl-6-committee',
          type: 'committee_review',
          status: 'current',
          jurisdiction: 'UK',
          expectedDate: new Date(now.getTime() + 10 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://parliament.uk/committees',
          sourceLevel: 'L0.5',
          sourceName: 'Science Committee',
          title: 'Select Committee review',
          summary: 'Committee examining bill provisions',
          originalText: '',
        },
        {
          id: 'tl-6-debate',
          type: 'debate',
          status: 'pending',
          jurisdiction: 'UK',
          expectedDate: new Date(now.getTime() + 30 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://hansard.parliament.uk',
          sourceLevel: 'L0',
          sourceName: 'House of Commons',
          title: 'Second Reading debate',
          summary: 'Full House debate scheduled',
          originalText: '',
        },
        {
          id: 'tl-6-vote',
          type: 'vote',
          status: 'pending',
          jurisdiction: 'UK',
          expectedDate: new Date(now.getTime() + 45 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://hansard.parliament.uk',
          sourceLevel: 'L0',
          sourceName: 'House of Commons',
          title: 'Second Reading vote',
          summary: 'Vote on bill advancement',
          originalText: '',
        }
      ],
      currentNode: 'committee_review',
      nextExpectedNode: 'debate',
      isActive: true,
      delayRisk: 0.50,
      reversalRisk: 0.25,
      scopeChangeCount: 3,
      relatedPolicies: ['eu-ai-act'],
      affectedEntities: ['OpenAI', 'Anthropic', 'DeepMind', 'Meta AI'],
      affectedAssets: ['MSFT', 'GOOGL', 'META', 'NVDA', 'AMZN']
    }
  ]
}

// Generate Mock List Changes
const generateMockListChanges = (): ListDiffReport[] => {
  const now = new Date()
  
  return [
    {
      reportId: 'sdn-update-2026-01',
      generatedAt: new Date(now.getTime() - 2 * 60 * 60 * 1000).toISOString(),
      publishedAt: new Date(now.getTime() - 2 * 60 * 60 * 1000).toISOString(),
      effectiveDate: new Date(now.getTime() + 24 * 60 * 60 * 1000).toISOString(),
      listType: 'sdn',
      listName: 'OFAC SDN List Update',
      sourceAgency: 'OFAC',
      jurisdiction: 'US',
      totalChanges: 12,
      additions: 8,
      removals: 2,
      modifications: 2,
      changes: [
        {
          id: 'lc-1',
          timestamp: new Date(now.getTime() - 2 * 60 * 60 * 1000).toISOString(),
          listType: 'sdn',
          jurisdiction: 'US',
          changeType: 'add',
          entity: {
            id: 'smic-sub-1',
            listType: 'sdn',
            jurisdiction: 'US',
            entityName: 'SMIC Advanced Technology Co., Ltd.',
            entityAliases: ['中芯国际先进技术'],
            entityType: 'company',
            country: 'China',
            addedDate: now.toISOString(),
            lastModified: now.toISOString(),
            reason: 'Support for PLA military modernization',
            legalBasis: 'E.O. 13959, E.O. 14032',
            sourceUrl: 'https://ofac.treasury.gov/sdn-list',
            industryImpact: ['Semiconductors', 'Technology'],
            estimatedMarketImpact: 'high'
          },
          sourceUrl: 'https://ofac.treasury.gov/recent-actions/20260122',
          sourceLevel: 'L0',
          effectiveDate: now.toISOString(),
          announcementDate: new Date(now.getTime() - 24 * 60 * 60 * 1000).toISOString(),
          affectedTickers: ['SMIC.HK', 'SMH', 'SOXX', 'ASML', 'LRCX', 'AMAT'],
          suggestedExposure: [
            { ticker: 'SMH', direction: 'bearish', confidence: 85, reasoning: 'China semi supply chain disruption' },
            { ticker: 'ASML', direction: 'ambiguous', confidence: 65, reasoning: 'Lost China revenue but less competition' },
            { ticker: 'LRCX', direction: 'bearish', confidence: 78, reasoning: 'Major China exposure' }
          ],
          originalText: 'SMIC Advanced Technology Co., Ltd. (a.k.a. SMCAT) added to SDN List...',
          translatedText: '中芯国际先进技术有限公司被加入SDN名单...'
        },
        {
          id: 'lc-2',
          timestamp: new Date(now.getTime() - 6 * 60 * 60 * 1000).toISOString(),
          listType: 'entity_list',
          jurisdiction: 'US',
          changeType: 'add',
          entity: {
            id: 'huawei-cloud-1',
            listType: 'entity_list',
            jurisdiction: 'US',
            entityName: 'Huawei Cloud Computing Technologies',
            entityAliases: ['华为云'],
            entityType: 'company',
            country: 'China',
            addedDate: now.toISOString(),
            lastModified: now.toISOString(),
            reason: 'Technology transfer concerns',
            legalBasis: 'EAR Section 744.11',
            sourceUrl: 'https://bis.doc.gov/entity-list',
            industryImpact: ['Cloud Computing', 'Technology'],
            estimatedMarketImpact: 'medium'
          },
          sourceUrl: 'https://bis.doc.gov/entity-list/updates/20260122',
          sourceLevel: 'L0',
          effectiveDate: now.toISOString(),
          announcementDate: new Date(now.getTime() - 48 * 60 * 60 * 1000).toISOString(),
          affectedTickers: ['BABA', 'AMZN', 'MSFT', 'GOOGL'],
          suggestedExposure: [
            { ticker: 'BABA', direction: 'bearish', confidence: 75, reasoning: 'Alibaba Cloud competitive benefit but broader China tech pressure' },
            { ticker: 'AMZN', direction: 'bullish', confidence: 68, reasoning: 'AWS competitive advantage in global cloud' }
          ],
          originalText: 'Huawei Cloud Computing Technologies Co., Ltd. added to Entity List...',
          translatedText: '华为云计算技术有限公司被加入实体清单...'
        }
      ],
      highImpactChanges: [
        {
          id: 'hic-1',
          timestamp: new Date(now.getTime() - 2 * 60 * 60 * 1000).toISOString(),
          listType: 'sdn',
          jurisdiction: 'US',
          changeType: 'add',
          entity: {
            id: 'smic-sub-1',
            listType: 'sdn',
            jurisdiction: 'US',
            entityName: 'SMIC Advanced Technology Co., Ltd.',
            entityAliases: ['中芯国际先进技术'],
            entityType: 'company',
            country: 'China',
            addedDate: now.toISOString(),
            lastModified: now.toISOString(),
            reason: 'Support for PLA military modernization',
            legalBasis: 'E.O. 13959, E.O. 14032',
            sourceUrl: 'https://ofac.treasury.gov/sdn-list',
            industryImpact: ['Semiconductors', 'Technology'],
            estimatedMarketImpact: 'high'
          },
          sourceUrl: 'https://ofac.treasury.gov/recent-actions/20260122',
          sourceLevel: 'L0',
          effectiveDate: now.toISOString(),
          announcementDate: new Date(now.getTime() - 24 * 60 * 60 * 1000).toISOString(),
          affectedTickers: ['SMIC.HK', 'SMH', 'SOXX'],
          suggestedExposure: [],
          originalText: 'SMIC Advanced Technology Co., Ltd. added to SDN List',
          translatedText: '中芯国际先进技术有限公司被加入SDN名单'
        }
      ],
      suggestedBasket: [
        { ticker: 'SOXX', weight: 0.3, direction: 'bearish', reasoning: 'Broad semi sector impact from supply chain disruption' },
        { ticker: 'FXI', weight: 0.25, direction: 'bearish', reasoning: 'China large cap exposure to sanctioned entities' },
        { ticker: 'KWEB', weight: 0.2, direction: 'bearish', reasoning: 'China tech internet most affected' },
        { ticker: 'AMAT', weight: 0.15, direction: 'bearish', reasoning: 'Semi equipment China revenue loss' },
        { ticker: 'GLD', weight: 0.1, direction: 'bullish', reasoning: 'Safe haven on geopolitical escalation' }
      ]
    }
  ]
}

// Generate Mock Jurisdiction Divergences
const generateMockDivergences = (): JurisdictionDivergence[] => {
  const now = new Date()
  
  return [
    {
      policyTopic: 'AI Regulation Framework',
      divergenceId: 'div-ai-reg-2026',
      states: [
        {
          jurisdiction: 'EU',
          policyState: 'implementing',
          effectiveDate: new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000).toISOString(),
          enforcementLevel: 'full',
          lastUpdate: new Date(now.getTime() - 5 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://eur-lex.europa.eu/ai-act'
        },
        {
          jurisdiction: 'US',
          policyState: 'emerging',
          enforcementLevel: 'none',
          lastUpdate: new Date(now.getTime() - 60 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://whitehouse.gov/ai-executive-order'
        },
        {
          jurisdiction: 'CN',
          policyState: 'negotiating',
          enforcementLevel: 'partial',
          lastUpdate: new Date(now.getTime() - 15 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://cac.gov.cn/ai-governance'
        }
      ],
      divergenceType: 'enforcement',
      divergenceSeverity: 'high',
      leadingJurisdiction: 'EU',
      laggingJurisdictions: ['US', 'CN'],
      catchUpProbability: 0.65,
      estimatedCatchUpDays: 180,
      historicalPrecedents: [
        {
          caseId: 'gdpr-global',
          description: 'GDPR led global privacy regulation',
          outcome: 'US states adopted similar rules within 2 years',
          duration: 730
        }
      ],
      arbitrageWindow: {
        startDate: new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000).toISOString(),
        estimatedEndDate: new Date(now.getTime() + 150 * 24 * 60 * 60 * 1000).toISOString(),
        suggestedStrategy: 'EU-listed AI companies face near-term compliance costs; US AI companies have runway',
        riskLevel: 'medium'
      }
    },
    {
      policyTopic: 'Semiconductor Export Controls',
      divergenceId: 'div-semi-export-2026',
      states: [
        {
          jurisdiction: 'US',
          policyState: 'implementing',
          effectiveDate: new Date(now.getTime() - 60 * 24 * 60 * 60 * 1000).toISOString(),
          enforcementLevel: 'full',
          lastUpdate: new Date(now.getTime() - 2 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://bis.doc.gov/chip-controls'
        },
        {
          jurisdiction: 'EU',
          policyState: 'negotiating',
          enforcementLevel: 'partial',
          lastUpdate: new Date(now.getTime() - 20 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://ec.europa.eu/trade/chip-export'
        },
        {
          jurisdiction: 'JP',
          policyState: 'implementing',
          effectiveDate: new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000).toISOString(),
          enforcementLevel: 'full',
          lastUpdate: new Date(now.getTime() - 10 * 24 * 60 * 60 * 1000).toISOString(),
          sourceUrl: 'https://meti.go.jp/export-control'
        }
      ],
      divergenceType: 'timing',
      divergenceSeverity: 'medium',
      leadingJurisdiction: 'US',
      laggingJurisdictions: ['EU'],
      catchUpProbability: 0.85,
      estimatedCatchUpDays: 90,
      historicalPrecedents: [
        {
          caseId: 'huawei-align',
          description: 'Huawei restrictions alignment',
          outcome: 'EU aligned with US within 18 months',
          duration: 540
        }
      ]
    }
  ]
}

// Generate Mock Immediate Actions
const generateMockImmediateActions = (): ImmediateAction[] => {
  const now = new Date()
  
  return [
    {
      id: 'ia-1',
      type: 'list_change',
      priority: 'critical',
      title: 'SMIC Subsidiary Added to SDN List',
      summary: 'OFAC added SMIC Advanced Technology to SDN list, effective immediately. All US persons must cease transactions.',
      actionRequired: 'Review portfolio for SMIC exposure. Consider hedging semiconductor positions.',
      deadline: new Date(now.getTime() + 24 * 60 * 60 * 1000).toISOString(),
      affectedAssets: ['SMH', 'SOXX', 'SMIC.HK', 'ASML', 'LRCX'],
      suggestedDirection: 'bearish',
      sourceUrl: 'https://ofac.treasury.gov/sdn-list',
      sourceLevel: 'L0',
      originalText: 'SMIC Advanced Technology Co., Ltd. is designated pursuant to E.O. 13959...',
      translatedText: '中芯国际先进技术有限公司根据第13959号行政命令被列入...',
      createdAt: new Date(now.getTime() - 2 * 60 * 60 * 1000).toISOString()
    },
    {
      id: 'ia-2',
      type: 'effective_date',
      priority: 'high',
      title: 'China Tariff Takes Effect in 10 Days',
      summary: '50% tariff on $300B Chinese imports becomes effective Feb 1, 2026. Published in Federal Register.',
      actionRequired: 'Position for tariff implementation. Monitor for last-minute exemptions.',
      deadline: new Date(now.getTime() + 10 * 24 * 60 * 60 * 1000).toISOString(),
      affectedAssets: ['FXI', 'KWEB', 'AAPL', 'NVDA', 'TSLA'],
      suggestedDirection: 'bearish',
      sourceUrl: 'https://federalregister.gov/doc/2026-01234',
      sourceLevel: 'L0',
      originalText: 'Effective date: February 1, 2026...',
      createdAt: new Date(now.getTime() - 5 * 24 * 60 * 60 * 1000).toISOString()
    },
    {
      id: 'ia-3',
      type: 'version_change',
      priority: 'medium',
      title: 'EU AI Act Scope Expanded (v2)',
      summary: 'DG CONNECT published amended guidelines expanding AI Act scope to cover more foundation models.',
      actionRequired: 'Reassess EU AI compliance exposure for tech holdings.',
      affectedAssets: ['GOOGL', 'META', 'MSFT', 'NVDA'],
      suggestedDirection: 'bearish',
      sourceUrl: 'https://ec.europa.eu/ai-act/guidelines-v2',
      sourceLevel: 'L0.5',
      originalText: 'Article 6a now includes general-purpose AI systems with...',
      createdAt: new Date(now.getTime() - 12 * 60 * 60 * 1000).toISOString()
    }
  ]
}

// Generate Historical time (supports 6-month lookback)
const generateHistoricalTime = (daysAgo: number, hoursVariance: number = 0): string => {
  const now = new Date()
  const time = new Date(now.getTime() - daysAgo * 24 * 60 * 60 * 1000 - hoursVariance * 60 * 60 * 1000)
  return time.toISOString()
}

const generateMockTopics = (): Topic[] => {
  const baseTime = new Date()
  
  // Core active topics (last 7 days)
  const recentTopics: Topic[] = [
    {
      id: 'china-tariff-2026',
      name: 'China Tariff Escalation 2026',
      state: 'implementing' as PolicyState,
      domain: 'trade' as Domain,
      score6h: 87.5,
      score24h: 92.3,
      score7d: 78.6,
      velocity: 15.2,
      narrativeDrift: 0.12,
      documents: [],
      entities: [
        { id: 'nvda', name: 'NVIDIA', type: 'company', ticker: 'NVDA', exposure: 'bearish', confidence: 85 },
        { id: 'aapl', name: 'Apple', type: 'company', ticker: 'AAPL', exposure: 'bearish', confidence: 78 },
        { id: 'china', name: 'China', type: 'country', exposure: 'bearish', confidence: 92 },
      ],
      l0Count: 2,
      l05Count: 3,
      l1Count: 8,
      l2Count: 5,
      inPolicyLoop: true,
      lastUpdated: baseTime.toISOString(),
      // Tradeable asset mappings
      tradeableAssets: [
        { ticker: 'FXI', name: 'iShares China Large-Cap ETF', type: 'etf', exposure: 'bearish', confidence: 92, reasoning: 'Chinese large-cap directly impacted by tariffs' },
        { ticker: 'KWEB', name: 'KraneShares China Internet ETF', type: 'etf', exposure: 'bearish', confidence: 88, reasoning: 'Chinese internet companies affected by trade war' },
        { ticker: 'YANG', name: 'Direxion Daily FTSE China Bear 3x', type: 'etf', exposure: 'bullish', confidence: 90, reasoning: 'Inverse China market leveraged ETF' },
        { ticker: 'USD/CNH', name: 'Offshore RMB', type: 'forex', exposure: 'bullish', confidence: 85, reasoning: 'Trade war negative for RMB, long USD' },
        { ticker: 'NVDA', name: 'NVIDIA', type: 'stock', exposure: 'bearish', confidence: 82, reasoning: 'China market accounts for 25%+ revenue' },
      ],
      // Narrative drift metrics
      driftMetrics: {
        l0_l1_drift: 0.08,
        l1_l2_drift: 0.18,
        overall: 0.12,
        riskLevel: 'low' as const,
        trend: 'stable' as const
      },
      // Policy loop validation results
      policyLoop: {
        confirmed: true,
        windowHours: 48,
        l0Evidence: { id: 'ev-loop-l0', text: 'Trump Truth Social: "25% tariffs on China starting Feb 1!"', span: [0, 58] as [number, number], sourceId: 'trump-truth', sourceName: 'Trump Truth Social', level: 'L0' as SourceLevel, url: 'https://truthsocial.com/@realDonaldTrump', publishedAt: new Date(baseTime.getTime() - 48*3600000).toISOString() },
        l05Evidence: { id: 'ev-loop-l05', text: 'White House Press Briefing confirms Executive Order signed today', span: [0, 64] as [number, number], sourceId: 'whitehouse', sourceName: 'White House Official', level: 'L0.5' as SourceLevel, url: 'https://whitehouse.gov/briefings', publishedAt: new Date(baseTime.getTime() - 24*3600000).toISOString() },
        l1Evidences: [
          { id: 'ev-loop-l1-1', text: 'Reuters confirms tariff order, citing White House sources', span: [0, 56] as [number, number], sourceId: 'reuters', sourceName: 'Reuters', level: 'L1' as SourceLevel, url: 'https://reuters.com/tariffs', publishedAt: new Date(baseTime.getTime() - 20*3600000).toISOString() },
          { id: 'ev-loop-l1-2', text: 'Bloomberg analysis: tariffs to impact $500B in trade', span: [0, 51] as [number, number], sourceId: 'bloomberg', sourceName: 'Bloomberg', level: 'L1' as SourceLevel, url: 'https://bloomberg.com/tariffs', publishedAt: new Date(baseTime.getTime() - 18*3600000).toISOString() },
        ],
        l2Evidence: { id: 'ev-loop-l2', text: 'Politico: Industry lobbyists scramble as tariff deadline looms', span: [0, 62] as [number, number], sourceId: 'politico', sourceName: 'Politico', level: 'L2' as SourceLevel, url: 'https://politico.com/tariffs', publishedAt: new Date(baseTime.getTime() - 6*3600000).toISOString() },
        completedAt: new Date(baseTime.getTime() - 6*3600000).toISOString()
      },
      // 新增：市场验证数�?
      validation: {
        signalTime: new Date(baseTime.getTime() - 48*3600000).toISOString(),
        validationTime: baseTime.toISOString(),
        directionHitRatio: 0.80,
        directionHits: 4,
        directionTotal: 5,
        avgPostMove24h: 2.35,
        scoreMovCorrelation: 0.72,
        forwardLookingRatio: 0.80,
        avgPreMove: 0.85,
        avgPostMove: 2.15,
        byAssetType: {
          stock: { hitRatio: 1.0, avgMove: 3.2, count: 1 },
          etf: { hitRatio: 0.75, avgMove: 2.8, count: 3 },
          futures: { hitRatio: 0, avgMove: 0, count: 0 },
          forex: { hitRatio: 1.0, avgMove: 1.2, count: 1 }
        },
        assetValidations: [
          { ticker: 'FXI', name: 'iShares China Large-Cap ETF', type: 'etf', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 92, returns: { ret1h: -0.8, ret6h: -1.5, ret24h: -2.8, ret3d: -4.2 }, preMove: 0.6, postMove: 2.8, isForwardLooking: true },
          { ticker: 'KWEB', name: 'KraneShares China Internet ETF', type: 'etf', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 88, returns: { ret1h: -1.2, ret6h: -2.1, ret24h: -3.5, ret3d: -5.1 }, preMove: 0.9, postMove: 3.5, isForwardLooking: true },
          { ticker: 'YANG', name: 'Direxion Daily FTSE China Bear 3x', type: 'etf', systemDirection: 'bullish', actualDirection: 'up', directionMatch: true, confidence: 90, returns: { ret1h: 2.4, ret6h: 4.5, ret24h: 8.2, ret3d: 12.5 }, preMove: 1.8, postMove: 8.2, isForwardLooking: true },
          { ticker: 'USD/CNH', name: 'Offshore RMB', type: 'forex', systemDirection: 'bullish', actualDirection: 'up', directionMatch: true, confidence: 85, returns: { ret1h: 0.3, ret6h: 0.6, ret24h: 1.2, ret3d: 1.8 }, preMove: 0.2, postMove: 1.2, isForwardLooking: true },
          { ticker: 'NVDA', name: 'NVIDIA', type: 'stock', systemDirection: 'bearish', actualDirection: 'up', directionMatch: false, confidence: 82, returns: { ret1h: 0.5, ret6h: 1.2, ret24h: 0.8, ret3d: -1.5 }, preMove: 1.5, postMove: 0.8, isForwardLooking: false }
        ],
        qualityGrade: 'A',
        qualityScore: 82
      },
      // 🆕 决策引擎字段 - 将在运行时计�?
      decisionScore: undefined,
      credibility: undefined,
      netExposure: undefined,
      infoHierarchy: undefined
    },
    {
      id: 'fed-rate-2026',
      name: 'Fed利率决议 (Q1 2026)',
      state: 'contested' as PolicyState,
      domain: 'rate' as Domain,
      score6h: 72.1,
      score24h: 68.9,
      score7d: 71.2,
      velocity: -3.5,
      narrativeDrift: 0.08,
      documents: [],
      entities: [
        { id: 'spy', name: 'S&P 500', type: 'company', ticker: 'SPY', exposure: 'ambiguous', confidence: 65 },
        { id: 'tlt', name: 'Treasury Bonds', type: 'company', ticker: 'TLT', exposure: 'bullish', confidence: 72 },
      ],
      l0Count: 0,
      l05Count: 2,
      l1Count: 12,
      l2Count: 8,
      inPolicyLoop: false,
      lastUpdated: baseTime.toISOString(),
      tradeableAssets: [
        { ticker: 'TLT', name: 'iShares 20+ Year Treasury Bond ETF', type: 'etf', exposure: 'bullish', confidence: 78, reasoning: 'Rate pause benefits long-term treasuries' },
        { ticker: 'TMF', name: 'Direxion Daily 20+ Yr Treasury Bull 3x', type: 'etf', exposure: 'bullish', confidence: 75, reasoning: 'Leveraged long on treasuries' },
        { ticker: 'XLF', name: 'Financial Select Sector SPDR', type: 'etf', exposure: 'ambiguous', confidence: 55, reasoning: '银行盈利受利率影响，方向不明' },
        { ticker: 'ZN', name: '10-Year T-Note Futures', type: 'futures', exposure: 'bullish', confidence: 72, reasoning: '利率下行预期' },
      ],
      driftMetrics: {
        l0_l1_drift: 0.05,
        l1_l2_drift: 0.12,
        overall: 0.08,
        riskLevel: 'low',
        trend: 'stable'
      },
      // 非闭环但低漂�?= 中等验证
      validation: {
        signalTime: new Date(baseTime.getTime() - 6*3600000).toISOString(),
        validationTime: baseTime.toISOString(),
        directionHitRatio: 0.75,
        directionHits: 3,
        directionTotal: 4,
        avgPostMove24h: 1.45,
        scoreMovCorrelation: 0.58,
        forwardLookingRatio: 0.75,
        avgPreMove: 0.8,
        avgPostMove: 1.3,
        byAssetType: {
          stock: { hitRatio: 0, avgMove: 0, count: 0 },
          etf: { hitRatio: 0.67, avgMove: 1.2, count: 3 },
          futures: { hitRatio: 1.0, avgMove: 0.8, count: 1 },
          forex: { hitRatio: 0, avgMove: 0, count: 0 }
        },
        assetValidations: [
          { ticker: 'TLT', name: 'iShares 20+ Year Treasury Bond ETF', type: 'etf', systemDirection: 'bullish', actualDirection: 'up', directionMatch: true, confidence: 78, returns: { ret1h: 0.3, ret6h: 0.8, ret24h: 1.5, ret3d: 2.2 }, preMove: 0.5, postMove: 1.5, isForwardLooking: true },
          { ticker: 'TMF', name: 'Direxion Daily 20+ Yr Treasury Bull 3x', type: 'etf', systemDirection: 'bullish', actualDirection: 'up', directionMatch: true, confidence: 75, returns: { ret1h: 0.9, ret6h: 2.4, ret24h: 4.5, ret3d: 6.5 }, preMove: 1.5, postMove: 4.5, isForwardLooking: true },
          { ticker: 'XLF', name: 'Financial Select Sector SPDR', type: 'etf', systemDirection: 'ambiguous', actualDirection: 'down', directionMatch: false, confidence: 55, returns: { ret1h: -0.2, ret6h: -0.5, ret24h: -0.8, ret3d: -0.5 }, preMove: 0.6, postMove: 0.4, isForwardLooking: false },
          { ticker: 'ZN', name: '10-Year T-Note Futures', type: 'futures', systemDirection: 'bullish', actualDirection: 'up', directionMatch: true, confidence: 72, returns: { ret1h: 0.1, ret6h: 0.3, ret24h: 0.8, ret3d: 1.2 }, preMove: 0.4, postMove: 0.8, isForwardLooking: true }
        ],
        qualityGrade: 'B',
        qualityScore: 72
      }
    },
    {
      id: 'russia-sanction',
      name: 'Russia Sanctions Expansion',
      state: 'emerging' as PolicyState,
      domain: 'sanction' as Domain,
      score6h: 45.8,
      score24h: 52.3,
      score7d: 38.1,
      velocity: 28.5,
      narrativeDrift: 0.25,
      documents: [],
      entities: [
        { id: 'xom', name: 'ExxonMobil', type: 'company', ticker: 'XOM', exposure: 'bearish', confidence: 68 },
        { id: 'russia', name: 'Russia', type: 'country', exposure: 'bearish', confidence: 95 },
      ],
      l0Count: 1,
      l05Count: 1,
      l1Count: 4,
      l2Count: 3,
      inPolicyLoop: false,
      lastUpdated: baseTime.toISOString(),
      tradeableAssets: [
        { ticker: 'RSX', name: 'VanEck Russia ETF', type: 'etf', exposure: 'bearish', confidence: 95, reasoning: '直接做空俄罗斯市场（如仍可交易）' },
        { ticker: 'XLE', name: 'Energy Select Sector SPDR', type: 'etf', exposure: 'ambiguous', confidence: 60, reasoning: '能源制裁影响复杂' },
        { ticker: 'USO', name: 'United States Oil Fund', type: 'etf', exposure: 'bullish', confidence: 65, reasoning: '供应减少可能推高油价' },
        { ticker: 'CL', name: 'Crude Oil Futures', type: 'futures', exposure: 'bullish', confidence: 68, reasoning: '制裁减少供应' },
        { ticker: 'NG', name: 'Natural Gas Futures', type: 'futures', exposure: 'bullish', confidence: 72, reasoning: '欧洲天然气供应受影响' },
      ],
      driftMetrics: {
        l0_l1_drift: 0.15,
        l1_l2_drift: 0.35,
        overall: 0.25,
        riskLevel: 'medium',
        trend: 'increasing'
      },
      // 非闭环信号的验证 - 方向命中率较�?
      validation: {
        signalTime: new Date(baseTime.getTime() - 12*3600000).toISOString(),
        validationTime: baseTime.toISOString(),
        directionHitRatio: 0.60,
        directionHits: 3,
        directionTotal: 5,
        avgPostMove24h: 1.85,
        scoreMovCorrelation: 0.45,
        forwardLookingRatio: 0.40,
        avgPreMove: 1.2,
        avgPostMove: 1.1,
        byAssetType: {
          stock: { hitRatio: 0, avgMove: 0, count: 0 },
          etf: { hitRatio: 0.67, avgMove: 1.5, count: 3 },
          futures: { hitRatio: 0.50, avgMove: 2.2, count: 2 },
          forex: { hitRatio: 0, avgMove: 0, count: 0 }
        },
        assetValidations: [
          { ticker: 'RSX', name: 'VanEck Russia ETF', type: 'etf', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 95, returns: { ret1h: -0.5, ret6h: -1.2, ret24h: -2.1, ret3d: -1.8 }, preMove: 1.5, postMove: 2.1, isForwardLooking: true },
          { ticker: 'XLE', name: 'Energy Select Sector SPDR', type: 'etf', systemDirection: 'ambiguous', actualDirection: 'up', directionMatch: true, confidence: 60, returns: { ret1h: 0.3, ret6h: 0.8, ret24h: 1.2, ret3d: 0.5 }, preMove: 0.8, postMove: 1.2, isForwardLooking: true },
          { ticker: 'USO', name: 'United States Oil Fund', type: 'etf', systemDirection: 'bullish', actualDirection: 'down', directionMatch: false, confidence: 65, returns: { ret1h: -0.2, ret6h: -0.5, ret24h: -1.1, ret3d: 0.8 }, preMove: 1.2, postMove: 0.8, isForwardLooking: false },
          { ticker: 'CL', name: 'Crude Oil Futures', type: 'futures', systemDirection: 'bullish', actualDirection: 'up', directionMatch: true, confidence: 68, returns: { ret1h: 0.4, ret6h: 1.2, ret24h: 2.5, ret3d: 1.8 }, preMove: 1.5, postMove: 2.5, isForwardLooking: true },
          { ticker: 'NG', name: 'Natural Gas Futures', type: 'futures', systemDirection: 'bullish', actualDirection: 'down', directionMatch: false, confidence: 72, returns: { ret1h: -0.8, ret6h: -1.5, ret24h: -1.9, ret3d: -2.2 }, preMove: 1.8, postMove: 0.5, isForwardLooking: false }
        ],
        qualityGrade: 'C',
        qualityScore: 55
      }
    },
    {
      id: 'chip-export-control',
      name: '芯片出口管制升级',
      state: 'implementing' as PolicyState,
      domain: 'trade' as Domain,
      score6h: 95.2,
      score24h: 88.7,
      score7d: 82.1,
      velocity: 8.3,
      narrativeDrift: 0.05,
      documents: [],
      entities: [
        { id: 'asml', name: 'ASML', type: 'company', ticker: 'ASML', exposure: 'bearish', confidence: 91 },
        { id: 'amd', name: 'AMD', type: 'company', ticker: 'AMD', exposure: 'bearish', confidence: 82 },
        { id: 'intc', name: 'Intel', type: 'company', ticker: 'INTC', exposure: 'ambiguous', confidence: 65 },
      ],
      l0Count: 3,
      l05Count: 4,
      l1Count: 15,
      l2Count: 7,
      inPolicyLoop: true,
      lastUpdated: baseTime.toISOString(),
      tradeableAssets: [
        { ticker: 'SOXX', name: 'iShares Semiconductor ETF', type: 'etf', exposure: 'bearish', confidence: 85, reasoning: 'Entire chip industry impacted by export restrictions' },
        { ticker: 'SOXS', name: 'Direxion Daily Semiconductor Bear 3x', type: 'etf', exposure: 'bullish', confidence: 82, reasoning: '反向做空半导体杠杆ETF' },
        { ticker: 'NVDA', name: 'NVIDIA', type: 'stock', exposure: 'bearish', confidence: 90, reasoning: 'AI芯片出口受限直接影响' },
        { ticker: 'ASML', name: 'ASML Holding', type: 'stock', exposure: 'bearish', confidence: 88, reasoning: 'Lithography equipment exports restricted' },
        { ticker: 'AMD', name: 'AMD', type: 'stock', exposure: 'bearish', confidence: 82, reasoning: '高性能芯片出口受限' },
      ],
      driftMetrics: {
        l0_l1_drift: 0.03,
        l1_l2_drift: 0.08,
        overall: 0.05,
        riskLevel: 'low',
        trend: 'stable'
      },
      policyLoop: {
        confirmed: true,
        windowHours: 72,
        l0Evidence: { id: 'ev-chip-l0', text: 'Commerce Secretary announces new AI chip export controls', span: [0, 56], sourceId: 'commerce', sourceName: 'Commerce (BIS)', level: 'L0.5', url: 'https://bis.gov/announcements', publishedAt: new Date(baseTime.getTime() - 72*3600000).toISOString() },
        l05Evidence: { id: 'ev-chip-l05', text: 'BIS publishes final rule on semiconductor export controls', span: [0, 57], sourceId: 'commerce', sourceName: 'Commerce (BIS)', level: 'L0.5', url: 'https://bis.gov/rules', publishedAt: new Date(baseTime.getTime() - 48*3600000).toISOString() },
        l1Evidences: [
          { id: 'ev-chip-l1-1', text: 'WSJ: Chip makers brace for impact of new export rules', span: [0, 52], sourceId: 'wsj', sourceName: 'Wall Street Journal', level: 'L1', url: 'https://wsj.com/chips', publishedAt: new Date(baseTime.getTime() - 36*3600000).toISOString() },
        ],
        l2Evidence: { id: 'ev-chip-l2', text: 'Axios: Tech lobbyists push back on chip export rules', span: [0, 51], sourceId: 'axios', sourceName: 'Axios', level: 'L2', url: 'https://axios.com/chips', publishedAt: new Date(baseTime.getTime() - 12*3600000).toISOString() },
        completedAt: new Date(baseTime.getTime() - 12*3600000).toISOString()
      },
      // 闭环 + 高DocScore = 高质量验�?
      validation: {
        signalTime: new Date(baseTime.getTime() - 72*3600000).toISOString(),
        validationTime: baseTime.toISOString(),
        directionHitRatio: 1.0,
        directionHits: 5,
        directionTotal: 5,
        avgPostMove24h: 4.25,
        scoreMovCorrelation: 0.88,
        forwardLookingRatio: 1.0,
        avgPreMove: 1.1,
        avgPostMove: 4.25,
        byAssetType: {
          stock: { hitRatio: 1.0, avgMove: 5.2, count: 3 },
          etf: { hitRatio: 1.0, avgMove: 3.8, count: 2 },
          futures: { hitRatio: 0, avgMove: 0, count: 0 },
          forex: { hitRatio: 0, avgMove: 0, count: 0 }
        },
        assetValidations: [
          { ticker: 'SOXX', name: 'iShares Semiconductor ETF', type: 'etf', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 85, returns: { ret1h: -1.2, ret6h: -2.5, ret24h: -4.1, ret3d: -6.2 }, preMove: 0.8, postMove: 4.1, isForwardLooking: true },
          { ticker: 'SOXS', name: 'Direxion Daily Semiconductor Bear 3x', type: 'etf', systemDirection: 'bullish', actualDirection: 'up', directionMatch: true, confidence: 82, returns: { ret1h: 3.5, ret6h: 7.2, ret24h: 11.8, ret3d: 18.5 }, preMove: 2.5, postMove: 11.8, isForwardLooking: true },
          { ticker: 'NVDA', name: 'NVIDIA', type: 'stock', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 90, returns: { ret1h: -2.1, ret6h: -4.5, ret24h: -6.8, ret3d: -9.2 }, preMove: 1.5, postMove: 6.8, isForwardLooking: true },
          { ticker: 'ASML', name: 'ASML Holding', type: 'stock', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 88, returns: { ret1h: -1.8, ret6h: -3.8, ret24h: -5.5, ret3d: -7.8 }, preMove: 1.2, postMove: 5.5, isForwardLooking: true },
          { ticker: 'AMD', name: 'AMD', type: 'stock', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 82, returns: { ret1h: -1.5, ret6h: -3.2, ret24h: -4.8, ret3d: -6.5 }, preMove: 0.9, postMove: 4.8, isForwardLooking: true }
        ],
        qualityGrade: 'A',
        qualityScore: 95
      }
    },
    {
      id: 'taiwan-tension',
      name: 'Taiwan Strait Tensions',
      state: 'contested' as PolicyState,
      domain: 'war' as Domain,
      score6h: 61.3,
      score24h: 58.2,
      score7d: 55.8,
      velocity: 5.2,
      narrativeDrift: 0.18,
      documents: [],
      entities: [
        { id: 'tsm', name: 'TSMC', type: 'company', ticker: 'TSM', exposure: 'bearish', confidence: 88 },
        { id: 'taiwan', name: 'Taiwan', type: 'country', exposure: 'bearish', confidence: 75 },
      ],
      l0Count: 1,
      l05Count: 2,
      l1Count: 6,
      l2Count: 10,
      inPolicyLoop: false,
      lastUpdated: baseTime.toISOString(),
      tradeableAssets: [
        { ticker: 'TSM', name: 'TSMC ADR', type: 'stock', exposure: 'bearish', confidence: 88, reasoning: '台湾地缘风险直接影响' },
        { ticker: 'EWT', name: 'iShares MSCI Taiwan ETF', type: 'etf', exposure: 'bearish', confidence: 85, reasoning: '台湾市场整体风险' },
        { ticker: 'SOXX', name: 'iShares Semiconductor ETF', type: 'etf', exposure: 'bearish', confidence: 75, reasoning: 'Chip supply chain risk' },
        { ticker: 'GLD', name: 'SPDR Gold Shares', type: 'etf', exposure: 'bullish', confidence: 70, reasoning: 'Geopolitical risk safe-haven demand' },
        { ticker: 'VXX', name: 'iPath Series B S&P 500 VIX', type: 'etf', exposure: 'bullish', confidence: 72, reasoning: 'Volatility spike expected' },
      ],
      driftMetrics: {
        l0_l1_drift: 0.12,
        l1_l2_drift: 0.25,
        overall: 0.18,
        riskLevel: 'medium',
        trend: 'increasing'
      },
      // 高漂�?+ 非闭�?= 中等验证质量
      validation: {
        signalTime: new Date(baseTime.getTime() - 24*3600000).toISOString(),
        validationTime: baseTime.toISOString(),
        directionHitRatio: 0.60,
        directionHits: 3,
        directionTotal: 5,
        avgPostMove24h: 1.95,
        scoreMovCorrelation: 0.52,
        forwardLookingRatio: 0.60,
        avgPreMove: 1.3,
        avgPostMove: 1.8,
        byAssetType: {
          stock: { hitRatio: 0.5, avgMove: 2.5, count: 1 },
          etf: { hitRatio: 0.75, avgMove: 1.8, count: 4 },
          futures: { hitRatio: 0, avgMove: 0, count: 0 },
          forex: { hitRatio: 0, avgMove: 0, count: 0 }
        },
        assetValidations: [
          { ticker: 'TSM', name: 'TSMC ADR', type: 'stock', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 88, returns: { ret1h: -0.8, ret6h: -1.5, ret24h: -2.5, ret3d: -1.8 }, preMove: 1.2, postMove: 2.5, isForwardLooking: true },
          { ticker: 'EWT', name: 'iShares MSCI Taiwan ETF', type: 'etf', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 85, returns: { ret1h: -0.5, ret6h: -1.2, ret24h: -1.8, ret3d: -1.2 }, preMove: 0.8, postMove: 1.8, isForwardLooking: true },
          { ticker: 'SOXX', name: 'iShares Semiconductor ETF', type: 'etf', systemDirection: 'bearish', actualDirection: 'up', directionMatch: false, confidence: 75, returns: { ret1h: 0.3, ret6h: 0.8, ret24h: 0.5, ret3d: -0.8 }, preMove: 1.5, postMove: 0.5, isForwardLooking: false },
          { ticker: 'GLD', name: 'SPDR Gold Shares', type: 'etf', systemDirection: 'bullish', actualDirection: 'up', directionMatch: true, confidence: 70, returns: { ret1h: 0.2, ret6h: 0.5, ret24h: 1.2, ret3d: 1.8 }, preMove: 0.6, postMove: 1.2, isForwardLooking: true },
          { ticker: 'VXX', name: 'iPath Series B S&P 500 VIX', type: 'etf', systemDirection: 'bullish', actualDirection: 'down', directionMatch: false, confidence: 72, returns: { ret1h: -1.5, ret6h: -2.8, ret24h: -3.5, ret3d: -5.2 }, preMove: 2.5, postMove: 1.2, isForwardLooking: false }
        ],
        qualityGrade: 'B',
        qualityScore: 68
      }
    },
    {
      id: 'eu-digital-tax',
      name: 'EU Digital Tax Dispute',
      state: 'exhausted' as PolicyState,
      domain: 'regulation' as Domain,
      score6h: 22.5,
      score24h: 28.1,
      score7d: 45.6,
      velocity: -18.2,
      narrativeDrift: 0.32,
      documents: [],
      entities: [
        { id: 'googl', name: 'Alphabet', type: 'company', ticker: 'GOOGL', exposure: 'bearish', confidence: 55 },
        { id: 'meta', name: 'Meta', type: 'company', ticker: 'META', exposure: 'bearish', confidence: 52 },
      ],
      l0Count: 0,
      l05Count: 0,
      l1Count: 2,
      l2Count: 4,
      inPolicyLoop: false,
      lastUpdated: baseTime.toISOString(),
      tradeableAssets: [
        { ticker: 'XLC', name: 'Communication Services Select Sector SPDR', type: 'etf', exposure: 'bearish', confidence: 52, reasoning: '科技巨头受数字税影响' },
        { ticker: 'GOOGL', name: 'Alphabet', type: 'stock', exposure: 'bearish', confidence: 55, reasoning: '欧洲业务税负增加' },
        { ticker: 'META', name: 'Meta', type: 'stock', exposure: 'bearish', confidence: 52, reasoning: 'Advertising business impacted' },
        { ticker: 'EUR/USD', name: '欧元/美元', type: 'forex', exposure: 'ambiguous', confidence: 40, reasoning: '贸易争端影响不明' },
      ],
      driftMetrics: {
        l0_l1_drift: 0.25,
        l1_l2_drift: 0.42,
        overall: 0.32,
        riskLevel: 'high',
        trend: 'decreasing'
      },
      // 衰退主题 + 高漂�?+ 低置信度 = 低质量验�?
      validation: {
        signalTime: new Date(baseTime.getTime() - 72*3600000).toISOString(),
        validationTime: baseTime.toISOString(),
        directionHitRatio: 0.25,
        directionHits: 1,
        directionTotal: 4,
        avgPostMove24h: 0.65,
        scoreMovCorrelation: 0.18,
        forwardLookingRatio: 0.25,
        avgPreMove: 0.9,
        avgPostMove: 0.5,
        byAssetType: {
          stock: { hitRatio: 0.0, avgMove: 0.8, count: 2 },
          etf: { hitRatio: 0.5, avgMove: 0.5, count: 1 },
          futures: { hitRatio: 0, avgMove: 0, count: 0 },
          forex: { hitRatio: 1.0, avgMove: 0.3, count: 1 }
        },
        assetValidations: [
          { ticker: 'XLC', name: 'Communication Services Select Sector SPDR', type: 'etf', systemDirection: 'bearish', actualDirection: 'up', directionMatch: false, confidence: 52, returns: { ret1h: 0.2, ret6h: 0.3, ret24h: 0.5, ret3d: 0.8 }, preMove: 0.8, postMove: 0.5, isForwardLooking: false },
          { ticker: 'GOOGL', name: 'Alphabet', type: 'stock', systemDirection: 'bearish', actualDirection: 'up', directionMatch: false, confidence: 55, returns: { ret1h: 0.3, ret6h: 0.8, ret24h: 1.2, ret3d: 0.5 }, preMove: 1.2, postMove: 0.8, isForwardLooking: false },
          { ticker: 'META', name: 'Meta', type: 'stock', systemDirection: 'bearish', actualDirection: 'up', directionMatch: false, confidence: 52, returns: { ret1h: 0.5, ret6h: 1.0, ret24h: 0.8, ret3d: 0.2 }, preMove: 0.9, postMove: 0.6, isForwardLooking: false },
          { ticker: 'EUR/USD', name: '欧元/美元', type: 'forex', systemDirection: 'ambiguous', actualDirection: 'flat', directionMatch: true, confidence: 40, returns: { ret1h: 0.0, ret6h: -0.1, ret24h: 0.2, ret3d: -0.1 }, preMove: 0.3, postMove: 0.2, isForwardLooking: false }
        ],
        qualityGrade: 'D',
        qualityScore: 32
      }
    },
    // ============== EU TOPIC EXAMPLES (Demonstrating EU Policy Coverage) ==============
    {
      id: 'eu-dma-enforcement',
      name: 'EU Digital Markets Act Enforcement',
      state: 'implementing' as PolicyState,
      domain: 'regulation' as Domain,
      score6h: 78.5,
      score24h: 82.3,
      score7d: 75.8,
      velocity: 12.3,
      narrativeDrift: 0.08,
      documents: [],
      entities: [
        { id: 'aapl-eu', name: 'Apple (EU)', type: 'company', ticker: 'AAPL', exposure: 'bearish', confidence: 88 },
        { id: 'googl-eu', name: 'Alphabet (EU)', type: 'company', ticker: 'GOOGL', exposure: 'bearish', confidence: 85 },
        { id: 'meta-eu', name: 'Meta (EU)', type: 'company', ticker: 'META', exposure: 'bearish', confidence: 82 },
      ],
      l0Count: 1,   // EU Commission President statement
      l05Count: 3,  // DG COMP, DG CONNECT implementing acts
      l1Count: 5,   // Reuters EU, Bloomberg EU, FT Europe
      l2Count: 2,   // Industry groups
      inPolicyLoop: true,
      lastUpdated: baseTime.toISOString(),
      tradeableAssets: [
        { ticker: 'AAPL', name: 'Apple Inc', type: 'stock', exposure: 'bearish', confidence: 88, reasoning: 'DMA forces App Store sideloading, revenue impact' },
        { ticker: 'GOOGL', name: 'Alphabet', type: 'stock', exposure: 'bearish', confidence: 85, reasoning: 'DMA forces search engine interoperability' },
        { ticker: 'META', name: 'Meta Platforms', type: 'stock', exposure: 'bearish', confidence: 82, reasoning: 'DMA requires messaging interoperability' },
        { ticker: 'XLK', name: 'Technology Select Sector SPDR', type: 'etf', exposure: 'bearish', confidence: 70, reasoning: 'Big tech overall under pressure' },
        { ticker: 'EUR/USD', name: '欧元/美元', type: 'forex', exposure: 'bullish', confidence: 55, reasoning: 'EU监管强化可能短期提振欧元' },
      ],
      driftMetrics: {
        l0_l1_drift: 0.05,
        l1_l2_drift: 0.12,
        overall: 0.08,
        riskLevel: 'low' as const,
        trend: 'stable' as const
      },
      policyLoop: {
        confirmed: true,
        windowHours: 72,
        l0Evidence: { id: 'ev-eu-l0', text: 'EU Commission President: "Big Tech must comply with DMA by March 6th or face penalties up to 10% of global revenue"', span: [0, 110] as [number, number], sourceId: 'eu-commission-president', sourceName: 'EU Commission President', level: 'L0' as SourceLevel, url: 'https://ec.europa.eu/commission', publishedAt: new Date(baseTime.getTime() - 72*3600000).toISOString() },
        l05Evidence: { id: 'ev-eu-l05', text: 'DG COMP releases Implementing Act C(2026)1234: Gatekeeper Compliance Requirements', span: [0, 78] as [number, number], sourceId: 'eu-dg-comp', sourceName: 'DG COMP (Competition)', level: 'L0.5' as SourceLevel, url: 'https://ec.europa.eu/competition', publishedAt: new Date(baseTime.getTime() - 48*3600000).toISOString() },
        l1Evidences: [
          { id: 'ev-eu-l1-1', text: 'Reuters EU: Apple scrambles to comply with DMA sideloading requirements', span: [0, 68] as [number, number], sourceId: 'reuters-eu', sourceName: 'Reuters EU', level: 'L1' as SourceLevel, url: 'https://reuters.com/eu', publishedAt: new Date(baseTime.getTime() - 36*3600000).toISOString() },
          { id: 'ev-eu-l1-2', text: 'Bloomberg EU: Google restructures search business ahead of DMA deadline', span: [0, 70] as [number, number], sourceId: 'bloomberg-eu', sourceName: 'Bloomberg EU', level: 'L1' as SourceLevel, url: 'https://bloomberg.com/eu', publishedAt: new Date(baseTime.getTime() - 24*3600000).toISOString() },
        ],
        l2Evidence: { id: 'ev-eu-l2', text: 'DigitalEurope warns DMA compliance costs may exceed EUR 5B for major platforms', span: [0, 72] as [number, number], sourceId: 'digital-europe', sourceName: 'DigitalEurope (Industry)', level: 'L2' as SourceLevel, url: 'https://digitaleurope.org', publishedAt: new Date(baseTime.getTime() - 12*3600000).toISOString() },
        completedAt: new Date(baseTime.getTime() - 12*3600000).toISOString()
      },
      validation: {
        signalTime: new Date(baseTime.getTime() - 48*3600000).toISOString(),
        validationTime: baseTime.toISOString(),
        directionHitRatio: 0.80,
        directionHits: 4,
        directionTotal: 5,
        avgPostMove24h: 2.85,
        scoreMovCorrelation: 0.72,
        forwardLookingRatio: 0.80,
        avgPreMove: 0.8,
        avgPostMove: 2.5,
        byAssetType: {
          stock: { hitRatio: 1.0, avgMove: 3.2, count: 3 },
          etf: { hitRatio: 1.0, avgMove: 1.8, count: 1 },
          futures: { hitRatio: 0, avgMove: 0, count: 0 },
          forex: { hitRatio: 0, avgMove: 0.5, count: 1 }
        },
        assetValidations: [
          { ticker: 'AAPL', name: 'Apple Inc', type: 'stock', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 88, returns: { ret1h: -0.8, ret6h: -2.1, ret24h: -3.5, ret3d: -4.2 }, preMove: 0.5, postMove: 3.5, isForwardLooking: true },
          { ticker: 'GOOGL', name: 'Alphabet', type: 'stock', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 85, returns: { ret1h: -0.6, ret6h: -1.8, ret24h: -2.8, ret3d: -3.5 }, preMove: 0.6, postMove: 2.8, isForwardLooking: true },
          { ticker: 'META', name: 'Meta Platforms', type: 'stock', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 82, returns: { ret1h: -0.5, ret6h: -1.5, ret24h: -2.2, ret3d: -2.8 }, preMove: 0.4, postMove: 2.2, isForwardLooking: true },
          { ticker: 'XLK', name: 'Technology Select Sector SPDR', type: 'etf', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 70, returns: { ret1h: -0.3, ret6h: -0.8, ret24h: -1.8, ret3d: -2.0 }, preMove: 0.5, postMove: 1.8, isForwardLooking: true },
          { ticker: 'EUR/USD', name: '欧元/美元', type: 'forex', systemDirection: 'bullish', actualDirection: 'down', directionMatch: false, confidence: 55, returns: { ret1h: -0.1, ret6h: -0.2, ret24h: -0.5, ret3d: -0.3 }, preMove: 0.3, postMove: 0.2, isForwardLooking: false }
        ],
        qualityGrade: 'A',
        qualityScore: 88
      }
    },
    {
      id: 'eu-carbon-tariff',
      name: 'EU CBAM Carbon Border Tariff',
      state: 'negotiating' as PolicyState, // EU-specific state
      domain: 'trade' as Domain,
      score6h: 55.2,
      score24h: 58.7,
      score7d: 52.3,
      velocity: 6.8,
      narrativeDrift: 0.15,
      documents: [],
      entities: [
        { id: 'steel-eu', name: 'EU Steel Sector', type: 'company', exposure: 'bullish', confidence: 72 },
        { id: 'x-us', name: 'US Steel', type: 'company', ticker: 'X', exposure: 'bearish', confidence: 68 },
        { id: 'nue', name: 'Nucor', type: 'company', ticker: 'NUE', exposure: 'bearish', confidence: 65 },
      ],
      l0Count: 1,   // EU Commission proposal
      l05Count: 1,  // DG TRADE draft
      l1Count: 4,   // Politico Europe, FT Europe
      l2Count: 4,   // Member state ministers, industry groups
      inPolicyLoop: false, // Still in negotiation
      lastUpdated: baseTime.toISOString(),
      tradeableAssets: [
        { ticker: 'X', name: 'United States Steel', type: 'stock', exposure: 'bearish', confidence: 68, reasoning: 'CBAM will increase US steel export costs to EU' },
        { ticker: 'NUE', name: 'Nucor Corporation', type: 'stock', exposure: 'bearish', confidence: 65, reasoning: 'High-carbon steel faces tariffs' },
        { ticker: 'SLX', name: 'VanEck Steel ETF', type: 'etf', exposure: 'bearish', confidence: 60, reasoning: '全球钢铁ETF受CBAM影响' },
        { ticker: 'EWG', name: 'iShares MSCI Germany ETF', type: 'etf', exposure: 'bullish', confidence: 55, reasoning: '德国绿色钢铁受益' },
      ],
      driftMetrics: {
        l0_l1_drift: 0.10,
        l1_l2_drift: 0.22,
        overall: 0.15,
        riskLevel: 'medium' as const,
        trend: 'increasing' as const
      },
      // No completed policy loop - still negotiating
      validation: {
        signalTime: new Date(baseTime.getTime() - 120*3600000).toISOString(),
        validationTime: baseTime.toISOString(),
        directionHitRatio: 0.50,
        directionHits: 2,
        directionTotal: 4,
        avgPostMove24h: 1.15,
        scoreMovCorrelation: 0.38,
        forwardLookingRatio: 0.50,
        avgPreMove: 0.9,
        avgPostMove: 1.0,
        byAssetType: {
          stock: { hitRatio: 0.5, avgMove: 1.2, count: 2 },
          etf: { hitRatio: 0.5, avgMove: 0.8, count: 2 },
          futures: { hitRatio: 0, avgMove: 0, count: 0 },
          forex: { hitRatio: 0, avgMove: 0, count: 0 }
        },
        assetValidations: [
          { ticker: 'X', name: 'United States Steel', type: 'stock', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 68, returns: { ret1h: -0.3, ret6h: -0.8, ret24h: -1.2, ret3d: -0.8 }, preMove: 0.8, postMove: 1.2, isForwardLooking: true },
          { ticker: 'NUE', name: 'Nucor Corporation', type: 'stock', systemDirection: 'bearish', actualDirection: 'up', directionMatch: false, confidence: 65, returns: { ret1h: 0.2, ret6h: 0.5, ret24h: 0.8, ret3d: 1.2 }, preMove: 1.0, postMove: 0.6, isForwardLooking: false },
          { ticker: 'SLX', name: 'VanEck Steel ETF', type: 'etf', systemDirection: 'bearish', actualDirection: 'down', directionMatch: true, confidence: 60, returns: { ret1h: -0.2, ret6h: -0.5, ret24h: -0.8, ret3d: -0.5 }, preMove: 0.6, postMove: 0.8, isForwardLooking: true },
          { ticker: 'EWG', name: 'iShares MSCI Germany ETF', type: 'etf', systemDirection: 'bullish', actualDirection: 'down', directionMatch: false, confidence: 55, returns: { ret1h: -0.1, ret6h: -0.3, ret24h: -0.5, ret3d: 0.2 }, preMove: 0.5, postMove: 0.3, isForwardLooking: false }
        ],
        qualityGrade: 'C',
        qualityScore: 52
      }
    }
  ]
  
  // 历史主题 (1�?6个月�? - 用于历史回溯分析
  const historicalTopics: Topic[] = [
    {
      id: 'fed-rate-dec-2025',
      name: 'Fed Rate Decision Dec 2025',
      state: 'digesting' as PolicyState,
      domain: 'rate' as Domain,
      score6h: 15.2,
      score24h: 22.5,
      score7d: 45.8,
      velocity: -8.5,
      narrativeDrift: 0.08,
      documents: [],
      entities: [
        { id: 'fed', name: 'Federal Reserve', type: 'government', ticker: 'TLT', exposure: 'bearish', confidence: 85 },
      ],
      l0Count: 1, l05Count: 2, l1Count: 5, l2Count: 8,
      inPolicyLoop: true,
      lastUpdated: generateHistoricalTime(14, 5),
      tradeableAssets: [
        { ticker: 'TLT', name: 'iShares 20+ Year Treasury Bond ETF', type: 'etf', exposure: 'bearish', confidence: 85, reasoning: 'Rate hike bearish for long bonds' },
        { ticker: 'XLF', name: 'Financial Select Sector SPDR', type: 'etf', exposure: 'bullish', confidence: 78, reasoning: 'Higher rates benefit banks' },
      ],
      driftMetrics: { l0_l1_drift: 0.05, l1_l2_drift: 0.12, overall: 0.08, riskLevel: 'low' as const, trend: 'stable' as const },
      validation: {
        signalTime: generateHistoricalTime(16, 0), validationTime: generateHistoricalTime(14, 0),
        directionHitRatio: 0.85, directionHits: 6, directionTotal: 7, avgPostMove24h: 1.8,
        scoreMovCorrelation: 0.68, forwardLookingRatio: 0.75, avgPreMove: 0.6, avgPostMove: 1.5,
        byAssetType: { stock: { hitRatio: 0.8, avgMove: 1.5, count: 2 }, etf: { hitRatio: 0.9, avgMove: 1.8, count: 5 }, futures: { hitRatio: 0, avgMove: 0, count: 0 }, forex: { hitRatio: 0, avgMove: 0, count: 0 } },
        assetValidations: [], qualityGrade: 'A', qualityScore: 82
      }
    },
    {
      id: 'opec-cut-nov-2025',
      name: 'OPEC+ Production Cut Nov 2025',
      state: 'exhausted' as PolicyState,
      domain: 'trade' as Domain,
      score6h: 8.5,
      score24h: 12.2,
      score7d: 28.5,
      velocity: -15.2,
      narrativeDrift: 0.22,
      documents: [],
      entities: [
        { id: 'opec', name: 'OPEC', type: 'organization', exposure: 'bullish', confidence: 75 },
      ],
      l0Count: 1, l05Count: 1, l1Count: 8, l2Count: 15,
      inPolicyLoop: true,
      lastUpdated: generateHistoricalTime(45, 12),
      tradeableAssets: [
        { ticker: 'USO', name: 'United States Oil Fund', type: 'etf', exposure: 'bullish', confidence: 82, reasoning: 'Production cut supports oil prices' },
        { ticker: 'XLE', name: 'Energy Select Sector SPDR', type: 'etf', exposure: 'bullish', confidence: 78, reasoning: 'Energy sector benefits from higher oil' },
      ],
      driftMetrics: { l0_l1_drift: 0.15, l1_l2_drift: 0.32, overall: 0.22, riskLevel: 'medium' as const, trend: 'decreasing' as const },
      validation: {
        signalTime: generateHistoricalTime(48, 0), validationTime: generateHistoricalTime(45, 0),
        directionHitRatio: 0.70, directionHits: 7, directionTotal: 10, avgPostMove24h: 2.2,
        scoreMovCorrelation: 0.55, forwardLookingRatio: 0.65, avgPreMove: 1.2, avgPostMove: 1.8,
        byAssetType: { stock: { hitRatio: 0.65, avgMove: 2.5, count: 4 }, etf: { hitRatio: 0.75, avgMove: 2.2, count: 6 }, futures: { hitRatio: 0, avgMove: 0, count: 0 }, forex: { hitRatio: 0, avgMove: 0, count: 0 } },
        assetValidations: [], qualityGrade: 'B', qualityScore: 68
      }
    },
    {
      id: 'ecb-tltro-oct-2025',
      name: 'ECB TLTRO Maturity Oct 2025',
      state: 'reversed' as PolicyState,
      domain: 'rate' as Domain,
      score6h: 5.2,
      score24h: 8.8,
      score7d: 18.5,
      velocity: -22.5,
      narrativeDrift: 0.35,
      documents: [],
      entities: [
        { id: 'ecb', name: 'European Central Bank', type: 'government', ticker: 'EWG', exposure: 'ambiguous', confidence: 62 },
      ],
      l0Count: 0, l05Count: 1, l1Count: 4, l2Count: 12,
      inPolicyLoop: false,
      lastUpdated: generateHistoricalTime(90, 8),
      tradeableAssets: [
        { ticker: 'FXE', name: 'CurrencyShares Euro Trust', type: 'etf', exposure: 'bearish', confidence: 68, reasoning: 'TLTRO maturity tightens liquidity' },
        { ticker: 'EUFN', name: 'iShares MSCI Europe Financials', type: 'etf', exposure: 'bearish', confidence: 72, reasoning: 'Bank funding costs increase' },
      ],
      driftMetrics: { l0_l1_drift: 0.25, l1_l2_drift: 0.48, overall: 0.35, riskLevel: 'high' as const, trend: 'increasing' as const },
      validation: {
        signalTime: generateHistoricalTime(95, 0), validationTime: generateHistoricalTime(90, 0),
        directionHitRatio: 0.45, directionHits: 4, directionTotal: 9, avgPostMove24h: 0.8,
        scoreMovCorrelation: 0.32, forwardLookingRatio: 0.40, avgPreMove: 0.9, avgPostMove: 0.7,
        byAssetType: { stock: { hitRatio: 0.4, avgMove: 0.8, count: 3 }, etf: { hitRatio: 0.5, avgMove: 0.9, count: 6 }, futures: { hitRatio: 0, avgMove: 0, count: 0 }, forex: { hitRatio: 0, avgMove: 0, count: 0 } },
        assetValidations: [], qualityGrade: 'D', qualityScore: 42
      }
    },
    {
      id: 'china-stimulus-sep-2025',
      name: 'China Stimulus Package Sep 2025',
      state: 'digesting' as PolicyState,
      domain: 'fiscal' as Domain,
      score6h: 12.5,
      score24h: 18.2,
      score7d: 35.8,
      velocity: -5.2,
      narrativeDrift: 0.15,
      documents: [],
      entities: [
        { id: 'pboc', name: 'PBoC', type: 'government', exposure: 'bullish', confidence: 80 },
        { id: 'ndrc', name: 'NDRC', type: 'government', exposure: 'bullish', confidence: 75 },
      ],
      l0Count: 2, l05Count: 3, l1Count: 12, l2Count: 20,
      inPolicyLoop: true,
      lastUpdated: generateHistoricalTime(120, 15),
      tradeableAssets: [
        { ticker: 'FXI', name: 'iShares China Large-Cap ETF', type: 'etf', exposure: 'bullish', confidence: 85, reasoning: 'Stimulus supports Chinese equities' },
        { ticker: 'KWEB', name: 'KraneShares China Internet ETF', type: 'etf', exposure: 'bullish', confidence: 82, reasoning: 'Tech benefits from easing' },
        { ticker: 'BABA', name: 'Alibaba Group', type: 'stock', exposure: 'bullish', confidence: 78, reasoning: 'Consumer spending boost' },
      ],
      driftMetrics: { l0_l1_drift: 0.08, l1_l2_drift: 0.22, overall: 0.15, riskLevel: 'low' as const, trend: 'stable' as const },
      validation: {
        signalTime: generateHistoricalTime(125, 0), validationTime: generateHistoricalTime(120, 0),
        directionHitRatio: 0.78, directionHits: 7, directionTotal: 9, avgPostMove24h: 3.2,
        scoreMovCorrelation: 0.72, forwardLookingRatio: 0.82, avgPreMove: 0.5, avgPostMove: 2.8,
        byAssetType: { stock: { hitRatio: 0.75, avgMove: 4.5, count: 3 }, etf: { hitRatio: 0.8, avgMove: 2.8, count: 6 }, futures: { hitRatio: 0, avgMove: 0, count: 0 }, forex: { hitRatio: 0, avgMove: 0, count: 0 } },
        assetValidations: [], qualityGrade: 'A', qualityScore: 78
      }
    },
    {
      id: 'us-debt-ceiling-aug-2025',
      name: 'US Debt Ceiling Crisis Aug 2025',
      state: 'exhausted' as PolicyState,
      domain: 'fiscal' as Domain,
      score6h: 3.5,
      score24h: 6.2,
      score7d: 15.8,
      velocity: -28.5,
      narrativeDrift: 0.42,
      documents: [],
      entities: [
        { id: 'treasury', name: 'US Treasury', type: 'government', exposure: 'ambiguous', confidence: 55 },
      ],
      l0Count: 1, l05Count: 2, l1Count: 15, l2Count: 35,
      inPolicyLoop: true,
      lastUpdated: generateHistoricalTime(160, 20),
      tradeableAssets: [
        { ticker: 'TLT', name: 'iShares 20+ Year Treasury Bond ETF', type: 'etf', exposure: 'bearish', confidence: 72, reasoning: 'Default risk premium' },
        { ticker: 'VXX', name: 'iPath Series B S&P 500 VIX', type: 'etf', exposure: 'bullish', confidence: 68, reasoning: 'Vol spike on uncertainty' },
      ],
      driftMetrics: { l0_l1_drift: 0.35, l1_l2_drift: 0.55, overall: 0.42, riskLevel: 'critical' as const, trend: 'decreasing' as const },
      validation: {
        signalTime: generateHistoricalTime(165, 0), validationTime: generateHistoricalTime(160, 0),
        directionHitRatio: 0.55, directionHits: 5, directionTotal: 9, avgPostMove24h: 1.5,
        scoreMovCorrelation: 0.42, forwardLookingRatio: 0.50, avgPreMove: 1.5, avgPostMove: 1.2,
        byAssetType: { stock: { hitRatio: 0.5, avgMove: 1.2, count: 2 }, etf: { hitRatio: 0.6, avgMove: 1.8, count: 7 }, futures: { hitRatio: 0, avgMove: 0, count: 0 }, forex: { hitRatio: 0, avgMove: 0, count: 0 } },
        assetValidations: [], qualityGrade: 'C', qualityScore: 52
      }
    }
  ]
  
  return [...recentTopics, ...historicalTopics]
}

const generateMockAlerts = (): Alert[] => {
  return [
    {
      id: 'alert-1',
      type: 'policy_loop',
      topic: generateMockTopics()[0],
      title: 'Policy Loop Confirmed: China Tariffs',
      summary: 'L0/L0.5/L1/L2 three-tier validation complete, entering policy implementation phase. White House confirms effective February 1st.',
      severity: 'critical',
      evidences: [
        { id: 'ev1', text: 'President Trump announced today that additional 25% tariffs on Chinese goods will take effect February 1st...', span: [0, 95], sourceId: 'whitehouse', sourceName: 'White House Official', level: 'L0', url: 'https://whitehouse.gov/briefings', publishedAt: new Date().toISOString() }
      ],
      createdAt: new Date().toISOString(),
      read: false
    },
    {
      id: 'alert-2',
      type: 'narrative_reversal',
      topic: generateMockTopics()[1],
      title: 'Narrative Reversal: Fed Rate Expectations',
      summary: 'Powell speech changed from "will maintain" to "considering options", uncertainty significantly increased.',
      severity: 'high',
      evidences: [
        { id: 'ev2', text: 'Fed Chair Powell stated the committee is "considering all options" for the March meeting...', span: [0, 88], sourceId: 'bloomberg', sourceName: 'Bloomberg', level: 'L1', url: 'https://bloomberg.com/news', publishedAt: new Date().toISOString() }
      ],
      createdAt: new Date(Date.now() - 3600000).toISOString(),
      read: false
    },
    {
      id: 'alert-3',
      type: 'high_impact',
      topic: generateMockTopics()[3],
      title: 'High Impact: Chip Export Ban Expanded',
      summary: 'Commerce Department officially added 12 Chinese AI companies to Entity List, effective immediately.',
      severity: 'critical',
      evidences: [
        { id: 'ev3', text: 'The Bureau of Industry and Security today added 12 Chinese AI companies to the Entity List...', span: [0, 92], sourceId: 'commerce', sourceName: 'Commerce (BIS)', level: 'L0.5', url: 'https://bis.gov/announcements', publishedAt: new Date().toISOString() }
      ],
      createdAt: new Date(Date.now() - 1800000).toISOString(),
      read: true
    },
    {
      id: 'alert-4',
      type: 'state_change',
      topic: generateMockTopics()[2],
      title: 'State Change: Russia Sanctions emerging to contested',
      summary: 'Topic changed from emerging to contested state, policy direction unclear, multi-party negotiation ongoing.',
      severity: 'medium',
      evidences: [],
      createdAt: new Date(Date.now() - 7200000).toISOString(),
      read: true
    }
  ]
}

const generateMockDocuments = (): Document[] => {
  return [
    {
      id: 'doc-1',
      title: 'Trump announces 25% tariffs on all Chinese imports starting February 1',
      source: SOURCES['whitehouse'],
      publishedAt: new Date().toISOString(),
      url: 'https://whitehouse.gov/briefings/tariffs-2026',
      summary: 'President Trump signed an executive order imposing 25% across-the-board tariffs on Chinese imports, citing national security concerns and trade imbalance.',
      entities: [
        { id: 'china', name: 'China', type: 'country', exposure: 'bearish', confidence: 95 },
        { id: 'nvda', name: 'NVIDIA', type: 'company', ticker: 'NVDA', exposure: 'bearish', confidence: 82 },
      ],
      topics: ['China Tariffs', 'Trade Policy'],
      actions: ['impose', 'sign'],
      sentiment: 'bearish',
      uncertainty: 0.1,
      docScore: 95.5,
      // 新增：DocScore分项拆解
      scoreBreakdown: {
        sourceWeight: 100,        // L0 White House = 100
        actionStrength: 1.8,      // "impose" + "sign" = 强执行动�?
        attributionMultiplier: 1.25,  // direct_quote
        freshness: 0.98,          // 刚发�?
        executionPower: 1.4,      // 总统签署EO = 高执行力
        uncertaintyPenalty: 0.95, // uncertainty=0.1 �?penalty=0.95
        finalScore: 95.5
      },
      evidences: [
        { id: 'ev-doc1', text: '"Effective February 1st, 2026, a 25% tariff will be imposed on all goods originating from the People\'s Republic of China"', span: [156, 275], sourceId: 'whitehouse', sourceName: 'White House Official', level: 'L0', url: 'https://whitehouse.gov/briefings/tariffs-2026', publishedAt: new Date().toISOString() }
      ],
      quotePrimary: '"This action is necessary to protect American workers and national security interests."'
    },
    {
      id: 'doc-2',
      title: 'Commerce Department expands chip export restrictions to cover advanced AI accelerators',
      source: SOURCES['commerce'],
      publishedAt: new Date(Date.now() - 1800000).toISOString(),
      url: 'https://bis.gov/chip-restrictions-2026',
      summary: 'BIS announced expanded export controls covering AI training chips and related technology, effective immediately.',
      entities: [
        { id: 'nvda', name: 'NVIDIA', type: 'company', ticker: 'NVDA', exposure: 'bearish', confidence: 90 },
        { id: 'amd', name: 'AMD', type: 'company', ticker: 'AMD', exposure: 'bearish', confidence: 85 },
        { id: 'asml', name: 'ASML', type: 'company', ticker: 'ASML', exposure: 'bearish', confidence: 88 },
      ],
      topics: ['Chip Export Control', 'Tech Decoupling'],
      actions: ['expand', 'restrict', 'control'],
      sentiment: 'bearish',
      uncertainty: 0.05,
      docScore: 92.3,
      scoreBreakdown: {
        sourceWeight: 85,         // L0.5 Commerce/BIS = 85
        actionStrength: 1.6,      // "expand" + "restrict" + "control" = 强执�?
        attributionMultiplier: 1.25,  // official announcement
        freshness: 0.92,          // 30分钟�?
        executionPower: 1.35,     // 即日生效
        uncertaintyPenalty: 0.97, // uncertainty=0.05
        finalScore: 92.3
      },
      evidences: [
        { id: 'ev-doc2', text: 'The rule covers any semiconductor device capable of 300 teraflops or higher performance in AI workloads', span: [89, 189], sourceId: 'commerce', sourceName: 'Commerce (BIS)', level: 'L0.5', url: 'https://bis.gov/chip-restrictions-2026', publishedAt: new Date().toISOString() }
      ],
      quotePrimary: '"These controls are essential to prevent advanced AI capabilities from reaching adversarial nations."'
    },
    {
      id: 'doc-3',
      title: 'Fed signals potential rate pause amid economic uncertainty',
      source: SOURCES['bloomberg'],
      publishedAt: new Date(Date.now() - 3600000).toISOString(),
      url: 'https://bloomberg.com/fed-rate-signal',
      summary: 'Federal Reserve officials hinted at a possible pause in rate adjustments, with Powell noting "mixed signals" in economic data.',
      entities: [
        { id: 'spy', name: 'S&P 500', type: 'company', ticker: 'SPY', exposure: 'bullish', confidence: 62 },
        { id: 'tlt', name: 'Treasury Bonds', type: 'company', ticker: 'TLT', exposure: 'bullish', confidence: 68 },
      ],
      topics: ['Fed利率', '货币政策'],
      actions: ['signal', 'pause', 'consider'],
      sentiment: 'ambiguous',
      uncertainty: 0.45,
      docScore: 68.2,
      scoreBreakdown: {
        sourceWeight: 88,         // L1 Bloomberg = 88
        actionStrength: 0.8,      // "signal", "consider" = 弱执行动�?
        attributionMultiplier: 1.0,   // mention (非直接引�?
        freshness: 0.85,          // 1小时�?
        executionPower: 0.7,      // Fed 尚未决定 = 低执行力
        uncertaintyPenalty: 0.72, // uncertainty=0.45 �?高惩�?
        finalScore: 68.2
      },
      evidences: [
        { id: 'ev-doc3', text: 'Powell stated the committee would "consider all available options" at the March meeting', span: [234, 318], sourceId: 'bloomberg', sourceName: 'Bloomberg', level: 'L1', url: 'https://bloomberg.com/fed-rate-signal', publishedAt: new Date().toISOString() }
      ],
      mentionPrimary: 'Sources familiar with Fed deliberations suggest internal disagreement on timing'
    },
    {
      id: 'doc-4',
      title: 'Treasury announces new Russia-related sanctions targeting energy sector',
      source: SOURCES['treasury'],
      publishedAt: new Date(Date.now() - 5400000).toISOString(),
      url: 'https://treasury.gov/russia-sanctions-2026',
      summary: 'OFAC designated 15 additional entities and 8 individuals connected to Russian energy exports.',
      entities: [
        { id: 'russia', name: 'Russia', type: 'country', exposure: 'bearish', confidence: 98 },
        { id: 'xom', name: 'ExxonMobil', type: 'company', ticker: 'XOM', exposure: 'ambiguous', confidence: 55 },
      ],
      topics: ['Russia Sanctions', 'Energy Sanctions'],
      actions: ['sanction', 'designate', 'target'],
      sentiment: 'bearish',
      uncertainty: 0.15,
      docScore: 88.7,
      scoreBreakdown: {
        sourceWeight: 90,         // L0.5 Treasury = 90
        actionStrength: 1.5,      // "sanction", "designate" = 中强执行
        attributionMultiplier: 1.25,  // official press release
        freshness: 0.78,          // 1.5小时�?
        executionPower: 1.25,     // 已执�?
        uncertaintyPenalty: 0.90, // uncertainty=0.15
        finalScore: 88.7
      },
      evidences: [
        { id: 'ev-doc4', text: 'OFAC has designated the following entities pursuant to Executive Order 14024', span: [45, 118], sourceId: 'treasury', sourceName: 'US Treasury', level: 'L0.5', url: 'https://treasury.gov/russia-sanctions-2026', publishedAt: new Date().toISOString() }
      ],
      quotePrimary: '"These sanctions will further degrade Russia\'s ability to fund its illegal war in Ukraine."'
    }
  ]
}

// ============== 🆕 空状态组件 ==============
interface EmptyStateProps {
  icon: React.ReactNode
  title: string
  description: string
  action?: {
    label: string
    onClick: () => void
  }
  isLoading?: boolean
  error?: string | null
  diagnostics?: {
    lastAttempt?: string
    source?: string
    retryCount?: number
  }
}

const EmptyState: React.FC<EmptyStateProps> = ({ 
  icon, 
  title, 
  description, 
  action, 
  isLoading, 
  error,
  diagnostics 
}) => {
  return (
    <div className="flex flex-col items-center justify-center py-12 px-4 text-center">
      {isLoading ? (
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500 mb-4" />
      ) : (
        <div className="text-gray-400 dark:text-gray-500 mb-4">
          {icon}
        </div>
      )}
      <h3 className="text-lg font-medium text-gray-900 dark:text-white mb-2">
        {isLoading ? '加载中...' : title}
      </h3>
      <p className="text-sm text-gray-500 dark:text-gray-400 max-w-md mb-4">
        {isLoading ? '正在获取最新数据，请稍候...' : description}
      </p>
      {error && (
        <div className="mb-4 p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg">
          <div className="flex items-center gap-2 text-red-600 dark:text-red-400 text-sm">
            <AlertCircle className="w-4 h-4" />
            <span>{error}</span>
          </div>
        </div>
      )}
      {diagnostics && (
        <div className="mb-4 p-3 bg-gray-50 dark:bg-gray-800 rounded-lg text-xs text-gray-500 dark:text-gray-400">
          {diagnostics.lastAttempt && (
            <div>上次尝试: {new Date(diagnostics.lastAttempt).toLocaleTimeString()}</div>
          )}
          {diagnostics.source && <div>数据源: {diagnostics.source}</div>}
          {diagnostics.retryCount !== undefined && (
            <div>重试次数: {diagnostics.retryCount}</div>
          )}
        </div>
      )}
      {action && !isLoading && (
        <button
          onClick={action.onClick}
          className="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg text-sm font-medium transition-colors"
        >
          {action.label}
        </button>
      )}
    </div>
  )
}

// ============== 🆕 加载状态指示器 ==============
interface LoadingIndicatorProps {
  source: string
  status: 'loading' | 'success' | 'error' | 'idle'
  message?: string
}

const LoadingIndicator: React.FC<LoadingIndicatorProps> = ({ source, status, message }) => {
  const statusConfig = {
    loading: { color: 'text-blue-500', icon: <RefreshCw className="w-3 h-3 animate-spin" />, bg: 'bg-blue-50 dark:bg-blue-900/20' },
    success: { color: 'text-green-500', icon: <CheckCircle className="w-3 h-3" />, bg: 'bg-green-50 dark:bg-green-900/20' },
    error: { color: 'text-red-500', icon: <XCircle className="w-3 h-3" />, bg: 'bg-red-50 dark:bg-red-900/20' },
    idle: { color: 'text-gray-400', icon: <Circle className="w-3 h-3" />, bg: 'bg-gray-50 dark:bg-gray-800' }
  }
  
  const config = statusConfig[status]
  
  return (
    <div className={`inline-flex items-center gap-1.5 px-2 py-1 rounded text-xs ${config.bg}`}>
      <span className={config.color}>{config.icon}</span>
      <span className="text-gray-600 dark:text-gray-300">{source}</span>
      {message && <span className="text-gray-400">({message})</span>}
    </div>
  )
}

// ============== 时间窗口转换工具 ==============
const getTimeWindowMs = (window: '6h' | '24h' | '7d'): number => {
  switch (window) {
    case '6h': return 6 * 60 * 60 * 1000
    case '24h': return 24 * 60 * 60 * 1000
    case '7d': return 7 * 24 * 60 * 60 * 1000
  }
}

// 检查时间是否在窗口�?
const isWithinTimeWindow = (dateStr: string, window: '6h' | '24h' | '7d'): boolean => {
  const date = new Date(dateStr)
  const now = new Date()
  const diff = now.getTime() - date.getTime()
  return diff <= getTimeWindowMs(window)
}

// ============== 组件 ==============

export function NewsIntelligence() {
  // i18n - using react-i18next
  const { t, i18n } = useTranslation()
  const language = i18n.language
  
  // 🆕 Global Time Window (unified across all pages)
  const { window: timeWindow } = useTimeWindow()
  
  // State
  const [topics, setTopics] = useState<Topic[]>([])
  const [alerts, setAlerts] = useState<Alert[]>([])
  const [documents, setDocuments] = useState<Document[]>([])
  const [breakingNews, setBreakingNews] = useState<BreakingNews[]>([])
  const [expandedNewsId, setExpandedNewsId] = useState<string | null>(null)  // 🆕 展开的快讯ID
  const [selectedTopic, setSelectedTopic] = useState<Topic | null>(null)
  const [selectedDocument, setSelectedDocument] = useState<Document | null>(null)
  const [activeTab, setActiveTab] = useState<'overview' | 'topics' | 'documents' | 'alerts' | 'entities' | 'breaking' | 'timeline' | 'listdiff' | 'divergence' | 'execution'>('overview')
  const [filterDomain, setFilterDomain] = useState<Domain | 'all'>('all')
  const [filterLevel, setFilterLevel] = useState<SourceLevel | 'all'>('all')
  const [searchQuery, setSearchQuery] = useState('')
  const [isRefreshing, setIsRefreshing] = useState(false)
  const [lastUpdate, setLastUpdate] = useState<Date>(new Date())
  const [displayTime, setDisplayTime] = useState<string>(() => 
    new Date().toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit' })
  )
  const [liveUpdateCount, setLiveUpdateCount] = useState(0)
  
  // 🆕 Real Data Loading State
  const [isLoadingRealData, setIsLoadingRealData] = useState(false)
  const [realDataError, setRealDataError] = useState<string | null>(null)
  const [useRealData, setUseRealData] = useState(true)  // 切换真实/模拟数据
  const [alertRules, setAlertRules] = useState<AlertRule[]>([])
  const [notificationPermission, setNotificationPermission] = useState<NotificationPermission>('default')
  const refreshIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  
  // 🆕 System Health State
  const [systemMetrics, setSystemMetrics] = useState<SystemMetrics | null>(null)
  const [dataSources, setDataSources] = useState<DataSourceHealth[]>([])
  const [showSystemHealth, setShowSystemHealth] = useState(true)
  
  // 🆕 Signal Supply Status - 信号供给可信度
  const [signalSupplyStatus, setSignalSupplyStatus] = useState<SignalSupplyStatus | null>(null)
  const [signalSources, setSignalSources] = useState<SignalSource[]>([])
  
  // 🆕 Executive Power Graph State
  const [execPowerNews, setExecPowerNews] = useState<ExecutiveNews[]>([])
  const [execPowerStats, setExecPowerStats] = useState<PowerGraphStats | null>(null)
  const [execPowerSources, setExecPowerSources] = useState<ExecutiveSource[]>([])
  const [execPowerFilter, setExecPowerFilter] = useState<'all' | 'L0' | 'L0.5' | 'L1' | 'L2'>('all')
  const [execPowerRegion, setExecPowerRegion] = useState<'all' | 'US' | 'EU' | 'CN' | 'INTL'>('all')
  
  // New Step 2 State: Timeline, List Diff, Divergence
  const [policyTimelines, setPolicyTimelines] = useState<PolicyTimeline[]>([])
  const [listDiffReports, setListDiffReports] = useState<ListDiffReport[]>([])
  const [jurisdictionDivergences, setJurisdictionDivergences] = useState<JurisdictionDivergence[]>([])
  const [immediateActions, setImmediateActions] = useState<ImmediateAction[]>([])
  const [selectedTimeline, setSelectedTimeline] = useState<PolicyTimeline | null>(null)
  const [selectedListReport, setSelectedListReport] = useState<ListDiffReport | null>(null)
  const [selectedDivergence, setSelectedDivergence] = useState<JurisdictionDivergence | null>(null)
  
  // 🆕 Keyboard Shortcuts Help State
  const [showShortcutsHelp, setShowShortcutsHelp] = useState(false)
  const searchInputRef = useRef<HTMLInputElement>(null)
  
  // 🆕 Persistence - Load saved settings on mount
  // 优化：不读取闭包中的 activeTab，直接检查 validTabs
  useEffect(() => {
    const savedSettings = getNewsSettings()
    const validTabs = ['overview', 'topics', 'documents', 'alerts', 'entities', 'breaking', 'timeline', 'listdiff', 'divergence', 'execution']
    if (savedSettings.activeTab && validTabs.includes(savedSettings.activeTab)) {
      setActiveTab(savedSettings.activeTab as typeof activeTab)
    }
  }, [])
  
  // 🆕 Persistence - Save settings when they change
  useEffect(() => {
    saveNewsSettings({ activeTab })
  }, [activeTab])
  
  // 🆕 Tab names for shortcuts
  const tabNames = ['overview', 'topics', 'documents', 'alerts', 'entities', 'breaking', 'timeline', 'listdiff', 'divergence', 'execution']
  
  // 🆕 Refresh data handler for keyboard shortcut
  const handleRefreshShortcut = useCallback(() => {
    if (!isRefreshing) {
      loadRealData?.()
    }
  }, [isRefreshing])
  
  // 🆕 Focus search input
  const focusSearch = useCallback(() => {
    searchInputRef.current?.focus()
  }, [])
  
  // 🆕 Keyboard Shortcuts Configuration
  const keyboardShortcuts: KeyboardShortcut[] = useMemo(() => [
    // Tab switching (1-9)
    ...createTabShortcuts(tabNames, (tab) => setActiveTab(tab as typeof activeTab), activeTab),
    // Refresh data
    { key: 'r', action: handleRefreshShortcut, description: 'Refresh data' },
    // Show help
    { key: '?', shiftKey: true, action: () => setShowShortcutsHelp(true), description: 'Show keyboard shortcuts' },
    // Focus search
    { key: '/', action: focusSearch, description: 'Focus search' },
    // Close modals
    { key: 'Escape', action: () => {
      setSelectedTopic(null)
      setSelectedDocument(null)
      setSelectedTimeline(null)
      setSelectedListReport(null)
      setSelectedDivergence(null)
      setShowShortcutsHelp(false)
    }, description: 'Close modals' },
  ], [activeTab, handleRefreshShortcut, focusSearch])
  
  // 🆕 Register Keyboard Shortcuts
  useKeyboardShortcuts(keyboardShortcuts, true)
  
  // 🆕 Real Data Loading Function with Health Tracking
  const loadRealData = useCallback(async () => {
    setIsLoadingRealData(true)
    setRealDataError(null)
    const loadStartTime = Date.now()
    
    try {
      // 并行加载多个数据源
      const [fedDocsResult, newsResult] = await Promise.allSettled([
        (async () => {
          const start = Date.now()
          try {
            const result = await newsDataService.getFederalRegisterDocuments({
              agencies: ['ofac', 'bis', 'treasury', 'commerce', 'ustr'],
              perPage: 30
            })
            systemHealthService.recordAPICall({
              timestamp: new Date().toISOString(),
              source: 'federal-register',
              endpoint: '/documents',
              method: 'GET',
              statusCode: 200,
              latencyMs: Date.now() - start,
              responseSize: JSON.stringify(result).length,
              success: true
            })
            return result
          } catch (err) {
            systemHealthService.recordAPICall({
              timestamp: new Date().toISOString(),
              source: 'federal-register',
              endpoint: '/documents',
              method: 'GET',
              statusCode: 500,
              latencyMs: Date.now() - start,
              responseSize: 0,
              success: false,
              error: err instanceof Error ? err.message : 'Unknown error'
            })
            throw err
          }
        })(),
        (async () => {
          const start = Date.now()
          try {
            const result = await newsDataService.getNewsHeadlines({
              q: '(tariff OR sanction OR trade OR semiconductor OR "export control") AND (China OR EU OR Trump)',
              language: 'en',
              sortBy: 'publishedAt',
              pageSize: 30
            })
            systemHealthService.recordAPICall({
              timestamp: new Date().toISOString(),
              source: 'newsapi',
              endpoint: '/everything',
              method: 'GET',
              statusCode: 200,
              latencyMs: Date.now() - start,
              responseSize: JSON.stringify(result).length,
              success: true
            })
            return result
          } catch (err) {
            systemHealthService.recordAPICall({
              timestamp: new Date().toISOString(),
              source: 'newsapi',
              endpoint: '/everything',
              method: 'GET',
              statusCode: 500,
              latencyMs: Date.now() - start,
              responseSize: 0,
              success: false,
              error: err instanceof Error ? err.message : 'Unknown error'
            })
            throw err
          }
        })()
      ])
      
      // 处理 Federal Register 数据 -> 转换为 Document 格式
      if (fedDocsResult.status === 'fulfilled' && fedDocsResult.value.length > 0) {
        const realDocs: Document[] = fedDocsResult.value.map((doc, idx) => {
          const domain = extractDomainFromText(doc.title + ' ' + (doc.abstract || ''))
          const sentiment = analyzeSentimentSimple(doc.title + ' ' + (doc.abstract || ''))
          
          return {
            id: `fed-${doc.document_number}`,
            title: doc.title,
            source: {
              id: 'federal-register',
              name: 'Federal Register',
              nameCn: '联邦公报',
              level: 'L0' as SourceLevel,
              tier: 'A' as SourceTier,
              weight: 95,
              jurisdiction: 'US' as Jurisdiction,
              domain: ['regulation', 'sanction'] as Domain[],
              executionPower: 95,
              description: 'Official US Government Publication'
            },
            publishedAt: doc.publication_date,
            url: doc.html_url,
            summary: doc.abstract || doc.title,
            entities: extractEntitiesFromText(doc.title + ' ' + (doc.abstract || '')),
            topics: doc.topics || [domain],
            actions: extractActionsFromText(doc.abstract || ''),
            sentiment,
            uncertainty: sentiment === 'ambiguous' ? 0.7 : 0.3,
            docScore: 85 + Math.random() * 10,
            scoreBreakdown: {
              sourceWeight: 95,
              levelMultiplier: 1.4,
              freshnessBonus: 10,
              volumeWeight: 1.0,
              domainSensitivity: 1.3,
              uncertaintyPenalty: 0.9,
              baseValue: 75
            },
            evidences: [{
              id: `ev-${idx}`,
              source: {
                id: 'federal-register',
                name: 'Federal Register',
                nameCn: '联邦公报',
                level: 'L0' as SourceLevel,
                tier: 'A' as SourceTier,
                weight: 95,
                jurisdiction: 'US' as Jurisdiction,
                domain: ['regulation'] as Domain[],
                executionPower: 95,
                description: ''
              },
              publishedAt: doc.publication_date,
              quote: doc.abstract?.substring(0, 200) || doc.title,
              url: doc.html_url,
              validatedAt: new Date().toISOString()
            }],
            quotePrimary: doc.abstract?.substring(0, 150),
            mentionPrimary: doc.agencies?.[0]?.name || 'Federal Agency'
          }
        })
        
        setDocuments(prev => [...realDocs, ...prev.filter(d => !d.id.startsWith('fed-'))].slice(0, 100))
      }
      
      // 处理新闻数据 -> 转换为 BreakingNews 格式
      if (newsResult.status === 'fulfilled' && newsResult.value.length > 0) {
        const realNews: BreakingNews[] = newsResult.value.map((item, idx) => {
          const sourceLevel = determineSourceLevelFromName(item.source)
          const domain = extractDomainFromText(item.title + ' ' + item.description)
          const sentiment = analyzeSentimentSimple(item.title + ' ' + item.description)
          
          return {
            id: `real-news-${idx}-${Date.now()}`,
            headline: item.title,
            source: {
              id: item.source.toLowerCase().replace(/\s+/g, '-'),
              name: item.source,
              nameCn: item.source,
              level: sourceLevel,
              tier: sourceLevel === 'L0' ? 'A' : sourceLevel === 'L0.5' ? 'A' : 'B' as SourceTier,
              weight: sourceLevel === 'L0' ? 95 : sourceLevel === 'L0.5' ? 85 : sourceLevel === 'L1' ? 70 : 50,
              jurisdiction: 'US' as Jurisdiction,
              domain: [domain],
              executionPower: sourceLevel === 'L0' ? 90 : 30,
              description: item.source
            },
            publishedAt: item.publishedAt,
            urgency: sourceLevel === 'L0' ? 'flash' : sourceLevel === 'L0.5' ? 'urgent' : 'breaking',
            topics: [domain],
            industries: [{
              industry: domain === 'trade' ? 'Trade' : domain === 'sanction' ? 'Finance' : 'Technology',
              icon: domain === 'trade' ? '📦' : domain === 'sanction' ? '🏦' : '💻',
              confidence: 70 + Math.floor(Math.random() * 25),
              direction: sentiment,
              reasoning: `Auto-analyzed from ${item.source}`,
              relatedETFs: domain === 'trade' ? ['SPY', 'EEM', 'FXI'] : ['SMH', 'SOXX', 'QQQ']
            }],
            sentiment,
            isRead: false
          }
        })
        
        setBreakingNews(prev => [...realNews, ...prev.filter(n => !n.id.startsWith('real-news-'))].slice(0, 50))
      }
      
      // 加载警报规则
      setAlertRules(realAlertService.getRules())
      
      // 加载服务中存储的警报
      const storedAlerts = realAlertService.getAlerts()
      if (storedAlerts.length > 0) {
        const convertedAlerts: Alert[] = storedAlerts.slice(0, 20).map(sa => ({
          id: sa.id,
          type: 'high_impact' as const,
          topic: topics[0] || { id: 'unknown', name: 'Unknown' } as Topic,
          title: sa.title,
          summary: sa.message,
          severity: sa.priority === 'critical' ? 'critical' : sa.priority === 'high' ? 'high' : 'medium',
          evidences: [],
          createdAt: sa.triggeredAt,
          read: sa.status !== 'active'
        }))
        setAlerts(prev => [...convertedAlerts, ...prev].slice(0, 50))
      }
      
      // 🆕 Generate Topics from real documents and news
      const generatedTopics = generateTopicsFromRealData(
        fedDocsResult.status === 'fulfilled' ? fedDocsResult.value : [],
        newsResult.status === 'fulfilled' ? newsResult.value : []
      )
      if (generatedTopics.length > 0) {
        setTopics(generatedTopics)
      }
      
      // Update system health counters
      systemHealthService.updateCounters(
        fedDocsResult.status === 'fulfilled' ? fedDocsResult.value.length : 0,
        0,
        generatedTopics.length
      )
      
      setLastUpdate(new Date())
      console.log('[NewsIntelligence] Real data loaded successfully', {
        docs: fedDocsResult.status === 'fulfilled' ? fedDocsResult.value.length : 0,
        news: newsResult.status === 'fulfilled' ? newsResult.value.length : 0,
        topics: generatedTopics.length,
        loadTime: Date.now() - loadStartTime
      })
      
    } catch (error) {
      console.error('[NewsIntelligence] Error loading real data:', error)
      setRealDataError(error instanceof Error ? error.message : 'Failed to load real data')
    } finally {
      setIsLoadingRealData(false)
    }
  }, []) // 移除topics依赖，避免循环更新
  
  // 🆕 Helper functions for data processing
  const extractDomainFromText = (text: string): Domain => {
    const lower = text.toLowerCase()
    if (/tariff|trade|import|export|customs|duty/i.test(lower)) return 'trade'
    if (/sanction|ofac|sdn|blocked|designated/i.test(lower)) return 'sanction'
    if (/semiconductor|chip|export control|bis|entity list/i.test(lower)) return 'export_control'
    if (/rate|fed|fomc|monetary|interest/i.test(lower)) return 'rate'
    if (/antitrust|merger|competition|ftc/i.test(lower)) return 'antitrust'
    return 'regulation'
  }
  
  const analyzeSentimentSimple = (text: string): ImpactDirection => {
    const positive = /growth|positive|increase|ease|relief|support|lift/i.test(text)
    const negative = /decline|negative|decrease|sanction|ban|restrict|penalty|fine|tariff/i.test(text)
    if (positive && !negative) return 'bullish'
    if (negative && !positive) return 'bearish'
    return 'ambiguous'
  }
  
  const determineSourceLevelFromName = (source: string): SourceLevel => {
    const lower = source.toLowerCase()
    if (/white house|treasury|ofac|bis|federal register|president/i.test(lower)) return 'L0'
    if (/fed|federal reserve|ecb|pboc|congress|senate/i.test(lower)) return 'L0.5'
    if (/reuters|bloomberg|associated press|afp|ft|wsj|financial times/i.test(lower)) return 'L1'
    return 'L2'
  }
  
  const extractEntitiesFromText = (text: string): Entity[] => {
    const entities: Entity[] = []
    const companyPattern = /\b(Huawei|SMIC|TikTok|ByteDance|Alibaba|Tencent|ZTE|DJI|Hikvision)\b/gi
    let match
    while ((match = companyPattern.exec(text)) !== null) {
      entities.push({
        id: match[1].toLowerCase(),
        name: match[1],
        type: 'company',
        country: 'China',
        exposure: 'direct',
        mentions: 1
      })
    }
    return entities.slice(0, 5)
  }
  
  const extractActionsFromText = (text: string): string[] => {
    const actions: string[] = []
    if (/added|designat/i.test(text)) actions.push('Entity Added')
    if (/removed|delisted/i.test(text)) actions.push('Entity Removed')
    if (/effective immediately/i.test(text)) actions.push('Immediate Effect')
    if (/comment period/i.test(text)) actions.push('Comment Period Open')
    return actions
  }
  
  // 🆕 Generate Topics from real data
  const generateTopicsFromRealData = (
    fedDocs: any[],
    newsItems: any[]
  ): Topic[] => {
    // Group by domain
    const domainGroups: Record<Domain, { docs: any[], news: any[] }> = {
      trade: { docs: [], news: [] },
      sanction: { docs: [], news: [] },
      export_control: { docs: [], news: [] },
      rate: { docs: [], news: [] },
      fiscal: { docs: [], news: [] },
      regulation: { docs: [], news: [] },
      war: { docs: [], news: [] },
      antitrust: { docs: [], news: [] }
    }
    
    // Categorize documents
    fedDocs.forEach(doc => {
      const text = (doc.title || '') + ' ' + (doc.abstract || '')
      const domain = extractDomainFromText(text)
      domainGroups[domain].docs.push(doc)
    })
    
    // Categorize news
    newsItems.forEach(item => {
      const text = (item.title || '') + ' ' + (item.description || '')
      const domain = extractDomainFromText(text)
      domainGroups[domain].news.push(item)
    })
    
    // Generate topics for domains with content
    const generatedTopics: Topic[] = []
    
    Object.entries(domainGroups).forEach(([domain, { docs, news }]) => {
      if (docs.length === 0 && news.length === 0) return
      
      // Calculate aggregate score
      const totalItems = docs.length + news.length
      const l0Count = docs.length // Federal Register = L0
      const l1Count = news.filter(n => determineSourceLevelFromName(n.source || '') === 'L1').length
      const l2Count = news.filter(n => determineSourceLevelFromName(n.source || '') === 'L2').length
      
      // Determine sentiment from all items
      let bullishCount = 0
      let bearishCount = 0
      ;[...docs, ...news].forEach(item => {
        const text = item.title + ' ' + (item.abstract || item.description || '')
        const sentiment = analyzeSentimentSimple(text)
        if (sentiment === 'bullish') bullishCount++
        if (sentiment === 'bearish') bearishCount++
      })
      
      const netBias = (bullishCount - bearishCount) / Math.max(1, totalItems)
      
      // Topic name based on domain and recent content
      const topicNames: Record<Domain, string> = {
        trade: 'US-China Trade Policy',
        sanction: 'OFAC Sanctions Updates',
        export_control: 'Semiconductor Export Controls',
        rate: 'Federal Reserve Rate Policy',
        fiscal: 'US Fiscal Policy',
        regulation: 'Regulatory Updates',
        war: 'Geopolitical Tensions',
        antitrust: 'Antitrust Enforcement'
      }
      
      // Most recent item for timestamp
      const allItems = [...docs, ...news].sort((a, b) => 
        new Date(b.publication_date || b.publishedAt || 0).getTime() - 
        new Date(a.publication_date || a.publishedAt || 0).getTime()
      )
      const mostRecent = allItems[0]
      const mostRecentTime = mostRecent?.publication_date || mostRecent?.publishedAt || new Date().toISOString()
      
      // Calculate score based on source quality
      const baseScore = 30 + (l0Count * 15) + (l1Count * 5) + (l2Count * 2)
      const score24h = Math.min(100, baseScore)
      
      // State estimation
      let state: 'emerging' | 'negotiating' | 'contested' | 'implementing' | 'digesting' | 'exhausted' | 'reversed' = 'emerging'
      if (docs.some(d => /effective immediately|hereby ordered/i.test(d.abstract || ''))) {
        state = 'implementing'
      } else if (docs.length > 3 || news.length > 10) {
        state = 'negotiating'
      }
      
      const topic: Topic = {
        id: `real-${domain}-${Date.now()}`,
        name: topicNames[domain as Domain],
        domain: domain as Domain,
        score24h,
        velocity: (bullishCount - bearishCount) * 2,
        lastUpdated: mostRecentTime,
        state,
        stateTransitions: [],
        sources: {
          L0: l0Count,
          'L0.5': 0,
          L1: l1Count,
          L2: l2Count
        },
        l0Count,
        l1Count,
        l2Count,
        totalCount: totalItems,
        topSources: docs.slice(0, 3).map(d => d.agencies?.[0]?.name || 'Federal Register'),
        entities: docs.flatMap(d => extractEntitiesFromText(d.title + ' ' + (d.abstract || ''))),
        tradeableAssets: domain === 'trade' ? [
          { ticker: 'FXI', name: 'China Large-Cap ETF', exposure: 'bearish' as const, weight: 0.4 },
          { ticker: 'EEM', name: 'Emerging Markets ETF', exposure: 'bearish' as const, weight: 0.3 },
          { ticker: 'SPY', name: 'S&P 500 ETF', exposure: 'ambiguous' as const, weight: 0.3 }
        ] : domain === 'export_control' ? [
          { ticker: 'SMH', name: 'Semiconductor ETF', exposure: bearishCount > bullishCount ? 'bearish' as const : 'bullish' as const, weight: 0.5 },
          { ticker: 'SOXX', name: 'Semiconductor Index', exposure: bearishCount > bullishCount ? 'bearish' as const : 'bullish' as const, weight: 0.3 },
          { ticker: 'NVDA', name: 'NVIDIA', exposure: bearishCount > bullishCount ? 'bearish' as const : 'bullish' as const, weight: 0.2 }
        ] : domain === 'sanction' ? [
          { ticker: 'RSX', name: 'Russia ETF', exposure: 'bearish' as const, weight: 0.4 },
          { ticker: 'USO', name: 'Oil Fund', exposure: 'bullish' as const, weight: 0.3 },
          { ticker: 'GLD', name: 'Gold ETF', exposure: 'bullish' as const, weight: 0.3 }
        ] : [],
        positionRecommendation: score24h >= 70 ? (bearishCount > bullishCount ? 'short' : 'long') : 'neutral',
        region: 'US'
      }
      
      generatedTopics.push(topic)
    })
    
    // Sort by score
    return generatedTopics.sort((a, b) => b.score24h - a.score24h)
  }
  
  // 🆕 Request notification permission
  const requestNotifications = useCallback(async () => {
    const granted = await realAlertService.requestNotificationPermission()
    setNotificationPermission(granted ? 'granted' : 'denied')
    return granted
  }, [])

  // 🆕 稳定的时钟更新 - 每秒更新一次显示时间
  useEffect(() => {
    const clockInterval = setInterval(() => {
      setDisplayTime(new Date().toLocaleTimeString('zh-CN', { 
        hour: '2-digit', 
        minute: '2-digit', 
        second: '2-digit' 
      }))
    }, 1000)
    return () => clearInterval(clockInterval)
  }, [])

  // 初始化数据 - 真实数据优先，API失败时使用模拟数据作为备用
  useEffect(() => {
    // 🆕 使用模拟数据初始化，确保页面有内容显示
    // 真实API数据加载成功后会替换这些数据
    setPolicyTimelines(generateMockTimelines())
    setListDiffReports(generateMockListChanges())
    setJurisdictionDivergences(generateMockDivergences())
    setImmediateActions(generateMockImmediateActions())
    setBreakingNews(generateBreakingNews())
    // 🆕 Topics 使用 mock 数据作为默认值，确保标签页有内容
    setTopics(generateMockTopics())
    setAlerts(generateMockAlerts())
    setDocuments(generateMockDocuments())
    
    // 🆕 初始化系统健康监控 - 10秒稳定刷新机制
    // 设计原则：数据在10秒内保持完全稳定，然后批量更新
    systemHealthService.simulateHealthCheck()
    setDataSources(systemHealthService.getDataSources())
    setSystemMetrics(systemHealthService.getMetrics())
    
    // 10秒定时刷新系统健康数据
    const healthRefreshInterval = setInterval(() => {
      systemHealthService.simulateHealthCheck()
      setDataSources(systemHealthService.getDataSources())
      setSystemMetrics(systemHealthService.getMetrics())
    }, 10000) // 固定10秒刷新
    
    // 🆕 初始化信号供给可信度控制台
    setSignalSources(signalSupplyController.getSources())
    setSignalSupplyStatus(signalSupplyController.getSupplyStatus())
    
    // 🆕 初始化执行权力图谱服务 - 静默后台运行
    // 设计原则：只在有真正新数据时才更新UI
    executivePowerService.start()
    setExecPowerSources(executivePowerService.getSources())
    setExecPowerStats(executivePowerService.getStats())
    
    // 10秒定时刷新执行权力数据
    const execPowerRefreshInterval = setInterval(() => {
      setExecPowerNews(executivePowerService.getAllNews())  // 🆕 修复：getNews → getAllNews
      setExecPowerStats(executivePowerService.getStats())
    }, 10000) // 固定10秒刷新
    
    // 加载真实数据 (尝试一次，失败则保留模拟数据)
    loadRealData()
    
    // 🆕 智能刷新机制 - 自适应刷新频率 (后台静默运行)
    // - 有新数据时: 10分钟刷新 (从5分钟增加)
    // - 连续3次无新数据: 延长到30分钟
    // - 连续5次无新数据: 延长到60分钟
    let noNewDataCount = 0
    let previousDataHash = ''
    let currentInterval = 10 * 60 * 1000  // 初始10分钟 (从5分钟增加)
    
    const calculateDataHash = () => {
      const metrics = systemHealthService.getMetrics()
      return `${metrics.documentsProcessed}-${metrics.alertsGenerated}-${metrics.topicsTracked}`
    }
    
    const adjustRefreshInterval = (hasNewData: boolean) => {
      if (hasNewData) {
        noNewDataCount = 0
        currentInterval = 10 * 60 * 1000  // 回到10分钟
      } else {
        noNewDataCount++
        if (noNewDataCount >= 5) {
          currentInterval = 60 * 60 * 1000  // 60分钟
        } else if (noNewDataCount >= 3) {
          currentInterval = 30 * 60 * 1000  // 30分钟
        }
      }
      
      // 重新设置定时器
      if (refreshIntervalRef.current) {
        clearInterval(refreshIntervalRef.current)
      }
      refreshIntervalRef.current = setInterval(smartRefresh, currentInterval)
      
      // 静默日志，不打印到控制台
      // console.log(`[NewsIntelligence] 刷新间隔调整: ${currentInterval / 60000}分钟`)
    }
    
    const smartRefresh = async () => {
      const beforeHash = calculateDataHash()
      // 静默加载 - 不触发 isLoadingRealData 状态变化
      await loadRealData()
      // 不再频繁调用 simulateHealthCheck，避免UI抖动
      const afterHash = calculateDataHash()
      
      const hasNewData = beforeHash !== afterHash
      previousDataHash = afterHash
      adjustRefreshInterval(hasNewData)
    }
    
    refreshIntervalRef.current = setInterval(smartRefresh, currentInterval)
    
    // 订阅实时警报
    const unsubscribe = realAlertService.subscribe((alert) => {
      const newAlert: Alert = {
        id: alert.id,
        type: 'high_impact',
        topic: topics[0] || { id: 'unknown', name: 'Unknown Topic' } as Topic,
        title: alert.title,
        summary: alert.message,
        severity: alert.priority === 'critical' ? 'critical' : alert.priority === 'high' ? 'high' : 'medium',
        evidences: [],
        createdAt: alert.triggeredAt,
        read: false
      }
      setAlerts(prev => [newAlert, ...prev].slice(0, 50))
      systemHealthService.updateCounters(0, 1, 0)
    })
    
    return () => {
      unsubscribe()
      clearInterval(healthRefreshInterval)
      clearInterval(execPowerRefreshInterval)
      executivePowerService.stop()
      if (refreshIntervalRef.current) {
        clearInterval(refreshIntervalRef.current)
      }
    }
  // 🆕 移除 topics 依赖！topics 变化不应该重启整个引擎
  // loadRealData 是 useCallback，不会变化
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  
  // 实时更新机制 - �?5秒检查各机构新消�?
  useEffect(() => {
    // 官方机构消息源列�?- 按重要性排�?
    const LIVE_SOURCE_POOL = [
      { sourceId: 'fed', headlines: ['FOMC minutes released: Policy stance unchanged', 'Fed Governor comments on inflation outlook', 'Balance sheet reduction pace maintained'], urgency: 'urgent' as const },
      { sourceId: 'treasury', headlines: ['Treasury issues guidance on new sanctions', 'Debt ceiling discussions ongoing', 'OFAC adds entities to SDN list'], urgency: 'breaking' as const },
      { sourceId: 'whitehouse', headlines: ['White House announces trade policy review', 'National Security Council statement on Taiwan', 'Executive order on AI regulation signed'], urgency: 'flash' as const },
      { sourceId: 'ecb', headlines: ['ECB maintains rates, signals vigilance', 'Lagarde: Inflation trajectory still uncertain', 'ECB balance sheet update published'], urgency: 'urgent' as const },
      { sourceId: 'pboc', headlines: ['PBoC sets yuan midpoint lower', 'MLF rate held steady at current level', 'RRR adjustment under consideration'], urgency: 'urgent' as const },
      { sourceId: 'boj', headlines: ['BoJ maintains YCC policy framework', 'Ueda: Will respond flexibly to data', 'JGB purchase schedule released'], urgency: 'breaking' as const },
      { sourceId: 'eu-dg-trade', headlines: ['DG TRADE opens investigation into imports', 'Trade defense measures announced', 'FTA negotiation round concluded'], urgency: 'breaking' as const },
      { sourceId: 'eu-dg-comp', headlines: ['DG COMP opens antitrust probe', 'Merger clearance with conditions', 'State aid investigation launched'], urgency: 'breaking' as const },
      { sourceId: 'opec', headlines: ['OPEC+ maintains production cuts', 'Ministerial meeting concludes', 'Supply adjustment for Q2 announced'], urgency: 'urgent' as const },
      { sourceId: 'imf', headlines: ['IMF revises global growth forecast', 'Article IV consultation completed', 'Financial stability warning issued'], urgency: 'breaking' as const },
      { sourceId: 'commerce', headlines: ['BIS adds entities to Entity List', 'Export control rules updated', 'New semiconductor restrictions announced'], urgency: 'urgent' as const },
      { sourceId: 'ustr', headlines: ['USTR initiates Section 301 review', 'Trade agreement implementation update', 'Market access negotiations progress'], urgency: 'breaking' as const },
      { sourceId: 'russia-cb', headlines: ['Bank of Russia holds key rate', 'Ruble intervention parameters set', 'Capital control measures reviewed'], urgency: 'breaking' as const },
      { sourceId: 'rbi', headlines: ['RBI policy announcement due', 'Inflation targeting framework review', 'Rupee intervention details emerge'], urgency: 'breaking' as const },
    ]
    
    // 🆕 模拟新闻生成器 - 仅用于演示
    // 设计原则：决策系统应该只显示真实数据
    // 这里将频率降低到5分钟，并只有2%概率生成
    // 在生产环境应该完全禁用这个定时器
    const SIMULATION_ENABLED = false  // 生产环境设为 false
    
    if (!SIMULATION_ENABLED) {
      // 生产模式：不运行模拟器，只依赖真实数据源
      return () => {}
    }
    
    const interval = setInterval(() => {
      // 2%概率产生新消息 (大幅降低)
      if (Math.random() < 0.02) {
        const randomSource = LIVE_SOURCE_POOL[Math.floor(Math.random() * LIVE_SOURCE_POOL.length)]
        const source = SOURCES[randomSource.sourceId]
        if (!source) return
        
        const headline = randomSource.headlines[Math.floor(Math.random() * randomSource.headlines.length)]
        const newNews: BreakingNews = {
          id: `bn-live-${Date.now()}`,
          headline: `[${source.name}] ${headline}`,
          source: source,
          publishedAt: new Date().toISOString(),
          urgency: randomSource.urgency,
          topics: [source.domain[0] || 'general'],
          industries: [{
            industry: 'Market Impact',
            icon: '📊',
            confidence: 70 + Math.floor(Math.random() * 25),
            direction: Math.random() > 0.5 ? 'bullish' : 'bearish',
            reasoning: `Auto-assessed from ${source.name}`,
            relatedETFs: ['SPY', 'QQQ', 'TLT']
          }],
          sentiment: Math.random() > 0.5 ? 'bullish' : 'bearish',
          isRead: false
        }
        
        // 静默添加 - 不触发任何其他状态更新
        setBreakingNews(prev => [newNews, ...prev].slice(0, 50))
        setLiveUpdateCount(prev => prev + 1)
      }
    }, 300000) // 5分钟检查一次 (从60秒改为300秒)
    
    return () => clearInterval(interval)
  }, [topics])
  
  // 计算增强的topics（含决策引擎计算�?
  const enhancedTopics = useMemo(() => {
    return topics.map(topic => {
      // 计算可信�?
      const credibility = calculateCredibility(topic.validation)
      
      // 计算净偏向 (-1 to 1)
      const bullishAssets = topic.tradeableAssets.filter(a => a.exposure === 'bullish').length
      const bearishAssets = topic.tradeableAssets.filter(a => a.exposure === 'bearish').length
      const totalAssets = topic.tradeableAssets.length || 1
      const netBias = (bullishAssets - bearishAssets) / totalAssets
      
      // 计算已定价指�?(基于状态和信号年龄)
      const pricingIn = topic.state === 'exhausted' ? 0.9 :
                       topic.state === 'digesting' ? 0.6 :
                       topic.state === 'implementing' ? 0.3 :
                       0.1
      
      // 🆕 Phase 3: 计算信号半衰�?(影响持续�?
      const signalHalfLife = calculateSignalHalfLife(topic)
      
      // 使用半衰期调整后的持续�?
      const persistence = signalHalfLife.persistenceScore
      
      // 计算漂移惩罚
      const driftPenalty = topic.driftMetrics.riskLevel === 'critical' ? 0.5 :
                          topic.driftMetrics.riskLevel === 'high' ? 0.65 :
                          topic.driftMetrics.riskLevel === 'medium' ? 0.8 :
                          1.0
      
      // 计算DecisionScore (使用本地版本)
      const decisionScore = calculateLocalDecisionScore(
        topic.score24h,
        netBias || 0.8, // 如果全是单向，给0.8
        topic.domain,
        pricingIn,
        persistence,
        driftPenalty,
        credibility.capMultiplier * 100
      )
      
      // 🆕 Phase 1: 计算仓位制度
      const positionRegime = calculatePositionRegime(decisionScore.finalScore)
      
      // 🆕 Phase 1: 计算时机评估
      const timing = calculateTiming(topic.validation)
      
      // 🆕 Phase 1: 生成系统立场
      const stance = generateSystemStance(topic, decisionScore, timing, credibility)
      
      // 🆕 Phase 1: NO_TRADE 可行动化
      const noTradeInfo = generateNoTradeInfo(topic, decisionScore, timing, credibility)
      
      // 计算净敞口矩阵
      const netExposure = calculateNetExposure(
        topic.entities,
        topic.tradeableAssets,
        topic.id,
        topic.name
      )
      
      // 生成信息层级
      const infoHierarchy = generateInfoHierarchy(topic, decisionScore, credibility)
      
      // 🆕 Phase 3: 拥挤风险 (需要全部topics，这里用简化版)
      const mainExposure = topic.tradeableAssets[0]?.exposure || 'ambiguous'
      const crowdingRisk = calculateCrowdingRisk(topics, mainExposure)
      
      // 应用拥挤风险覆盖到仓位上�?
      const adjustedPositionCap = positionRegime.positionCap * crowdingRisk.positionCapOverlay
      
      return {
        ...topic,
        stateDefinition: POLICY_STATE_DEFINITIONS[topic.state],
        decisionScore,
        credibility,
        netExposure,
        infoHierarchy,
        // 🆕 Phase 1
        positionRegime: {
          ...positionRegime,
          positionCap: adjustedPositionCap // 应用拥挤风险调整
        },
        timing,
        stance,
        noTradeInfo,
        // 🆕 Phase 3
        signalHalfLife,
        crowdingRisk
      }
    })
  }, [topics])
  
  // 🆕 Phase 2: 冲突解决器缓�?(按实�?
  const entityConflictResolvers = useMemo(() => {
    const resolvers = new Map<string, ConflictResolver>()
    const allTickers = new Set<string>()
    
    enhancedTopics.forEach(t => {
      t.tradeableAssets?.forEach(a => allTickers.add(a.ticker))
    })
    
    allTickers.forEach(ticker => {
      resolvers.set(ticker, calculateConflictResolver(enhancedTopics, ticker))
    })
    
    return resolvers
  }, [enhancedTopics])
  
  // 🆕 Phase 3: 假信号审�?
  const falseSignalAudit = useMemo(() => {
    return generateFalseSignalAudit(enhancedTopics)
  }, [enhancedTopics])
  
  // 🆕 晨会/周报生成
  const morningReport = useMemo(() => {
    return generateReport(enhancedTopics, 'morning')
  }, [enhancedTopics])
  
  // 刷新数据 - 使用真实数据加载
  const handleRefresh = useCallback(async () => {
    setIsRefreshing(true)
    try {
      await loadRealData()
      // 不再调用 simulateHealthCheck - 避免延迟数字闪烁
      // 真实的延迟数据会在 loadRealData 中通过 recordAPICall 更新
      setLastUpdate(new Date())
    } finally {
      setIsRefreshing(false)
    }
  }, [loadRealData])
  
  // 计算统计 - 使用增强后的topics
  const stats = useMemo(() => {
    const criticalAlerts = alerts.filter(a => a.severity === 'critical' && !a.read).length
    const policyLoops = enhancedTopics.filter(t => t.inPolicyLoop).length
    const implementing = enhancedTopics.filter(t => t.state === 'implementing').length
    const highVelocity = enhancedTopics.filter(t => Math.abs(t.velocity) > 15).length
    const unreadBreaking = breakingNews.filter(b => !b.isRead).length
    const highConviction = enhancedTopics.filter(t => t.decisionScore?.actionTier === 'high-conviction').length
    const tradeable = enhancedTopics.filter(t => t.decisionScore?.actionTier === 'trade').length
    return { criticalAlerts, policyLoops, implementing, highVelocity, unreadBreaking, highConviction, tradeable }
  }, [alerts, enhancedTopics, breakingNews])
  
  // ============== 筛选后的数�?==============
  
  // 按时间窗口筛选的Topics
  const filteredTopics = useMemo(() => {
    let filtered = enhancedTopics
    
    // 按来源层级筛�?
    if (filterLevel !== 'all') {
      filtered = filtered.filter(topic => {
        // 检查topic是否有该层级的来�?
        switch (filterLevel) {
          case 'L0': return topic.l0Count > 0
          case 'L0.5': return topic.l05Count > 0
          case 'L1': return topic.l1Count > 0
          case 'L2': return topic.l2Count > 0
          default: return true
        }
      })
    }
    
    // 按时间窗口筛选（基于lastUpdated�?
    filtered = filtered.filter(topic => {
      return isWithinTimeWindow(topic.lastUpdated, timeWindow)
    })
    
    // 按搜索关键词筛�?
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase()
      filtered = filtered.filter(topic => 
        topic.name.toLowerCase().includes(query) ||
        topic.domain.toLowerCase().includes(query) ||
        topic.entities.some(e => e.name.toLowerCase().includes(query)) ||
        topic.tradeableAssets.some(a => a.ticker.toLowerCase().includes(query) || a.name.toLowerCase().includes(query))
      )
    }
    
    // 按时间窗口对应的分数排序（支持扩展的时间窗口）
    const scoreKey = timeWindow === '1h' || timeWindow === '6h' ? 'score6h' : 
                     timeWindow === '24h' ? 'score24h' : 'score7d'
    return filtered.sort((a, b) => (b[scoreKey] as number) - (a[scoreKey] as number))
  }, [enhancedTopics, filterLevel, timeWindow, searchQuery])
  
  // 按时间窗口和来源筛选的Alerts
  const filteredAlerts = useMemo(() => {
    let filtered = alerts
    
    // 按来源层级筛�?
    if (filterLevel !== 'all') {
      filtered = filtered.filter(alert => {
        return alert.evidences.some(e => e.level === filterLevel)
      })
    }
    
    // 按时间窗口筛�?
    filtered = filtered.filter(alert => {
      return isWithinTimeWindow(alert.createdAt, timeWindow)
    })
    
    // 按搜索关键词筛�?
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase()
      filtered = filtered.filter(alert => 
        alert.title.toLowerCase().includes(query) ||
        alert.summary.toLowerCase().includes(query)
      )
    }
    
    return filtered.sort((a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime())
  }, [alerts, filterLevel, timeWindow, searchQuery])
  
  // 按时间窗口筛选的Breaking News
  const filteredBreakingNews = useMemo(() => {
    let filtered = breakingNews
    
    // 按来源层级筛选 (安全访问)
    if (filterLevel !== 'all') {
      filtered = filtered.filter(news => news.source?.level === filterLevel)
    }
    
    // 按时间窗口筛选
    filtered = filtered.filter(news => {
      return isWithinTimeWindow(news.publishedAt, timeWindow)
    })
    
    // 按搜索关键词筛选 (安全访问)
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase()
      filtered = filtered.filter(news => 
        news.headline.toLowerCase().includes(query) ||
        (news.source?.name || '').toLowerCase().includes(query)
      )
    }
    
    return filtered.sort((a, b) => new Date(b.publishedAt).getTime() - new Date(a.publishedAt).getTime())
  }, [breakingNews, filterLevel, timeWindow, searchQuery])
  
  // 按时间窗口筛选的Documents
  const filteredDocuments = useMemo(() => {
    let filtered = documents
    
    // 按来源层级筛选 (安全访问)
    if (filterLevel !== 'all') {
      filtered = filtered.filter(doc => doc.source?.level === filterLevel)
    }
    
    // 按时间窗口筛选
    filtered = filtered.filter(doc => {
      return isWithinTimeWindow(doc.publishedAt, timeWindow)
    })
    
    // 按搜索关键词筛选 (安全访问)
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase()
      filtered = filtered.filter(doc => 
        doc.title.toLowerCase().includes(query) ||
        doc.summary.toLowerCase().includes(query) ||
        (doc.source?.name || '').toLowerCase().includes(query)
      )
    }
    
    return filtered.sort((a, b) => new Date(b.publishedAt).getTime() - new Date(a.publishedAt).getTime())
  }, [documents, filterLevel, timeWindow, searchQuery])
  
  // 获取状态颜�?- 7阶段状态机 (含EU Negotiating)
  const getStateColor = (state: PolicyState) => {
    switch (state) {
      case 'emerging': return 'text-cyan-400 bg-cyan-500/10 border-cyan-500/40'
      case 'negotiating': return 'text-amber-400 bg-amber-500/10 border-amber-500/40'  // EU-specific
      case 'contested': return 'text-yellow-400 bg-yellow-500/10 border-yellow-500/40'
      case 'implementing': return 'text-red-400 bg-red-500/10 border-red-500/40'
      case 'digesting': return 'text-blue-400 bg-blue-500/10 border-blue-500/40'
      case 'exhausted': return 'text-gray-400 bg-gray-500/10 border-gray-500/40'
      case 'reversed': return 'text-purple-400 bg-purple-500/10 border-purple-500/40'
    }
  }
  
  const getStateLabel = (state: PolicyState) => {
    return POLICY_STATE_DEFINITIONS[state]?.label || state
  }
  
  // 获取行动层级样式 - 更专业的配色
  const getActionTierStyle = (tier: ActionTier) => {
    switch (tier) {
      case 'high-conviction': return 'text-emerald-400 bg-emerald-500/10 border-emerald-500/50'
      case 'trade': return 'text-blue-400 bg-blue-500/10 border-blue-500/50'
      case 'watch': return 'text-amber-400 bg-amber-500/10 border-amber-500/50'
      case 'no-trade': return 'text-neutral-400 bg-neutral-500/10 border-neutral-500/50'
    }
  }
  
  const getLevelColor = (level: SourceLevel) => {
    switch (level) {
      case 'L0': return 'text-red-400 bg-red-500/10 border-red-500/50'
      case 'L0.5': return 'text-orange-400 bg-orange-500/10 border-orange-500/50'
      case 'L1': return 'text-blue-400 bg-blue-500/10 border-blue-500/50'
      case 'L2': return 'text-gray-400 bg-gray-500/10 border-gray-500/50'
    }
  }
  
  const getSeverityColor = (severity: Alert['severity']) => {
    switch (severity) {
      case 'critical': return 'bg-red-500/10 border-red-500/50 text-red-400'
      case 'high': return 'bg-orange-500/10 border-orange-500/50 text-orange-400'
      case 'medium': return 'bg-yellow-500/10 border-yellow-500/50 text-yellow-400'
      case 'low': return 'bg-gray-500/10 border-gray-500/50 text-gray-400'
    }
  }
  
  // 更专业的紧急度标签 - 移除表情�?
  const getUrgencyStyle = (urgency: BreakingNews['urgency']) => {
    switch (urgency) {
      case 'flash': return { bg: 'bg-red-500/20', border: 'border-red-500/60', text: 'text-red-400', label: 'FLASH', desc: 'Flash Alert' }
      case 'urgent': return { bg: 'bg-orange-500/10', border: 'border-orange-500/50', text: 'text-orange-400', label: 'URGENT', desc: 'Urgent' }
      case 'breaking': return { bg: 'bg-yellow-500/10', border: 'border-yellow-500/50', text: 'text-yellow-400', label: 'BREAKING', desc: '突发' }
      case 'developing': return { bg: 'bg-blue-500/10', border: 'border-blue-500/50', text: 'text-blue-400', label: 'DEVELOPING', desc: 'Developing' }
    }
  }
  
  const getImpactIcon = (direction: ImpactDirection) => {
    switch (direction) {
      case 'bullish': return <TrendingUp className="w-3 h-3 text-emerald-400" />
      case 'bearish': return <TrendingDown className="w-3 h-3 text-red-400" />
      case 'ambiguous': return <Activity className="w-3 h-3 text-amber-400" />
    }
  }
  
  const getDomainIcon = (domain: Domain) => {
    switch (domain) {
      case 'trade': return <Globe className="w-3.5 h-3.5" />
      case 'sanction': return <Shield className="w-3.5 h-3.5" />
      case 'war': return <Swords className="w-3.5 h-3.5" />
      case 'rate': return <DollarSign className="w-3.5 h-3.5" />
      case 'fiscal': return <Landmark className="w-3.5 h-3.5" />
      case 'regulation': return <Scale className="w-3.5 h-3.5" />
    }
  }
  
  // 获取来源区域标签
  const getRegionLabel = (region?: SourceRegion) => {
    switch (region) {
      case 'US': return { label: 'US', color: 'text-blue-400 bg-blue-500/10' }
      case 'EU': return { label: 'EU', color: 'text-amber-400 bg-amber-500/10' }
      case 'CN': return { label: 'CN', color: 'text-red-400 bg-red-500/10' }
      case 'INTL': return { label: 'INTL', color: 'text-purple-400 bg-purple-500/10' }
      default: return { label: '', color: '' }
    }
  }
  
  const formatTimeAgo = (dateStr: string) => {
    const diff = Date.now() - new Date(dateStr).getTime()
    const mins = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)
    const suffix = t('time.ago')
    if (mins < 60) return `${mins}m ${suffix}`
    if (hours < 24) return `${hours}h ${suffix}`
    return `${Math.floor(hours / 24)}d ${suffix}`
  }

  // Professional white theme helper classes
  const cardClass = "bg-white rounded-xl shadow-sm border border-gray-100 hover:shadow-md transition-shadow"
  const headerClass = "text-sm font-semibold text-gray-900 tracking-wide"
  const subTextClass = "text-xs text-gray-500"
  const badgeClass = "text-[10px] px-2 py-0.5 rounded-full font-medium"
  
  // 🆕 提升到组件级别，避免每次 render 创建新对象
  const urgencyColors = useMemo(() => ({
    flash: 'bg-red-50 border-red-200 border-l-4 border-l-red-500',
    urgent: 'bg-orange-50 border-orange-200 border-l-4 border-l-orange-500',
    breaking: 'bg-amber-50 border-amber-200 border-l-4 border-l-amber-500',
    developing: 'bg-blue-50 border-blue-200',
    update: 'bg-gray-50 border-gray-200'
  }), [])
  
  return (
    <div className="h-screen w-screen overflow-hidden bg-gray-50 text-gray-900 flex flex-col font-sans">
      {/* Global Navbar */}
      <GlobalNavbar 
        accountBalance={125000}
        dailyPnL={2350}
        weeklyPnL={8920}
        showMetrics={false}
        compact={true}
      />
      
      {/* 🆕 Data Source Status Bar */}
      <div className="h-10 bg-white border-b border-gray-200 px-4 flex items-center justify-between text-xs">
        <div className="flex items-center gap-4">
          {/* Data Mode Toggle */}
          <button
            onClick={() => setUseRealData(!useRealData)}
            className={`flex items-center gap-2 px-3 py-1.5 rounded-lg font-medium transition-all ${
              useRealData 
                ? 'bg-emerald-100 text-emerald-700 border border-emerald-200' 
                : 'bg-gray-100 text-gray-600 border border-gray-200'
            }`}
          >
            <Database className="w-3.5 h-3.5" />
            {useRealData ? '实时数据' : '模拟数据'}
            {isLoadingRealData && <RefreshCw className="w-3 h-3 animate-spin" />}
          </button>
          
          {/* API Status Indicators */}
          <div className="flex items-center gap-3 text-gray-500">
            <div className="flex items-center gap-1.5">
              <div className={`w-2 h-2 rounded-full ${useRealData && !realDataError ? 'bg-emerald-500' : 'bg-gray-300'}`} />
              <span>Federal Register</span>
            </div>
            <div className="flex items-center gap-1.5">
              <div className={`w-2 h-2 rounded-full ${useRealData && !realDataError ? 'bg-emerald-500' : 'bg-gray-300'}`} />
              <span>OFAC SDN</span>
            </div>
            <div className="flex items-center gap-1.5">
              <div className={`w-2 h-2 rounded-full ${useRealData && !realDataError ? 'bg-amber-500' : 'bg-gray-300'}`} />
              <span>NewsAPI</span>
            </div>
          </div>
          
          {/* Error Display */}
          {realDataError && (
            <div className="flex items-center gap-1.5 text-red-600 bg-red-50 px-2 py-1 rounded">
              <AlertCircle className="w-3.5 h-3.5" />
              <span>{realDataError}</span>
            </div>
          )}
        </div>
        
        <div className="flex items-center gap-4">
          {/* Notification Permission */}
          <button
            onClick={requestNotifications}
            className={`flex items-center gap-1.5 px-2 py-1 rounded transition-colors ${
              notificationPermission === 'granted' 
                ? 'text-emerald-600 bg-emerald-50' 
                : 'text-gray-500 hover:bg-gray-100'
            }`}
          >
            {notificationPermission === 'granted' ? <Bell className="w-3.5 h-3.5" /> : <BellOff className="w-3.5 h-3.5" />}
            <span>{notificationPermission === 'granted' ? '通知已启用' : '启用通知'}</span>
          </button>
          
          {/* Alert Rules Count */}
          <div className="flex items-center gap-1.5 text-gray-500">
            <Settings className="w-3.5 h-3.5" />
            <span>{alertRules.filter(r => r.enabled).length} 条规则激活</span>
          </div>
          
          {/* Last Update - 使用稳定的时钟显示 */}
          <div className="text-gray-400 font-mono text-xs">
            最后更新: {displayTime}
          </div>
          
          {/* Refresh Button */}
          <button
            onClick={() => loadRealData()}
            disabled={isLoadingRealData}
            className="flex items-center gap-1.5 px-2 py-1 rounded bg-blue-600 text-white hover:bg-blue-700 transition-colors disabled:opacity-50"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isLoadingRealData ? 'animate-spin' : ''}`} />
            <span>刷新</span>
          </button>
        </div>
      </div>
      
      {/* Main Content */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left Sidebar - Alert Center */}
        <div className="w-72 border-r border-gray-200 flex flex-col bg-white">
          {/* Header */}
          <div className="p-4 border-b border-gray-100">
            <div className="flex items-center justify-between mb-4">
              <div className="flex items-center gap-2">
                <div className="w-8 h-8 rounded-lg bg-red-50 flex items-center justify-center">
                  <AlertTriangle className="w-4 h-4 text-red-500" />
                </div>
                <span className={headerClass}>{t('news.alertCenter')}</span>
              </div>
              <button 
                onClick={handleRefresh}
                className={`p-2 rounded-lg hover:bg-gray-100 transition-colors ${isRefreshing ? 'animate-spin' : ''}`}
              >
                <RefreshCw className="w-4 h-4 text-gray-400" />
              </button>
            </div>
            
            {/* Key Stats */}
            <div className="grid grid-cols-2 gap-2">
              <div className="bg-emerald-50 rounded-lg p-3 border border-emerald-100">
                <div className="text-[10px] text-emerald-600 font-medium uppercase tracking-wide">{t('news.highConviction')}</div>
                <div className="text-2xl font-bold text-emerald-600 mt-1">{filteredTopics.filter(t => t.decisionScore?.actionTier === 'high-conviction').length}</div>
              </div>
              <div className="bg-orange-50 rounded-lg p-3 border border-orange-100">
                <div className="text-[10px] text-orange-600 font-medium uppercase tracking-wide">{t('news.confirmed')}</div>
                <div className="text-2xl font-bold text-orange-600 mt-1">{filteredTopics.filter(t => t.inPolicyLoop).length}</div>
              </div>
              <div className="bg-violet-50 rounded-lg p-3 border border-violet-100">
                <div className="text-[10px] text-violet-600 font-medium uppercase tracking-wide">{t('news.executing')}</div>
                <div className="text-2xl font-bold text-violet-600 mt-1">{filteredTopics.filter(t => t.state === 'implementing').length}</div>
              </div>
              <div className="bg-blue-50 rounded-lg p-3 border border-blue-100">
                <div className="text-[10px] text-blue-600 font-medium uppercase tracking-wide">{t('news.tradeable')}</div>
                <div className="text-2xl font-bold text-blue-600 mt-1">{filteredTopics.filter(t => t.decisionScore?.actionTier === 'trade').length}</div>
              </div>
            </div>
            
            {/* Filter Status */}
            <div className="mt-3 flex items-center justify-between text-[10px]">
              <span className="text-gray-400 font-medium">
                {t(`time.${timeWindow}`)} · {filterLevel === 'all' ? t('common.all') : filterLevel}
              </span>
              <span className="text-blue-600 font-medium">
                {filteredTopics.length}/{enhancedTopics.length} {t('news.topics').toLowerCase()}
              </span>
            </div>
          </div>
          
          {/* Live Alerts */}
          <div className="flex-1 overflow-auto">
            <div className="p-3 border-b border-gray-100 flex items-center justify-between sticky top-0 bg-white/95 backdrop-blur-sm z-10">
              <div className="flex items-center gap-2">
                <span className="text-xs font-semibold text-gray-800">{t('news.liveAlerts')}</span>
                {liveUpdateCount > 0 && (
                  <span className="w-2 h-2 bg-green-500 rounded-full" />
                )}
              </div>
              <span className={`${badgeClass} bg-red-100 text-red-600`}>
                {filteredAlerts.filter(a => !a.read).length} {t('news.unread')}
              </span>
            </div>
            
            <div className="p-2 space-y-2">
              {filteredAlerts.length === 0 ? (
                <div className="text-center py-8 text-gray-400 text-sm">{t('news.noAlerts')}</div>
              ) : (
                filteredAlerts.slice(0, 8).map(alert => (
                  <div 
                    key={alert.id}
                    className={`p-3 rounded-xl border cursor-pointer transition-all hover:shadow-md ${
                      alert.severity === 'critical' ? 'bg-red-50 border-red-200 border-l-4 border-l-red-500' :
                      alert.severity === 'high' ? 'bg-orange-50 border-orange-200 border-l-4 border-l-orange-500' :
                      alert.severity === 'medium' ? 'bg-amber-50 border-amber-200' :
                      'bg-gray-50 border-gray-200'
                    } ${alert.read ? 'opacity-60' : ''}`}
                    onClick={() => setSelectedTopic(alert.topic)}
                  >
                    <div className="flex items-start justify-between mb-2">
                      <span className="text-sm font-medium text-gray-900 line-clamp-1 flex-1">{alert.title}</span>
                      <span className="text-[10px] text-gray-400 ml-2 whitespace-nowrap">{formatTimeAgo(alert.createdAt)}</span>
                    </div>
                    <p className="text-xs text-gray-500 line-clamp-2 mb-2">{alert.summary}</p>
                    {alert.evidences.length > 0 && (
                      <div className="flex items-center gap-2">
                        <span className={`text-[10px] px-2 py-0.5 rounded-full font-medium ${
                          alert.evidences[0].level === 'L0' ? 'bg-blue-100 text-blue-700' :
                          alert.evidences[0].level === 'L0.5' ? 'bg-cyan-100 text-cyan-700' :
                          alert.evidences[0].level === 'L1' ? 'bg-gray-100 text-gray-700' :
                          'bg-gray-50 text-gray-500'
                        }`}>
                          {alert.evidences[0].level}
                        </span>
                        <span className="text-[10px] text-gray-500">{alert.evidences[0].sourceName}</span>
                      </div>
                    )}
                    {alert.severity === 'critical' && (
                      <div className="mt-2 text-[10px] text-red-600 bg-red-100 px-2 py-1 rounded-lg font-medium">
                        {t('news.immediateAction')}
                      </div>
                    )}
                  </div>
                ))
              )}
            </div>
          </div>
          
          {/* Source Coverage */}
          <div className="border-t border-gray-100 p-4">
            <div className="text-[10px] text-gray-500 font-semibold uppercase tracking-wider mb-3">{t('news.sourceCoverage')}</div>
            <div className="grid grid-cols-4 gap-2 text-center">
              <div className="p-2 bg-blue-50 rounded-lg border border-blue-100">
                <div className="text-[9px] text-blue-500 font-medium">US</div>
                <div className="text-sm font-bold text-blue-700">12</div>
              </div>
              <div className="p-2 bg-amber-50 rounded-lg border border-amber-100">
                <div className="text-[9px] text-amber-500 font-medium">EU</div>
                <div className="text-sm font-bold text-amber-700">16</div>
              </div>
              <div className="p-2 bg-red-50 rounded-lg border border-red-100">
                <div className="text-[9px] text-red-500 font-medium">CN</div>
                <div className="text-sm font-bold text-red-700">8</div>
              </div>
              <div className="p-2 bg-violet-50 rounded-lg border border-violet-100">
                <div className="text-[9px] text-violet-500 font-medium">INTL</div>
                <div className="text-sm font-bold text-violet-700">32</div>
              </div>
            </div>
            <div className="mt-2 text-[10px] text-gray-400 text-center">
              {Object.keys(SOURCES).length} {t('news.officialSources')}
            </div>
          </div>
        </div>
        
        {/* Main Content Area */}
        <div className="flex-1 flex flex-col bg-gray-50 overflow-hidden">
          {/* Tab Navigation */}
          <div className="min-h-14 bg-white border-b border-gray-200 flex items-center px-4 gap-1 flex-wrap py-2">
            {[
              { key: 'overview', labelKey: 'news.overview', icon: Activity },
              { key: 'breaking', labelKey: 'news.breaking', icon: Zap, badge: stats.unreadBreaking },
              { key: 'topics', labelKey: 'news.topics', icon: Target },
              { key: 'timeline', labelKey: 'news.timeline', icon: GitBranch },
              { key: 'listdiff', labelKey: 'news.listChanges', icon: List },
              { key: 'divergence', labelKey: 'news.divergence', icon: GitCompare },
              { key: 'execution', labelKey: 'news.executionPower', icon: Shield },
              { key: 'documents', labelKey: 'news.documents', icon: FileText },
              { key: 'alerts', labelKey: 'news.alerts', icon: AlertTriangle },
              { key: 'entities', labelKey: 'news.entities', icon: Building2 }
            ].map(tab => (
              <button
                key={tab.key}
                onClick={() => setActiveTab(tab.key as typeof activeTab)}
                className={`px-3 py-1.5 rounded-lg text-xs font-medium flex items-center gap-1.5 transition-all whitespace-nowrap flex-shrink-0 ${
                  activeTab === tab.key 
                    ? 'bg-blue-50 text-blue-700 border border-blue-200' 
                    : 'text-gray-500 hover:bg-gray-100 hover:text-gray-700'
                }`}
              >
                <tab.icon className="w-3.5 h-3.5 flex-shrink-0" />
                <span className="truncate max-w-[80px]">{t(tab.labelKey)}</span>
                {'badge' in tab && tab.badge && tab.badge > 0 && (
                  <span className="ml-0.5 px-1.5 py-0.5 text-[9px] bg-red-500 text-white rounded-full flex-shrink-0">{tab.badge}</span>
                )}
              </button>
            ))}
            
            {/* Filters */}
            <div className="ml-auto flex items-center gap-2 flex-wrap justify-end">
              {/* Language Selector */}
              <LanguageSelector variant="compact" />
              
              {/* 🆕 Global Time Window Selector */}
              <TimeWindowSelector size="sm" showLabel={false} />
              
              {/* Source Level */}
              <select 
                value={filterLevel}
                onChange={e => setFilterLevel(e.target.value as typeof filterLevel)}
                className="bg-gray-100 border-0 rounded-lg px-2 py-1.5 text-[11px] text-gray-600 font-medium cursor-pointer hover:bg-gray-200 transition-colors focus:ring-2 focus:ring-blue-500 focus:outline-none min-w-[80px]"
              >
                <option value="all">{t('level.all')}</option>
                <option value="L0">{t('level.l0')}</option>
                <option value="L0.5">{t('level.l0.5')}</option>
                <option value="L1">{t('level.l1')}</option>
                <option value="L2">{t('level.l2')}</option>
              </select>
              
              {/* Search */}
              <div className="relative">
                <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-gray-400" />
                <input 
                  ref={searchInputRef}
                  type="text"
                  placeholder={t('common.search') + '...'}
                  value={searchQuery}
                  onChange={e => setSearchQuery(e.target.value)}
                  className="bg-gray-100 border-0 rounded-lg pl-8 pr-3 py-1.5 text-xs w-32 focus:outline-none focus:ring-2 focus:ring-blue-500 text-gray-700 placeholder:text-gray-400"
                />
              </div>
              
              {/* Results Count */}
              <div className="text-[9px] text-gray-400 font-medium px-2 py-1 bg-gray-100 rounded-lg whitespace-nowrap flex-shrink-0">
                {filteredTopics.length}T · {filteredAlerts.length}A · {filteredBreakingNews.length}N
              </div>
              
              {/* Live Indicator */}
              <div className="flex items-center gap-1 px-2 py-1 bg-green-50 rounded-lg border border-green-100 flex-shrink-0">
                <span className="w-1.5 h-1.5 bg-green-500 rounded-full" />
                <span className="text-[9px] font-medium text-green-700">{t('common.live')}</span>
              </div>
            </div>
          </div>
          
          {/* Content Area */}
          <div className="flex-1 overflow-auto p-6">
            {/* Overview Tab */}
            {activeTab === 'overview' && (
              <div className="space-y-6">
                
                {/* 🆕 Data Source Health Panel - 静态显示，不频繁刷新 */}
                {showSystemHealth && dataSources.length > 0 && (
                  <div className={`${cardClass} border border-gray-200`}>
                    <div className="p-3 border-b border-gray-100 flex items-center justify-between bg-gray-50">
                      <div className="flex items-center gap-2">
                        <Activity className="w-4 h-4 text-blue-500" />
                        <span className="text-xs font-semibold text-gray-700">{t('news.systemHealth') || '系统健康'}</span>
                        {/* 连接状态指示器 - 静态显示，不闪动 */}
                        <span className="flex items-center gap-1 px-2 py-0.5 bg-emerald-100 rounded-full text-[10px] text-emerald-600">
                          <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                          已连接
                        </span>
                      </div>
                      <div className="flex items-center gap-3">
                        <span className="text-[10px] text-gray-500 font-mono">
                          {t('time.lastUpdated')}: {displayTime}
                        </span>
                        <button
                          onClick={() => setShowSystemHealth(false)}
                          className="text-gray-400 hover:text-gray-600"
                        >
                          <ChevronDown className="w-4 h-4" />
                        </button>
                      </div>
                    </div>
                    <div className="p-3">
                      <div className="grid grid-cols-6 gap-3">
                        {dataSources.slice(0, 6).map(source => (
                          <div 
                            key={source.name} 
                            className={`p-2 rounded-lg border ${
                              source.status === 'healthy' ? 'bg-emerald-50 border-emerald-200' :
                              source.status === 'degraded' ? 'bg-amber-50 border-amber-200' :
                              source.status === 'down' ? 'bg-red-50 border-red-200' :
                              'bg-gray-50 border-gray-200'
                            }`}
                          >
                            <div className="flex items-center justify-between mb-1">
                              <span className="text-[10px] font-medium text-gray-700 truncate">{source.name}</span>
                              <span className={`w-2 h-2 rounded-full ${
                                source.status === 'healthy' ? 'bg-emerald-500' :
                                source.status === 'degraded' ? 'bg-amber-500' :
                                source.status === 'down' ? 'bg-red-500' :
                                'bg-gray-400'
                              }`} />
                            </div>
                            <div className="flex items-center justify-between text-[9px] text-gray-500">
                              <span>{Math.round((source.successRate || 0) * 100)}%</span>
                              <span>{Math.round(source.latencyMs || 0)}ms</span>
                            </div>
                          </div>
                        ))}
                      </div>
                      {systemMetrics && (
                        <div className="mt-3 pt-3 border-t border-gray-100 grid grid-cols-4 gap-4 text-center">
                          <div>
                            <div className="text-lg font-bold text-blue-600">{systemMetrics.documentsProcessed}</div>
                            <div className="text-[9px] text-gray-500">Docs Processed</div>
                          </div>
                          <div>
                            <div className="text-lg font-bold text-amber-600">{systemMetrics.alertsGenerated}</div>
                            <div className="text-[9px] text-gray-500">Alerts</div>
                          </div>
                          <div>
                            <div className="text-lg font-bold text-violet-600">{systemMetrics.topicsTracked}</div>
                            <div className="text-[9px] text-gray-500">Topics</div>
                          </div>
                          <div>
                            <div className="text-lg font-bold text-emerald-600">{isNaN(systemMetrics.avgLatencyMs) ? 0 : Math.round(systemMetrics.avgLatencyMs)}ms</div>
                            <div className="text-[9px] text-gray-500">Avg Latency</div>
                          </div>
                        </div>
                      )}
                    </div>
                  </div>
                )}
                
                {/* ========== STEP 3: IMMEDIATE ACTION BANNER ========== */}
                {immediateActions.filter(a => a.priority === 'critical' || a.priority === 'high').length > 0 && (
                  <div className={`${cardClass} border-2 border-red-300 bg-gradient-to-r from-red-50 to-orange-50`}>
                    <div className="p-4 border-b border-red-200 flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-red-500 to-orange-500 flex items-center justify-center">
                          <AlertTriangle className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h2 className="text-base font-bold text-red-800">{t('immediateAction.title')}</h2>
                          <p className="text-xs text-red-600">{t('news.immediateAction')}</p>
                        </div>
                      </div>
                      <span className="px-3 py-1.5 bg-red-500 text-white rounded-full text-xs font-bold">
                        {immediateActions.filter(a => a.priority === 'critical').length} {t('immediateAction.critical')}
                      </span>
                    </div>
                    <div className="p-4 space-y-3 max-h-80 overflow-auto">
                      {immediateActions
                        .filter(a => a.priority === 'critical' || a.priority === 'high')
                        .map(action => (
                          <div 
                            key={action.id}
                            className={`p-4 rounded-xl border-l-4 bg-white shadow-sm ${
                              action.priority === 'critical' ? 'border-l-red-500' : 'border-l-orange-500'
                            }`}
                          >
                            {/* Line 1: Conclusion/Title */}
                            <div className="flex items-start justify-between mb-2">
                              <div className="flex items-center gap-2">
                                <span className={`text-[10px] px-2 py-0.5 rounded font-bold ${
                                  action.priority === 'critical' ? 'bg-red-500 text-white' : 'bg-orange-500 text-white'
                                }`}>
                                  {action.priority.toUpperCase()}
                                </span>
                                <span className={`text-[10px] px-2 py-0.5 rounded font-medium ${
                                  action.type === 'list_change' ? 'bg-red-100 text-red-700' :
                                  action.type === 'effective_date' ? 'bg-emerald-100 text-emerald-700' :
                                  'bg-purple-100 text-purple-700'
                                }`}>
                                  {action.type.replace('_', ' ').toUpperCase()}
                                </span>
                              </div>
                              {action.deadline && (
                                <div className="flex items-center gap-1 text-[10px] text-red-600 font-medium">
                                  <Clock className="w-3 h-3" />
                                  {Math.ceil((new Date(action.deadline).getTime() - Date.now()) / (24 * 60 * 60 * 1000))} {t('common.days')}
                                </div>
                              )}
                            </div>
                            <h3 className="text-sm font-bold text-gray-900 mb-1">{action.title}</h3>
                            
                            {/* Line 2: Action Label + Risk */}
                            <div className="flex items-center gap-2 mb-2">
                              <span className={`text-xs px-2 py-1 rounded-lg font-bold ${
                                action.suggestedDirection === 'bearish' 
                                  ? 'bg-red-100 text-red-700 border border-red-200' 
                                  : 'bg-emerald-100 text-emerald-700 border border-emerald-200'
                              }`}>
                                {action.suggestedDirection === 'bearish' ? '↓ BEARISH' : '↑ BULLISH'}
                              </span>
                              {action.affectedAssets && action.affectedAssets.slice(0, 4).map((ticker, idx) => (
                                <span key={idx} className="text-[10px] px-1.5 py-0.5 bg-gray-100 rounded text-gray-600">
                                  {ticker}
                                </span>
                              ))}
                              {action.affectedAssets && action.affectedAssets.length > 4 && (
                                <span className="text-[10px] text-gray-400">+{action.affectedAssets.length - 4}</span>
                              )}
                            </div>
                            
                            {/* Line 3: Action Required */}
                            <div className="text-xs text-gray-600 bg-gray-50 rounded-lg p-2">
                              <span className="font-medium text-gray-800">{t('immediateAction.actionRequired')}:</span> {action.actionRequired}
                            </div>
                            
                            {/* Bilingual Evidence (collapsible) */}
                            {action.originalText && (
                              <details className="mt-2">
                                <summary className="text-[10px] text-blue-600 cursor-pointer hover:underline">
                                  {t('immediateAction.originalText')} / {t('immediateAction.translatedText')}
                                </summary>
                                <div className="mt-2 p-2 bg-blue-50 rounded text-[10px] space-y-1">
                                  <div className="text-gray-700"><span className="font-medium">EN:</span> "{action.originalText}"</div>
                                  {action.translatedText && (
                                    <div className="text-gray-700"><span className="font-medium">CN:</span> "{action.translatedText}"</div>
                                  )}
                                </div>
                              </details>
                            )}
                          </div>
                        ))}
                    </div>
                  </div>
                )}
                
                {/* ========== Three-Line Topic Cards (Decision First) ========== */}
                <div>
                  <div className="flex items-center justify-between mb-4">
                    <h3 className="text-base font-semibold text-gray-900">{t('news.topPolicySignals')}</h3>
                    <span className="text-xs text-gray-400">{filteredTopics.length} {t('news.activeTopics')}</span>
                  </div>
                  
                  <div className="grid grid-cols-2 gap-4">
                    {filteredTopics.slice(0, 6).map(topic => {
                      // Calculate action label
                      const actionLabel = topic.decisionScore?.actionTier === 'high-conviction' ? 'TRADE' :
                                          topic.decisionScore?.actionTier === 'trade' ? 'TRADE' :
                                          topic.decisionScore?.actionTier === 'monitor' ? 'WATCH' : 'NO TRADE'
                      const actionColor = actionLabel === 'TRADE' ? 'bg-emerald-500 text-white' :
                                          actionLabel === 'WATCH' ? 'bg-amber-500 text-white' : 'bg-gray-400 text-white'
                      
                      // Primary direction from assets
                      const primaryDirection = topic.tradeableAssets.length > 0 
                        ? topic.tradeableAssets.filter(a => a.exposure === 'bearish').length > 
                          topic.tradeableAssets.filter(a => a.exposure === 'bullish').length 
                          ? 'bearish' : 'bullish'
                        : 'neutral'
                      
                      return (
                        <div 
                          key={topic.id}
                          className={`${cardClass} p-4 cursor-pointer border-l-4 ${
                            actionLabel === 'TRADE' ? 'border-l-emerald-500' :
                            actionLabel === 'WATCH' ? 'border-l-amber-500' : 'border-l-gray-300'
                          }`}
                          onClick={() => setSelectedTopic(topic)}
                        >
                          {/* Line 1: Conclusion/Title with Score */}
                          <div className="flex items-start justify-between mb-2">
                            <div className="flex-1">
                              <div className="flex items-center gap-2 mb-1">
                                <span className={`text-[10px] px-2 py-0.5 rounded-full font-bold ${actionColor}`}>
                                  {actionLabel}
                                </span>
                                <span className={`text-[10px] px-2 py-0.5 rounded font-medium ${
                                  topic.state === 'implementing' ? 'bg-violet-100 text-violet-700' :
                                  topic.state === 'effective' ? 'bg-emerald-100 text-emerald-700' :
                                  topic.state === 'announced' ? 'bg-blue-100 text-blue-700' :
                                  'bg-gray-100 text-gray-600'
                                }`}>
                                  {t(`state.${topic.state}`)}
                                </span>
                                {topic.inPolicyLoop && (
                                  <span className="text-[10px] px-1.5 py-0.5 bg-green-100 text-green-700 rounded">
                                    ✓{t('news.policyLoop')}
                                  </span>
                                )}
                              </div>
                              <h4 className="text-sm font-semibold text-gray-900 line-clamp-1">{topic.name}</h4>
                            </div>
                            {topic.decisionScore && (
                              <div className={`w-10 h-10 rounded-lg flex items-center justify-center text-sm font-bold ${
                                (topic.decisionScore.score ?? 0) >= 80 ? 'bg-emerald-100 text-emerald-700' :
                                (topic.decisionScore.score ?? 0) >= 60 ? 'bg-blue-100 text-blue-700' :
                                (topic.decisionScore.score ?? 0) >= 40 ? 'bg-amber-100 text-amber-700' :
                                'bg-gray-100 text-gray-600'
                              }`}>
                                <SafeNumber value={topic.decisionScore.score} fallback="--" decimals={0} />
                              </div>
                            )}
                          </div>
                          
                          {/* Line 2: Direction + Top Assets */}
                          <div className="flex items-center gap-2 mb-2">
                            <span className={`text-[10px] px-2 py-0.5 rounded font-bold ${
                              primaryDirection === 'bearish' ? 'bg-red-100 text-red-700' :
                              primaryDirection === 'bullish' ? 'bg-emerald-100 text-emerald-700' :
                              'bg-gray-100 text-gray-600'
                            }`}>
                              {primaryDirection === 'bearish' ? '↓' : primaryDirection === 'bullish' ? '↑' : '~'} {primaryDirection.toUpperCase()}
                            </span>
                            {topic.tradeableAssets.slice(0, 3).map((asset, idx) => (
                              <span key={idx} className="text-[10px] px-1.5 py-0.5 bg-gray-100 rounded text-gray-600">
                                {asset.ticker}
                              </span>
                            ))}
                            {topic.tradeableAssets.length > 3 && (
                              <span className="text-[10px] text-gray-400">+{topic.tradeableAssets.length - 3}</span>
                            )}
                          </div>
                          
                          {/* Line 3: Risk Warning / Validation */}
                          <div className="flex items-center justify-between text-[10px]">
                            <div className="flex items-center gap-2">
                              {topic.driftMetrics && topic.driftMetrics.riskLevel !== 'low' && (
                                <span className={`px-1.5 py-0.5 rounded ${
                                  topic.driftMetrics.riskLevel === 'high' ? 'bg-red-100 text-red-700' : 'bg-amber-100 text-amber-700'
                                }`}>
                                  ⚠Drift: {topic.driftMetrics.riskLevel}
                                </span>
                              )}
                              {topic.validation && (
                                <span className={`px-1.5 py-0.5 rounded ${
                                  topic.validation.qualityGrade === 'A' ? 'bg-emerald-100 text-emerald-700' :
                                  topic.validation.qualityGrade === 'B' ? 'bg-blue-100 text-blue-700' :
                                  'bg-gray-100 text-gray-600'
                                }`}>
                                  {t('common.grade')} {topic.validation.qualityGrade} · {(topic.validation.directionHitRatio * 100).toFixed(0)}%
                                </span>
                              )}
                            </div>
                            <span className="text-gray-400">{formatTimeAgo(topic.lastUpdated)}</span>
                          </div>
                        </div>
                      )
                    })}
                  </div>
                </div>
                
                {/* ========== NEW FEATURE 1: Quick Trade Signals Panel ========== */}
                <div className={cardClass}>
                  <div className="p-4 border-b border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-emerald-500 to-teal-500 flex items-center justify-center">
                          <Target className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h3 className="text-sm font-semibold text-gray-900">{t('news.quickTradeSignals') || 'Quick Trade Signals'}</h3>
                          <p className="text-[10px] text-gray-500">{t('news.realTimeRecommendations') || 'Real-time trading recommendations based on policy signals'}</p>
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        <span className="text-[10px] px-2 py-1 bg-emerald-100 text-emerald-700 rounded-full font-medium">
                          {enhancedTopics.filter(t => t.decisionScore?.actionTier === 'high-conviction' || t.decisionScore?.actionTier === 'trade').length} Active
                        </span>
                      </div>
                    </div>
                  </div>
                  <div className="p-4">
                    <div className="grid grid-cols-3 gap-4">
                      {/* Long Signals */}
                      <div className="space-y-2">
                        <div className="flex items-center gap-2 text-xs font-semibold text-emerald-700">
                          <TrendingUp className="w-4 h-4" />
                          <span>LONG Signals</span>
                        </div>
                        <div className="space-y-2 max-h-32 overflow-auto">
                          {enhancedTopics
                            .filter(t => t.decisionScore?.actionTier === 'high-conviction' || t.decisionScore?.actionTier === 'trade')
                            .flatMap(t => t.tradeableAssets.filter(a => a.exposure === 'bullish'))
                            .slice(0, 4)
                            .map((asset, idx) => (
                              <div key={idx} className="flex items-center justify-between p-2 bg-emerald-50 rounded-lg border border-emerald-200">
                                <span className="text-xs font-semibold text-emerald-800">{asset.ticker}</span>
                                <span className="text-[10px] text-emerald-600">
                                  +<SafeNumber value={asset.weight * 100} decimals={0} fallback="--" />%
                                </span>
                              </div>
                            ))}
                          {enhancedTopics.filter(t => t.decisionScore?.actionTier === 'high-conviction' || t.decisionScore?.actionTier === 'trade').flatMap(t => t.tradeableAssets.filter(a => a.exposure === 'bullish')).length === 0 && (
                            <div className="text-[10px] text-gray-400 p-2">No active long signals</div>
                          )}
                        </div>
                      </div>
                      
                      {/* Short Signals */}
                      <div className="space-y-2">
                        <div className="flex items-center gap-2 text-xs font-semibold text-red-700">
                          <TrendingDown className="w-4 h-4" />
                          <span>SHORT Signals</span>
                        </div>
                        <div className="space-y-2 max-h-32 overflow-auto">
                          {enhancedTopics
                            .filter(t => t.decisionScore?.actionTier === 'high-conviction' || t.decisionScore?.actionTier === 'trade')
                            .flatMap(t => t.tradeableAssets.filter(a => a.exposure === 'bearish'))
                            .slice(0, 4)
                            .map((asset, idx) => (
                              <div key={idx} className="flex items-center justify-between p-2 bg-red-50 rounded-lg border border-red-200">
                                <span className="text-xs font-semibold text-red-800">{asset.ticker}</span>
                                <span className="text-[10px] text-red-600">
                                  -<SafeNumber value={asset.weight * 100} decimals={0} fallback="--" />%
                                </span>
                              </div>
                            ))}
                          {enhancedTopics.filter(t => t.decisionScore?.actionTier === 'high-conviction' || t.decisionScore?.actionTier === 'trade').flatMap(t => t.tradeableAssets.filter(a => a.exposure === 'bearish')).length === 0 && (
                            <div className="text-[10px] text-gray-400 p-2">No active short signals</div>
                          )}
                        </div>
                      </div>
                      
                      {/* Position Summary */}
                      <div className="space-y-2">
                        <div className="flex items-center gap-2 text-xs font-semibold text-gray-700">
                          <Activity className="w-4 h-4" />
                          <span>Portfolio Action</span>
                        </div>
                        <div className="space-y-2">
                          <div className="p-3 bg-gray-50 rounded-lg border border-gray-200">
                            <div className="text-[10px] text-gray-500 mb-1">Net Exposure</div>
                            <div className={`text-lg font-bold ${
                              enhancedTopics.flatMap(t => t.tradeableAssets).filter(a => a.exposure === 'bullish').length > 
                              enhancedTopics.flatMap(t => t.tradeableAssets).filter(a => a.exposure === 'bearish').length 
                                ? 'text-emerald-600' : 'text-red-600'
                            }`}>
                              {enhancedTopics.flatMap(t => t.tradeableAssets).filter(a => a.exposure === 'bullish').length > 
                               enhancedTopics.flatMap(t => t.tradeableAssets).filter(a => a.exposure === 'bearish').length 
                                ? 'NET LONG' : 'NET SHORT'}
                            </div>
                          </div>
                          <div className="p-3 bg-violet-50 rounded-lg border border-violet-200">
                            <div className="text-[10px] text-violet-500 mb-1">Conviction Level</div>
                            <div className="text-lg font-bold text-violet-600">
                              {enhancedTopics.filter(t => t.decisionScore?.score && t.decisionScore.score >= 70).length > 0 ? 'HIGH' : 'MODERATE'}
                            </div>
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
                
                {/* ========== NEW FEATURE 2: Real Data Source Status + Signal Timeline ========== */}
                <div className={cardClass}>
                  <div className="p-4 border-b border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-blue-500 to-indigo-500 flex items-center justify-center">
                          <Clock className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h3 className="text-sm font-semibold text-gray-900">{t('news.signalTimeline') || 'Policy Signal Pipeline'}</h3>
                          <p className="text-[10px] text-gray-500">{t('news.recentMajorEvents') || 'Real-time feeds from official & verified sources'}</p>
                        </div>
                      </div>
                      {/* 数据源状态指示器 */}
                      <div className="flex items-center gap-2">
                        <div className="flex items-center gap-1 px-2 py-1 bg-emerald-50 rounded-lg border border-emerald-200">
                          <span className="w-2 h-2 rounded-full bg-emerald-500"></span>
                          <span className="text-[10px] text-emerald-700 font-medium">5 Official APIs</span>
                        </div>
                        <div className="flex items-center gap-1 px-2 py-1 bg-amber-50 rounded-lg border border-amber-200">
                          <span className="w-2 h-2 rounded-full bg-amber-500"></span>
                          <span className="text-[10px] text-amber-700 font-medium">3 Pending</span>
                        </div>
                      </div>
                    </div>
                  </div>
                  
                  {/* 数据源分层显示 */}
                  <div className="p-4 border-b border-gray-100 bg-gray-50">
                    <div className="grid grid-cols-3 gap-3">
                      {/* Official APIs */}
                      <div className="p-3 bg-white rounded-lg border border-gray-200">
                        <div className="flex items-center gap-2 mb-2">
                          <span className="w-2 h-2 rounded-full bg-blue-500"></span>
                          <span className="text-[10px] font-semibold text-gray-700">官方 API</span>
                        </div>
                        <div className="space-y-1">
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">Federal Register</span>
                            <span className="text-emerald-600">✓ Connected</span>
                          </div>
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">Regulations.gov</span>
                            <span className="text-amber-600">⏳ Pending</span>
                          </div>
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">OFAC SDN List</span>
                            <span className="text-emerald-600">✓ Connected</span>
                          </div>
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">EUR-Lex</span>
                            <span className="text-emerald-600">✓ Connected</span>
                          </div>
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">Library of Congress</span>
                            <span className="text-amber-600">⏳ Pending</span>
                          </div>
                        </div>
                      </div>
                      
                      {/* Semi-Official (RSS/Scrape) */}
                      <div className="p-3 bg-white rounded-lg border border-gray-200">
                        <div className="flex items-center gap-2 mb-2">
                          <span className="w-2 h-2 rounded-full bg-violet-500"></span>
                          <span className="text-[10px] font-semibold text-gray-700">半官方 (RSS)</span>
                        </div>
                        <div className="space-y-1">
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">Fed Speeches</span>
                            <span className="text-emerald-600">✓ RSS</span>
                          </div>
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">ECB Press</span>
                            <span className="text-emerald-600">✓ RSS</span>
                          </div>
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">Bank of England</span>
                            <span className="text-emerald-600">✓ RSS</span>
                          </div>
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">PBoC (中国央行)</span>
                            <span className="text-amber-600">⏳ Scrape</span>
                          </div>
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">MOFCOM (商务部)</span>
                            <span className="text-amber-600">⏳ Scrape</span>
                          </div>
                        </div>
                      </div>
                      
                      {/* Third-Party */}
                      <div className="p-3 bg-white rounded-lg border border-gray-200">
                        <div className="flex items-center gap-2 mb-2">
                          <span className="w-2 h-2 rounded-full bg-amber-500"></span>
                          <span className="text-[10px] font-semibold text-gray-700">第三方</span>
                        </div>
                        <div className="space-y-1">
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">NewsAPI</span>
                            <span className="text-emerald-600">✓ API Key</span>
                          </div>
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">Truth Social</span>
                            <span className="text-gray-400">⚠️ 无官方API</span>
                          </div>
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">Reuters Wire</span>
                            <span className="text-gray-400">💳 需订阅</span>
                          </div>
                          <div className="flex items-center justify-between text-[9px]">
                            <span className="text-gray-600">Bloomberg API</span>
                            <span className="text-gray-400">💳 需订阅</span>
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>
                  
                  {/* 信号时间线 */}
                  <div className="p-4">
                    <div className="flex items-center justify-between mb-3">
                      <span className="text-[10px] font-semibold text-gray-600 uppercase tracking-wide">Latest Signals</span>
                      <span className="text-[9px] text-gray-400">From connected sources only</span>
                    </div>
                    <div className="relative">
                      {/* Timeline */}
                      <div className="absolute left-4 top-0 bottom-0 w-0.5 bg-gradient-to-b from-blue-500 via-violet-500 to-gray-300"></div>
                      <div className="space-y-4 pl-10">
                        {[...breakingNews, ...documents.map(d => ({
                          id: d.id,
                          headline: d.title,
                          publishedAt: d.publishedAt,
                          source: d.source,
                          urgency: 'update' as const,
                          sentiment: d.sentiment
                        }))]
                          .sort((a, b) => new Date(b.publishedAt).getTime() - new Date(a.publishedAt).getTime())
                          .slice(0, 6)
                          .map((item, idx) => (
                            <div key={item.id} className="relative">
                              <div className={`absolute -left-[26px] w-4 h-4 rounded-full border-2 border-white shadow ${
                                idx === 0 ? 'bg-blue-500' :
                                idx === 1 ? 'bg-violet-500' :
                                'bg-gray-400'
                              }`}></div>
                              <div className="flex items-start justify-between">
                                <div className="flex-1">
                                  <div className="flex items-center gap-2 mb-1">
                                    <span className={`text-[9px] px-1.5 py-0.5 rounded font-medium ${
                                      item.source?.level === 'L0' ? 'bg-blue-100 text-blue-700' :
                                      item.source?.level === 'L0.5' ? 'bg-cyan-100 text-cyan-700' :
                                      'bg-gray-100 text-gray-600'
                                    }`}>
                                      {item.source?.level || 'L2'}
                                    </span>
                                    <span className={`text-[9px] px-1.5 py-0.5 rounded ${
                                      item.source?.id?.includes('federal') || item.source?.id?.includes('ofac') 
                                        ? 'bg-emerald-50 text-emerald-700' 
                                        : 'bg-gray-50 text-gray-500'
                                    }`}>
                                      {item.source?.id?.includes('federal') ? '✓ Official' : 'Source'}
                                    </span>
                                    <span className="text-[9px] text-gray-400">
                                      {formatTimeAgo(item.publishedAt)}
                                    </span>
                                  </div>
                                  <p className="text-xs text-gray-800 line-clamp-2">{'headline' in item ? item.headline : item.headline}</p>
                                  <span className="text-[10px] text-gray-500">{item.source?.name}</span>
                                </div>
                                <div className={`ml-2 text-[10px] px-2 py-1 rounded ${
                                  ('sentiment' in item ? item.sentiment : 'ambiguous') === 'bullish' ? 'bg-emerald-100 text-emerald-700' :
                                  ('sentiment' in item ? item.sentiment : 'ambiguous') === 'bearish' ? 'bg-red-100 text-red-700' :
                                  'bg-gray-100 text-gray-600'
                                }`}>
                                  {('sentiment' in item ? item.sentiment : 'ambiguous') === 'bullish' ? '↑' : ('sentiment' in item ? item.sentiment : 'ambiguous') === 'bearish' ? '↓' : '~'}
                                </div>
                              </div>
                            </div>
                          ))}
                        {breakingNews.length === 0 && documents.length === 0 && (
                          <div className="text-[10px] text-gray-400 py-4">Waiting for data from connected sources...</div>
                        )}
                      </div>
                    </div>
                  </div>
                </div>
                
                {/* ========== NEW FEATURE 3: Policy Correlation Heatmap ========== */}
                <div className={cardClass}>
                  <div className="p-4 border-b border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-orange-500 to-pink-500 flex items-center justify-center">
                          <Layers className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h3 className="text-sm font-semibold text-gray-900">{t('news.policyCorrelation') || 'Policy Correlation Matrix'}</h3>
                          <p className="text-[10px] text-gray-500">{t('news.crossDomainImpact') || 'Cross-domain policy impact relationships'}</p>
                        </div>
                      </div>
                    </div>
                  </div>
                  <div className="p-4">
                    <div className="grid grid-cols-5 gap-1">
                      {/* Header Row */}
                      <div className="p-2"></div>
                      {['Trade', 'Sanction', 'Tech', 'Rate'].map(domain => (
                        <div key={domain} className="p-2 text-center text-[10px] font-semibold text-gray-600">{domain}</div>
                      ))}
                      
                      {/* Data Rows */}
                      {[
                        { domain: 'Trade', values: [1.0, 0.7, 0.6, 0.4] },
                        { domain: 'Sanction', values: [0.7, 1.0, 0.8, 0.3] },
                        { domain: 'Tech', values: [0.6, 0.8, 1.0, 0.2] },
                        { domain: 'Rate', values: [0.4, 0.3, 0.2, 1.0] }
                      ].map(row => (
                        <>
                          <div key={row.domain} className="p-2 text-[10px] font-semibold text-gray-600 flex items-center">{row.domain}</div>
                          {row.values.map((val, idx) => (
                            <div 
                              key={idx}
                              className={`p-2 text-center text-[10px] font-medium rounded ${
                                val >= 0.8 ? 'bg-red-200 text-red-800' :
                                val >= 0.6 ? 'bg-orange-200 text-orange-800' :
                                val >= 0.4 ? 'bg-amber-100 text-amber-800' :
                                'bg-gray-100 text-gray-600'
                              }`}
                            >
                              {val === 1.0 ? '●' : val.toFixed(1)}
                            </div>
                          ))}
                        </>
                      ))}
                    </div>
                    <div className="mt-3 flex items-center justify-center gap-4 text-[9px] text-gray-500">
                      <span className="flex items-center gap-1"><span className="w-3 h-3 rounded bg-red-200"></span>High (≥0.8)</span>
                      <span className="flex items-center gap-1"><span className="w-3 h-3 rounded bg-orange-200"></span>Medium (0.6-0.8)</span>
                      <span className="flex items-center gap-1"><span className="w-3 h-3 rounded bg-amber-100"></span>Low (0.4-0.6)</span>
                      <span className="flex items-center gap-1"><span className="w-3 h-3 rounded bg-gray-100"></span>Minimal (&lt;0.4)</span>
                    </div>
                  </div>
                </div>
                
                {/* ========== 🆕 NEW FEATURE 4: Executive Power Graph Live Feed ========== */}
                <div className={cardClass}>
                  <div className="p-4 border-b border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-violet-600 to-purple-600 flex items-center justify-center">
                          <GitBranch className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h3 className="text-sm font-semibold text-gray-900">{t('news.executivePowerFeed') || 'Executive Power Graph Feed'}</h3>
                          <p className="text-[10px] text-gray-500">{t('news.realTimeAuthority') || 'Real-time news from authoritative sources by hierarchy'}</p>
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        {/* Level Filter */}
                        <div className="flex items-center gap-1">
                          {(['all', 'L0', 'L0.5', 'L1', 'L2'] as const).map(level => (
                            <button
                              key={level}
                              onClick={() => setExecPowerFilter(level)}
                              className={`px-2 py-1 text-[10px] rounded transition-colors ${
                                execPowerFilter === level
                                  ? level === 'L0' ? 'bg-red-500 text-white' 
                                    : level === 'L0.5' ? 'bg-orange-500 text-white'
                                    : level === 'L1' ? 'bg-blue-500 text-white'
                                    : level === 'L2' ? 'bg-gray-500 text-white'
                                    : 'bg-violet-500 text-white'
                                  : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                              }`}
                            >
                              {level === 'all' ? 'All' : level}
                            </button>
                          ))}
                        </div>
                        {/* Region Filter */}
                        <div className="flex items-center gap-1 ml-2">
                          {(['all', 'US', 'EU', 'CN', 'INTL'] as const).map(region => (
                            <button
                              key={region}
                              onClick={() => setExecPowerRegion(region)}
                              className={`px-2 py-1 text-[10px] rounded transition-colors ${
                                execPowerRegion === region
                                  ? 'bg-violet-500 text-white'
                                  : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                              }`}
                            >
                              {region === 'all' ? '🌐' : region === 'US' ? '🇺🇸' : region === 'EU' ? '🇪🇺' : region === 'CN' ? '🇨🇳' : '🌍'}
                            </button>
                          ))}
                        </div>
                        {/* Stats Badge */}
                        <span className="text-[10px] px-2 py-1 bg-violet-100 text-violet-700 rounded-full font-medium ml-2">
                          {execPowerNews.length} items
                        </span>
                        <button
                          onClick={() => executivePowerService.refreshAll()}
                          className="p-1.5 rounded-lg hover:bg-gray-100 transition-colors"
                        >
                          <RefreshCw className="w-4 h-4 text-gray-500" />
                        </button>
                      </div>
                    </div>
                  </div>
                  
                  {/* Stats Row */}
                  {execPowerStats && (
                    <div className="px-4 py-2 bg-gray-50 border-b border-gray-100 flex items-center justify-between">
                      <div className="flex items-center gap-4">
                        <div className="flex items-center gap-1">
                          <span className="w-2 h-2 rounded-full bg-red-500"></span>
                          <span className="text-[10px] text-gray-600">L0: <strong>{execPowerStats.byLevel['L0']}</strong></span>
                        </div>
                        <div className="flex items-center gap-1">
                          <span className="w-2 h-2 rounded-full bg-orange-500"></span>
                          <span className="text-[10px] text-gray-600">L0.5: <strong>{execPowerStats.byLevel['L0.5']}</strong></span>
                        </div>
                        <div className="flex items-center gap-1">
                          <span className="w-2 h-2 rounded-full bg-blue-500"></span>
                          <span className="text-[10px] text-gray-600">L1: <strong>{execPowerStats.byLevel['L1']}</strong></span>
                        </div>
                        <div className="flex items-center gap-1">
                          <span className="w-2 h-2 rounded-full bg-gray-400"></span>
                          <span className="text-[10px] text-gray-600">L2: <strong>{execPowerStats.byLevel['L2']}</strong></span>
                        </div>
                      </div>
                      <div className="flex items-center gap-3">
                        <span className="text-[10px] text-gray-500">
                          {execPowerStats.activeSources}/{execPowerStats.totalSources} sources
                        </span>
                        <span className="text-[10px] text-gray-400">
                          Updated: {new Date(execPowerStats.latestUpdate).toLocaleTimeString()}
                        </span>
                      </div>
                    </div>
                  )}
                  
                  {/* News Feed */}
                  <div className="p-4">
                    <div className="grid grid-cols-2 gap-3 max-h-[480px] overflow-auto">
                      {execPowerNews
                        .filter(news => execPowerFilter === 'all' || news.level === execPowerFilter)
                        .filter(news => execPowerRegion === 'all' || news.region === execPowerRegion)
                        .slice(0, 20)
                        .map(news => (
                          <div 
                            key={news.id}
                            className={`p-3 rounded-lg border-l-4 bg-white shadow-sm hover:shadow-md transition-shadow cursor-pointer ${
                              news.level === 'L0' ? 'border-l-red-500' :
                              news.level === 'L0.5' ? 'border-l-orange-500' :
                              news.level === 'L1' ? 'border-l-blue-500' :
                              'border-l-gray-400'
                            }`}
                          >
                            {/* Header Row */}
                            <div className="flex items-center justify-between mb-2">
                              <div className="flex items-center gap-2">
                                <span className={`text-[9px] px-1.5 py-0.5 rounded font-bold ${
                                  news.level === 'L0' ? 'bg-red-100 text-red-700' :
                                  news.level === 'L0.5' ? 'bg-orange-100 text-orange-700' :
                                  news.level === 'L1' ? 'bg-blue-100 text-blue-700' :
                                  'bg-gray-100 text-gray-600'
                                }`}>
                                  {news.level}
                                </span>
                                <span className={`text-[9px] px-1.5 py-0.5 rounded font-medium ${
                                  news.urgency === 'flash' ? 'bg-red-500 text-white animate-pulse' :
                                  news.urgency === 'urgent' ? 'bg-orange-100 text-orange-700' :
                                  news.urgency === 'breaking' ? 'bg-blue-100 text-blue-700' :
                                  'bg-gray-100 text-gray-500'
                                }`}>
                                  {news.urgency.toUpperCase()}
                                </span>
                                <span className="text-[9px] text-gray-400">
                                  {news.region === 'US' ? '🇺🇸' : news.region === 'EU' ? '🇪🇺' : news.region === 'CN' ? '🇨🇳' : '🌍'}
                                </span>
                              </div>
                              <div className="flex items-center gap-1">
                                <span className={`text-[9px] px-1.5 py-0.5 rounded ${
                                  news.sentiment === 'bullish' ? 'bg-emerald-100 text-emerald-700' :
                                  news.sentiment === 'bearish' ? 'bg-red-100 text-red-700' :
                                  'bg-gray-100 text-gray-600'
                                }`}>
                                  {news.sentiment === 'bullish' ? '↑' : news.sentiment === 'bearish' ? '↓' : '~'}
                                </span>
                                <span className={`text-[10px] font-bold ${
                                  news.impactScore >= 80 ? 'text-red-600' :
                                  news.impactScore >= 60 ? 'text-orange-600' :
                                  news.impactScore >= 40 ? 'text-blue-600' :
                                  'text-gray-500'
                                }`}>
                                  {news.impactScore}
                                </span>
                              </div>
                            </div>
                            
                            {/* Headline */}
                            <h4 className="text-xs font-semibold text-gray-900 mb-1.5 line-clamp-2">
                              {news.headline}
                            </h4>
                            
                            {/* Source & Time */}
                            <div className="flex items-center justify-between text-[10px]">
                              <div className="flex items-center gap-1.5">
                                <span className="text-gray-600 font-medium">{news.source?.name || 'Unknown'}</span>
                                {news.isVerified && (
                                  <CheckCircle className="w-3 h-3 text-emerald-500" />
                                )}
                              </div>
                              <span className="text-gray-400">
                                {formatTimeAgo(news.publishedAt)}
                              </span>
                            </div>
                            
                            {/* Tags Row */}
                            {(news.relatedTopics.length > 0 || news.affectedAssets.length > 0) && (
                              <div className="mt-2 flex items-center gap-1 flex-wrap">
                                {news.domains.slice(0, 2).map((domain, idx) => (
                                  <span key={idx} className="text-[9px] px-1.5 py-0.5 bg-violet-50 text-violet-600 rounded">
                                    {domain}
                                  </span>
                                ))}
                                {news.affectedAssets.slice(0, 3).map((asset, idx) => (
                                  <span key={idx} className="text-[9px] px-1.5 py-0.5 bg-blue-50 text-blue-600 rounded font-mono">
                                    ${asset}
                                  </span>
                                ))}
                                {news.affectedAssets.length > 3 && (
                                  <span className="text-[9px] text-gray-400">+{news.affectedAssets.length - 3}</span>
                                )}
                              </div>
                            )}
                          </div>
                        ))}
                      
                      {/* Empty State */}
                      {execPowerNews.filter(news => 
                        (execPowerFilter === 'all' || news.level === execPowerFilter) &&
                        (execPowerRegion === 'all' || news.region === execPowerRegion)
                      ).length === 0 && (
                        <div className="col-span-2 py-8 text-center">
                          <div className="w-16 h-16 mx-auto mb-4 rounded-full bg-gray-100 flex items-center justify-center">
                            <Newspaper className="w-8 h-8 text-gray-400" />
                          </div>
                          <p className="text-sm text-gray-500 mb-2">No news available</p>
                          <p className="text-[10px] text-gray-400">Loading from authoritative sources...</p>
                          <button
                            onClick={() => executivePowerService.refreshAll()}
                            className="mt-3 px-4 py-2 bg-violet-500 text-white text-xs rounded-lg hover:bg-violet-600 transition-colors"
                          >
                            Refresh Now
                          </button>
                        </div>
                      )}
                    </div>
                  </div>
                  
                  {/* 🆕 Source Status Overview */}
                  <div className="px-4 py-3 bg-gradient-to-r from-gray-50 to-violet-50 border-t border-gray-100">
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-[10px] font-semibold text-gray-700">{t('news.sourceStatus') || 'Source Status Overview'}</span>
                      <span className="text-[9px] text-gray-400">
                        {execPowerSources.filter(s => s.isActive).length}/{execPowerSources.length} active
                      </span>
                    </div>
                    <div className="grid grid-cols-4 gap-2">
                      {(['L0', 'L0.5', 'L1', 'L2'] as const).map(level => {
                        const sources = execPowerSources.filter(s => s.level === level)
                        const activeSources = sources.filter(s => s.isActive)
                        const newsCount = execPowerNews.filter(n => n.level === level).length
                        return (
                          <div 
                            key={level}
                            className={`p-2 rounded-lg border ${
                              level === 'L0' ? 'bg-red-50 border-red-200' :
                              level === 'L0.5' ? 'bg-orange-50 border-orange-200' :
                              level === 'L1' ? 'bg-blue-50 border-blue-200' :
                              'bg-gray-50 border-gray-200'
                            }`}
                          >
                            <div className="flex items-center justify-between mb-1">
                              <span className={`text-[10px] font-bold ${
                                level === 'L0' ? 'text-red-700' :
                                level === 'L0.5' ? 'text-orange-700' :
                                level === 'L1' ? 'text-blue-700' :
                                'text-gray-700'
                              }`}>
                                {level}
                              </span>
                              <span className={`text-[9px] ${
                                activeSources.length === sources.length ? 'text-emerald-600' : 'text-amber-600'
                              }`}>
                                {activeSources.length}/{sources.length}
                              </span>
                            </div>
                            <div className="flex items-center gap-1">
                              {sources.slice(0, 3).map(source => (
                                <div 
                                  key={source.id}
                                  className={`w-2 h-2 rounded-full ${
                                    source.isActive ? 'bg-emerald-500' : 'bg-gray-300'
                                  }`}
                                  title={source.name}
                                />
                              ))}
                              {sources.length > 3 && (
                                <span className="text-[8px] text-gray-400">+{sources.length - 3}</span>
                              )}
                              <span className="ml-auto text-[9px] font-medium text-gray-600">{newsCount}</span>
                            </div>
                          </div>
                        )
                      })}
                    </div>
                  </div>
                  
                  {/* Source Hierarchy Legend */}
                  <div className="px-4 py-3 bg-gray-50 border-t border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-4 text-[9px]">
                        <div className="flex items-center gap-1">
                          <span className="w-2 h-2 rounded bg-red-500"></span>
                          <span className="text-gray-600"><strong>L0</strong> Decision Makers (White House, OFAC, Federal Register)</span>
                        </div>
                        <div className="flex items-center gap-1">
                          <span className="w-2 h-2 rounded bg-orange-500"></span>
                          <span className="text-gray-600"><strong>L0.5</strong> Execution Agencies (Treasury, Fed, BIS)</span>
                        </div>
                        <div className="flex items-center gap-1">
                          <span className="w-2 h-2 rounded bg-blue-500"></span>
                          <span className="text-gray-600"><strong>L1</strong> Wire Services (Reuters, Bloomberg)</span>
                        </div>
                        <div className="flex items-center gap-1">
                          <span className="w-2 h-2 rounded bg-gray-400"></span>
                          <span className="text-gray-600"><strong>L2</strong> Secondary Sources</span>
                        </div>
                      </div>
                      <a
                        href="#"
                        className="text-[10px] text-violet-600 hover:underline"
                        onClick={(e) => {
                          e.preventDefault()
                          setActiveTab('execution')
                        }}
                      >
                        View Full Power Graph →
                      </a>
                    </div>
                  </div>
                </div>
                
                {/* ========== Signal Supply Credibility Console / 信号供给可信度控制台 ========== */}
                {/* 核心职责：不是告诉你 API 活着没，而是告诉你 能不能用/值不值得信/现在该不该动 */}
                <details className="group" open={showSystemHealth}>
                  <summary className={`${cardClass} p-4 cursor-pointer list-none flex items-center justify-between`}>
                    <div className="flex items-center gap-3">
                      <div className={`w-10 h-10 rounded-xl flex items-center justify-center ${
                        signalSupplyStatus?.overallCredibility === 'high' 
                          ? 'bg-gradient-to-br from-emerald-500 to-green-500' 
                          : signalSupplyStatus?.overallCredibility === 'medium' 
                            ? 'bg-gradient-to-br from-blue-500 to-cyan-500'
                            : signalSupplyStatus?.overallCredibility === 'degraded'
                              ? 'bg-gradient-to-br from-amber-500 to-orange-500'
                              : 'bg-gradient-to-br from-red-500 to-pink-500'
                      }`}>
                        <Shield className="w-5 h-5 text-white" />
                      </div>
                      <div>
                        <h3 className="text-sm font-semibold text-gray-900">信号供给可信度</h3>
                        <div className="flex items-center gap-2 text-[10px]">
                          <span className={`px-1.5 py-0.5 rounded ${
                            signalSupplyStatus?.overallCredibility === 'high' 
                              ? 'bg-emerald-100 text-emerald-700' 
                              : signalSupplyStatus?.overallCredibility === 'medium'
                                ? 'bg-blue-100 text-blue-700'
                                : signalSupplyStatus?.overallCredibility === 'degraded'
                                  ? 'bg-amber-100 text-amber-700'
                                  : 'bg-red-100 text-red-700'
                          }`}>
                            {signalSupplyStatus?.overallCredibility === 'high' ? '🟢 HIGH' :
                             signalSupplyStatus?.overallCredibility === 'medium' ? '🔵 MEDIUM' :
                             signalSupplyStatus?.overallCredibility === 'degraded' ? '🟡 DEGRADED' :
                             '🔴 CRITICAL'}
                          </span>
                          <span className="text-gray-400">|</span>
                          <span className={`px-1.5 py-0.5 rounded ${
                            signalSupplyStatus?.decisionGuidance.canMakeDecision
                              ? 'bg-emerald-50 text-emerald-600'
                              : 'bg-red-50 text-red-600'
                          }`}>
                            {signalSupplyStatus?.decisionGuidance.canMakeDecision ? '✓ 可决策' : '✗ 不可决策'}
                          </span>
                          <span className="text-gray-400">|</span>
                          <span className="text-gray-500">置信度 {signalSupplyStatus?.decisionGuidance.confidenceLevel || 0}%</span>
                        </div>
                      </div>
                    </div>
                    <div className="flex items-center gap-3">
                      <button
                        onClick={(e) => { 
                          e.preventDefault()
                          // 刷新信号供给状态
                          setSignalSupplyStatus(signalSupplyController.getSupplyStatus())
                        }}
                        className={`p-2 rounded-lg hover:bg-gray-100 transition-colors`}
                      >
                        <RefreshCw className="w-4 h-4 text-gray-500" />
                      </button>
                      <ChevronDown className="w-5 h-5 text-gray-400 group-open:rotate-180 transition-transform" />
                    </div>
                  </summary>
                  
                  <div className="mt-3 space-y-4">
                    {/* ========== Layer 1: 信号供给总览 ========== */}
                    <div className="grid grid-cols-4 gap-3">
                      {/* 状态驱动源 State Drivers */}
                      <div className={`${cardClass} p-4`}>
                        <div className="flex items-center gap-2 mb-3">
                          <div className="w-8 h-8 rounded-lg bg-emerald-100 flex items-center justify-center">
                            <Shield className="w-4 h-4 text-emerald-600" />
                          </div>
                          <div>
                            <div className="text-xs font-semibold text-gray-700">状态驱动源</div>
                            <div className="text-[10px] text-gray-400">State Drivers</div>
                          </div>
                        </div>
                        <div className="flex items-end justify-between">
                          <div className={`text-2xl font-bold ${
                            (signalSupplyStatus?.stateDrivers.online || 0) >= 3 ? 'text-emerald-600' : 
                            (signalSupplyStatus?.stateDrivers.online || 0) >= 1 ? 'text-amber-600' : 
                            'text-red-600'
                          }`}>
                            {signalSupplyStatus?.stateDrivers.online || 0}/{signalSupplyStatus?.stateDrivers.total || 0}
                          </div>
                          <div className={`text-[10px] px-2 py-1 rounded ${
                            signalSupplyStatus?.modelInputStatus.stateDrivingInputs === 'ok' ? 'bg-emerald-100 text-emerald-700' :
                            signalSupplyStatus?.modelInputStatus.stateDrivingInputs === 'partial' ? 'bg-amber-100 text-amber-700' :
                            'bg-red-100 text-red-700'
                          }`}>
                            {signalSupplyStatus?.modelInputStatus.stateDrivingInputs === 'ok' ? '✓ OK' :
                             signalSupplyStatus?.modelInputStatus.stateDrivingInputs === 'partial' ? '◐ 部分' :
                             '✗ 不足'}
                          </div>
                        </div>
                        <div className="text-[9px] text-gray-400 mt-2">可直接改变政策状态</div>
                      </div>
                      
                      {/* 验证源 Validators */}
                      <div className={`${cardClass} p-4`}>
                        <div className="flex items-center gap-2 mb-3">
                          <div className="w-8 h-8 rounded-lg bg-blue-100 flex items-center justify-center">
                            <CheckCircle className="w-4 h-4 text-blue-600" />
                          </div>
                          <div>
                            <div className="text-xs font-semibold text-gray-700">验证源</div>
                            <div className="text-[10px] text-gray-400">Validators</div>
                          </div>
                        </div>
                        <div className="flex items-end justify-between">
                          <div className={`text-2xl font-bold ${
                            (signalSupplyStatus?.validators.online || 0) >= 2 ? 'text-blue-600' : 
                            (signalSupplyStatus?.validators.online || 0) >= 1 ? 'text-amber-600' : 
                            'text-gray-400'
                          }`}>
                            {signalSupplyStatus?.validators.online || 0}/{signalSupplyStatus?.validators.total || 0}
                          </div>
                          <div className={`text-[10px] px-2 py-1 rounded ${
                            signalSupplyStatus?.modelInputStatus.validationInputs === 'ok' ? 'bg-blue-100 text-blue-700' :
                            signalSupplyStatus?.modelInputStatus.validationInputs === 'partial' ? 'bg-amber-100 text-amber-700' :
                            'bg-gray-100 text-gray-600'
                          }`}>
                            {signalSupplyStatus?.modelInputStatus.validationInputs === 'ok' ? '✓ 充足' :
                             signalSupplyStatus?.modelInputStatus.validationInputs === 'partial' ? '◐ 有限' :
                             '✗ 不足'}
                          </div>
                        </div>
                        <div className="text-[9px] text-gray-400 mt-2">交叉验证信号</div>
                      </div>
                      
                      {/* 早期指标 Early Indicators */}
                      <div className={`${cardClass} p-4`}>
                        <div className="flex items-center gap-2 mb-3">
                          <div className="w-8 h-8 rounded-lg bg-violet-100 flex items-center justify-center">
                            <Zap className="w-4 h-4 text-violet-600" />
                          </div>
                          <div>
                            <div className="text-xs font-semibold text-gray-700">早期指标</div>
                            <div className="text-[10px] text-gray-400">Early Indicators</div>
                          </div>
                        </div>
                        <div className="flex items-end justify-between">
                          <div className="text-2xl font-bold text-violet-600">
                            {signalSupplyStatus?.earlyIndicators.online || 0}/{signalSupplyStatus?.earlyIndicators.total || 0}
                          </div>
                          <div className={`text-[10px] px-2 py-1 rounded ${
                            signalSupplyStatus?.modelInputStatus.earlySignalNoiseLevel === 'low' ? 'bg-emerald-100 text-emerald-700' :
                            signalSupplyStatus?.modelInputStatus.earlySignalNoiseLevel === 'medium' ? 'bg-amber-100 text-amber-700' :
                            'bg-red-100 text-red-700'
                          }`}>
                            噪音: {signalSupplyStatus?.modelInputStatus.earlySignalNoiseLevel === 'low' ? '低' :
                                   signalSupplyStatus?.modelInputStatus.earlySignalNoiseLevel === 'medium' ? '中' : '高'}
                          </div>
                        </div>
                        <div className="text-[9px] text-gray-400 mt-2">提前信号 不改状态</div>
                      </div>
                      
                      {/* 背景信息 Context Providers */}
                      <div className={`${cardClass} p-4`}>
                        <div className="flex items-center gap-2 mb-3">
                          <div className="w-8 h-8 rounded-lg bg-gray-100 flex items-center justify-center">
                            <Database className="w-4 h-4 text-gray-600" />
                          </div>
                          <div>
                            <div className="text-xs font-semibold text-gray-700">背景源</div>
                            <div className="text-[10px] text-gray-400">Context</div>
                          </div>
                        </div>
                        <div className="flex items-end justify-between">
                          <div className="text-2xl font-bold text-gray-600">
                            {signalSupplyStatus?.contextProviders.online || 0}/{signalSupplyStatus?.contextProviders.total || 0}
                          </div>
                        </div>
                        <div className="text-[9px] text-gray-400 mt-2">仅提供背景</div>
                      </div>
                    </div>
                    
                    {/* ========== Layer 2: 按信号角色分组的来源列表 ========== */}
                    <div className={cardClass}>
                      <div className="p-4 border-b border-gray-100">
                        <div className="flex items-center justify-between">
                          <span className="text-xs font-semibold text-gray-700">信号来源详情 (按功能分组)</span>
                          <div className="flex items-center gap-2">
                            {/* 权威等级图例 */}
                            {(['A', 'B', 'C', 'D'] as SourceAuthority[]).map(auth => (
                              <span key={auth} className={`text-[10px] px-2 py-1 rounded ${
                                auth === 'A' ? 'bg-emerald-100 text-emerald-700' :
                                auth === 'B' ? 'bg-blue-100 text-blue-700' :
                                auth === 'C' ? 'bg-amber-100 text-amber-700' :
                                'bg-gray-100 text-gray-500'
                              }`}>
                                {auth}: {signalSources.filter(s => s.authority === auth).length}
                              </span>
                            ))}
                          </div>
                        </div>
                      </div>
                      <div className="p-4">
                        <div className="grid grid-cols-4 gap-4">
                          {/* 状态驱动源 */}
                          <div className="space-y-2">
                            <div className="text-xs font-semibold px-2 py-1 rounded bg-emerald-50 text-emerald-700 flex items-center gap-2">
                              <Shield className="w-3 h-3" />
                              状态驱动源 (A/B级)
                            </div>
                            <div className="space-y-1 max-h-40 overflow-auto">
                              {signalSources.filter(s => s.role === 'state-driver').map(source => (
                                <div key={source.id} className="flex items-center justify-between p-2 bg-gray-50 rounded text-[10px]">
                                  <div className="flex items-center gap-2">
                                    <div className={`w-2 h-2 rounded-full ${
                                      source.operational.status === 'online' ? 'bg-emerald-500' :
                                      source.operational.status === 'degraded' ? 'bg-amber-500' :
                                      source.operational.status === 'offline' ? 'bg-red-500' :
                                      'bg-gray-400'
                                    }`} />
                                    <span className={`font-semibold ${
                                      source.authority === 'A' ? 'text-emerald-700' : 'text-blue-700'
                                    }`}>{source.authority}</span>
                                    <span className="text-gray-700 truncate max-w-[80px]" title={source.name}>{source.nameZh}</span>
                                  </div>
                                  <span className={`text-[9px] px-1 py-0.5 rounded ${
                                    source.jurisdiction === 'US' ? 'bg-blue-50 text-blue-600' :
                                    source.jurisdiction === 'EU' ? 'bg-amber-50 text-amber-600' :
                                    source.jurisdiction === 'CN' ? 'bg-red-50 text-red-600' :
                                    'bg-violet-50 text-violet-600'
                                  }`}>{source.jurisdiction}</span>
                                </div>
                              ))}
                            </div>
                          </div>
                          
                          {/* 验证源 */}
                          <div className="space-y-2">
                            <div className="text-xs font-semibold px-2 py-1 rounded bg-blue-50 text-blue-700 flex items-center gap-2">
                              <CheckCircle className="w-3 h-3" />
                              验证源 (交叉确认)
                            </div>
                            <div className="space-y-1 max-h-40 overflow-auto">
                              {signalSources.filter(s => s.role === 'validator').map(source => (
                                <div key={source.id} className="flex items-center justify-between p-2 bg-gray-50 rounded text-[10px]">
                                  <div className="flex items-center gap-2">
                                    <div className={`w-2 h-2 rounded-full ${
                                      source.operational.status === 'online' ? 'bg-emerald-500' :
                                      source.operational.status === 'degraded' ? 'bg-amber-500' :
                                      'bg-gray-400'
                                    }`} />
                                    <span className="font-semibold text-amber-700">{source.authority}</span>
                                    <span className="text-gray-700 truncate max-w-[80px]" title={source.name}>{source.nameZh}</span>
                                  </div>
                                  <span className="text-[9px] text-gray-400">权重:{Math.round(source.decisionWeight)}</span>
                                </div>
                              ))}
                            </div>
                          </div>
                          
                          {/* 早期指标 */}
                          <div className="space-y-2">
                            <div className="text-xs font-semibold px-2 py-1 rounded bg-violet-50 text-violet-700 flex items-center gap-2">
                              <Zap className="w-3 h-3" />
                              早期指标 (预警)
                            </div>
                            <div className="space-y-1 max-h-40 overflow-auto">
                              {signalSources.filter(s => s.role === 'early-indicator').map(source => (
                                <div key={source.id} className="flex items-center justify-between p-2 bg-gray-50 rounded text-[10px]">
                                  <div className="flex items-center gap-2">
                                    <div className={`w-2 h-2 rounded-full ${
                                      source.operational.status === 'online' ? 'bg-emerald-500' :
                                      source.operational.status === 'degraded' ? 'bg-amber-500' :
                                      'bg-gray-400'
                                    }`} />
                                    <span className="font-semibold text-gray-500">{source.authority}</span>
                                    <span className="text-gray-700 truncate max-w-[80px]" title={source.name}>{source.nameZh}</span>
                                  </div>
                                  {source.operational.hasError && (
                                    <span className="text-[9px] text-red-500">⚠ {source.operational.errorMessage?.slice(0,10)}</span>
                                  )}
                                </div>
                              ))}
                              {signalSources.filter(s => s.role === 'early-indicator').length === 0 && (
                                <div className="text-[10px] text-gray-400 p-2">无早期指标源</div>
                              )}
                            </div>
                          </div>
                          
                          {/* 背景源 */}
                          <div className="space-y-2">
                            <div className="text-xs font-semibold px-2 py-1 rounded bg-gray-100 text-gray-700 flex items-center gap-2">
                              <Database className="w-3 h-3" />
                              背景信息源
                            </div>
                            <div className="space-y-1 max-h-40 overflow-auto">
                              {signalSources.filter(s => s.role === 'context-provider').map(source => (
                                <div key={source.id} className="flex items-center justify-between p-2 bg-gray-50 rounded text-[10px]">
                                  <div className="flex items-center gap-2">
                                    <div className={`w-2 h-2 rounded-full ${
                                      source.operational.status === 'online' ? 'bg-emerald-500' :
                                      'bg-gray-400'
                                    }`} />
                                    <span className="text-gray-700 truncate max-w-[100px]" title={source.name}>{source.nameZh}</span>
                                  </div>
                                </div>
                              ))}
                              {signalSources.filter(s => s.role === 'context-provider').length === 0 && (
                                <div className="text-[10px] text-gray-400 p-2">无背景源</div>
                              )}
                            </div>
                          </div>
                        </div>
                      </div>
                    </div>
                    
                    {/* ========== Layer 3: 决策建议与警告 ========== */}
                    <div className={cardClass}>
                      <div className="p-4 border-b border-gray-100">
                        <span className="text-xs font-semibold text-gray-700">决策系统状态</span>
                      </div>
                      <div className="p-4">
                        <div className="grid grid-cols-5 gap-4">
                          {/* 决策可行性 */}
                          <div className={`p-4 rounded-xl border ${
                            signalSupplyStatus?.decisionGuidance.canMakeDecision 
                              ? 'bg-emerald-50 border-emerald-200' 
                              : 'bg-red-50 border-red-200'
                          }`}>
                            <div className="text-[10px] text-gray-500 font-medium uppercase tracking-wide mb-1">决策可行性</div>
                            <div className={`text-2xl font-bold ${
                              signalSupplyStatus?.decisionGuidance.canMakeDecision 
                                ? 'text-emerald-600' 
                                : 'text-red-600'
                            }`}>
                              {signalSupplyStatus?.decisionGuidance.canMakeDecision ? '✓ 可决策' : '✗ 不可'}
                            </div>
                          </div>
                          
                          {/* 置信度 */}
                          <div className={`p-4 rounded-xl border ${
                            (signalSupplyStatus?.decisionGuidance.confidenceLevel || 0) >= 70 
                              ? 'bg-blue-50 border-blue-200' 
                              : 'bg-gray-50 border-gray-200'
                          }`}>
                            <div className="text-[10px] text-gray-500 font-medium uppercase tracking-wide mb-1">置信度</div>
                            <div className={`text-2xl font-bold ${
                              (signalSupplyStatus?.decisionGuidance.confidenceLevel || 0) >= 70 
                                ? 'text-blue-600' 
                                : 'text-gray-600'
                            }`}>
                              {signalSupplyStatus?.decisionGuidance.confidenceLevel || 0}%
                            </div>
                          </div>
                          
                          {/* 状态驱动输入 */}
                          <div className={`p-4 rounded-xl border ${
                            signalSupplyStatus?.modelInputStatus.stateDrivingInputs === 'ok' 
                              ? 'bg-emerald-50 border-emerald-200' 
                              : signalSupplyStatus?.modelInputStatus.stateDrivingInputs === 'partial'
                                ? 'bg-amber-50 border-amber-200'
                                : 'bg-red-50 border-red-200'
                          }`}>
                            <div className="text-[10px] text-gray-500 font-medium uppercase tracking-wide mb-1">驱动源状态</div>
                            <div className={`text-lg font-bold ${
                              signalSupplyStatus?.modelInputStatus.stateDrivingInputs === 'ok' 
                                ? 'text-emerald-600' 
                                : signalSupplyStatus?.modelInputStatus.stateDrivingInputs === 'partial'
                                  ? 'text-amber-600'
                                  : 'text-red-600'
                            }`}>
                              {signalSupplyStatus?.modelInputStatus.stateDrivingInputs === 'ok' ? '完整' :
                               signalSupplyStatus?.modelInputStatus.stateDrivingInputs === 'partial' ? '部分' : '不足'}
                            </div>
                          </div>
                          
                          {/* 缺失来源 */}
                          <div className="p-4 rounded-xl bg-orange-50 border border-orange-200 col-span-2">
                            <div className="text-[10px] text-gray-500 font-medium uppercase tracking-wide mb-1">缺失关键源</div>
                            <div className="text-sm">
                              {signalSupplyStatus?.decisionGuidance.missingCriticalSources.length === 0 ? (
                                <span className="text-emerald-600 font-medium">无缺失 ✓</span>
                              ) : (
                                <div className="flex flex-wrap gap-1">
                                  {signalSupplyStatus?.decisionGuidance.missingCriticalSources.map(name => (
                                    <span key={name} className="text-[10px] px-1.5 py-0.5 bg-red-100 text-red-700 rounded">{name}</span>
                                  ))}
                                </div>
                              )}
                            </div>
                          </div>
                        </div>
                        
                        {/* 警告信息 */}
                        {signalSupplyStatus?.decisionGuidance.warnings && signalSupplyStatus.decisionGuidance.warnings.length > 0 && (
                          <div className="mt-4 p-3 bg-amber-50 border border-amber-200 rounded-xl">
                            <div className="text-[10px] font-medium text-amber-700 mb-2">⚠ 系统警告</div>
                            <div className="space-y-1">
                              {signalSupplyStatus.decisionGuidance.warnings.map((warning, idx) => (
                                <div key={idx} className="text-[11px] text-amber-800 flex items-center gap-2">
                                  <AlertCircle className="w-3 h-3" />
                                  {warning}
                                </div>
                              ))}
                            </div>
                          </div>
                        )}
                      </div>
                    </div>
                  </div>
                </details>
              </div>
            )}
            
            {/* Breaking News Tab */}
            {activeTab === 'breaking' && (
              <div className="space-y-4">
                <div className={cardClass}>
                  <div className="p-5 border-b border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-red-500 to-orange-500 flex items-center justify-center">
                          <Zap className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h2 className="text-base font-semibold text-gray-900">{t('news.breaking')}</h2>
                          <p className="text-xs text-gray-500">{Object.keys(SOURCES).length} {t('news.officialSources')}</p>
                        </div>
                      </div>
                      <div className="flex items-center gap-3">
                        <div className="flex items-center gap-1.5 px-3 py-1.5 bg-green-50 rounded-lg border border-green-100">
                          <span className="w-2 h-2 bg-green-500 rounded-full" />
                          <span className="text-[10px] font-medium text-green-700 font-mono">{t('common.live')}: {displayTime}</span>
                        </div>
                        <span className="text-xs text-gray-400">
                          {filteredBreakingNews.length}/{breakingNews.length} {t('news.shown')}
                        </span>
                        <button
                          onClick={() => setBreakingNews(prev => prev.map(n => ({ ...n, isRead: true })))}
                          className="text-xs px-3 py-1.5 bg-gray-100 text-gray-600 rounded-lg hover:bg-gray-200 font-medium transition-colors"
                        >
                          {t('news.markAllRead')}
                        </button>
                      </div>
                    </div>
                  </div>
                  
                  <div className="p-5 space-y-3">
                    {filteredBreakingNews.length === 0 ? (
                      <EmptyState
                        icon={<Zap className="w-12 h-12" />}
                        title={t('news.noBreakingNews') || 'No Breaking News'}
                        description={t('news.noBreakingNewsDesc') || 'No breaking news matches your current filters. Try adjusting the filters or wait for new updates.'}
                        isLoading={isLoadingRealData}
                        action={{
                          label: t('common.refresh'),
                          onClick: () => loadRealData()
                        }}
                      />
                    ) : filteredBreakingNews.map((news, index) => {
                      // urgencyColors 已提升到组件级别 useMemo
                      const isExpanded = expandedNewsId === news.id
                      return (
                        <div 
                          key={news.id}
                          className={`rounded-xl border cursor-pointer transition-all hover:shadow-md ${urgencyColors[news.urgency]} ${news.isRead ? 'opacity-75' : ''}`}
                        >
                          {/* 卡片头部 - 点击展开/收起 */}
                          <div 
                            className="p-4"
                            onClick={() => {
                              if (!news.isRead) {
                                setBreakingNews(prev => prev.map(n => n.id === news.id ? { ...n, isRead: true } : n))
                              }
                              setExpandedNewsId(isExpanded ? null : news.id)
                            }}
                          >
                            <div className="flex items-start justify-between mb-2">
                              <div className="flex-1">
                                <div className="flex items-center gap-2 mb-1 flex-wrap">
                                  <span className={`${badgeClass} ${
                                    news.urgency === 'flash' ? 'bg-red-200 text-red-800' :
                                    news.urgency === 'urgent' ? 'bg-orange-200 text-orange-800' :
                                    news.urgency === 'breaking' ? 'bg-amber-200 text-amber-800' :
                                    'bg-gray-200 text-gray-700'
                                  }`}>
                                    {t(`urgency.${news.urgency}`)}
                                  </span>
                                  {/* 安全访问 source */}
                                  {news.source && (
                                    <span className={`${badgeClass} ${
                                      news.source.level === 'L0' ? 'bg-blue-100 text-blue-700' :
                                      news.source.level === 'L0.5' ? 'bg-cyan-100 text-cyan-700' :
                                      'bg-gray-100 text-gray-600'
                                    }`}>
                                      {news.source.level}
                                    </span>
                                  )}
                                  {/* 决策评分 - 只有达到门槛才显示 */}
                                  {news.decisionScore !== undefined && news.decisionScore !== 0 && (
                                    <span className={`${badgeClass} font-bold ${
                                      news.decisionScore >= 50 ? 'bg-emerald-200 text-emerald-800' :
                                      news.decisionScore >= 25 ? 'bg-blue-200 text-blue-800' :
                                      news.decisionScore < 0 ? 'bg-red-200 text-red-800' :
                                      'bg-gray-200 text-gray-700'
                                    }`}>
                                      Score: {news.decisionScore}
                                    </span>
                                  )}
                                  {/* 早期事件用"预期影响"代替分数 */}
                                  {(news.decisionScore === undefined || news.decisionScore === 0) && news.confidence !== undefined && news.confidence < 0.5 && (
                                    <span className={`${badgeClass} bg-purple-100 text-purple-700`}>
                                      预期影响
                                    </span>
                                  )}
                                  {/* 可交易性 */}
                                  {news.tradeability === 'tradeable' && (
                                    <span className={`${badgeClass} bg-green-200 text-green-800 font-semibold`}>
                                      ✓ 可交易
                                    </span>
                                  )}
                                  {news.tradeability === 'monitor' && (
                                    <span className={`${badgeClass} bg-amber-100 text-amber-700`}>
                                      ◐ 监控
                                    </span>
                                  )}
                                  {/* 安全访问 source.name */}
                                  <span className="text-xs text-gray-600 font-medium">{news.source?.name || 'Unknown'}</span>
                                  {/* 时间显示：简化为相对时间 */}
                                  <span className="text-[10px] text-gray-400 tabular-nums">
                                    {formatTimeAgo(news.publishedAt)}
                                  </span>
                                  {!news.isRead && <span className="w-2 h-2 bg-red-500 rounded-full animate-pulse" />}
                                  {/* 展开指示器 */}
                                  <ChevronDown className={`w-4 h-4 text-gray-400 transition-transform ${isExpanded ? 'rotate-180' : ''}`} />
                                </div>
                                <h3 className="text-sm font-semibold text-gray-900">{news.headline}</h3>
                              </div>
                              <span className={`${badgeClass} ${
                                news.sentiment === 'bullish' ? 'bg-emerald-100 text-emerald-700' :
                                news.sentiment === 'bearish' ? 'bg-red-100 text-red-700' :
                                'bg-gray-100 text-gray-600'
                              }`}>
                                {t(`sentiment.${news.sentiment}`)}
                              </span>
                            </div>
                            
                            {/* Industry Impact - 始终显示 */}
                            <div className="mt-3 pt-3 border-t border-gray-100/50">
                              <div className="text-[10px] text-gray-500 font-medium mb-2">{t('news.industryImpact')}</div>
                              <div className="flex flex-wrap gap-2">
                                {(news.industries ?? []).slice(0, 4).map((ind, idx) => (
                                  <span 
                                    key={idx}
                                    className={`text-[10px] px-2 py-1 rounded-lg ${
                                      ind.direction === 'bullish' ? 'bg-emerald-100 text-emerald-700' :
                                      ind.direction === 'bearish' ? 'bg-red-100 text-red-700' :
                                      'bg-gray-100 text-gray-600'
                                    }`}
                                  >
                                    {ind.icon} {ind.industry} ({ind.confidence}%)
                                  </span>
                                ))}
                              </div>
                            </div>
                          </div>

                          {/* 🆕 展开详情面板 */}
                          {isExpanded && (
                            <div className="px-4 pb-4 space-y-4 border-t border-gray-200/50 bg-white/50">
                              {/* 摘要 */}
                              {news.summary && (
                                <div className="pt-3">
                                  <div className="text-[10px] text-gray-500 font-medium mb-1">摘要</div>
                                  <p className="text-sm text-gray-700">{news.summary}</p>
                                </div>
                              )}

                              {/* 原文 */}
                              {news.originalText && (
                                <div className="p-3 bg-gray-50 rounded-lg border border-gray-100">
                                  <div className="text-[10px] text-gray-500 font-medium mb-1">原文</div>
                                  <p className="text-xs text-gray-600 italic">"{news.originalText}"</p>
                                  {news.translatedText && (
                                    <p className="text-xs text-gray-500 mt-2">{news.translatedText}</p>
                                  )}
                                </div>
                              )}

                              {/* 辖区信息 */}
                              {news.jurisdiction && (
                                <div className="grid grid-cols-4 gap-3">
                                  <div className="p-2 bg-blue-50 rounded-lg">
                                    <div className="text-[10px] text-blue-600 font-medium">辖区</div>
                                    <div className="text-xs font-semibold text-blue-800">{news.jurisdiction.region}</div>
                                  </div>
                                  <div className="p-2 bg-purple-50 rounded-lg">
                                    <div className="text-[10px] text-purple-600 font-medium">权力层级</div>
                                    <div className="text-xs font-semibold text-purple-800">{news.jurisdiction.authorityLevel}</div>
                                  </div>
                                  <div className="p-2 bg-cyan-50 rounded-lg">
                                    <div className="text-[10px] text-cyan-600 font-medium">执行机构</div>
                                    <div className="text-xs font-semibold text-cyan-800">{news.jurisdiction.executingBody}</div>
                                  </div>
                                  <div className="p-2 bg-amber-50 rounded-lg">
                                    <div className="text-[10px] text-amber-600 font-medium">执行权力</div>
                                    <div className={`text-xs font-semibold ${
                                      news.jurisdiction.enforcementPower === 'full' ? 'text-green-800' :
                                      news.jurisdiction.enforcementPower === 'partial' ? 'text-amber-800' :
                                      'text-gray-600'
                                    }`}>
                                      {news.jurisdiction.enforcementPower === 'full' ? '✓ 完全执行权' :
                                       news.jurisdiction.enforcementPower === 'partial' ? '◐ 部分执行权' :
                                       '○ 仅信号'}
                                    </div>
                                  </div>
                                </div>
                              )}

                              {/* 决策评分详情 */}
                              {news.decisionScore !== undefined && (
                                <div className="p-3 bg-gradient-to-r from-blue-50 to-purple-50 rounded-lg border border-blue-100">
                                  <div className="flex items-center justify-between mb-2">
                                    <div className="text-[10px] text-blue-600 font-medium">决策评分</div>
                                    <div className={`text-lg font-bold ${
                                      news.decisionScore >= 50 ? 'text-emerald-600' :
                                      news.decisionScore >= 25 ? 'text-blue-600' :
                                      news.decisionScore < 0 ? 'text-red-600' :
                                      'text-gray-600'
                                    }`}>
                                      {news.decisionScore}
                                    </div>
                                  </div>
                                  <div className="flex items-center gap-4 text-xs">
                                    <span className="text-gray-600">置信度: <span className="font-semibold">{((news.confidence || 0) * 100).toFixed(0)}%</span></span>
                                    <span className={`font-semibold ${
                                      news.tradeability === 'tradeable' ? 'text-green-600' :
                                      news.tradeability === 'monitor' ? 'text-amber-600' :
                                      'text-gray-500'
                                    }`}>
                                      {news.tradeability === 'tradeable' ? '✓ 可交易' :
                                       news.tradeability === 'monitor' ? '◐ 监控中' :
                                       '○ 忽略'}
                                    </span>
                                  </div>
                                </div>
                              )}

                              {/* 受影响资产 */}
                              {news.affectedAssets && news.affectedAssets.length > 0 && (
                                <div>
                                  <div className="text-[10px] text-gray-500 font-medium mb-2">受影响资产</div>
                                  <div className="flex flex-wrap gap-2">
                                    {news.affectedAssets.map((asset, idx) => (
                                      <span 
                                        key={idx}
                                        className={`text-xs px-2 py-1 rounded-lg font-medium ${
                                          asset.direction === 'bullish' ? 'bg-emerald-100 text-emerald-700' :
                                          asset.direction === 'bearish' ? 'bg-red-100 text-red-700' :
                                          'bg-gray-100 text-gray-600'
                                        }`}
                                      >
                                        {asset.direction === 'bullish' ? '↑' : asset.direction === 'bearish' ? '↓' : '→'} {asset.ticker} ({asset.confidence}%)
                                      </span>
                                    ))}
                                  </div>
                                </div>
                              )}

                              {/* 关键风险 & 下一触发点 */}
                              <div className="grid grid-cols-2 gap-3">
                                {news.keyRisk && (
                                  <div className="p-2 bg-red-50 rounded-lg border border-red-100">
                                    <div className="text-[10px] text-red-600 font-medium mb-1">⚠️ 关键风险</div>
                                    <p className="text-xs text-red-700">{news.keyRisk}</p>
                                  </div>
                                )}
                                {news.nextTrigger && (
                                  <div className="p-2 bg-blue-50 rounded-lg border border-blue-100">
                                    <div className="text-[10px] text-blue-600 font-medium mb-1">⏱️ 下一验证点</div>
                                    <p className="text-xs text-blue-700">{news.nextTrigger}</p>
                                  </div>
                                )}
                              </div>

                              {/* 关联政策 */}
                              {news.relatedPolicies && news.relatedPolicies.length > 0 && (
                                <div className="flex items-center gap-2">
                                  <span className="text-[10px] text-gray-500">关联政策:</span>
                                  {news.relatedPolicies.map((policy, idx) => (
                                    <span key={idx} className="text-[10px] px-2 py-0.5 bg-gray-100 text-gray-600 rounded">
                                      {policy}
                                    </span>
                                  ))}
                                </div>
                              )}
                            </div>
                          )}
                        </div>
                      )
                    })}
                  </div>
                </div>
                
                {/* Global Source Monitoring */}
                <div className={cardClass}>
                  <div className="p-5 border-b border-gray-100">
                    <div className="flex items-center gap-3">
                      <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-cyan-500 to-blue-500 flex items-center justify-center">
                        <Globe className="w-5 h-5 text-white" />
                      </div>
                      <h3 className="text-base font-semibold text-gray-900">{t('news.monitoring')}</h3>
                    </div>
                  </div>
                  
                  <div className="p-5 grid grid-cols-4 gap-4">
                    {/* US */}
                    <div className="p-4 bg-blue-50 rounded-xl border border-blue-100">
                      <div className="text-xs font-semibold text-blue-800 mb-3">{t('region.us')}</div>
                      <div className="space-y-1.5">
                        {Object.values(SOURCES).filter(s => ['trump-truth', 'whitehouse', 'treasury', 'fed', 'commerce'].includes(s.id)).slice(0, 5).map(src => (
                          <div key={src.id} className="flex items-center justify-between text-[10px]">
                            <span className="text-gray-700 truncate max-w-[100px]">{src.name}</span>
                            <span className={`px-1.5 py-0.5 rounded ${
                              src.level === 'L0' ? 'bg-blue-200 text-blue-800' :
                              src.level === 'L0.5' ? 'bg-cyan-200 text-cyan-800' :
                              'bg-gray-200 text-gray-600'
                            }`}>{src.level}</span>
                          </div>
                        ))}
                      </div>
                    </div>
                    
                    {/* Europe */}
                    <div className="p-4 bg-amber-50 rounded-xl border border-amber-100">
                      <div className="text-xs font-semibold text-amber-800 mb-3">{t('region.eu')}</div>
                      <div className="space-y-1.5">
                        {Object.values(SOURCES).filter(s => ['ecb', 'eu-commission', 'bundesbank', 'boe', 'snb'].includes(s.id)).map(src => (
                          <div key={src.id} className="flex items-center justify-between text-[10px]">
                            <span className="text-gray-700 truncate max-w-[100px]">{src.name}</span>
                            <span className={`px-1.5 py-0.5 rounded ${
                              src.level === 'L0' ? 'bg-blue-200 text-blue-800' :
                              src.level === 'L0.5' ? 'bg-cyan-200 text-cyan-800' :
                              'bg-gray-200 text-gray-600'
                            }`}>{src.level}</span>
                          </div>
                        ))}
                      </div>
                    </div>
                    
                    {/* Asia */}
                    <div className="p-4 bg-red-50 rounded-xl border border-red-100">
                      <div className="text-xs font-semibold text-red-800 mb-3">{t('region.asia')}</div>
                      <div className="space-y-1.5">
                        {Object.values(SOURCES).filter(s => ['pboc', 'mofcom', 'ndrc', 'boj', 'meti'].includes(s.id)).slice(0, 5).map(src => (
                          <div key={src.id} className="flex items-center justify-between text-[10px]">
                            <span className="text-gray-700 truncate max-w-[100px]">{src.name}</span>
                            <span className={`px-1.5 py-0.5 rounded ${
                              src.level === 'L0' ? 'bg-blue-200 text-blue-800' :
                              src.level === 'L0.5' ? 'bg-cyan-200 text-cyan-800' :
                              'bg-gray-200 text-gray-600'
                            }`}>{src.level}</span>
                          </div>
                        ))}
                      </div>
                    </div>
                    
                    {/* International */}
                    <div className="p-4 bg-violet-50 rounded-xl border border-violet-100">
                      <div className="text-xs font-semibold text-violet-800 mb-3">{t('region.intl')}</div>
                      <div className="space-y-1.5">
                        {Object.values(SOURCES).filter(s => ['imf', 'worldbank', 'wto', 'bis'].includes(s.id)).map(src => (
                          <div key={src.id} className="flex items-center justify-between text-[10px]">
                            <span className="text-gray-700 truncate max-w-[100px]">{src.name}</span>
                            <span className={`px-1.5 py-0.5 rounded ${
                              src.level === 'L0' ? 'bg-blue-200 text-blue-800' :
                              src.level === 'L0.5' ? 'bg-cyan-200 text-cyan-800' :
                              'bg-gray-200 text-gray-600'
                            }`}>{src.level}</span>
                          </div>
                        ))}
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            )}
            
            {/* Topics Tab */}
            {activeTab === 'topics' && (
              <div className="space-y-4">
                {isLoadingRealData && topics.length === 0 ? (
                  <EmptyState
                    icon={<Target className="w-12 h-12" />}
                    title={t('news.loadingTopics')}
                    description={t('news.loadingTopicsDesc')}
                    isLoading={true}
                  />
                ) : filteredTopics.length === 0 ? (
                  <EmptyState
                    icon={<Target className="w-12 h-12" />}
                    title={t('news.noTopics')}
                    description={t('news.noTopicsDesc')}
                    error={realDataError}
                    action={{
                      label: t('common.refresh'),
                      onClick: () => loadRealData()
                    }}
                    diagnostics={{
                      lastAttempt: lastUpdate.toISOString(),
                      source: 'Federal Register / NewsAPI',
                      retryCount: 0
                    }}
                  />
                ) : (
                  <div className="grid grid-cols-2 gap-4">
                    {filteredTopics.map(topic => (
                      <div 
                        key={topic.id}
                        className={`${cardClass} p-5 cursor-pointer`}
                        onClick={() => setSelectedTopic(topic)}
                      >
                        <div className="flex items-start gap-4">
                          {/* Score Circle */}
                          <div className={`w-16 h-16 rounded-2xl flex flex-col items-center justify-center ${
                            topic.decisionScore?.score >= 80 ? 'bg-emerald-100' :
                            topic.decisionScore?.score >= 60 ? 'bg-blue-100' :
                            topic.decisionScore?.score >= 40 ? 'bg-amber-100' :
                            'bg-gray-100'
                          }`}>
                            <span className={`text-2xl font-bold ${
                              topic.decisionScore?.score >= 80 ? 'text-emerald-700' :
                              topic.decisionScore?.score >= 60 ? 'text-blue-700' :
                              topic.decisionScore?.score >= 40 ? 'text-amber-700' :
                              'text-gray-600'
                            }`}>
                              {topic.decisionScore?.score || 0}
                            </span>
                            <span className="text-[9px] text-gray-500 uppercase">{t('decision.score')}</span>
                          </div>
                          
                          {/* Content */}
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2 mb-2 flex-wrap">
                              <span className={`${badgeClass} ${
                                topic.decisionScore?.actionTier === 'high-conviction' ? 'bg-emerald-100 text-emerald-700' :
                                topic.decisionScore?.actionTier === 'trade' ? 'bg-blue-100 text-blue-700' :
                                topic.decisionScore?.actionTier === 'monitor' ? 'bg-amber-100 text-amber-700' :
                                'bg-gray-100 text-gray-600'
                              }`}>
                                {topic.decisionScore?.actionTier === 'high-conviction' ? t('decision.highConviction') :
                                 topic.decisionScore?.actionTier === 'trade' ? t('decision.actionable') :
                                 topic.decisionScore?.actionTier === 'monitor' ? t('decision.monitor') :
                                 t('decision.noAction')}
                              </span>
                              <span className={`${badgeClass} ${
                                topic.state === 'implementing' ? 'bg-violet-100 text-violet-700' :
                                topic.state === 'announced' ? 'bg-blue-100 text-blue-700' :
                                topic.state === 'effective' ? 'bg-emerald-100 text-emerald-700' :
                                'bg-gray-100 text-gray-600'
                              }`}>
                                {t(`state.${topic.state}`)}
                              </span>
                              {topic.inPolicyLoop && (
                                <span className={`${badgeClass} bg-orange-100 text-orange-700`}>
                                  {t('news.confirmed')}
                                </span>
                              )}
                            </div>
                            
                            <h3 className="text-sm font-semibold text-gray-900 mb-2 line-clamp-2">{topic.name}</h3>
                            
                            <div className="flex items-center gap-4 text-[10px] text-gray-500 mb-3">
                              <span>L0: {topic.l0Count}</span>
                              <span>L1: {topic.l1Count}</span>
                              <span>{topic.tradeableAssets.length} {t('common.assets')}</span>
                              <span>{formatTimeAgo(topic.lastUpdated)}</span>
                            </div>
                            
                            {/* Tradeable Assets */}
                            <div className="flex flex-wrap gap-1.5">
                              {topic.tradeableAssets.slice(0, 4).map(asset => (
                                <span 
                                  key={asset.ticker}
                                  className={`text-[10px] px-2 py-1 rounded-lg font-medium ${
                                    asset.exposure === 'bullish' ? 'bg-emerald-100 text-emerald-700' :
                                    asset.exposure === 'bearish' ? 'bg-red-100 text-red-700' :
                                    'bg-gray-100 text-gray-600'
                                  }`}
                                >
                                  {asset.ticker}
                                </span>
                              ))}
                              {topic.tradeableAssets.length > 4 && (
                                <span className="text-[10px] px-2 py-1 text-gray-400">+{topic.tradeableAssets.length - 4}</span>
                              )}
                            </div>
                          </div>
                          
                          {/* Validation Grade */}
                          {topic.validation && (
                            <div className="text-center">
                              <div className={`w-10 h-10 rounded-xl flex items-center justify-center text-lg font-bold ${
                                topic.validation.qualityGrade === 'A' ? 'bg-emerald-100 text-emerald-700' :
                                topic.validation.qualityGrade === 'B' ? 'bg-blue-100 text-blue-700' :
                                topic.validation.qualityGrade === 'C' ? 'bg-amber-100 text-amber-700' :
                                'bg-red-100 text-red-700'
                              }`}>
                                {topic.validation.qualityGrade}
                              </div>
                              <span className="text-[9px] text-gray-400 mt-1 block">{t('common.grade')}</span>
                            </div>
                          )}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}
            
            {/* Documents Tab - Enhanced */}
            {activeTab === 'documents' && (
              <div className="space-y-4">
                {/* 🆕 Document Search & Filters */}
                <div className={`${cardClass} p-4`}>
                  <div className="flex items-center gap-4">
                    <div className="flex-1 relative">
                      <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
                      <input
                        type="text"
                        placeholder="搜索文档标题、实体、话题..."
                        value={searchQuery}
                        onChange={(e) => setSearchQuery(e.target.value)}
                        className="w-full pl-10 pr-4 py-2 rounded-lg bg-gray-50 border border-gray-200 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
                      />
                    </div>
                    <select
                      value={filterLevel}
                      onChange={(e) => setFilterLevel(e.target.value as SourceLevel | 'all')}
                      className="px-3 py-2 rounded-lg bg-gray-50 border border-gray-200 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                    >
                      <option value="all">所有层级</option>
                      <option value="L0">L0 - 最高权威</option>
                      <option value="L0.5">L0.5 - 执行机构</option>
                      <option value="L1">L1 - 权威媒体</option>
                      <option value="L2">L2 - 次级来源</option>
                    </select>
                    <select
                      value={filterDomain}
                      onChange={(e) => setFilterDomain(e.target.value as Domain | 'all')}
                      className="px-3 py-2 rounded-lg bg-gray-50 border border-gray-200 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                    >
                      <option value="all">所有领域</option>
                      <option value="trade">贸易</option>
                      <option value="sanction">制裁</option>
                      <option value="export_control">出口管制</option>
                      <option value="rate">利率</option>
                      <option value="regulation">监管</option>
                    </select>
                    <button
                      onClick={() => {
                        // 导入真实文档到 documentService
                        documents.forEach(doc => {
                          realDocumentService.importDocument({
                            title: doc.title,
                            content: doc.summary,
                            url: doc.url,
                            source: 'federal_register',
                            publishedAt: doc.publishedAt
                          })
                        })
                      }}
                      className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-blue-600 text-white text-sm font-medium hover:bg-blue-700 transition-colors"
                    >
                      <Download className="w-4 h-4" />
                      导入全部
                    </button>
                  </div>
                  
                  {/* 🆕 Quick Stats */}
                  <div className="flex items-center gap-6 mt-4 pt-4 border-t border-gray-100">
                    <div className="flex items-center gap-2">
                      <div className="w-8 h-8 rounded-lg bg-blue-100 flex items-center justify-center">
                        <FileText className="w-4 h-4 text-blue-600" />
                      </div>
                      <div>
                        <div className="text-lg font-bold text-gray-900">{documents.length}</div>
                        <div className="text-[10px] text-gray-500 uppercase">总文档</div>
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      <div className="w-8 h-8 rounded-lg bg-emerald-100 flex items-center justify-center">
                        <CheckCircle className="w-4 h-4 text-emerald-600" />
                      </div>
                      <div>
                        <div className="text-lg font-bold text-gray-900">{documents.filter(d => d.source?.level === 'L0').length}</div>
                        <div className="text-[10px] text-gray-500 uppercase">L0 来源</div>
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      <div className="w-8 h-8 rounded-lg bg-red-100 flex items-center justify-center">
                        <TrendingDown className="w-4 h-4 text-red-600" />
                      </div>
                      <div>
                        <div className="text-lg font-bold text-gray-900">{documents.filter(d => d.sentiment === 'bearish').length}</div>
                        <div className="text-[10px] text-gray-500 uppercase">看跌信号</div>
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      <div className="w-8 h-8 rounded-lg bg-violet-100 flex items-center justify-center">
                        <Bookmark className="w-4 h-4 text-violet-600" />
                      </div>
                      <div>
                        <div className="text-lg font-bold text-gray-900">{realDocumentService.getBookmarkedDocuments().length}</div>
                        <div className="text-[10px] text-gray-500 uppercase">已收藏</div>
                      </div>
                    </div>
                  </div>
                </div>
                
                {/* 🆕 Document List */}
                {documents
                  .filter(doc => {
                    if (filterLevel !== 'all' && doc.source?.level !== filterLevel) return false
                    if (searchQuery && !doc.title.toLowerCase().includes(searchQuery.toLowerCase())) return false
                    return true
                  })
                  .map(doc => (
                  <div 
                    key={doc.id}
                    className={`${cardClass} p-4 cursor-pointer group`}
                    onClick={() => setSelectedDocument(doc)}
                  >
                    <div className="flex items-start gap-4">
                      <div className={`w-12 h-12 rounded-xl flex items-center justify-center ${
                        doc.source?.level === 'L0' ? 'bg-blue-100' :
                        doc.source?.level === 'L0.5' ? 'bg-cyan-100' :
                        'bg-gray-100'
                      }`}>
                        <FileText className={`w-6 h-6 ${
                          doc.source?.level === 'L0' ? 'text-blue-600' :
                          doc.source?.level === 'L0.5' ? 'text-cyan-600' :
                          'text-gray-500'
                        }`} />
                      </div>
                      
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-1">
                          <span className={`${badgeClass} ${
                            doc.source?.level === 'L0' ? 'bg-blue-100 text-blue-700' :
                            doc.source?.level === 'L0.5' ? 'bg-cyan-100 text-cyan-700' :
                            'bg-gray-100 text-gray-600'
                          }`}>
                            {doc.source?.level || 'L2'}
                          </span>
                          <span className="text-xs text-gray-600 font-medium">{doc.source?.name || 'Unknown'}</span>
                          <span className="text-[10px] text-gray-400">{formatTimeAgo(doc.publishedAt)}</span>
                          {doc.id.startsWith('fed-') && (
                            <span className="text-[10px] px-1.5 py-0.5 bg-emerald-100 text-emerald-700 rounded font-medium">实时</span>
                          )}
                        </div>
                        <h3 className="text-sm font-semibold text-gray-900 mb-1 line-clamp-1 group-hover:text-blue-600 transition-colors">{doc.title}</h3>
                        <p className="text-xs text-gray-500 line-clamp-2">{doc.summary}</p>
                        
                        {/* 🆕 Topics & Entities */}
                        <div className="flex items-center gap-2 mt-2 flex-wrap">
                          {doc.topics.slice(0, 3).map((topic, idx) => (
                            <span key={idx} className="text-[10px] px-2 py-0.5 bg-gray-100 text-gray-600 rounded-full">{topic}</span>
                          ))}
                          {doc.entities.slice(0, 2).map((entity, idx) => (
                            <span key={idx} className="text-[10px] px-2 py-0.5 bg-violet-100 text-violet-700 rounded-full">{entity.name}</span>
                          ))}
                        </div>
                      </div>
                      
                      <div className="flex flex-col items-end gap-2">
                        <span className={`${badgeClass} ${
                          doc.sentiment === 'bullish' ? 'bg-emerald-100 text-emerald-700' :
                          doc.sentiment === 'bearish' ? 'bg-red-100 text-red-700' :
                          'bg-gray-100 text-gray-600'
                        }`}>
                          {doc.sentiment === 'bullish' ? '↑ 看涨' : doc.sentiment === 'bearish' ? '↓ 看跌' : '- 中性'}
                        </span>
                        <span className="text-xs text-gray-500 font-medium">
                          DocScore: <span className={doc.docScore >= 80 ? 'text-emerald-600' : doc.docScore >= 60 ? 'text-amber-600' : 'text-gray-600'}>{Math.round(doc.docScore)}</span>
                        </span>
                        
                        {/* 🆕 Action Buttons */}
                        <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                          <button 
                            onClick={(e) => { e.stopPropagation(); window.open(doc.url, '_blank'); }}
                            className="p-1.5 rounded hover:bg-gray-100"
                            title="打开原文"
                          >
                            <ExternalLink className="w-3.5 h-3.5 text-gray-400" />
                          </button>
                          <button 
                            onClick={(e) => { e.stopPropagation(); }}
                            className="p-1.5 rounded hover:bg-gray-100"
                            title="收藏"
                          >
                            <BookmarkPlus className="w-3.5 h-3.5 text-gray-400" />
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
            
            {/* Alerts Tab - Enhanced */}
            {activeTab === 'alerts' && (
              <div className="space-y-4">
                {/* 🆕 Alert Management Header */}
                <div className={`${cardClass} p-4`}>
                  <div className="flex items-center justify-between mb-4">
                    <div className="flex items-center gap-3">
                      <div className="w-10 h-10 rounded-xl bg-red-100 flex items-center justify-center">
                        <AlertTriangle className="w-5 h-5 text-red-600" />
                      </div>
                      <div>
                        <h3 className="text-base font-semibold text-gray-900">警报中心</h3>
                        <p className="text-xs text-gray-500">实时监控政策变化与市场信号</p>
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => requestNotifications()}
                        className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium transition-colors ${
                          notificationPermission === 'granted'
                            ? 'bg-emerald-100 text-emerald-700'
                            : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                        }`}
                      >
                        {notificationPermission === 'granted' ? <Bell className="w-4 h-4" /> : <BellOff className="w-4 h-4" />}
                        {notificationPermission === 'granted' ? '通知已启用' : '启用通知'}
                      </button>
                      <button
                        onClick={() => realAlertService.resetToDefaultRules()}
                        className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-gray-100 text-gray-600 text-sm font-medium hover:bg-gray-200 transition-colors"
                      >
                        <RefreshCw className="w-4 h-4" />
                        重置规则
                      </button>
                    </div>
                  </div>
                  
                  {/* 🆕 Alert Stats */}
                  <div className="grid grid-cols-5 gap-3">
                    <div className="bg-red-50 rounded-lg p-3 border border-red-100">
                      <div className="text-[10px] text-red-600 font-medium uppercase">严重</div>
                      <div className="text-2xl font-bold text-red-600">{filteredAlerts.filter(a => a.severity === 'critical').length}</div>
                    </div>
                    <div className="bg-orange-50 rounded-lg p-3 border border-orange-100">
                      <div className="text-[10px] text-orange-600 font-medium uppercase">高</div>
                      <div className="text-2xl font-bold text-orange-600">{filteredAlerts.filter(a => a.severity === 'high').length}</div>
                    </div>
                    <div className="bg-amber-50 rounded-lg p-3 border border-amber-100">
                      <div className="text-[10px] text-amber-600 font-medium uppercase">中</div>
                      <div className="text-2xl font-bold text-amber-600">{filteredAlerts.filter(a => a.severity === 'medium').length}</div>
                    </div>
                    <div className="bg-gray-50 rounded-lg p-3 border border-gray-200">
                      <div className="text-[10px] text-gray-600 font-medium uppercase">低</div>
                      <div className="text-2xl font-bold text-gray-600">{filteredAlerts.filter(a => a.severity === 'low').length}</div>
                    </div>
                    <div className="bg-blue-50 rounded-lg p-3 border border-blue-100">
                      <div className="text-[10px] text-blue-600 font-medium uppercase">未读</div>
                      <div className="text-2xl font-bold text-blue-600">{filteredAlerts.filter(a => !a.read).length}</div>
                    </div>
                  </div>
                </div>
                
                {/* 🆕 Alert Rules Section */}
                <div className={`${cardClass} p-4`}>
                  <div className="flex items-center justify-between mb-3">
                    <h4 className="text-sm font-semibold text-gray-900 flex items-center gap-2">
                      <Settings className="w-4 h-4 text-gray-500" />
                      警报规则 ({alertRules.filter(r => r.enabled).length}/{alertRules.length} 激活)
                    </h4>
                    <button className="text-xs text-blue-600 hover:underline">+ 添加规则</button>
                  </div>
                  <div className="grid grid-cols-2 gap-2">
                    {alertRules.slice(0, 6).map(rule => (
                      <div 
                        key={rule.id}
                        className={`flex items-center justify-between p-2.5 rounded-lg border ${
                          rule.enabled ? 'bg-emerald-50 border-emerald-200' : 'bg-gray-50 border-gray-200'
                        }`}
                      >
                        <div className="flex items-center gap-2">
                          <div className={`w-2 h-2 rounded-full ${rule.enabled ? 'bg-emerald-500' : 'bg-gray-300'}`} />
                          <span className="text-xs font-medium text-gray-700">{rule.name}</span>
                        </div>
                        <div className="flex items-center gap-2">
                          <span className={`text-[10px] px-1.5 py-0.5 rounded ${
                            rule.priority === 'critical' ? 'bg-red-100 text-red-700' :
                            rule.priority === 'high' ? 'bg-orange-100 text-orange-700' :
                            'bg-gray-100 text-gray-600'
                          }`}>
                            {rule.priority}
                          </span>
                          <button
                            onClick={() => {
                              realAlertService.toggleRule(rule.id, !rule.enabled)
                              setAlertRules(realAlertService.getRules())
                            }}
                            className={`w-8 h-4 rounded-full transition-colors ${rule.enabled ? 'bg-emerald-500' : 'bg-gray-300'}`}
                          >
                            <div className={`w-3 h-3 rounded-full bg-white shadow transition-transform ${rule.enabled ? 'translate-x-4' : 'translate-x-0.5'}`} />
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
                
                {/* Alert List */}
                <div className="space-y-2">
                  {filteredAlerts.map(alert => (
                    <div 
                      key={alert.id}
                      className={`${cardClass} p-4 cursor-pointer group ${
                        alert.severity === 'critical' ? 'border-l-4 border-l-red-500' :
                        alert.severity === 'high' ? 'border-l-4 border-l-orange-500' :
                        alert.severity === 'medium' ? 'border-l-4 border-l-amber-500' :
                        ''
                      } ${alert.read ? 'opacity-60' : ''}`}
                      onClick={() => setSelectedTopic(alert.topic)}
                    >
                      <div className="flex items-start gap-4">
                        <div className={`w-10 h-10 rounded-xl flex items-center justify-center ${
                          alert.severity === 'critical' ? 'bg-red-100' :
                          alert.severity === 'high' ? 'bg-orange-100' :
                          alert.severity === 'medium' ? 'bg-amber-100' :
                          'bg-gray-100'
                        }`}>
                          <AlertTriangle className={`w-5 h-5 ${
                            alert.severity === 'critical' ? 'text-red-600' :
                            alert.severity === 'high' ? 'text-orange-600' :
                            alert.severity === 'medium' ? 'text-amber-600' :
                            'text-gray-500'
                          }`} />
                        </div>
                        
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2 mb-1">
                            <span className={`${badgeClass} ${
                              alert.severity === 'critical' ? 'bg-red-100 text-red-700' :
                              alert.severity === 'high' ? 'bg-orange-100 text-orange-700' :
                              alert.severity === 'medium' ? 'bg-amber-100 text-amber-700' :
                              'bg-gray-100 text-gray-600'
                            }`}>
                              {alert.severity === 'critical' ? '🔴 严重' : 
                               alert.severity === 'high' ? '🟠 高' : 
                               alert.severity === 'medium' ? '🟡 中' : '🟢 低'}
                            </span>
                            <span className="text-[10px] text-gray-400">{formatTimeAgo(alert.createdAt)}</span>
                            {!alert.read && <span className="w-2 h-2 bg-red-500 rounded-full animate-pulse" />}
                          </div>
                          <h3 className="text-sm font-semibold text-gray-900 mb-1 group-hover:text-blue-600 transition-colors">{alert.title}</h3>
                          <p className="text-xs text-gray-500 line-clamp-2">{alert.summary}</p>
                        </div>
                        
                        {/* 🆕 Alert Actions */}
                        <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                          <button
                            onClick={(e) => {
                              e.stopPropagation()
                              setAlerts(prev => prev.map(a => a.id === alert.id ? {...a, read: true} : a))
                            }}
                            className="p-1.5 rounded hover:bg-gray-100"
                            title="标记已读"
                          >
                            <CheckCircle className="w-4 h-4 text-gray-400" />
                          </button>
                          <button
                            onClick={(e) => {
                              e.stopPropagation()
                              setAlerts(prev => prev.filter(a => a.id !== alert.id))
                            }}
                            className="p-1.5 rounded hover:bg-gray-100"
                            title="忽略"
                          >
                            <XCircle className="w-4 h-4 text-gray-400" />
                          </button>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}
            
            {/* Entities Tab */}
            {activeTab === 'entities' && (
              <div className={cardClass}>
                <div className="p-5 border-b border-gray-100">
                  <h3 className="text-base font-semibold text-gray-900">{t('news.entities')}</h3>
                </div>
                <div className="p-5 grid grid-cols-4 gap-4">
                  {/* US */}
                  <div className="p-4 bg-blue-50 rounded-xl border border-blue-100">
                    <div className="text-xs font-semibold text-blue-800 mb-3 flex items-center gap-2">
                      <Landmark className="w-4 h-4" />
                      {t('region.us')}
                    </div>
                    <div className="space-y-2">
                      {Object.values(SOURCES).filter(s => ['whitehouse', 'treasury', 'fed', 'commerce', 'ustr', 'dod', 'sec'].includes(s.id)).slice(0, 6).map(src => (
                        <div key={src.id} className="flex items-center justify-between text-[11px] p-2 bg-white rounded-lg">
                          <span className="text-gray-700 font-medium">{src.name}</span>
                          <span className={`px-1.5 py-0.5 rounded text-[9px] font-medium ${
                            src.level === 'L0' ? 'bg-blue-200 text-blue-800' :
                            src.level === 'L0.5' ? 'bg-cyan-200 text-cyan-800' :
                            'bg-gray-200 text-gray-600'
                          }`}>{src.level}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                  
                  {/* Europe */}
                  <div className="p-4 bg-amber-50 rounded-xl border border-amber-100">
                    <div className="text-xs font-semibold text-amber-800 mb-3 flex items-center gap-2">
                      <Building2 className="w-4 h-4" />
                      {t('region.eu')}
                    </div>
                    <div className="space-y-2">
                      {Object.values(SOURCES).filter(s => ['ecb', 'eu-commission', 'bundesbank', 'boe', 'snb'].includes(s.id)).slice(0, 6).map(src => (
                        <div key={src.id} className="flex items-center justify-between text-[11px] p-2 bg-white rounded-lg">
                          <span className="text-gray-700 font-medium">{src.name}</span>
                          <span className={`px-1.5 py-0.5 rounded text-[9px] font-medium ${
                            src.level === 'L0' ? 'bg-blue-200 text-blue-800' :
                            src.level === 'L0.5' ? 'bg-cyan-200 text-cyan-800' :
                            'bg-gray-200 text-gray-600'
                          }`}>{src.level}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                  
                  {/* Asia */}
                  <div className="p-4 bg-red-50 rounded-xl border border-red-100">
                    <div className="text-xs font-semibold text-red-800 mb-3 flex items-center gap-2">
                      <Globe className="w-4 h-4" />
                      {t('region.asia')}
                    </div>
                    <div className="space-y-2">
                      {Object.values(SOURCES).filter(s => ['pboc', 'mofcom', 'ndrc', 'boj', 'meti', 'rbi'].includes(s.id)).slice(0, 6).map(src => (
                        <div key={src.id} className="flex items-center justify-between text-[11px] p-2 bg-white rounded-lg">
                          <span className="text-gray-700 font-medium">{src.name}</span>
                          <span className={`px-1.5 py-0.5 rounded text-[9px] font-medium ${
                            src.level === 'L0' ? 'bg-blue-200 text-blue-800' :
                            src.level === 'L0.5' ? 'bg-cyan-200 text-cyan-800' :
                            'bg-gray-200 text-gray-600'
                          }`}>{src.level}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                  
                  {/* International */}
                  <div className="p-4 bg-violet-50 rounded-xl border border-violet-100">
                    <div className="text-xs font-semibold text-violet-800 mb-3 flex items-center gap-2">
                      <Scale className="w-4 h-4" />
                      {t('region.intl')}
                    </div>
                    <div className="space-y-2">
                      {Object.values(SOURCES).filter(s => ['imf', 'worldbank', 'wto', 'bis', 'opec'].includes(s.id)).slice(0, 6).map(src => (
                        <div key={src.id} className="flex items-center justify-between text-[11px] p-2 bg-white rounded-lg">
                          <span className="text-gray-700 font-medium">{src.name}</span>
                          <span className={`px-1.5 py-0.5 rounded text-[9px] font-medium ${
                            src.level === 'L0' ? 'bg-blue-200 text-blue-800' :
                            src.level === 'L0.5' ? 'bg-cyan-200 text-cyan-800' :
                            'bg-gray-200 text-gray-600'
                          }`}>{src.level}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
              </div>
            )}
            
            {/* ========== STEP 2: NEW TABS ========== */}
            
            {/* Timeline Engine Tab - Policy Lifecycle Visualization */}
            {activeTab === 'timeline' && (
              <div className="space-y-6">
                {/* Timeline Header */}
                <div className={cardClass}>
                  <div className="p-5 border-b border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-indigo-500 to-purple-500 flex items-center justify-center">
                          <GitBranch className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h2 className="text-base font-semibold text-gray-900">{t('timeline.title')}</h2>
                          <p className="text-xs text-gray-500">{t('timeline.subtitle')}</p>
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-gray-400">{policyTimelines.length} {t('timeline.activePolicies')}</span>
                      </div>
                    </div>
                  </div>
                  
                  {/* Timeline Legend */}
                  <div className="px-5 py-3 bg-gray-50 border-b border-gray-100">
                    <div className="flex items-center gap-4 text-[10px]">
                      <span className="flex items-center gap-1.5"><span className="w-3 h-3 rounded-full bg-blue-500" />{t('timeline.signal')}</span>
                      <span className="flex items-center gap-1.5"><span className="w-3 h-3 rounded-full bg-amber-500" />{t('timeline.draft')}</span>
                      <span className="flex items-center gap-1.5"><span className="w-3 h-3 rounded-full bg-orange-500" />{t('timeline.review')}</span>
                      <span className="flex items-center gap-1.5"><span className="w-3 h-3 rounded-full bg-purple-500" />{t('timeline.approval')}</span>
                      <span className="flex items-center gap-1.5"><span className="w-3 h-3 rounded-full bg-cyan-500" />{t('timeline.publication')}</span>
                      <span className="flex items-center gap-1.5"><span className="w-3 h-3 rounded-full bg-emerald-500" />{t('timeline.effective')}</span>
                      <span className="flex items-center gap-1.5"><span className="w-3 h-3 rounded-full bg-red-500" />{t('timeline.enforcement')}</span>
                    </div>
                  </div>
                </div>
                
                {/* Timeline Cards */}
                <div className="space-y-4">
                  {policyTimelines.length === 0 ? (
                    <EmptyState
                      icon={<GitBranch className="w-12 h-12" />}
                      title={t('news.noTimeline')}
                      description={t('news.noTimelineDesc')}
                      isLoading={isLoadingRealData}
                    />
                  ) : policyTimelines.map(timeline => {
                    const completedNodes = timeline.nodes.filter(n => n.status === 'completed').length
                    const progress = (completedNodes / timeline.nodes.length) * 100
                    
                    return (
                      <div 
                        key={timeline.policyId}
                        className={`${cardClass} cursor-pointer hover:border-indigo-200`}
                        onClick={() => setSelectedTimeline(selectedTimeline?.policyId === timeline.policyId ? null : timeline)}
                      >
                        <div className="p-5">
                          {/* Header */}
                          <div className="flex items-start justify-between mb-4">
                            <div className="flex-1">
                              <div className="flex items-center gap-2 mb-1">
                                <span className={`${badgeClass} ${
                                  timeline.jurisdiction === 'US' ? 'bg-blue-100 text-blue-700' :
                                  timeline.jurisdiction === 'EU' ? 'bg-amber-100 text-amber-700' :
                                  timeline.jurisdiction === 'CN' ? 'bg-red-100 text-red-700' :
                                  'bg-gray-100 text-gray-600'
                                }`}>
                                  {timeline.jurisdiction}
                                </span>
                                <span className={`${badgeClass} ${
                                  timeline.domain === 'trade' ? 'bg-orange-100 text-orange-700' :
                                  timeline.domain === 'regulation' ? 'bg-purple-100 text-purple-700' :
                                  timeline.domain === 'export_control' ? 'bg-red-100 text-red-700' :
                                  'bg-gray-100 text-gray-600'
                                }`}>
                                  {timeline.domain}
                                </span>
                                {timeline.hasExecutionAuthority && (
                                  <span className={`${badgeClass} bg-emerald-100 text-emerald-700`}>
                                    <Shield className="w-3 h-3 inline mr-0.5" />
                                    {t('timeline.hasAuthority')}
                                  </span>
                                )}
                              </div>
                              <h3 className="text-sm font-semibold text-gray-900">{timeline.policyName}</h3>
                            </div>
                            <div className="text-right">
                              {timeline.effectiveDate && (
                                <div className="text-xs text-emerald-600 font-medium">
                                  {t('timeline.effectiveDate')}: {new Date(timeline.effectiveDate).toLocaleDateString()}
                                </div>
                              )}
                              {timeline.estimatedEffectiveDate && !timeline.effectiveDate && (
                                <div className="text-xs text-amber-600 font-medium">
                                  {t('timeline.estimatedDate')}: {new Date(timeline.estimatedEffectiveDate).toLocaleDateString()}
                                </div>
                              )}
                            </div>
                          </div>
                          
                          {/* Progress Bar */}
                          <div className="mb-4">
                            <div className="flex items-center justify-between text-[10px] mb-1">
                              <span className="text-gray-500">{t('timeline.progress')}</span>
                              <span className="text-gray-700 font-medium">{completedNodes}/{timeline.nodes.length} {t('timeline.stages')}</span>
                            </div>
                            <div className="h-2 bg-gray-100 rounded-full overflow-hidden">
                              <div 
                                className="h-full bg-gradient-to-r from-indigo-500 to-purple-500 rounded-full transition-all"
                                style={{ width: `${progress}%` }}
                              />
                            </div>
                          </div>
                          
                          {/* Visual Timeline */}
                          <div className="relative">
                            <div className="absolute top-4 left-0 right-0 h-0.5 bg-gray-200" />
                            <div className="flex items-start justify-between relative">
                              {timeline.nodes.map((node, idx) => {
                                const nodeColors: Record<TimelineNodeType, string> = {
                                  signal: 'bg-blue-500',
                                  draft: 'bg-amber-500',
                                  committee_review: 'bg-orange-500',
                                  approval: 'bg-purple-500',
                                  publication: 'bg-cyan-500',
                                  effective: 'bg-emerald-500',
                                  enforcement: 'bg-red-500',
                                  amendment: 'bg-pink-500',
                                  sunset: 'bg-gray-500'
                                }
                                const statusStyles: Record<TimelineNodeStatus, string> = {
                                  completed: 'ring-2 ring-offset-2',
                                  current: 'ring-4 ring-offset-2 animate-pulse',
                                  pending: 'opacity-50',
                                  skipped: 'opacity-30 line-through'
                                }
                                
                                return (
                                  <div key={node.id} className="flex flex-col items-center" style={{ flex: 1 }}>
                                    <div className={`w-8 h-8 rounded-full ${nodeColors[node.type]} ${statusStyles[node.status]} flex items-center justify-center z-10`}>
                                      {node.status === 'completed' ? (
                                        <CheckCircle className="w-4 h-4 text-white" />
                                      ) : node.status === 'current' ? (
                                        <Clock className="w-4 h-4 text-white" />
                                      ) : (
                                        <span className="text-[10px] text-white font-bold">{idx + 1}</span>
                                      )}
                                    </div>
                                    <div className="mt-2 text-center">
                                      <div className="text-[9px] font-medium text-gray-700 capitalize">{node.type.replace('_', ' ')}</div>
                                      {node.completedAt && (
                                        <div className="text-[8px] text-gray-400">{new Date(node.completedAt).toLocaleDateString()}</div>
                                      )}
                                    </div>
                                  </div>
                                )
                              })}
                            </div>
                          </div>
                          
                          {/* Risk Indicators */}
                          {(timeline.delayRisk || timeline.reversalRisk) && (
                            <div className="mt-4 pt-4 border-t border-gray-100 flex items-center gap-4">
                              {timeline.delayRisk && (
                                <div className="flex items-center gap-1.5">
                                  <Clock className="w-3.5 h-3.5 text-amber-500" />
                                  <span className="text-[10px] text-amber-600">{t('timeline.delayRisk')}: {(timeline.delayRisk * 100).toFixed(0)}%</span>
                                </div>
                              )}
                              {timeline.reversalRisk && (
                                <div className="flex items-center gap-1.5">
                                  <AlertTriangle className="w-3.5 h-3.5 text-red-500" />
                                  <span className="text-[10px] text-red-600">{t('timeline.reversalRisk')}: {(timeline.reversalRisk * 100).toFixed(0)}%</span>
                                </div>
                              )}
                            </div>
                          )}
                        </div>
                        
                        {/* Expanded Detail View */}
                        {selectedTimeline?.policyId === timeline.policyId && (
                          <div className="border-t border-gray-100 bg-gray-50 p-5">
                            <h4 className="text-xs font-semibold text-gray-700 mb-3">{t('timeline.nodeDetails')}</h4>
                            <div className="space-y-3">
                              {timeline.nodes.map(node => (
                                <div key={node.id} className="bg-white rounded-lg p-3 border border-gray-100">
                                  <div className="flex items-start justify-between mb-2">
                                    <div className="flex items-center gap-2">
                                      <span className={`${badgeClass} capitalize ${
                                        node.status === 'completed' ? 'bg-emerald-100 text-emerald-700' :
                                        node.status === 'current' ? 'bg-blue-100 text-blue-700' :
                                        'bg-gray-100 text-gray-600'
                                      }`}>
                                        {node.type.replace('_', ' ')}
                                      </span>
                                      <span className={`${badgeClass} ${
                                        node.status === 'completed' ? 'bg-emerald-50 text-emerald-600' :
                                        node.status === 'current' ? 'bg-blue-50 text-blue-600' :
                                        'bg-gray-50 text-gray-500'
                                      }`}>
                                        {node.status}
                                      </span>
                                    </div>
                                    {node.sourceEvidence && (
                                      <a 
                                        href={node.sourceEvidence.url}
                                        target="_blank"
                                        rel="noopener noreferrer"
                                        className="text-[10px] text-blue-600 hover:underline"
                                        onClick={e => e.stopPropagation()}
                                      >
                                        {node.sourceEvidence.sourceName} �?
                                      </a>
                                    )}
                                  </div>
                                  {node.description && (
                                    <p className="text-xs text-gray-600 mb-2">{node.description}</p>
                                  )}
                                  {node.sourceEvidence && (
                                    <div className="bg-gray-50 rounded p-2 text-[10px] text-gray-500 italic">
                                      "{node.sourceEvidence.text}"
                                    </div>
                                  )}
                                </div>
                              ))}
                            </div>
                          </div>
                        )}
                      </div>
                    )
                  })}
                </div>
              </div>
            )}
            
            {/* List Diff Engine Tab - Sanctions/Entity/Tariff Changes */}
            {activeTab === 'listdiff' && (
              <div className="space-y-6">
                {/* List Diff Header */}
                <div className={cardClass}>
                  <div className="p-5 border-b border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-red-500 to-pink-500 flex items-center justify-center">
                          <List className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h2 className="text-base font-semibold text-gray-900">{t('listChanges.title')}</h2>
                          <p className="text-xs text-gray-500">{t('listChanges.subtitle')}</p>
                        </div>
                      </div>
                      <div className="flex items-center gap-3">
                        <div className="flex items-center gap-2">
                          <span className="flex items-center gap-1 px-2 py-1 bg-red-50 rounded-lg text-[10px] text-red-600 font-medium">
                            <Plus className="w-3 h-3" />
                            {listDiffReports.reduce((sum, r) => sum + r.changes.filter(c => c.changeType === 'add').length, 0)} {t('listChanges.added')}
                          </span>
                          <span className="flex items-center gap-1 px-2 py-1 bg-amber-50 rounded-lg text-[10px] text-amber-600 font-medium">
                            <Edit3 className="w-3 h-3" />
                            {listDiffReports.reduce((sum, r) => sum + r.changes.filter(c => c.changeType === 'modify').length, 0)} {t('listChanges.modified')}
                          </span>
                          <span className="flex items-center gap-1 px-2 py-1 bg-emerald-50 rounded-lg text-[10px] text-emerald-600 font-medium">
                            <Minus className="w-3 h-3" />
                            {listDiffReports.reduce((sum, r) => sum + r.changes.filter(c => c.changeType === 'remove').length, 0)} {t('listChanges.removed')}
                          </span>
                        </div>
                      </div>
                    </div>
                  </div>
                  
                  {/* List Type Legend */}
                  <div className="px-5 py-3 bg-gray-50">
                    <div className="flex items-center gap-4 text-[10px]">
                      <span className="flex items-center gap-1.5 px-2 py-1 bg-red-100 rounded text-red-700">SDN List</span>
                      <span className="flex items-center gap-1.5 px-2 py-1 bg-orange-100 rounded text-orange-700">Entity List</span>
                      <span className="flex items-center gap-1.5 px-2 py-1 bg-amber-100 rounded text-amber-700">Tariff Schedule</span>
                      <span className="flex items-center gap-1.5 px-2 py-1 bg-purple-100 rounded text-purple-700">Export Control</span>
                      <span className="flex items-center gap-1.5 px-2 py-1 bg-blue-100 rounded text-blue-700">EU Sanctions</span>
                    </div>
                  </div>
                </div>
                
                {/* List Diff Reports */}
                <div className="space-y-4">
                  {listDiffReports.length === 0 ? (
                    <EmptyState
                      icon={<List className="w-12 h-12" />}
                      title={t('news.noListChanges')}
                      description={t('news.noListChangesDesc')}
                      isLoading={isLoadingRealData}
                    />
                  ) : listDiffReports.map(report => (
                    <div 
                      key={report.reportId}
                      className={`${cardClass} cursor-pointer hover:border-red-200`}
                      onClick={() => setSelectedListReport(selectedListReport?.reportId === report.reportId ? null : report)}
                    >
                      <div className="p-5">
                        {/* Header */}
                        <div className="flex items-start justify-between mb-4">
                          <div className="flex-1">
                            <div className="flex items-center gap-2 mb-1">
                              <span className={`${badgeClass} ${
                                report.listType === 'sdn' ? 'bg-red-100 text-red-700' :
                                report.listType === 'entity' ? 'bg-orange-100 text-orange-700' :
                                report.listType === 'tariff' ? 'bg-amber-100 text-amber-700' :
                                report.listType === 'export_control' ? 'bg-purple-100 text-purple-700' :
                                'bg-blue-100 text-blue-700'
                              }`}>
                                {report.listType.toUpperCase().replace('_', ' ')}
                              </span>
                              <span className={`${badgeClass} ${
                                report.jurisdiction === 'US' ? 'bg-blue-100 text-blue-700' :
                                report.jurisdiction === 'EU' ? 'bg-amber-100 text-amber-700' :
                                'bg-gray-100 text-gray-600'
                              }`}>
                                {report.jurisdiction}
                              </span>
                              <span className="text-[10px] text-gray-500">{report.sourceAgency}</span>
                            </div>
                            <h3 className="text-sm font-semibold text-gray-900">{report.listName}</h3>
                          </div>
                          <div className="text-right">
                            <div className="text-xs text-gray-500">
                              {t('listChanges.published')}: {new Date(report.publishedAt).toLocaleDateString()}
                            </div>
                            {report.effectiveDate && (
                              <div className="text-xs text-emerald-600 font-medium">
                                {t('listChanges.effective')}: {new Date(report.effectiveDate).toLocaleDateString()}
                              </div>
                            )}
                          </div>
                        </div>
                        
                        {/* Change Summary */}
                        <div className="grid grid-cols-4 gap-3 mb-4">
                          <div className="p-3 bg-red-50 rounded-lg border border-red-100 text-center">
                            <div className="text-2xl font-bold text-red-600">
                              {report.changes.filter(c => c.changeType === 'add').length}
                            </div>
                            <div className="text-[10px] text-red-500 font-medium">{t('listChanges.added')}</div>
                          </div>
                          <div className="p-3 bg-amber-50 rounded-lg border border-amber-100 text-center">
                            <div className="text-2xl font-bold text-amber-600">
                              {report.changes.filter(c => c.changeType === 'modify').length}
                            </div>
                            <div className="text-[10px] text-amber-500 font-medium">{t('listChanges.modified')}</div>
                          </div>
                          <div className="p-3 bg-emerald-50 rounded-lg border border-emerald-100 text-center">
                            <div className="text-2xl font-bold text-emerald-600">
                              {report.changes.filter(c => c.changeType === 'remove').length}
                            </div>
                            <div className="text-[10px] text-emerald-500 font-medium">{t('listChanges.removed')}</div>
                          </div>
                          <div className="p-3 bg-gray-50 rounded-lg border border-gray-100 text-center">
                            <div className="text-2xl font-bold text-gray-600">
                              {report.changes.length}
                            </div>
                            <div className="text-[10px] text-gray-500 font-medium">{t('listChanges.total')}</div>
                          </div>
                        </div>
                        
                        {/* High Impact Changes Preview */}
                        {report.highImpactChanges.length > 0 && (
                          <div className="mb-4 p-3 bg-red-50 rounded-lg border border-red-200">
                            <div className="flex items-center gap-2 mb-2">
                              <AlertTriangle className="w-4 h-4 text-red-500" />
                              <span className="text-xs font-semibold text-red-700">{t('listChanges.highImpact')}</span>
                            </div>
                            <div className="space-y-1">
                              {report.highImpactChanges.slice(0, 3).map((change, idx) => (
                                <div key={idx} className="text-[11px] text-red-600">
                                  •{change.entity?.entityName || change.entity?.htsCode || change.id}
                                </div>
                              ))}
                            </div>
                          </div>
                        )}
                        
                        {/* Suggested Basket */}
                        {report.suggestedBasket && report.suggestedBasket.length > 0 && (
                          <div className="flex flex-wrap gap-2">
                            {report.suggestedBasket.slice(0, 5).map((item, idx) => (
                              <span 
                                key={idx}
                                className={`text-[10px] px-2 py-1 rounded-lg font-medium ${
                                  item.direction === 'bearish' ? 'bg-red-100 text-red-700' : 'bg-emerald-100 text-emerald-700'
                                }`}
                              >
                                {item.direction === 'bearish' ? '↓' : '↑'} {item.ticker} ({(item.weight * 100).toFixed(0)}%)
                              </span>
                            ))}
                          </div>
                        )}
                      </div>
                      
                      {/* Expanded Detail View */}
                      {selectedListReport?.reportId === report.reportId && (
                        <div className="border-t border-gray-100 bg-gray-50 p-5">
                          <h4 className="text-xs font-semibold text-gray-700 mb-3">{t('listChanges.allChanges')}</h4>
                          <div className="space-y-2 max-h-96 overflow-auto">
                            {report.changes.map(change => (
                              <div 
                                key={change.id}
                                className={`bg-white rounded-lg p-3 border ${
                                  change.changeType === 'add' ? 'border-l-4 border-l-red-500' :
                                  change.changeType === 'remove' ? 'border-l-4 border-l-emerald-500' :
                                  'border-l-4 border-l-amber-500'
                                }`}
                              >
                                <div className="flex items-start justify-between mb-2">
                                  <div className="flex items-center gap-2">
                                    <span className={`${badgeClass} ${
                                      change.changeType === 'add' ? 'bg-red-100 text-red-700' :
                                      change.changeType === 'remove' ? 'bg-emerald-100 text-emerald-700' :
                                      'bg-amber-100 text-amber-700'
                                    }`}>
                                      {change.changeType.toUpperCase()}
                                    </span>
                                    <span className="text-xs font-medium text-gray-800">
                                      {change.entity?.entityName || change.entity?.id || 'Unknown Entity'}
                                    </span>
                                  </div>
                                  <span className="text-[10px] text-gray-400">
                                    {new Date(change.timestamp).toLocaleString()}
                                  </span>
                                </div>
                                {change.entity?.reason && (
                                  <div className="text-[10px] text-gray-500 mb-1">
                                    <span className="font-medium">{t('listChanges.program')}:</span> {change.entity.reason}
                                  </div>
                                )}
                                {change.entity?.country && (
                                  <div className="text-[10px] text-gray-500 mb-1">
                                    <span className="font-medium">{t('listChanges.country')}:</span> {change.entity.country}
                                  </div>
                                )}
                                {change.previousState && (
                                  <div className="mt-2 p-2 bg-gray-50 rounded text-[10px]">
                                    <div className="text-red-500 line-through">Previous: {JSON.stringify(change.previousState).substring(0, 100)}</div>
                                    <div className="text-emerald-600">Current: {change.entity?.entityName}</div>
                                  </div>
                                )}
                                {change.affectedTickers && change.affectedTickers.length > 0 && (
                                  <div className="mt-2 flex flex-wrap gap-1">
                                    {change.affectedTickers.map((ticker, i) => (
                                      <span key={i} className="text-[9px] px-1.5 py-0.5 bg-blue-100 text-blue-700 rounded">
                                        {ticker}
                                      </span>
                                    ))}
                                  </div>
                                )}
                              </div>
                            ))}
                          </div>
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              </div>
            )}
            
            {/* Divergence Detection Tab - Multi-Jurisdiction Analysis */}
            {activeTab === 'divergence' && (
              <div className="space-y-6">
                {/* Divergence Header */}
                <div className={cardClass}>
                  <div className="p-5 border-b border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-cyan-500 to-teal-500 flex items-center justify-center">
                          <GitCompare className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h2 className="text-base font-semibold text-gray-900">{t('divergence.title')}</h2>
                          <p className="text-xs text-gray-500">{t('divergence.subtitle')}</p>
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        <span className={`${badgeClass} bg-cyan-100 text-cyan-700`}>
                          {jurisdictionDivergences.filter(d => d.arbitrageWindow).length} {t('divergence.arbitrageWindows')}
                        </span>
                      </div>
                    </div>
                  </div>
                  
                  {/* Jurisdiction Status */}
                  <div className="px-5 py-3 bg-gray-50 flex items-center gap-4">
                    {['US', 'EU', 'CN', 'JP', 'UK'].map(jurisdiction => {
                      const count = jurisdictionDivergences.filter(d => 
                        d.states.some(s => s.jurisdiction === jurisdiction)
                      ).length
                      return (
                        <div key={jurisdiction} className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg ${
                          jurisdiction === 'US' ? 'bg-blue-100' :
                          jurisdiction === 'EU' ? 'bg-amber-100' :
                          jurisdiction === 'CN' ? 'bg-red-100' :
                          jurisdiction === 'JP' ? 'bg-pink-100' :
                          'bg-violet-100'
                        }`}>
                          <span className="text-xs font-semibold">{jurisdiction}</span>
                          <span className="text-[10px] text-gray-500">{count}</span>
                        </div>
                      )
                    })}
                  </div>
                </div>
                
                {/* Divergence Cards */}
                <div className="space-y-4">
                  {jurisdictionDivergences.length === 0 ? (
                    <EmptyState
                      icon={<GitCompare className="w-12 h-12" />}
                      title={t('news.noDivergence')}
                      description={t('news.noDivergenceDesc')}
                      isLoading={isLoadingRealData}
                    />
                  ) : jurisdictionDivergences.map(divergence => (
                    <div 
                      key={divergence.divergenceId}
                      className={`${cardClass} cursor-pointer hover:border-cyan-200`}
                      onClick={() => setSelectedDivergence(selectedDivergence?.divergenceId === divergence.divergenceId ? null : divergence)}
                    >
                      <div className="p-5">
                        {/* Header */}
                        <div className="flex items-start justify-between mb-4">
                          <div className="flex-1">
                            <div className="flex items-center gap-2 mb-1">
                              <span className={`${badgeClass} ${
                                divergence.divergenceSeverity === 'critical' ? 'bg-red-100 text-red-700' :
                                divergence.divergenceSeverity === 'high' ? 'bg-orange-100 text-orange-700' :
                                divergence.divergenceSeverity === 'medium' ? 'bg-amber-100 text-amber-700' :
                                'bg-gray-100 text-gray-600'
                              }`}>
                                {divergence.divergenceSeverity.toUpperCase()} {t('divergence.severity')}
                              </span>
                              <span className={`${badgeClass} bg-cyan-100 text-cyan-700`}>
                                {divergence.divergenceType}
                              </span>
                            </div>
                            <h3 className="text-sm font-semibold text-gray-900">{divergence.policyTopic}</h3>
                          </div>
                          {divergence.arbitrageWindow && (
                            <div className="p-2 bg-emerald-50 rounded-lg border border-emerald-200">
                              <div className="text-[10px] text-emerald-600 font-medium">{t('divergence.arbitrageOpen')}</div>
                              <div className="text-xs font-semibold text-emerald-700">
                                {divergence.estimatedCatchUpDays} {t('common.days')}
                              </div>
                            </div>
                          )}
                        </div>
                        
                        {/* Jurisdiction States Comparison */}
                        <div className="grid grid-cols-3 gap-3 mb-4">
                          {divergence.states.map(state => (
                            <div 
                              key={state.jurisdiction}
                              className={`p-3 rounded-lg border ${
                                state.jurisdiction === divergence.leadingJurisdiction 
                                  ? 'bg-emerald-50 border-emerald-200' 
                                  : divergence.laggingJurisdictions.includes(state.jurisdiction)
                                    ? 'bg-amber-50 border-amber-200'
                                    : 'bg-gray-50 border-gray-200'
                              }`}
                            >
                              <div className="flex items-center justify-between mb-2">
                                <span className={`text-xs font-bold ${
                                  state.jurisdiction === 'US' ? 'text-blue-700' :
                                  state.jurisdiction === 'EU' ? 'text-amber-700' :
                                  state.jurisdiction === 'CN' ? 'text-red-700' :
                                  state.jurisdiction === 'JP' ? 'text-pink-700' :
                                  'text-gray-700'
                                }`}>
                                  {state.jurisdiction}
                                </span>
                                {state.jurisdiction === divergence.leadingJurisdiction && (
                                  <span className="text-[9px] px-1.5 py-0.5 bg-emerald-200 text-emerald-800 rounded font-medium">
                                    {t('divergence.leading')}
                                  </span>
                                )}
                              </div>
                              <div className={`text-[10px] px-2 py-1 rounded ${
                                state.policyState === 'implementing' ? 'bg-violet-100 text-violet-700' :
                                state.policyState === 'effective' ? 'bg-emerald-100 text-emerald-700' :
                                state.policyState === 'negotiating' ? 'bg-amber-100 text-amber-700' :
                                state.policyState === 'emerging' ? 'bg-blue-100 text-blue-700' :
                                'bg-gray-100 text-gray-600'
                              }`}>
                                {state.policyState}
                              </div>
                              <div className="mt-2 text-[9px] text-gray-500">
                                {t('divergence.enforcement')}: {state.enforcementLevel}
                              </div>
                              {state.effectiveDate && (
                                <div className="text-[9px] text-emerald-600">
                                  {t('timeline.effective')}: {new Date(state.effectiveDate).toLocaleDateString()}
                                </div>
                              )}
                            </div>
                          ))}
                        </div>
                        
                        {/* Catch-up Probability */}
                        <div className="flex items-center gap-4 mb-4">
                          <div className="flex-1">
                            <div className="flex items-center justify-between text-[10px] mb-1">
                              <span className="text-gray-500">{t('divergence.catchUpProbability')}</span>
                              <span className="text-gray-700 font-medium">{(divergence.catchUpProbability * 100).toFixed(0)}%</span>
                            </div>
                            <div className="h-2 bg-gray-100 rounded-full overflow-hidden">
                              <div 
                                className="h-full bg-gradient-to-r from-cyan-500 to-teal-500 rounded-full"
                                style={{ width: `${divergence.catchUpProbability * 100}%` }}
                              />
                            </div>
                          </div>
                          <div className="text-right">
                            <div className="text-[10px] text-gray-500">{t('divergence.estimatedDays')}</div>
                            <div className="text-lg font-bold text-cyan-600">{divergence.estimatedCatchUpDays}</div>
                          </div>
                        </div>
                        
                        {/* Arbitrage Window Details */}
                        {divergence.arbitrageWindow && (
                          <div className="p-3 bg-emerald-50 rounded-lg border border-emerald-200">
                            <div className="flex items-center gap-2 mb-2">
                              <TrendingUp className="w-4 h-4 text-emerald-600" />
                              <span className="text-xs font-semibold text-emerald-700">{t('divergence.tradingOpportunity')}</span>
                              <span className={`ml-auto text-[10px] px-2 py-0.5 rounded ${
                                divergence.arbitrageWindow.riskLevel === 'low' ? 'bg-emerald-200 text-emerald-800' :
                                divergence.arbitrageWindow.riskLevel === 'medium' ? 'bg-amber-200 text-amber-800' :
                                'bg-red-200 text-red-800'
                              }`}>
                                {divergence.arbitrageWindow.riskLevel} {t('common.risk')}
                              </span>
                            </div>
                            <p className="text-xs text-emerald-700">{divergence.arbitrageWindow.suggestedStrategy}</p>
                          </div>
                        )}
                      </div>
                      
                      {/* Expanded Detail View */}
                      {selectedDivergence?.divergenceId === divergence.divergenceId && divergence.historicalPrecedents && (
                        <div className="border-t border-gray-100 bg-gray-50 p-5">
                          <h4 className="text-xs font-semibold text-gray-700 mb-3">{t('divergence.historicalPrecedents')}</h4>
                          <div className="space-y-2">
                            {divergence.historicalPrecedents.map(precedent => (
                              <div key={precedent.caseId} className="bg-white rounded-lg p-3 border border-gray-100">
                                <div className="text-xs font-medium text-gray-800 mb-1">{precedent.description}</div>
                                <div className="text-[10px] text-gray-500">{t('common.outcome')}: {precedent.outcome}</div>
                                <div className="text-[10px] text-gray-400">{t('common.duration')}: {precedent.duration} {t('common.days')}</div>
                              </div>
                            ))}
                          </div>
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              </div>
            )}
            
            {/* Execution Power Map Tab */}
            {activeTab === 'execution' && (
              <div className="space-y-6">
                {/* Execution Power Header */}
                <div className={cardClass}>
                  <div className="p-5 border-b border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-violet-500 to-purple-500 flex items-center justify-center">
                          <Shield className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h2 className="text-base font-semibold text-gray-900">{t('executionPower.title')}</h2>
                          <p className="text-xs text-gray-500">{t('executionPower.subtitle')}</p>
                        </div>
                      </div>
                    </div>
                  </div>
                  
                  {/* Power Level Legend */}
                  <div className="px-5 py-3 bg-gray-50">
                    <div className="flex items-center gap-4 text-[10px]">
                      <span className="flex items-center gap-1.5"><span className="w-3 h-3 rounded bg-emerald-500" />{t('executionPower.legislative')}</span>
                      <span className="flex items-center gap-1.5"><span className="w-3 h-3 rounded bg-blue-500" />{t('executionPower.executive')}</span>
                      <span className="flex items-center gap-1.5"><span className="w-3 h-3 rounded bg-amber-500" />{t('executionPower.regulatory')}</span>
                      <span className="flex items-center gap-1.5"><span className="w-3 h-3 rounded bg-gray-500" />{t('executionPower.advisory')}</span>
                    </div>
                  </div>
                </div>
                
                {/* Execution Agencies by Jurisdiction */}
                <div className="grid grid-cols-2 gap-4">
                  {/* US Agencies */}
                  <div className={cardClass}>
                    <div className="p-4 border-b border-gray-100 bg-blue-50">
                      <div className="flex items-center gap-2">
                        <Landmark className="w-5 h-5 text-blue-600" />
                        <span className="text-sm font-semibold text-blue-800">{t('region.us')} {t('executionPower.agencies')}</span>
                      </div>
                    </div>
                    <div className="p-4 space-y-3">
                      {Object.values(SOURCES)
                        .filter(s => s.region === 'US' && ['L0', 'L0.5'].includes(s.level))
                        .slice(0, 8)
                        .map(src => (
                          <div key={src.id} className="flex items-center justify-between p-2 bg-gray-50 rounded-lg">
                            <div className="flex items-center gap-2">
                              <div className={`w-2 h-6 rounded ${
                                src.level === 'L0' ? 'bg-emerald-500' : 'bg-blue-500'
                              }`} />
                              <div>
                                <div className="text-xs font-medium text-gray-800">{src.name}</div>
                                <div className="text-[9px] text-gray-500">{src.category}</div>
                              </div>
                            </div>
                            <span className={`text-[9px] px-1.5 py-0.5 rounded font-medium ${
                              src.level === 'L0' ? 'bg-blue-100 text-blue-700' : 'bg-cyan-100 text-cyan-700'
                            }`}>
                              {src.level}
                            </span>
                          </div>
                        ))}
                    </div>
                  </div>
                  
                  {/* EU Agencies */}
                  <div className={cardClass}>
                    <div className="p-4 border-b border-gray-100 bg-amber-50">
                      <div className="flex items-center gap-2">
                        <Building2 className="w-5 h-5 text-amber-600" />
                        <span className="text-sm font-semibold text-amber-800">{t('region.eu')} {t('executionPower.agencies')}</span>
                      </div>
                    </div>
                    <div className="p-4 space-y-3">
                      {Object.values(SOURCES)
                        .filter(s => s.region === 'EU' && ['L0', 'L0.5'].includes(s.level))
                        .slice(0, 8)
                        .map(src => (
                          <div key={src.id} className="flex items-center justify-between p-2 bg-gray-50 rounded-lg">
                            <div className="flex items-center gap-2">
                              <div className={`w-2 h-6 rounded ${
                                src.level === 'L0' ? 'bg-emerald-500' : 'bg-blue-500'
                              }`} />
                              <div>
                                <div className="text-xs font-medium text-gray-800">{src.name}</div>
                                <div className="text-[9px] text-gray-500">{src.category}</div>
                              </div>
                            </div>
                            <span className={`text-[9px] px-1.5 py-0.5 rounded font-medium ${
                              src.level === 'L0' ? 'bg-blue-100 text-blue-700' : 'bg-cyan-100 text-cyan-700'
                            }`}>
                              {src.level}
                            </span>
                          </div>
                        ))}
                    </div>
                  </div>
                  
                  {/* China Agencies */}
                  <div className={cardClass}>
                    <div className="p-4 border-b border-gray-100 bg-red-50">
                      <div className="flex items-center gap-2">
                        <Building2 className="w-5 h-5 text-red-600" />
                        <span className="text-sm font-semibold text-red-800">{t('region.china')} {t('executionPower.agencies')}</span>
                      </div>
                    </div>
                    <div className="p-4 space-y-3">
                      {Object.values(SOURCES)
                        .filter(s => s.region === 'CN' && ['L0', 'L0.5'].includes(s.level))
                        .slice(0, 8)
                        .map(src => (
                          <div key={src.id} className="flex items-center justify-between p-2 bg-gray-50 rounded-lg">
                            <div className="flex items-center gap-2">
                              <div className={`w-2 h-6 rounded ${
                                src.level === 'L0' ? 'bg-emerald-500' : 'bg-blue-500'
                              }`} />
                              <div>
                                <div className="text-xs font-medium text-gray-800">{src.name}</div>
                                <div className="text-[9px] text-gray-500">{src.category}</div>
                              </div>
                            </div>
                            <span className={`text-[9px] px-1.5 py-0.5 rounded font-medium ${
                              src.level === 'L0' ? 'bg-blue-100 text-blue-700' : 'bg-cyan-100 text-cyan-700'
                            }`}>
                              {src.level}
                            </span>
                          </div>
                        ))}
                    </div>
                  </div>
                  
                  {/* International Bodies */}
                  <div className={cardClass}>
                    <div className="p-4 border-b border-gray-100 bg-violet-50">
                      <div className="flex items-center gap-2">
                        <Globe className="w-5 h-5 text-violet-600" />
                        <span className="text-sm font-semibold text-violet-800">{t('region.intl')} {t('executionPower.bodies')}</span>
                      </div>
                    </div>
                    <div className="p-4 space-y-3">
                      {Object.values(SOURCES)
                        .filter(s => s.region === 'INTL' && ['L0', 'L0.5'].includes(s.level))
                        .slice(0, 8)
                        .map(src => (
                          <div key={src.id} className="flex items-center justify-between p-2 bg-gray-50 rounded-lg">
                            <div className="flex items-center gap-2">
                              <div className={`w-2 h-6 rounded ${
                                src.level === 'L0' ? 'bg-emerald-500' : 'bg-blue-500'
                              }`} />
                              <div>
                                <div className="text-xs font-medium text-gray-800">{src.name}</div>
                                <div className="text-[9px] text-gray-500">{src.category}</div>
                              </div>
                            </div>
                            <span className={`text-[9px] px-1.5 py-0.5 rounded font-medium ${
                              src.level === 'L0' ? 'bg-blue-100 text-blue-700' : 'bg-cyan-100 text-cyan-700'
                            }`}>
                              {src.level}
                            </span>
                          </div>
                        ))}
                    </div>
                  </div>
                </div>
                
                {/* 🆕 Domain Impact Heatmap */}
                <div className={cardClass}>
                  <div className="p-4 border-b border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-orange-500 to-red-500 flex items-center justify-center">
                          <Layers className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h3 className="text-sm font-semibold text-gray-900">{t('news.domainImpact') || 'Domain Impact Heatmap'}</h3>
                          <p className="text-[10px] text-gray-500">{t('news.crossDomainAnalysis') || 'News distribution by policy domain and authority level'}</p>
                        </div>
                      </div>
                    </div>
                  </div>
                  <div className="p-4">
                    <div className="grid grid-cols-9 gap-1 text-center">
                      {/* Header Row */}
                      <div className="p-2 text-[9px] font-semibold text-gray-500">Level</div>
                      {(['trade', 'sanction', 'rate', 'fiscal', 'regulation', 'war', 'antitrust', 'export_control'] as const).map(domain => (
                        <div key={domain} className="p-2 text-[9px] font-semibold text-gray-600 truncate">
                          {domain === 'export_control' ? 'Export' : domain.charAt(0).toUpperCase() + domain.slice(1)}
                        </div>
                      ))}
                      
                      {/* Data Rows */}
                      {(['L0', 'L0.5', 'L1', 'L2'] as const).map(level => (
                        <>
                          <div key={level} className={`p-2 text-[10px] font-bold ${
                            level === 'L0' ? 'text-red-700 bg-red-50' :
                            level === 'L0.5' ? 'text-orange-700 bg-orange-50' :
                            level === 'L1' ? 'text-blue-700 bg-blue-50' :
                            'text-gray-600 bg-gray-50'
                          } rounded-l`}>{level}</div>
                          {(['trade', 'sanction', 'rate', 'fiscal', 'regulation', 'war', 'antitrust', 'export_control'] as const).map(domain => {
                            const count = execPowerNews.filter(n => 
                              n.level === level && n.domains.includes(domain)
                            ).length
                            const maxCount = 10  // for normalization
                            const intensity = Math.min(count / maxCount, 1)
                            return (
                              <div 
                                key={domain}
                                className={`p-2 text-[10px] font-medium rounded ${
                                  count === 0 ? 'bg-gray-100 text-gray-400' :
                                  intensity >= 0.7 ? 'bg-red-400 text-white' :
                                  intensity >= 0.4 ? 'bg-orange-300 text-orange-900' :
                                  intensity >= 0.2 ? 'bg-amber-200 text-amber-800' :
                                  'bg-yellow-100 text-yellow-700'
                                }`}
                              >
                                {count > 0 ? count : '–'}
                              </div>
                            )
                          })}
                        </>
                      ))}
                    </div>
                    <div className="mt-3 flex items-center justify-center gap-4 text-[9px] text-gray-500">
                      <span className="flex items-center gap-1"><span className="w-3 h-3 rounded bg-red-400"></span>High Activity</span>
                      <span className="flex items-center gap-1"><span className="w-3 h-3 rounded bg-orange-300"></span>Medium</span>
                      <span className="flex items-center gap-1"><span className="w-3 h-3 rounded bg-amber-200"></span>Low</span>
                      <span className="flex items-center gap-1"><span className="w-3 h-3 rounded bg-gray-100"></span>None</span>
                    </div>
                  </div>
                </div>
                
                {/* 🆕 Executive Power Live Feed - Full View */}
                <div className={cardClass}>
                  <div className="p-4 border-b border-gray-100">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-violet-600 to-purple-600 flex items-center justify-center">
                          <Newspaper className="w-5 h-5 text-white" />
                        </div>
                        <div>
                          <h3 className="text-sm font-semibold text-gray-900">{t('news.liveNewsFeed') || 'Live News from Executive Sources'}</h3>
                          <p className="text-[10px] text-gray-500">{t('news.realTimeFromPowerGraph') || 'Real-time updates from L0/L0.5/L1 sources'}</p>
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        {/* Level Filter */}
                        <div className="flex items-center gap-1">
                          {(['all', 'L0', 'L0.5', 'L1', 'L2'] as const).map(level => (
                            <button
                              key={level}
                              onClick={() => setExecPowerFilter(level)}
                              className={`px-2 py-1 text-[10px] rounded transition-colors ${
                                execPowerFilter === level
                                  ? level === 'L0' ? 'bg-red-500 text-white' 
                                    : level === 'L0.5' ? 'bg-orange-500 text-white'
                                    : level === 'L1' ? 'bg-blue-500 text-white'
                                    : level === 'L2' ? 'bg-gray-500 text-white'
                                    : 'bg-violet-500 text-white'
                                  : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                              }`}
                            >
                              {level === 'all' ? 'All' : level}
                            </button>
                          ))}
                        </div>
                        {/* Region Filter */}
                        <div className="flex items-center gap-1 ml-2">
                          {(['all', 'US', 'EU', 'CN', 'INTL'] as const).map(region => (
                            <button
                              key={region}
                              onClick={() => setExecPowerRegion(region)}
                              className={`px-2 py-1 text-[10px] rounded transition-colors ${
                                execPowerRegion === region
                                  ? 'bg-violet-500 text-white'
                                  : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                              }`}
                            >
                              {region === 'all' ? '🌐' : region === 'US' ? '🇺🇸' : region === 'EU' ? '🇪🇺' : region === 'CN' ? '🇨🇳' : '🌍'}
                            </button>
                          ))}
                        </div>
                        <button
                          onClick={() => executivePowerService.refreshAll()}
                          className="ml-2 p-2 rounded-lg hover:bg-gray-100 transition-colors"
                        >
                          <RefreshCw className="w-4 h-4 text-gray-500" />
                        </button>
                      </div>
                    </div>
                  </div>
                  
                  {/* Stats Summary */}
                  {execPowerStats && (
                    <div className="px-4 py-3 bg-gradient-to-r from-violet-50 to-purple-50 border-b border-gray-100">
                      <div className="grid grid-cols-8 gap-4">
                        <div className="text-center">
                          <div className="text-lg font-bold text-violet-600">{execPowerStats.totalNews}</div>
                          <div className="text-[9px] text-gray-500">Total News</div>
                        </div>
                        <div className="text-center">
                          <div className="text-lg font-bold text-red-600">{execPowerStats.byLevel['L0']}</div>
                          <div className="text-[9px] text-gray-500">L0</div>
                        </div>
                        <div className="text-center">
                          <div className="text-lg font-bold text-orange-600">{execPowerStats.byLevel['L0.5']}</div>
                          <div className="text-[9px] text-gray-500">L0.5</div>
                        </div>
                        <div className="text-center">
                          <div className="text-lg font-bold text-blue-600">{execPowerStats.byLevel['L1']}</div>
                          <div className="text-[9px] text-gray-500">L1</div>
                        </div>
                        <div className="text-center">
                          <div className="text-lg font-bold text-gray-600">{execPowerStats.byLevel['L2']}</div>
                          <div className="text-[9px] text-gray-500">L2</div>
                        </div>
                        <div className="text-center">
                          <div className="text-lg font-bold text-blue-600">{execPowerStats.byRegion['US']}</div>
                          <div className="text-[9px] text-gray-500">🇺🇸 US</div>
                        </div>
                        <div className="text-center">
                          <div className="text-lg font-bold text-amber-600">{execPowerStats.byRegion['EU']}</div>
                          <div className="text-[9px] text-gray-500">🇪🇺 EU</div>
                        </div>
                        <div className="text-center">
                          <div className="text-lg font-bold text-red-600">{execPowerStats.byRegion['CN']}</div>
                          <div className="text-[9px] text-gray-500">🇨🇳 CN</div>
                        </div>
                      </div>
                    </div>
                  )}
                  
                  {/* News Grid */}
                  <div className="p-4">
                    {execPowerNews.length === 0 ? (
                      <EmptyState
                        icon={<Newspaper className="w-12 h-12" />}
                        title={t('news.noExecPower')}
                        description={t('news.noExecPowerDesc')}
                        isLoading={true}
                        action={{
                          label: t('common.refresh'),
                          onClick: () => executivePowerService.refreshAll()
                        }}
                      />
                    ) : (
                    <div className="space-y-3 max-h-[600px] overflow-auto">
                      {execPowerNews
                        .filter(news => execPowerFilter === 'all' || news.level === execPowerFilter)
                        .filter(news => execPowerRegion === 'all' || news.region === execPowerRegion)
                        .map(news => (
                          <div 
                            key={news.id}
                            className={`p-4 rounded-xl border-l-4 bg-white shadow-sm hover:shadow-md transition-all cursor-pointer ${
                              news.level === 'L0' ? 'border-l-red-500 bg-red-50/30' :
                              news.level === 'L0.5' ? 'border-l-orange-500 bg-orange-50/30' :
                              news.level === 'L1' ? 'border-l-blue-500' :
                              'border-l-gray-400'
                            }`}
                          >
                            {/* Row 1: Tags & Scores */}
                            <div className="flex items-center justify-between mb-2">
                              <div className="flex items-center gap-2 flex-wrap">
                                <span className={`text-[10px] px-2 py-0.5 rounded font-bold ${
                                  news.level === 'L0' ? 'bg-red-500 text-white' :
                                  news.level === 'L0.5' ? 'bg-orange-500 text-white' :
                                  news.level === 'L1' ? 'bg-blue-500 text-white' :
                                  'bg-gray-500 text-white'
                                }`}>
                                  {news.level}
                                </span>
                                <span className={`text-[10px] px-2 py-0.5 rounded font-medium ${
                                  news.urgency === 'flash' ? 'bg-red-100 text-red-700 animate-pulse' :
                                  news.urgency === 'urgent' ? 'bg-orange-100 text-orange-700' :
                                  news.urgency === 'breaking' ? 'bg-blue-100 text-blue-700' :
                                  'bg-gray-100 text-gray-600'
                                }`}>
                                  {news.urgency.toUpperCase()}
                                </span>
                                <span className="text-[10px] text-gray-400">
                                  {news.region === 'US' ? '🇺🇸' : news.region === 'EU' ? '🇪🇺' : news.region === 'CN' ? '🇨🇳' : '🌍'} {news.source?.name || 'Unknown'}
                                </span>
                                {news.isVerified && (
                                  <span className="text-[10px] text-emerald-600 flex items-center gap-0.5">
                                    <CheckCircle className="w-3 h-3" /> Verified
                                  </span>
                                )}
                              </div>
                              <div className="flex items-center gap-2">
                                <span className={`text-[10px] px-2 py-0.5 rounded font-bold ${
                                  news.sentiment === 'bullish' ? 'bg-emerald-100 text-emerald-700' :
                                  news.sentiment === 'bearish' ? 'bg-red-100 text-red-700' :
                                  'bg-gray-100 text-gray-600'
                                }`}>
                                  {news.sentiment === 'bullish' ? '↑ BULLISH' : 
                                   news.sentiment === 'bearish' ? '↓ BEARISH' : 
                                   '~ NEUTRAL'}
                                </span>
                                <div className={`w-10 h-6 rounded flex items-center justify-center text-xs font-bold ${
                                  news.impactScore >= 80 ? 'bg-red-100 text-red-700' :
                                  news.impactScore >= 60 ? 'bg-orange-100 text-orange-700' :
                                  news.impactScore >= 40 ? 'bg-blue-100 text-blue-700' :
                                  'bg-gray-100 text-gray-600'
                                }`}>
                                  {news.impactScore}
                                </div>
                              </div>
                            </div>
                            
                            {/* Row 2: Headline */}
                            <h4 className="text-sm font-semibold text-gray-900 mb-2">{news.headline}</h4>
                            
                            {/* Row 3: Summary */}
                            <p className="text-xs text-gray-600 mb-2 line-clamp-2">{news.summary}</p>
                            
                            {/* Row 4: Domains & Assets */}
                            <div className="flex items-center justify-between">
                              <div className="flex items-center gap-1.5 flex-wrap">
                                {news.domains.map((domain, idx) => (
                                  <span key={idx} className="text-[9px] px-1.5 py-0.5 bg-violet-100 text-violet-700 rounded">
                                    {domain}
                                  </span>
                                ))}
                                {news.affectedAssets.slice(0, 5).map((asset, idx) => (
                                  <span key={idx} className={`text-[9px] px-1.5 py-0.5 rounded font-mono ${
                                    news.sentiment === 'bullish' ? 'bg-emerald-50 text-emerald-700' :
                                    news.sentiment === 'bearish' ? 'bg-red-50 text-red-700' :
                                    'bg-gray-100 text-gray-600'
                                  }`}>
                                    ${asset}
                                  </span>
                                ))}
                                {news.affectedAssets.length > 5 && (
                                  <span className="text-[9px] text-gray-400">+{news.affectedAssets.length - 5}</span>
                                )}
                              </div>
                              <div className="flex items-center gap-2 text-[10px] text-gray-400">
                                <Clock className="w-3 h-3" />
                                {formatTimeAgo(news.publishedAt)}
                                <a 
                                  href={news.url} 
                                  target="_blank" 
                                  rel="noopener noreferrer"
                                  className="text-violet-600 hover:underline"
                                >
                                  <ExternalLink className="w-3 h-3" />
                                </a>
                              </div>
                            </div>
                            
                            {/* Row 5: Related Topics */}
                            {news.relatedTopics.length > 0 && (
                              <div className="mt-2 pt-2 border-t border-gray-100 flex items-center gap-1.5">
                                <span className="text-[9px] text-gray-500">Topics:</span>
                                {news.relatedTopics.map((topic, idx) => (
                                  <span key={idx} className="text-[9px] px-1.5 py-0.5 bg-blue-50 text-blue-700 rounded">
                                    {topic}
                                  </span>
                                ))}
                              </div>
                            )}
                          </div>
                        ))}
                      
                      {/* Empty State */}
                      {execPowerNews.filter(news => 
                        (execPowerFilter === 'all' || news.level === execPowerFilter) &&
                        (execPowerRegion === 'all' || news.region === execPowerRegion)
                      ).length === 0 && (
                        <div className="py-12 text-center">
                          <div className="w-20 h-20 mx-auto mb-4 rounded-full bg-gradient-to-br from-violet-100 to-purple-100 flex items-center justify-center">
                            <Newspaper className="w-10 h-10 text-violet-400" />
                          </div>
                          <p className="text-sm text-gray-600 mb-2">{t('news.noNewsAvailable') || 'No news available for current filters'}</p>
                          <p className="text-[10px] text-gray-400 mb-4">{t('news.loadingFromSources') || 'Fetching from authoritative sources...'}</p>
                          <button
                            onClick={() => executivePowerService.refreshAll()}
                            className="px-6 py-2 bg-violet-500 text-white text-sm rounded-lg hover:bg-violet-600 transition-colors"
                          >
                            {t('common.refreshNow') || 'Refresh Now'}
                          </button>
                        </div>
                      )}
                    </div>
                    )}
                  </div>
                </div>
                
                {/* Immediate Actions Panel */}
                {immediateActions.length > 0 && (
                  <div className={`${cardClass} border-2 border-red-200`}>
                    <div className="p-4 bg-red-50 border-b border-red-200">
                      <div className="flex items-center gap-2">
                        <AlertTriangle className="w-5 h-5 text-red-600" />
                        <span className="text-sm font-semibold text-red-800">{t('immediateAction.title')}</span>
                        <span className="ml-auto text-xs px-2 py-1 bg-red-200 text-red-800 rounded-full font-medium">
                          {immediateActions.filter(a => a.priority === 'critical').length} {t('immediateAction.critical')}
                        </span>
                      </div>
                    </div>
                    <div className="p-4 space-y-3">
                      {immediateActions.map(action => (
                        <div 
                          key={action.id}
                          className={`p-4 rounded-lg border-l-4 ${
                            action.priority === 'critical' ? 'bg-red-50 border-l-red-500' :
                            action.priority === 'high' ? 'bg-orange-50 border-l-orange-500' :
                            'bg-amber-50 border-l-amber-500'
                          }`}
                        >
                          <div className="flex items-start justify-between mb-2">
                            <div className="flex items-center gap-2">
                              <span className={`${badgeClass} ${
                                action.priority === 'critical' ? 'bg-red-200 text-red-800' :
                                action.priority === 'high' ? 'bg-orange-200 text-orange-800' :
                                'bg-amber-200 text-amber-800'
                              }`}>
                                {action.priority.toUpperCase()}
                              </span>
                              <span className={`${badgeClass} bg-gray-100 text-gray-600`}>
                                {action.type.replace('_', ' ')}
                              </span>
                              <span className={`${badgeClass} ${
                                action.sourceLevel === 'L0' ? 'bg-blue-100 text-blue-700' : 'bg-cyan-100 text-cyan-700'
                              }`}>
                                {action.sourceLevel}
                              </span>
                            </div>
                            {action.deadline && (
                              <span className="text-[10px] text-red-600 font-medium">
                                {t('immediateAction.deadline')}: {new Date(action.deadline).toLocaleDateString()}
                              </span>
                            )}
                          </div>
                          <h4 className="text-sm font-semibold text-gray-900 mb-1">{action.title}</h4>
                          <p className="text-xs text-gray-600 mb-2">{action.summary}</p>
                          <div className="p-2 bg-white rounded border border-gray-200 mb-2">
                            <div className="text-[10px] text-gray-500 font-medium mb-1">{t('immediateAction.actionRequired')}</div>
                            <div className="text-xs text-gray-800">{action.actionRequired}</div>
                          </div>
                          {action.affectedAssets && (
                            <div className="flex flex-wrap gap-1">
                              {action.affectedAssets.map((ticker, idx) => (
                                <span 
                                  key={idx}
                                  className={`text-[9px] px-1.5 py-0.5 rounded font-medium ${
                                    action.suggestedDirection === 'bearish' ? 'bg-red-100 text-red-700' : 'bg-emerald-100 text-emerald-700'
                                  }`}
                                >
                                  {action.suggestedDirection === 'bearish' ? '↓' : '↑'} {ticker}
                                </span>
                              ))}
                            </div>
                          )}
                          {action.originalText && (
                            <div className="mt-2 p-2 bg-gray-100 rounded text-[10px]">
                              <div className="text-gray-500 mb-1">{t('immediateAction.originalText')}:</div>
                              <div className="text-gray-700 italic">"{action.originalText}"</div>
                              {action.translatedText && (
                                <>
                                  <div className="text-gray-500 mt-2 mb-1">{t('immediateAction.translatedText')}:</div>
                                  <div className="text-gray-700 italic">"{action.translatedText}"</div>
                                </>
                              )}
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      </div>
      
      {/* 🆕 Keyboard Shortcuts Help Modal */}
      {showShortcutsHelp && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={() => setShowShortcutsHelp(false)}>
          <div className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl max-w-md w-full mx-4 overflow-hidden" onClick={e => e.stopPropagation()}>
            <div className="bg-gradient-to-r from-blue-600 to-indigo-600 text-white px-6 py-4">
              <div className="flex items-center gap-3">
                <HelpCircle className="w-6 h-6" />
                <h2 className="text-lg font-semibold">键盘快捷键</h2>
              </div>
              <p className="text-blue-100 text-sm mt-1">Keyboard Shortcuts</p>
            </div>
            <div className="p-6 space-y-4 max-h-[60vh] overflow-auto">
              {/* Navigation */}
              <div>
                <h3 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-2">导航 / Navigation</h3>
                <div className="space-y-2">
                  <div className="flex items-center justify-between py-1">
                    <span className="text-sm text-gray-600 dark:text-gray-400">切换标签页</span>
                    <span className="text-xs bg-gray-100 dark:bg-gray-700 px-2 py-1 rounded font-mono">1 - 9</span>
                  </div>
                </div>
              </div>
              
              {/* Actions */}
              <div>
                <h3 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-2">操作 / Actions</h3>
                <div className="space-y-2">
                  <div className="flex items-center justify-between py-1">
                    <span className="text-sm text-gray-600 dark:text-gray-400">刷新数据</span>
                    <span className="text-xs bg-gray-100 dark:bg-gray-700 px-2 py-1 rounded font-mono">R</span>
                  </div>
                  <div className="flex items-center justify-between py-1">
                    <span className="text-sm text-gray-600 dark:text-gray-400">聚焦搜索</span>
                    <span className="text-xs bg-gray-100 dark:bg-gray-700 px-2 py-1 rounded font-mono">/</span>
                  </div>
                  <div className="flex items-center justify-between py-1">
                    <span className="text-sm text-gray-600 dark:text-gray-400">关闭弹窗</span>
                    <span className="text-xs bg-gray-100 dark:bg-gray-700 px-2 py-1 rounded font-mono">Esc</span>
                  </div>
                </div>
              </div>
              
              {/* Help */}
              <div>
                <h3 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-2">帮助 / Help</h3>
                <div className="space-y-2">
                  <div className="flex items-center justify-between py-1">
                    <span className="text-sm text-gray-600 dark:text-gray-400">显示快捷键帮助</span>
                    <span className="text-xs bg-gray-100 dark:bg-gray-700 px-2 py-1 rounded font-mono">Shift + ?</span>
                  </div>
                </div>
              </div>
            </div>
            <div className="px-6 py-4 bg-gray-50 dark:bg-gray-900/50 border-t border-gray-200 dark:border-gray-700">
              <button
                onClick={() => setShowShortcutsHelp(false)}
                className="w-full py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg font-medium transition-colors"
              >
                关闭 / Close
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

export default NewsIntelligence
