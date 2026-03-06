/**
 * 政策状态机服务
 * 强制绑定评分与状态，禁止脱钩
 */

export type PolicyState = 
  | 'monitoring'       // 评分 < 20 或 null
  | 'signal_detected'  // 评分 20-40
  | 'policy_forming'   // 评分 40-60
  | 'policy_confirmed' // 评分 60-80 + L0 证据
  | 'implementation'   // 评分 > 80 + L0 证据 + 官方确认
  | 'frozen'           // 数据不足，冻结上一状态

export interface StateTransitionCondition {
  type: 'score_range' | 'has_l0_evidence' | 'time_in_state' | 'no_contradicting_evidence' | 'score_stable'
  // score_range
  min?: number
  max?: number
  // has_l0_evidence
  count?: number
  // time_in_state
  minHours?: number
  // score_stable
  windowHours?: number
  maxDrift?: number
}

export interface StateTransitionRule {
  from: PolicyState | '*'
  to: PolicyState
  conditions: StateTransitionCondition[]
  required: 'all' | 'any'
}

export interface StateContext {
  score: number | null
  l0EvidenceCount: number
  l0_5EvidenceCount: number
  l1EvidenceCount: number
  l2EvidenceCount: number
  hasContradiction: boolean
  scoreHistory: { value: number, at: Date }[]
}

export interface StateChangeEvent {
  topicId: string
  fromState: PolicyState
  toState: PolicyState
  timestamp: Date
  triggeredBy: {
    scoreValue: number | null
    evidenceIds: string[]
    conditionsMet: string[]
  }
}

export interface StateHistoryEntry {
  state: PolicyState
  enteredAt: Date
  exitedAt?: Date
  reason: string
}

// 状态配置
export const STATE_CONFIG: Record<PolicyState, {
  label: string
  labelEn: string
  description: string
  minScore: number | null
  maxScore: number | null
  color: string
  bgColor: string
}> = {
  monitoring: {
    label: '监控中',
    labelEn: 'Monitoring',
    description: '当前无足够信号，持续观察',
    minScore: null,
    maxScore: 19,
    color: 'text-gray-600',
    bgColor: 'bg-gray-100'
  },
  signal_detected: {
    label: '信号检测',
    labelEn: 'Signal Detected',
    description: '检测到初步信号，需要进一步确认',
    minScore: 20,
    maxScore: 39,
    color: 'text-blue-600',
    bgColor: 'bg-blue-100'
  },
  policy_forming: {
    label: '政策形成中',
    labelEn: 'Policy Forming',
    description: '政策正在讨论或起草阶段',
    minScore: 40,
    maxScore: 59,
    color: 'text-yellow-600',
    bgColor: 'bg-yellow-100'
  },
  policy_confirmed: {
    label: '政策已确认',
    labelEn: 'Policy Confirmed',
    description: '政策已由官方来源确认',
    minScore: 60,
    maxScore: 79,
    color: 'text-orange-600',
    bgColor: 'bg-orange-100'
  },
  implementation: {
    label: '实施中',
    labelEn: 'Implementation',
    description: '政策正在执行或已生效',
    minScore: 80,
    maxScore: 100,
    color: 'text-red-600',
    bgColor: 'bg-red-100'
  },
  frozen: {
    label: '已冻结',
    labelEn: 'Frozen',
    description: '数据不足，保持上一状态',
    minScore: null,
    maxScore: null,
    color: 'text-gray-500',
    bgColor: 'bg-gray-200'
  }
}

