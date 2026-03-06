/**
 * Policy-to-Market Intelligence Engine
 * 
 * 核心哲学：让系统像一个"政策投资委员会秘书"，而不是新闻聚合器
 * 
 * 主要功能：
 * 1. Anti-Jitter 稳定层 - 防止状态横跳
 * 2. 决策评分计算 - 真正可用的交易信号
 * 3. 辖区模型 - 精确到执行机构层级
 * 4. 可交易性门槛 - 只有达标才能标记"可交易"
 */

// ==================== 类型定义 ====================

/** 验证级别 */
export type ValidationLevel = 'L0' | 'L0.5' | 'L1' | 'L2'

/** 事件分类 */
export type EventClassification = 'rumor' | 'official_signal' | 'legal_text' | 'enforcement_action'

/** 权力层级 */
export type AuthorityLevel = 'supranational' | 'federal' | 'ministry' | 'agency' | 'state' | 'local'

/** 执行权力 */
export type EnforcementPower = 'full' | 'partial' | 'signaling' | 'none'

/** 交易偏向 */
export type TradeBias = 'bullish' | 'bearish' | 'neutral'

/** 辖区模型 - 精确到执行机构 */
export interface JurisdictionModel {
  region: string              // EU / US / CN / JP / UK / ...
  authorityLevel: AuthorityLevel
  executingBody: string       // EU Commission DG COMP, US BIS, CN MIIT, etc.
  enforcementPower: EnforcementPower
  executionAuthority: boolean // 是否有直接执行权
}

/** 执行机构注册表 */
export const EXECUTING_BODIES: Record<string, JurisdictionModel> = {
  // US
  'us-whitehouse': { region: 'US', authorityLevel: 'federal', executingBody: 'White House', enforcementPower: 'full', executionAuthority: true },
  'us-treasury': { region: 'US', authorityLevel: 'federal', executingBody: 'Treasury / OFAC', enforcementPower: 'full', executionAuthority: true },
  'us-commerce-bis': { region: 'US', authorityLevel: 'agency', executingBody: 'Commerce BIS', enforcementPower: 'full', executionAuthority: true },
  'us-ustr': { region: 'US', authorityLevel: 'agency', executingBody: 'USTR', enforcementPower: 'partial', executionAuthority: true },
  'us-fed': { region: 'US', authorityLevel: 'agency', executingBody: 'Federal Reserve', enforcementPower: 'full', executionAuthority: true },
  'us-sec': { region: 'US', authorityLevel: 'agency', executingBody: 'SEC', enforcementPower: 'full', executionAuthority: true },
  'us-congress': { region: 'US', authorityLevel: 'federal', executingBody: 'Congress', enforcementPower: 'signaling', executionAuthority: false },
  
  // EU
  'eu-commission': { region: 'EU', authorityLevel: 'supranational', executingBody: 'EU Commission', enforcementPower: 'partial', executionAuthority: true },
  'eu-dg-comp': { region: 'EU', authorityLevel: 'agency', executingBody: 'DG COMP', enforcementPower: 'full', executionAuthority: true },
  'eu-dg-trade': { region: 'EU', authorityLevel: 'agency', executingBody: 'DG Trade', enforcementPower: 'partial', executionAuthority: true },
  'eu-ecb': { region: 'EU', authorityLevel: 'supranational', executingBody: 'ECB', enforcementPower: 'full', executionAuthority: true },
  'eu-council': { region: 'EU', authorityLevel: 'supranational', executingBody: 'EU Council', enforcementPower: 'signaling', executionAuthority: false },
  'eu-parliament': { region: 'EU', authorityLevel: 'supranational', executingBody: 'EU Parliament', enforcementPower: 'signaling', executionAuthority: false },
  
  // CN
  'cn-state-council': { region: 'CN', authorityLevel: 'federal', executingBody: 'State Council', enforcementPower: 'full', executionAuthority: true },
  'cn-mofcom': { region: 'CN', authorityLevel: 'ministry', executingBody: 'MOFCOM', enforcementPower: 'full', executionAuthority: true },
  'cn-pboc': { region: 'CN', authorityLevel: 'ministry', executingBody: 'PBoC', enforcementPower: 'full', executionAuthority: true },
  'cn-cac': { region: 'CN', authorityLevel: 'agency', executingBody: 'CAC', enforcementPower: 'full', executionAuthority: true },
  'cn-miit': { region: 'CN', authorityLevel: 'ministry', executingBody: 'MIIT', enforcementPower: 'full', executionAuthority: true },
  'cn-samr': { region: 'CN', authorityLevel: 'agency', executingBody: 'SAMR', enforcementPower: 'full', executionAuthority: true },
  
  // JP
  'jp-boj': { region: 'JP', authorityLevel: 'agency', executingBody: 'Bank of Japan', enforcementPower: 'full', executionAuthority: true },
  'jp-mof': { region: 'JP', authorityLevel: 'ministry', executingBody: 'MOF', enforcementPower: 'partial', executionAuthority: true },
  'jp-meti': { region: 'JP', authorityLevel: 'ministry', executingBody: 'METI', enforcementPower: 'partial', executionAuthority: true },
  
  // UK
  'uk-boe': { region: 'UK', authorityLevel: 'agency', executingBody: 'Bank of England', enforcementPower: 'full', executionAuthority: true },
  'uk-fca': { region: 'UK', authorityLevel: 'agency', executingBody: 'FCA', enforcementPower: 'full', executionAuthority: true },
  'uk-parliament': { region: 'UK', authorityLevel: 'federal', executingBody: 'Parliament', enforcementPower: 'signaling', executionAuthority: false },
}

