/**
 * 评分-状态绑定状态机（增强版）
 * 
 * 核心规则：
 * 1. score = 0/null/NaN → 必须回退到 MONITORING
 * 2. 状态只能通过状态机方法变更
 * 3. 所有状态转换必须验证评分前置条件
 */

import { clamp } from './safemath'

// ============== 类型定义 ==============

export type BoundPolicyState = 
  | 'monitoring'      // 监控中，无行动
  | 'signal_detected' // 信号已检测
  | 'analyzing'       // 分析中
  | 'actionable'      // 可行动
  | 'executing'       // 执行中
  | 'confirmed'       // 已确认
  | 'exhausted'       // 已耗尽
  | 'frozen'          // 冻结（异常状态）

export interface StateScoreBinding {
  state: BoundPolicyState
  minScore: number
  maxScore: number
  label: string
  labelCn: string
  color: string
  canTrade: boolean
}

// ============== 核心配置：评分-状态强绑定 ==============

export const STATE_SCORE_BINDINGS: StateScoreBinding[] = [
  { 
    state: 'frozen', 
    minScore: -Infinity, 
    maxScore: -1, 
    label: 'FROZEN', 
    labelCn: '冻结',
    color: 'bg-gray-500',
    canTrade: false 
  },
  { 
    state: 'monitoring', 
    minScore: 0, 
    maxScore: 19.99, 
    label: 'MONITORING', 
    labelCn: '监控中',
    color: 'bg-gray-400',
    canTrade: false 
  },
  { 
    state: 'signal_detected', 
    minScore: 20, 
    maxScore: 34.99, 
    label: 'SIGNAL DETECTED', 
    labelCn: '信号检测',
    color: 'bg-blue-400',
    canTrade: false 
  },
  { 
    state: 'analyzing', 
    minScore: 35, 
    maxScore: 49.99, 
    label: 'ANALYZING', 
    labelCn: '分析中',
    color: 'bg-yellow-400',
    canTrade: false 
  },
  { 
    state: 'actionable', 
    minScore: 50, 
    maxScore: 64.99, 
    label: 'ACTIONABLE', 
    labelCn: '可行动',
    color: 'bg-orange-400',
    canTrade: true 
  },
  { 
    state: 'executing', 
    minScore: 65, 
    maxScore: 79.99, 
    label: 'EXECUTING', 
    labelCn: '执行中',
    color: 'bg-red-400',
    canTrade: true 
  },
  { 
    state: 'confirmed', 
    minScore: 80, 
    maxScore: 100, 
    label: 'CONFIRMED', 
    labelCn: '已确认',
    color: 'bg-green-500',
    canTrade: true 
  },
  { 
    state: 'exhausted', 
    minScore: 0, 
    maxScore: 19.99,  // 与 monitoring 相同区间，通过 flag 区分
    label: 'EXHAUSTED', 
    labelCn: '已耗尽',
    color: 'bg-purple-400',
    canTrade: false 
  },
]

// ============== 绑定状态机 ==============

export interface BoundStateMachineContext {
  score: number
  previousScore: number
  previousState: BoundPolicyState
  wasHighScore: boolean  // 曾经达到过 actionable 以上
  lastTransitionTime: number
  transitionCount: number
  isValid: boolean
}

export class BoundStateMachine {
  private context: BoundStateMachineContext
  private listeners: Set<(ctx: BoundStateMachineContext) => void> = new Set()

  constructor(initialScore: number = 0) {
    const sanitizedScore = this.sanitizeScore(initialScore)
    this.context = {
      score: sanitizedScore,
      previousScore: sanitizedScore,
      previousState: this.deriveState(sanitizedScore, false),
      wasHighScore: sanitizedScore >= 50,
      lastTransitionTime: Date.now(),
      transitionCount: 0,
      isValid: true
    }
  }

  // ============== 核心：净化评分 ==============

  /**
   * 评分净化规则：
   * - NaN → 0
   * - Infinity → 100
   * - -Infinity → 0
   * - null/undefined → 0
   * - 负数 → 0（触发 frozen 检查）
   */
  private sanitizeScore(score: number | null | undefined): number {
    if (score === null || score === undefined) return 0
    if (Number.isNaN(score)) return 0
    if (!Number.isFinite(score)) return score > 0 ? 100 : 0
    return clamp(score, -1, 100)  // -1 用于 frozen 状态
  }

  // ============== 核心：评分→状态映射 ==============

  /**
   * 根据评分推导状态
   * ⚠️ 这是唯一的状态决策逻辑
   */
  private deriveState(score: number, wasHighScore: boolean): BoundPolicyState {
    // 异常值检测
    if (score < 0) return 'frozen'
    
    // 高分回落到低分 → exhausted（而非 monitoring）
    if (wasHighScore && score < 20) {
      return 'exhausted'
    }

    // 正常评分区间映射
    if (score >= 80) return 'confirmed'
    if (score >= 65) return 'executing'
    if (score >= 50) return 'actionable'
    if (score >= 35) return 'analyzing'
    if (score >= 20) return 'signal_detected'
    
    return 'monitoring'
  }

  // ============== 公开 API ==============