// 状态转移规则
const STATE_TRANSITIONS: StateTransitionRule[] = [
  // monitoring → signal_detected
  {
    from: 'monitoring',
    to: 'signal_detected',
    conditions: [
      { type: 'score_range', min: 20, max: 100 }
    ],
    required: 'all'
  },
  
  // signal_detected → policy_forming
  {
    from: 'signal_detected',
    to: 'policy_forming',
    conditions: [
      { type: 'score_range', min: 40, max: 100 },
      { type: 'time_in_state', minHours: 1 }  // 防止瞬间跳转
    ],
    required: 'all'
  },
  
  // policy_forming → policy_confirmed
  {
    from: 'policy_forming',
    to: 'policy_confirmed',
    conditions: [
      { type: 'score_range', min: 60, max: 100 },
      { type: 'has_l0_evidence', count: 1 },  // 必须有 L0 证据
      { type: 'score_stable', windowHours: 2, maxDrift: 10 }
    ],
    required: 'all'
  },
  
  // policy_confirmed → implementation
  {
    from: 'policy_confirmed',
    to: 'implementation',
    conditions: [
      { type: 'score_range', min: 80, max: 100 },
      { type: 'has_l0_evidence', count: 2 },  // 需要多个 L0 证据
      { type: 'no_contradicting_evidence' }
    ],
    required: 'all'
  },
  
  // 降级规则
  {
    from: 'signal_detected',
    to: 'monitoring',
    conditions: [
      { type: 'score_range', min: 0, max: 19 }
    ],
    required: 'all'
  },
  {
    from: 'policy_forming',
    to: 'signal_detected',
    conditions: [
      { type: 'score_range', min: 20, max: 39 }
    ],
    required: 'all'
  },
  {
    from: 'policy_confirmed',
    to: 'policy_forming',
    conditions: [
      { type: 'score_range', min: 40, max: 59 }
    ],
    required: 'all'
  },
  {
    from: 'implementation',
    to: 'policy_confirmed',
    conditions: [
      { type: 'score_range', min: 60, max: 79 }
    ],
    required: 'all'
  }
]

/**
 * 政策状态机
 * 核心原则：状态由评分驱动，不可手动覆盖
 */
export class PolicyStateMachine {
  private state: PolicyState = 'monitoring'
  private stateEnteredAt: Date = new Date()
  private stateHistory: StateHistoryEntry[] = []
  private topicId: string
  
  constructor(topicId: string, initialState?: PolicyState) {
    this.topicId = topicId
    if (initialState) {
      this.state = initialState
    }
  }
  
  /**
   * 获取当前状态
   */
  getState(): PolicyState {
    return this.state
  }
  
  /**
   * 获取状态配置
   */
  getStateConfig(): typeof STATE_CONFIG[PolicyState] {
    return STATE_CONFIG[this.state]
  }
  
  /**
   * 获取状态历史
   */
  getHistory(): StateHistoryEntry[] {
    return [...this.stateHistory]
  }
  
  /**
   * 获取在当前状态的时间（小时）
   */
  getTimeInState(): number {
    return (Date.now() - this.stateEnteredAt.getTime()) / (1000 * 60 * 60)
  }
  
  /**
   * 🔴 核心方法：根据评分和上下文更新状态
   * 这是唯一允许改变状态的方法
   */
  updateFromContext(context: StateContext): {
    newState: PolicyState
    transitioned: boolean
    reason: string
    conditionsMet: string[]
  } {
    const { score } = context
    
    // 🔴 规则 1：评分为 null 时冻结
    if (score === null) {
      if (this.state !== 'frozen') {
        return this.transitionTo('frozen', 'Score is null (insufficient data)', [])
      }
      return {
        newState: 'frozen',
        transitioned: false,
        reason: 'Score is null (insufficient data)',
        conditionsMet: []
      }
    }
    
    // 🔴 规则 2：从冻结状态恢复
    if (this.state === 'frozen' && score !== null) {
      const targetState = this.getStateForScore(score)
      return this.transitionTo(targetState, `Recovered from frozen with score ${score}`, ['score_recovered'])
    }
    
    // 🔴 规则 3：检查所有可能的转移
    for (const rule of STATE_TRANSITIONS) {
      if (rule.from !== this.state && rule.from !== '*') continue
      if (rule.to === this.state) continue
      
      const result = this.checkConditions(rule.conditions, context, rule.required)
      if (result.met) {
        return this.transitionTo(rule.to, result.reason, result.conditionsMet)
      }
    }
    
    // 没有转移发生
    return {
      newState: this.state,
      transitioned: false,
      reason: 'No transition conditions met',
      conditionsMet: []
    }
  }
  
  /**
   * 根据评分获取应该处于的状态
   */
  getStateForScore(score: number | null): PolicyState {
    if (score === null || !Number.isFinite(score)) return 'frozen'
    if (score >= 80) return 'implementation'
    if (score >= 60) return 'policy_confirmed'
    if (score >= 40) return 'policy_forming'
    if (score >= 20) return 'signal_detected'
    return 'monitoring'
  }
  
  /**
   * 验证当前状态与评分是否一致
   */
  isConsistent(score: number | null): boolean {
    const expectedState = this.getStateForScore(score)
    // 允许一些偏差：某些状态需要额外条件（如 L0 证据）
    // 所以可能评分达到了但状态还没转移
    return this.state === expectedState || this.state === 'frozen'
  }
  
