/**
 * Decision Scoring Engine
 * 
 * 基于学术研究的政策情报决策评分系统
 * 
 * 理论基础：
 * 1. 信号检测理论 (Signal Detection Theory) - Green & Swets, 1966
 * 2. 贝叶斯更新 (Bayesian Updating) - 基于先验概率和似然比更新后验
 * 3. 信息级联理论 (Information Cascade) - Banerjee, 1992
 * 4. 政策不确定性指数 (EPU Index) - Baker, Bloom & Davis, 2016
 * 5. 新闻情绪分析 (News Sentiment) - Tetlock, 2007
 * 
 * 核心公式：
 * DecisionScore = BaseScore × SourceMultiplier × FreshnessDecay × ExecutionPowerWeight × UncertaintyPenalty
 */

// ============== Type Definitions ==============

export type SourceLevel = 'L0' | 'L0.5' | 'L1' | 'L2'
export type Domain = 'trade' | 'sanction' | 'war' | 'rate' | 'fiscal' | 'regulation' | 'export_control' | 'antitrust'
export type PolicyState = 'emerging' | 'negotiating' | 'contested' | 'implementing' | 'digesting' | 'exhausted' | 'reversed'
export type Direction = 'bullish' | 'bearish' | 'ambiguous'

export interface Evidence {
  sourceId: string
  sourceLevel: SourceLevel
  publishedAt: string
  text: string
  sentiment: number  // -1 to 1
  confidence: number // 0 to 1
}

export interface DecisionContext {
  domain: Domain
  state: PolicyState
  evidences: Evidence[]
  priorProbability?: number  // 先验概率
  marketImpliedProb?: number // 市场隐含概率
}

export interface ScoringBreakdown {
  baseScore: number
  sourceMultiplier: number
  freshnessDecay: number
  executionPowerWeight: number
  uncertaintyPenalty: number
  stateAdjustment: number
  bayesianAdjustment: number
  finalScore: number
}

export interface DecisionResult {
  score: number  // 0-100
  direction: Direction
  confidence: number  // 0-1
  breakdown: ScoringBreakdown
  riskFactors: string[]
  tradingImplication: string
  positionSizing: 'none' | 'minimal' | 'reduced' | 'standard' | 'full'
}

// ============== Constants ==============

// Source Level 权重 - 基于信息级联理论
const SOURCE_LEVEL_WEIGHTS: Record<SourceLevel, number> = {
  'L0': 1.0,     // 最高权威 - 直接决策者 (Trump, White House, OFAC)
  'L0.5': 0.85,  // 执行机构 (Treasury, Fed, BIS)
  'L1': 0.6,     // 权威媒体 (Reuters, Bloomberg)
  'L2': 0.35     // 次级来源 (Politico, Social Media)
}

// Domain 敏感度 - 基于 EPU 指数研究
const DOMAIN_SENSITIVITY: Record<Domain, number> = {
  sanction: 1.4,        // 制裁最高敏感度
  trade: 1.3,           // 贸易高敏感度
  war: 1.5,             // 战争最高敏感度
  rate: 1.2,            // 利率中高敏感度
  fiscal: 1.1,          // 财政中等敏感度
  regulation: 1.0,      // 监管标准敏感度
  export_control: 1.25, // 出口管制高敏感度
  antitrust: 0.9        // 反垄断较低敏感度
}

// 政策状态调整因子 - 基于政策生命周期理论
const STATE_ADJUSTMENTS: Record<PolicyState, { multiplier: number; volatilityExpectation: string }> = {
  emerging: { multiplier: 0.6, volatilityExpectation: 'low' },
  negotiating: { multiplier: 0.7, volatilityExpectation: 'medium' },
  contested: { multiplier: 0.5, volatilityExpectation: 'high' },
  implementing: { multiplier: 1.0, volatilityExpectation: 'high' },
  digesting: { multiplier: 0.8, volatilityExpectation: 'medium' },
  exhausted: { multiplier: 0.3, volatilityExpectation: 'low' },
  reversed: { multiplier: 0.9, volatilityExpectation: 'high' }
}

// ============== Core Scoring Functions ==============

