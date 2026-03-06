/**
 * Services Index
 * 
 * 统一导出所有服务模块
 */

// 数据服务
export { newsDataService } from './newsDataService'
export type { 
  RawNewsItem, 
  SDNEntry, 
  EntityListEntry, 
  FederalRegisterDocument 
} from './newsDataService'

// 决策评分引擎
export { 
  calculateDecisionScore,
  calculateFreshnessDecay,
  calculateExecutionPowerWeight,
  calculateUncertaintyPenalty,
  bayesianUpdate,
  aggregateSentiment,
  calculateEvidenceConsensus
} from './decisionScoringEngine'
export type {
  SourceLevel,
  Domain,
  PolicyState,
  Direction,
  Evidence,
  DecisionContext,
  ScoringBreakdown,
  DecisionResult
} from './decisionScoringEngine'

// 警报服务
export { alertService, DEFAULT_ALERT_RULES } from './alertService'
export type {
  AlertPriority,
  AlertChannel,
  AlertStatus,
  RuleConditionType,
  AlertRule,
  RuleCondition,
  Alert,
  AlertStats,
  NotificationPayload,
  AlertService
} from './alertService'

// 文档服务
export { documentService, DocumentParser } from './documentService'
export type {
  DocumentType,
  DocumentStatus,
  DocumentSource,
  PolicyDocument,
  ExtractedEntity,
  Citation,
  Amendment,
  ImpactAssessment,
  DocumentSearchParams,
  DocumentSearchResult,
  DocumentService
} from './documentService'

// Hook
export { useNewsIntelligence } from './useNewsIntelligence'
export type {
  NewsIntelligenceState,
  ProcessedHeadline,
  ProcessedFederalDoc,
  ProcessedSDNEntry
} from './useNewsIntelligence'

// 系统健康服务
export { systemHealthService } from './systemHealthService'
export type {
  DataSourceHealth,
  SystemMetrics,
  APICallRecord,
  RefreshSchedule
} from './systemHealthService'

// 执行权力图谱服务
export { executivePowerService } from './executivePowerService'
export type {
  SourceLevel as ExecSourceLevel,
  SourceRegion,
  PolicyDomain,
  ExecutiveSource,
  ExecutiveNews,
  PowerGraphNode,
  PowerGraphStats
} from './executivePowerService'
// 稳定 Signal Line 服务
export { 
  StableSignalLine, 
  createThrottledSignalLine,
  DEFAULT_SIGNAL_LINE_CONFIG 
} from './stableSignalLine'
export type { 
  SignalLineConfig, 
  SignalLineResult 
} from './stableSignalLine'

// 政策状态机服务
export { 
  PolicyStateMachine, 
  STATE_CONFIG as POLICY_STATE_CONFIG 
} from './policyStateMachine'
export type {
  PolicyState as MachinePolicyState,
  StateContext,
  StateChangeEvent,
  StateHistoryEntry,
  StateTransitionRule
} from './policyStateMachine'

// 持久化服务
export { 
  default as persistenceService,
  setItem,
  getItem,
  removeItem,
  setCachedItem,
  getCachedItem,
  saveNewsSettings,
  getNewsSettings,
  saveTradingSettings,
  getTradingSettings,
  STORAGE_KEYS
} from './persistenceService'
export type {
  NewsFilters,
  NewsSettings,
  TradingSettings
} from './persistenceService'

// 实时数据缓冲
export {
  RealtimeBuffer,
  SignalHysteresis,
  StableList,
  classifyChange,
  shallowEqual,
  createStableKeyGenerator,
  newsBuffer,
  alertBuffer,
  signalHysteresis
} from '../lib/realtimeBuffer'
export type {
  BufferConfig,
  BufferedItem,
  HysteresisConfig,
  StableListConfig,
  ChangeType,
  ChangeClassification
} from '../lib/realtimeBuffer'

// 绑定状态机
export {
  BoundStateMachine,
  createBoundStateMachine,
  getStateForScore,
  getStateConfig,
  useBoundStateMachine,
  STATE_SCORE_BINDINGS
} from '../lib/boundStateMachine'
export type {
  BoundPolicyState,
  StateScoreBinding,
  BoundStateMachineContext
} from '../lib/boundStateMachine'

// 🆕 新闻到市场流水线 - News to Market Pipeline
export {
  NewsToMarketPipeline,
  newsToMarketPipeline,
  generateMarketFromNews,
  processNewsBatch,
  getTodayMarketCount,
  SAMPLE_NEWS_FOR_TESTING
} from './newsToMarketPipeline'
export type {
  EventType,
  MarketTier,
  ExtractedEvent,
  GeneratedMarket
} from './newsToMarketPipeline'

// 🆕 每日市场生成器 - Daily Market Generator
export {
  DailyMarketGenerator,
  dailyMarketGenerator,
  useDailyGeneratedMarkets
} from './dailyMarketGenerator'