// ==================== Anti-Jitter 稳定层 ====================

/** 事件状态缓存 */
interface EventStateCache {
  eventId: string
  state: string
  confidence: number
  lastUpdated: number
  lockedUntil: number  // 状态锁定到期时间
  confidenceHistory: { value: number; timestamp: number }[]
}

/** 状态锁定配置 */
const STABILITY_CONFIG = {
  MIN_STATE_DURATION_MS: 30 * 60 * 1000,  // 30分钟最小状态持续
  CONFIDENCE_WINDOW_SIZE: 3,               // 置信度平滑窗口大小
  DOWNGRADE_THRESHOLD: 0.7,                // 降级阈值（需要更高权威矛盾）
  UPGRADE_THRESHOLD: 0.3,                  // 升级阈值（更容易升级）
}

class AntiJitterLayer {
  private stateCache: Map<string, EventStateCache> = new Map()

  /**
   * 获取稳定化后的状态
   */
  getStableState(
    eventId: string,
    newState: string,
    newConfidence: number,
    sourceAuthority: ValidationLevel
  ): { state: string; confidence: number; isLocked: boolean } {
    const now = Date.now()
    const cached = this.stateCache.get(eventId)

    // 新事件：直接接受
    if (!cached) {
      this.stateCache.set(eventId, {
        eventId,
        state: newState,
        confidence: newConfidence,
        lastUpdated: now,
        lockedUntil: now + STABILITY_CONFIG.MIN_STATE_DURATION_MS,
        confidenceHistory: [{ value: newConfidence, timestamp: now }]
      })
      return { state: newState, confidence: newConfidence, isLocked: false }
    }

    // 状态锁定期内
    if (now < cached.lockedUntil) {
      // 只有更高权威才能打破锁定
      if (this.canOverrideLock(cached, newState, sourceAuthority)) {
        return this.updateState(cached, newState, newConfidence, now)
      }
      // 返回锁定的状态
      return { state: cached.state, confidence: this.smoothedConfidence(cached), isLocked: true }
    }

    // 锁定期外：应用平滑逻辑
    return this.updateState(cached, newState, newConfidence, now)
  }

  /**
   * 判断是否可以打破状态锁定
   */
  private canOverrideLock(
    cached: EventStateCache,
    newState: string,
    sourceAuthority: ValidationLevel
  ): boolean {
    // L2（法律/执法行动）可以打破任何锁定
    if (sourceAuthority === 'L2') return true
    
    // L1 可以打破 L0/L0.5 设置的锁定（需要多官方确认）
    if (sourceAuthority === 'L1' && cached.confidence < 0.8) return true
    
    // 状态升级更容易被接受
    const stateOrder = ['rumor', 'monitoring', 'signal_detected', 'analyzing', 'contested', 'actionable', 'implementing', 'confirmed']
    const currentIndex = stateOrder.indexOf(cached.state)
    const newIndex = stateOrder.indexOf(newState)
    if (newIndex > currentIndex) return true
    
    return false
  }

  /**
   * 更新状态
   */
  private updateState(
    cached: EventStateCache,
    newState: string,
    newConfidence: number,
    now: number
  ): { state: string; confidence: number; isLocked: boolean } {
    // 更新置信度历史
    cached.confidenceHistory.push({ value: newConfidence, timestamp: now })
    if (cached.confidenceHistory.length > STABILITY_CONFIG.CONFIDENCE_WINDOW_SIZE) {
      cached.confidenceHistory.shift()
    }

    cached.state = newState
    cached.confidence = newConfidence
    cached.lastUpdated = now
    cached.lockedUntil = now + STABILITY_CONFIG.MIN_STATE_DURATION_MS

    return { state: newState, confidence: this.smoothedConfidence(cached), isLocked: false }
  }