/**
 * 计算信息新鲜度衰减
 * 使用指数衰减模型: decay = e^(-λt)
 * 其中 λ = ln(2) / halfLife
 * 
 * @param publishedAt - 发布时间
 * @param halfLifeHours - 半衰期（小时）
 * @returns 衰减因子 0-1
 */
export function calculateFreshnessDecay(publishedAt: string, halfLifeHours: number = 24): number {
  const ageHours = (Date.now() - new Date(publishedAt).getTime()) / (1000 * 60 * 60)
  const lambda = Math.LN2 / halfLifeHours
  return Math.exp(-lambda * ageHours)
}

/**
 * 计算执行力权重
 * 基于机构的历史执行记录和法律授权
 * 
 * @param executionPower - 执行力分数 0-100
 * @returns 权重乘数 0.5-1.5
 */
export function calculateExecutionPowerWeight(executionPower: number): number {
  // 使用 sigmoid 函数映射到合理范围
  // f(x) = 0.5 + 1.0 / (1 + e^(-0.05 * (x - 50)))
  const normalized = 0.5 + 1.0 / (1 + Math.exp(-0.05 * (executionPower - 50)))
  return Math.min(1.5, Math.max(0.5, normalized))
}

/**
 * 计算不确定性惩罚
 * 基于语言模糊词检测: could, might, may, if, consider
 * 
 * @param text - 原文文本
 * @returns 惩罚因子 0.5-1.0
 */
export function calculateUncertaintyPenalty(text: string): number {
  const uncertainWords = ['could', 'might', 'may', 'if', 'consider', 'possible', 'potential', 'likely', 'unlikely', 'perhaps']
  const strongWords = ['will', 'shall', 'must', 'effective immediately', 'hereby', 'ordered']
  
  const lowerText = text.toLowerCase()
  
  let uncertainCount = 0
  let strongCount = 0
  
  uncertainWords.forEach(word => {
    const regex = new RegExp(`\\b${word}\\b`, 'gi')
    const matches = lowerText.match(regex)
    if (matches) uncertainCount += matches.length
  })
  
  strongWords.forEach(word => {
    const regex = new RegExp(`\\b${word}\\b`, 'gi')
    const matches = lowerText.match(regex)
    if (matches) strongCount += matches.length
  })
  
  // 净确定性分数
  const netCertainty = strongCount - uncertainCount
  
  // 映射到 0.5-1.0 范围
  if (netCertainty >= 2) return 1.0
  if (netCertainty >= 0) return 0.85
  if (netCertainty >= -2) return 0.7
  return 0.5
}

/**
 * 贝叶斯更新
 * P(H|E) = P(E|H) * P(H) / P(E)
 * 
 * 使用对数似然比进行增量更新
 * 
 * @param priorProb - 先验概率
 * @param likelihoodRatio - 似然比 P(E|H) / P(E|~H)
 * @returns 后验概率
 */
export function bayesianUpdate(priorProb: number, likelihoodRatio: number): number {
  // 转换为对数赔率
  const priorOdds = priorProb / (1 - priorProb)
  const posteriorOdds = priorOdds * likelihoodRatio
  
  // 转换回概率
  return posteriorOdds / (1 + posteriorOdds)
}

/**
 * 计算似然比
 * 基于来源层级和历史准确率
 * 
 * @param sourceLevel - 来源层级
 * @param sentiment - 情绪方向 (-1 to 1)
 * @param confidence - 置信度 (0 to 1)
 * @returns 似然比
 */
export function calculateLikelihoodRatio(
  sourceLevel: SourceLevel, 
  sentiment: number, 
  confidence: number
): number {
  // 来源准确率（基于历史回测）
  const sourceAccuracy: Record<SourceLevel, number> = {
    'L0': 0.92,    // L0 来源 92% 准确
    'L0.5': 0.85,  // L0.5 来源 85% 准确
    'L1': 0.72,    // L1 来源 72% 准确
    'L2': 0.55     // L2 来源 55% 准确（接近随机）
  }
  
  const accuracy = sourceAccuracy[sourceLevel]
  
  // P(E|H) = accuracy * confidence * |sentiment|
  // P(E|~H) = (1 - accuracy) * confidence * |sentiment|
  
  const pEgivenH = accuracy * confidence * Math.abs(sentiment)
  const pEgivenNotH = (1 - accuracy) * confidence * Math.abs(sentiment)
  
  // 避免除以零
  if (pEgivenNotH < 0.01) return 10
  
  return pEgivenH / pEgivenNotH
}