  private transitionTo(
    newState: PolicyState, 
    reason: string,
    conditionsMet: string[]
  ): {
    newState: PolicyState
    transitioned: boolean
    reason: string
    conditionsMet: string[]
  } {
    const oldState = this.state
    
    // 记录历史
    this.stateHistory.push({
      state: oldState,
      enteredAt: this.stateEnteredAt,
      exitedAt: new Date(),
      reason: `Exited: ${reason}`
    })
    
    // 更新状态
    this.state = newState
    this.stateEnteredAt = new Date()
    
    console.log(`[PolicyStateMachine] ${this.topicId}: ${oldState} → ${newState} (${reason})`)
    
    return {
      newState,
      transitioned: true,
      reason,
      conditionsMet
    }
  }
  
  private checkConditions(
    conditions: StateTransitionCondition[],
    context: StateContext,
    required: 'all' | 'any'
  ): { met: boolean, reason: string, conditionsMet: string[] } {
    const results: { condition: StateTransitionCondition, met: boolean, reason: string }[] = []
    
    for (const condition of conditions) {
      const result = this.checkSingleCondition(condition, context)
      results.push({ condition, ...result })
    }
    
    const conditionsMet = results.filter(r => r.met).map(r => r.reason)
    
    if (required === 'all') {
      const allMet = results.every(r => r.met)
      return {
        met: allMet,
        reason: allMet ? conditionsMet.join(', ') : `Not all conditions met`,
        conditionsMet
      }
    } else {
      const anyMet = results.some(r => r.met)
      return {
        met: anyMet,
        reason: anyMet ? conditionsMet.join(', ') : `No conditions met`,
        conditionsMet
      }
    }
  }
  
  private checkSingleCondition(
    condition: StateTransitionCondition,
    context: StateContext
  ): { met: boolean, reason: string } {
    switch (condition.type) {
      case 'score_range': {
        const { min = 0, max = 100 } = condition
        const score = context.score ?? -1
        const met = score >= min && score <= max
        return {
          met,
          reason: `score_range(${min}-${max}): ${score}`
        }
      }
      
      case 'has_l0_evidence': {
        const required = condition.count ?? 1
        const met = context.l0EvidenceCount >= required
        return {
          met,
          reason: `has_l0_evidence(${required}): ${context.l0EvidenceCount}`
        }
      }
      
      case 'time_in_state': {
        const minHours = condition.minHours ?? 0
        const hoursInState = this.getTimeInState()
        const met = hoursInState >= minHours
        return {
          met,
          reason: `time_in_state(${minHours}h): ${hoursInState.toFixed(2)}h`
        }
      }
      
      case 'no_contradicting_evidence': {
        const met = !context.hasContradiction
        return {
          met,
          reason: `no_contradiction: ${!context.hasContradiction}`
        }
      }
      
      case 'score_stable': {
        const { windowHours = 2, maxDrift = 10 } = condition
        const cutoff = new Date(Date.now() - windowHours * 60 * 60 * 1000)
        const recentScores = context.scoreHistory.filter(s => s.at >= cutoff)
        
        if (recentScores.length < 2) {
          return { met: false, reason: 'score_stable: insufficient history' }
        }
        
        const values = recentScores.map(s => s.value)
        const max = Math.max(...values)
        const min = Math.min(...values)
        const drift = max - min
        const met = drift <= maxDrift
        
        return {
          met,
          reason: `score_stable(${windowHours}h, drift ${maxDrift}): drift ${drift.toFixed(1)}`
        }
      }
      
      default:
        return { met: false, reason: `unknown condition type` }
    }
  }
  
  /**
   * 导出状态快照（用于持久化）
   */
  exportSnapshot(): {
    state: PolicyState
    stateEnteredAt: string
    history: StateHistoryEntry[]
  } {
    return {
      state: this.state,
      stateEnteredAt: this.stateEnteredAt.toISOString(),
      history: this.stateHistory.map(h => ({
        ...h,
        enteredAt: h.enteredAt,
        exitedAt: h.exitedAt
      }))
    }
  }
  
  /**
   * 从快照恢复（仅限初始化时使用）
   */
  static fromSnapshot(topicId: string, snapshot: {
    state: PolicyState
    stateEnteredAt: string
    history: StateHistoryEntry[]
  }): PolicyStateMachine {
    const machine = new PolicyStateMachine(topicId)
    machine.state = snapshot.state
    machine.stateEnteredAt = new Date(snapshot.stateEnteredAt)
    machine.stateHistory = snapshot.history
    return machine
  }
}

export default PolicyStateMachine