  /**
   * 计算平滑置信度
   */
  private smoothedConfidence(cached: EventStateCache): number {
    if (cached.confidenceHistory.length === 0) return cached.confidence
    
    const weights = [0.5, 0.3, 0.2] // 最近的权重最高
    let sum = 0
    let weightSum = 0
    
    for (let i = 0; i < cached.confidenceHistory.length; i++) {
      const weight = weights[i] || 0.1
      sum += cached.confidenceHistory[cached.confidenceHistory.length - 1 - i].value * weight
      weightSum += weight
    }
    
    return weightSum > 0 ? sum / weightSum : cached.confidence
  }

  /**
   * 清除过期缓存
   */
  cleanup(maxAgeMs: number = 24 * 60 * 60 * 1000) {
    const now = Date.now()
    for (const [eventId, cache] of this.stateCache) {
      if (now - cache.lastUpdated > maxAgeMs) {
        this.stateCache.delete(eventId)
      }
    }
  }
}

export const antiJitter = new AntiJitterLayer()

// ==================== 决策评分计算 ====================

/** 决策评分输入 */
export interface DecisionScoreInput {
  policyImpactMagnitude: number  // 1-5
  validationLevel: ValidationLevel
  assetExposure: number          // 0-1
  timeToEffectMonths: number     // 月数
  volatilityPenalty?: number     // 0-1 可选
  jurisdictionAmbiguity?: number // 0-1 可选
  hasExecutionAuthority: boolean
}

/** 决策评分结果 */
export interface DecisionScoreResult {
  score: number              // -100 to +100
  confidence: number         // 0-1
  tradeability: 'tradeable' | 'monitor' | 'ignore'
  tradeBias: TradeBias
  breakdown: {
    impactContribution: number
    certaintyContribution: number
    exposureContribution: number
    timeWeightContribution: number
    penalties: number
  }
  reasoning: string
}

/**
 * 验证级别对应的确定性系数
 */
const VALIDATION_CERTAINTY: Record<ValidationLevel, number> = {
  'L0': 0.25,   // 市场传闻/媒体泄露
  'L0.5': 0.50, // 单一官方提及
  'L1': 0.75,   // 多官方确认
  'L2': 1.0,    // 法律/监管执法
}

/**
 * 计算时间权重
 */
function calculateTimeWeight(months: number): number {
  if (months <= 1) return 1.0        // 即时
  if (months <= 3) return 0.8        // 1-3个月
  if (months <= 6) return 0.5        // 3-6个月
  if (months <= 12) return 0.3       // 6-12个月
  return 0.15                         // >12个月
}

/**
 * 计算决策评分
 * 
 * Score = (Impact × Certainty × Exposure × TimeWeight) - Penalties
 */
export function calculateDecisionScore(input: DecisionScoreInput): DecisionScoreResult {
  const {
    policyImpactMagnitude,
    validationLevel,
    assetExposure,
    timeToEffectMonths,
    volatilityPenalty = 0,
    jurisdictionAmbiguity = 0,
    hasExecutionAuthority
  } = input

  // 基础因子
  const impactNormalized = policyImpactMagnitude / 5  // 归一化到 0-1
  const certainty = VALIDATION_CERTAINTY[validationLevel]
  const timeWeight = calculateTimeWeight(timeToEffectMonths)
  
  // 执行权力惩罚
  const executionPenalty = hasExecutionAuthority ? 0 : 0.3

  // 计算各部分贡献
  const impactContribution = impactNormalized * 40
  const certaintyContribution = certainty * 30
  const exposureContribution = assetExposure * 20
  const timeWeightContribution = timeWeight * 10
  
  // 总惩罚
  const penalties = (volatilityPenalty * 15) + (jurisdictionAmbiguity * 10) + (executionPenalty * 15)
  
  // 原始分数
  let rawScore = impactContribution + certaintyContribution + exposureContribution + timeWeightContribution - penalties
  
  // 归一化到 -100 ~ +100
  const score = Math.round(Math.max(-100, Math.min(100, rawScore)))
  
  // 置信度
  const confidence = certainty * (1 - jurisdictionAmbiguity) * (hasExecutionAuthority ? 1 : 0.7)
  
  // 可交易性判断
  let tradeability: 'tradeable' | 'monitor' | 'ignore'
  if (Math.abs(score) >= 25 && timeToEffectMonths <= 6 && assetExposure >= 0.3) {
    tradeability = 'tradeable'
  } else if (Math.abs(score) >= 10) {
    tradeability = 'monitor'
  } else {
    tradeability = 'ignore'
  }
  
  // 交易偏向
  const tradeBias: TradeBias = score > 10 ? 'bullish' : score < -10 ? 'bearish' : 'neutral'
  
  // 推理说明
  const reasoning = generateReasoning(input, score, tradeability)

  return {
    score,
    confidence: Math.round(confidence * 100) / 100,
    tradeability,
    tradeBias,
    breakdown: {
      impactContribution: Math.round(impactContribution * 10) / 10,
      certaintyContribution: Math.round(certaintyContribution * 10) / 10,
      exposureContribution: Math.round(exposureContribution * 10) / 10,
      timeWeightContribution: Math.round(timeWeightContribution * 10) / 10,
      penalties: Math.round(penalties * 10) / 10
    },
    reasoning
  }
}