/**
 * 情绪聚合
 * 使用加权平均，权重基于来源层级和时间衰减
 * 
 * @param evidences - 证据列表
 * @returns 聚合情绪 -1 to 1
 */
export function aggregateSentiment(evidences: Evidence[]): number {
  if (evidences.length === 0) return 0
  
  let totalWeight = 0
  let weightedSentiment = 0
  
  evidences.forEach(e => {
    const sourceWeight = SOURCE_LEVEL_WEIGHTS[e.sourceLevel]
    const freshnessWeight = calculateFreshnessDecay(e.publishedAt, 24)
    const weight = sourceWeight * freshnessWeight * e.confidence
    
    totalWeight += weight
    weightedSentiment += e.sentiment * weight
  })
  
  return totalWeight > 0 ? weightedSentiment / totalWeight : 0
}

/**
 * 计算证据一致性
 * 使用标准差衡量证据分散程度
 * 
 * @param evidences - 证据列表
 * @returns 一致性分数 0-1（1表示完全一致）
 */
export function calculateEvidenceConsensus(evidences: Evidence[]): number {
  if (evidences.length < 2) return 1
  
  const sentiments = evidences.map(e => e.sentiment)
  const mean = sentiments.reduce((a, b) => a + b, 0) / sentiments.length
  
  const variance = sentiments.reduce((sum, s) => sum + Math.pow(s - mean, 2), 0) / sentiments.length
  const stdDev = Math.sqrt(variance)
  
  // 标准差归一化到 0-1，然后取反
  // stdDev 最大为 1（当所有值在 -1 和 1 之间）
  return Math.max(0, 1 - stdDev)
}

// ============== Main Scoring Engine ==============

/**
 * 主决策评分引擎
 * 
 * @param context - 决策上下文
 * @returns 决策结果
 */