  /**
   * 更新评分（唯一的状态变更入口）
   */
  updateScore(newScore: number | null | undefined): BoundStateMachineContext {
    const sanitized = this.sanitizeScore(newScore)
    const previousState = this.context.previousState
    const wasHighScore = this.context.wasHighScore || sanitized >= 50
    
    const newState = this.deriveState(sanitized, wasHighScore)
    
    const hasStateChange = newState !== previousState

    this.context = {
      score: sanitized,
      previousScore: this.context.score,
      previousState: newState,
      wasHighScore,
      lastTransitionTime: hasStateChange ? Date.now() : this.context.lastTransitionTime,
      transitionCount: hasStateChange 
        ? this.context.transitionCount + 1 
        : this.context.transitionCount,
      isValid: sanitized >= 0
    }

    // 通知监听者
    if (hasStateChange) {
      this.notifyListeners()
    }

    return this.context
  }

  /**
   * 获取当前状态
   */
  getState(): BoundPolicyState {
    return this.context.previousState
  }

  /**
   * 获取完整上下文
   */
  getContext(): Readonly<BoundStateMachineContext> {
    return { ...this.context }
  }

  /**
   * 获取状态配置
   */
  getStateConfig(): StateScoreBinding {
    const state = this.context.previousState
    return STATE_SCORE_BINDINGS.find(b => b.state === state) 
      || STATE_SCORE_BINDINGS[1] // fallback to monitoring
  }

  /**
   * 是否可交易
   */
  canTrade(): boolean {
    return this.getStateConfig().canTrade
  }

  /**
   * 订阅状态变化
   */
  subscribe(listener: (ctx: BoundStateMachineContext) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  /**
   * 重置到初始状态
   */
  reset(): void {
    this.context = {
      score: 0,
      previousScore: 0,
      previousState: 'monitoring',
      wasHighScore: false,
      lastTransitionTime: Date.now(),
      transitionCount: 0,
      isValid: true
    }
    this.notifyListeners()
  }

  private notifyListeners(): void {
    this.listeners.forEach(listener => listener(this.context))
  }

  // ============== 调试工具 ==============

  /**
   * 验证评分-状态一致性
   */
  validate(): { valid: boolean; errors: string[] } {
    const errors: string[] = []
    const { score, previousState } = this.context
    const expectedState = this.deriveState(score, this.context.wasHighScore)

    if (previousState !== expectedState) {
      errors.push(`State mismatch: score=${score} should be ${expectedState}, but is ${previousState}`)
    }

    if (score === 0 && previousState === 'confirmed') {
      errors.push('CRITICAL: score=0 but state=confirmed (impossible)')
    }

    if (score === 0 && previousState === 'executing') {
      errors.push('CRITICAL: score=0 but state=executing (impossible)')
    }

    if (Number.isNaN(score)) {
      errors.push('CRITICAL: score is NaN')
    }

    return { valid: errors.length === 0, errors }
  }

  /**
   * 生成审计日志
   */
  getAuditLog(): string {
    const config = this.getStateConfig()
    return `[StateMachine] score=${this.context.score.toFixed(1)} → ` +
      `state=${this.context.previousState} (${config.labelCn}) | ` +
      `canTrade=${this.canTrade()} | ` +
      `transitions=${this.context.transitionCount} | ` +
      `wasHighScore=${this.context.wasHighScore}`
  }
}

// ============== 工厂函数 ==============

/**
 * 创建评分绑定的状态机
 */
export function createBoundStateMachine(initialScore?: number): BoundStateMachine {
  return new BoundStateMachine(initialScore)
}

/**
 * 快速获取评分对应的状态（无状态机）
 */
export function getStateForScore(
  score: number | null | undefined, 
  wasHighScore: boolean = false
): BoundPolicyState {
  if (score === null || score === undefined || Number.isNaN(score)) {
    return 'monitoring'
  }
  if (score < 0) return 'frozen'
  if (wasHighScore && score < 20) return 'exhausted'
  if (score >= 80) return 'confirmed'
  if (score >= 65) return 'executing'
  if (score >= 50) return 'actionable'
  if (score >= 35) return 'analyzing'
  if (score >= 20) return 'signal_detected'
  return 'monitoring'
}

/**
 * 获取状态配置
 */
export function getStateConfig(state: BoundPolicyState): StateScoreBinding {
  return STATE_SCORE_BINDINGS.find(b => b.state === state) 
    || STATE_SCORE_BINDINGS[1]
}

// ============== React Hook ==============

import { useState, useCallback, useRef, useEffect } from 'react'

export function useBoundStateMachine(initialScore: number = 0) {
  const machineRef = useRef<BoundStateMachine>(new BoundStateMachine(initialScore))
  const [context, setContext] = useState(() => machineRef.current.getContext())

  useEffect(() => {
    return machineRef.current.subscribe(setContext)
  }, [])

  const updateScore = useCallback((score: number | null | undefined) => {
    return machineRef.current.updateScore(score)
  }, [])

  const reset = useCallback(() => {
    machineRef.current.reset()
  }, [])

  return {
    state: context.previousState,
    score: context.score,
    canTrade: machineRef.current.canTrade(),
    config: machineRef.current.getStateConfig(),
    context,
    updateScore,
    reset,
    validate: () => machineRef.current.validate(),
    getAuditLog: () => machineRef.current.getAuditLog()
  }
}