/**
 * 生成评分推理说明
 */
function generateReasoning(
  input: DecisionScoreInput,
  score: number,
  tradeability: string
): string {
  const parts: string[] = []
  
  if (input.policyImpactMagnitude >= 4) {
    parts.push('高影响力政策')
  } else if (input.policyImpactMagnitude <= 2) {
    parts.push('低影响力政策')
  }
  
  if (input.validationLevel === 'L2') {
    parts.push('法律级确认')
  } else if (input.validationLevel === 'L0') {
    parts.push('仅传闻级别')
  }
  
  if (input.timeToEffectMonths <= 1) {
    parts.push('即时生效')
  } else if (input.timeToEffectMonths > 6) {
    parts.push('长期政策')
  }
  
  if (!input.hasExecutionAuthority) {
    parts.push('缺乏执行权力')
  }
  
  if (tradeability === 'tradeable') {
    parts.push('→ 可交易')
  } else if (tradeability === 'monitor') {
    parts.push('→ 监控中')
  }
  
  return parts.join(' | ')
}

// ==================== 标准输出格式 ====================

/** 政策事件标准输出 */
export interface PolicyEventOutput {
  eventId: string
  eventName: string
  currentState: string
  decisionScore: number
  confidence: number
  jurisdictions: JurisdictionModel[]
  affectedAssets: Array<{
    ticker: string
    exposure: 'direct' | 'indirect'
    direction: TradeBias
  }>
  tradeBias: TradeBias
  tradeability: 'tradeable' | 'monitor' | 'ignore'
  keyRisk: string
  nextValidationTrigger: string
  lastUpdated: string
  isLocked: boolean
}

/**
 * 生成标准化政策事件输出
 */
export function generatePolicyOutput(
  eventId: string,
  eventName: string,
  state: string,
  scoreInput: DecisionScoreInput,
  jurisdictions: JurisdictionModel[],
  affectedAssets: Array<{ ticker: string; exposure: 'direct' | 'indirect'; direction: TradeBias }>,
  keyRisk: string,
  nextTrigger: string,
  sourceAuthority: ValidationLevel = 'L1'
): PolicyEventOutput {
  // 应用稳定层
  const scoreResult = calculateDecisionScore(scoreInput)
  const stableState = antiJitter.getStableState(eventId, state, scoreResult.confidence, sourceAuthority)

  return {
    eventId,
    eventName,
    currentState: stableState.state,
    decisionScore: scoreResult.score,
    confidence: stableState.confidence,
    jurisdictions,
    affectedAssets,
    tradeBias: scoreResult.tradeBias,
    tradeability: scoreResult.tradeability,
    keyRisk,
    nextValidationTrigger: nextTrigger,
    lastUpdated: new Date().toISOString(),
    isLocked: stableState.isLocked
  }
}

// ==================== 事件分类器 ====================

/**
 * 对输入事件进行分类
 */
export function classifyEvent(
  headline: string,
  sourceLevel: ValidationLevel,
  hasLegalDocument: boolean,
  hasEnforcementAction: boolean
): EventClassification {
  if (hasEnforcementAction) return 'enforcement_action'
  if (hasLegalDocument) return 'legal_text'
  if (sourceLevel === 'L0.5' || sourceLevel === 'L1') return 'official_signal'
  return 'rumor'
}

/**
 * 根据事件分类调整置信度
 */
export function adjustConfidenceByClassification(
  baseConfidence: number,
  classification: EventClassification
): number {
  const multipliers: Record<EventClassification, number> = {
    'rumor': 0.5,
    'official_signal': 0.8,
    'legal_text': 0.95,
    'enforcement_action': 1.0
  }
  return Math.min(1, baseConfidence * multipliers[classification])
}

// ==================== 导出 ====================

export {
  AntiJitterLayer,
  STABILITY_CONFIG,
  VALIDATION_CERTAINTY
}