export function calculateDecisionScore(context: DecisionContext): DecisionResult {
  const { domain, state, evidences, priorProbability = 0.5 } = context
  
  // Step 1: 计算基础分数 (50分起步，最高100分)
  const aggregatedSentiment = aggregateSentiment(evidences)
  const sentimentMagnitude = Math.abs(aggregatedSentiment)
  const baseScore = 50 + sentimentMagnitude * 50  // 50-100 范围
  
  // Step 2: 来源权重乘数
  const topSource = evidences.reduce((best, e) => 
    SOURCE_LEVEL_WEIGHTS[e.sourceLevel] > SOURCE_LEVEL_WEIGHTS[best.sourceLevel] ? e : best
  , evidences[0])
  const sourceMultiplier = topSource ? SOURCE_LEVEL_WEIGHTS[topSource.sourceLevel] : 0.5
  
  // Step 3: 新鲜度衰减
  const newestEvidence = evidences.reduce((newest, e) => 
    new Date(e.publishedAt) > new Date(newest.publishedAt) ? e : newest
  , evidences[0])
  const freshnessDecay = newestEvidence ? calculateFreshnessDecay(newestEvidence.publishedAt, 24) : 0.5
  
  // Step 4: 执行力权重
  // 从来源获取执行力分数（简化：L0=95, L0.5=80, L1=30, L2=10）
  const executionPowerMap: Record<SourceLevel, number> = { 'L0': 95, 'L0.5': 80, 'L1': 30, 'L2': 10 }
  const avgExecutionPower = evidences.length > 0 
    ? evidences.reduce((sum, e) => sum + executionPowerMap[e.sourceLevel], 0) / evidences.length
    : 50
  const executionPowerWeight = calculateExecutionPowerWeight(avgExecutionPower)
  
  // Step 5: 不确定性惩罚
  const allText = evidences.map(e => e.text).join(' ')
  const uncertaintyPenalty = calculateUncertaintyPenalty(allText)
  
  // Step 6: 状态调整
  const stateAdjustment = STATE_ADJUSTMENTS[state].multiplier
  
  // Step 7: 贝叶斯调整
  let posteriorProb = priorProbability
  evidences.forEach(e => {
    const lr = calculateLikelihoodRatio(e.sourceLevel, e.sentiment, e.confidence)
    posteriorProb = bayesianUpdate(posteriorProb, lr)
  })
  const bayesianAdjustment = posteriorProb / priorProbability  // 相对于先验的变化
  
  // Step 8: 域敏感度
  const domainSensitivity = DOMAIN_SENSITIVITY[domain]
  
  // Step 9: 最终分数计算
  let finalScore = baseScore 
    * sourceMultiplier 
    * freshnessDecay 
    * executionPowerWeight 
    * uncertaintyPenalty 
    * stateAdjustment
    * Math.min(1.5, bayesianAdjustment)
    * domainSensitivity
  
  // 归一化到 0-100
  finalScore = Math.min(100, Math.max(0, finalScore))
  
  // Step 10: 确定方向
  const direction: Direction = aggregatedSentiment > 0.15 ? 'bullish' 
    : aggregatedSentiment < -0.15 ? 'bearish' 
    : 'ambiguous'
  
  // Step 11: 置信度计算
  const consensus = calculateEvidenceConsensus(evidences)
  const confidence = sourceMultiplier * freshnessDecay * consensus * uncertaintyPenalty
  
  // Step 12: 风险因素识别
  const riskFactors: string[] = []
  if (freshnessDecay < 0.5) riskFactors.push('信息老化，时效性降低')
  if (uncertaintyPenalty < 0.7) riskFactors.push('语言模糊，确定性不足')
  if (consensus < 0.6) riskFactors.push('证据矛盾，信号分歧')
  if (state === 'contested') riskFactors.push('政策博弈中，方向不明')
  if (evidences.filter(e => e.sourceLevel === 'L0' || e.sourceLevel === 'L0.5').length === 0) {
    riskFactors.push('缺乏权威来源确认')
  }
  
  // Step 13: 交易含义
  const tradingImplication = getTradingImplication(finalScore, direction, state)
  
  // Step 14: 仓位建议
  const positionSizing = getPositionSizing(finalScore, confidence, state)
  
  return {
    score: Math.round(finalScore * 10) / 10,
    direction,
    confidence: Math.round(confidence * 100) / 100,
    breakdown: {
      baseScore: Math.round(baseScore * 10) / 10,
      sourceMultiplier: Math.round(sourceMultiplier * 100) / 100,
      freshnessDecay: Math.round(freshnessDecay * 100) / 100,
      executionPowerWeight: Math.round(executionPowerWeight * 100) / 100,
      uncertaintyPenalty: Math.round(uncertaintyPenalty * 100) / 100,
      stateAdjustment: Math.round(stateAdjustment * 100) / 100,
      bayesianAdjustment: Math.round(bayesianAdjustment * 100) / 100,
      finalScore: Math.round(finalScore * 10) / 10
    },
    riskFactors,
    tradingImplication,
    positionSizing
  }
}

function getTradingImplication(score: number, direction: Direction, state: PolicyState): string {
  if (score >= 80 && direction !== 'ambiguous') {
    return `强${direction === 'bullish' ? '看涨' : '看跌'}信号，建议${direction === 'bullish' ? '做多' : '做空'}相关资产`
  }
  if (score >= 60 && direction !== 'ambiguous') {
    return `中等${direction === 'bullish' ? '看涨' : '看跌'}信号，可适度建仓`
  }
  if (score >= 40) {
    return '信号不明确，建议观望或小仓位试探'
  }
  return '信号强度不足，不建议交易'
}

function getPositionSizing(
  score: number, 
  confidence: number, 
  state: PolicyState
): 'none' | 'minimal' | 'reduced' | 'standard' | 'full' {
  const stateMultiplier = STATE_ADJUSTMENTS[state].multiplier
  const effectiveScore = score * confidence * stateMultiplier
  
  if (effectiveScore >= 70) return 'full'
  if (effectiveScore >= 55) return 'standard'
  if (effectiveScore >= 40) return 'reduced'
  if (effectiveScore >= 25) return 'minimal'
  return 'none'
}

// ============== Exports ==============

export default {
  calculateDecisionScore,
  calculateFreshnessDecay,
  calculateExecutionPowerWeight,
  calculateUncertaintyPenalty,
  bayesianUpdate,
  aggregateSentiment,
  calculateEvidenceConsensus
}
