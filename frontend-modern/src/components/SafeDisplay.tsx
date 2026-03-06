/**
 * 安全数值显示组件
 * 防止 NaN 和无效值污染 UI
 */

import { Circle, AlertTriangle, TrendingUp, TrendingDown, Minus } from 'lucide-react'
import { safePercentChange, clamp } from '@/lib/safemath'

interface SafeNumberProps {
  value: number | null | undefined
  fallback?: string
  decimals?: number
  className?: string
  prefix?: string
  suffix?: string
}

/**
 * 安全数字显示 - 自动处理 NaN/null/undefined
 */
export function SafeNumber({ 
  value, 
  fallback = '--', 
  decimals = 2,
  className = '',
  prefix = '',
  suffix = ''
}: SafeNumberProps) {
  if (value === null || value === undefined || !Number.isFinite(value)) {
    return <span className={`text-gray-400 ${className}`}>{fallback}</span>
  }
  return <span className={className}>{prefix}{value.toFixed(decimals)}{suffix}</span>
}

interface SafePercentChangeProps {
  current: number | null | undefined
  previous: number | null | undefined
  maxDisplay?: number
  decimals?: number
  showIcon?: boolean
  className?: string
  fallbackDisplay?: string
}

/**
 * 安全百分比变化显示 - 永远不显示 NaN%
 */
export function SafePercentChange({ 
  current, 
  previous,
  maxDisplay = 999,
  decimals = 1,
  showIcon = true,
  className = '',
  fallbackDisplay = '--'
}: SafePercentChangeProps) {
  // 验证输入
  if (current === null || current === undefined || !Number.isFinite(current)) {
    return <span className={`text-gray-400 ${className}`}>{fallbackDisplay}</span>
  }
  if (previous === null || previous === undefined || !Number.isFinite(previous)) {
    return <span className={`text-gray-400 ${className}`}>{fallbackDisplay}</span>
  }
  
  const change = safePercentChange(current, previous)
  
  if (change === null) {
    return <span className={`text-gray-400 ${className}`}>N/A</span>
  }
  
  const displayChange = clamp(change, -maxDisplay, maxDisplay)
  const isPositive = displayChange > 0
  const isNegative = displayChange < 0
  const isCapped = Math.abs(change) > maxDisplay
  
  const colorClass = isPositive 
    ? 'text-emerald-500' 
    : isNegative 
      ? 'text-red-500' 
      : 'text-gray-500'
  
  const Icon = isPositive ? TrendingUp : isNegative ? TrendingDown : Minus
  
  return (
    <span className={`inline-flex items-center gap-1 ${colorClass} ${className}`}>
      {showIcon && <Icon className="w-3 h-3" />}
      <span>
        {isPositive ? '+' : ''}{displayChange.toFixed(decimals)}%
        {isCapped && <sup>+</sup>}
      </span>
    </span>
  )
}

type PolicyState = 
  | 'monitoring'
  | 'signal_detected'
  | 'policy_forming'
  | 'policy_confirmed'
  | 'implementation'
  | 'frozen'

interface ScoreDisplayProps {
  score: number | null | undefined
  status?: 'valid' | 'insufficient_data' | 'frozen' | 'conflicting'
  showMax?: boolean
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

/**
 * 评分显示组件 - 区分 0分/不可计算/冻结态
 */
export function ScoreDisplay({ 
  score, 
  status = 'valid',
  showMax = true,
  size = 'md',
  className = ''
}: ScoreDisplayProps) {
  const sizeClasses = {
    sm: 'text-lg',
    md: 'text-2xl',
    lg: 'text-4xl'
  }
  
  // 不可计算状态
  if (score === null || score === undefined || !Number.isFinite(score)) {
    return (
      <div className={`flex items-center gap-2 text-gray-400 ${className}`}>
        <Circle className="w-4 h-4" />
        <span className="text-sm">
          {status === 'frozen' ? '已冻结' : '数据不足'}
        </span>
      </div>
    )
  }
  
  // 零分状态（有意义的零，表示无信号）
  if (score === 0) {
    return (
      <div className={`flex items-center gap-2 text-gray-500 ${className}`}>
        <span className={`font-bold ${sizeClasses[size]}`}>0</span>
        {showMax && <span className="text-xs text-gray-400">/100</span>}
        <span className="text-xs text-gray-400 ml-1">无信号</span>
      </div>
    )
  }
  
  // 正常评分
  const getScoreColor = (s: number) => {
    if (s >= 80) return 'text-red-600'
    if (s >= 60) return 'text-orange-500'
    if (s >= 40) return 'text-yellow-500'
    if (s >= 20) return 'text-blue-500'
    return 'text-gray-500'
  }
  
  return (
    <div className={`flex items-center gap-1 ${className}`}>
      <span className={`font-bold ${sizeClasses[size]} ${getScoreColor(score)}`}>
        {score.toFixed(0)}
      </span>
      {showMax && <span className="text-xs text-gray-400">/100</span>}
      {status === 'conflicting' && (
        <span title="数据冲突">
          <AlertTriangle className="w-4 h-4 text-amber-500 ml-1" />
        </span>
      )}
    </div>
  )
}

interface PolicyStateDisplayProps {
  state: PolicyState
  score: number | null | undefined
  showWarning?: boolean
  className?: string
}

const STATE_CONFIG: Record<PolicyState, {
  label: string
  labelEn: string
  color: string
  bgColor: string
  minScore: number | null
  maxScore: number | null
}> = {
  monitoring: {
    label: '监控中',
    labelEn: 'Monitoring',
    color: 'text-gray-600',
    bgColor: 'bg-gray-100',
    minScore: null,
    maxScore: 19
  },
  signal_detected: {
    label: '信号检测',
    labelEn: 'Signal Detected',
    color: 'text-blue-600',
    bgColor: 'bg-blue-100',
    minScore: 20,
    maxScore: 39
  },
  policy_forming: {
    label: '政策形成中',
    labelEn: 'Policy Forming',
    color: 'text-yellow-600',
    bgColor: 'bg-yellow-100',
    minScore: 40,
    maxScore: 59
  },
  policy_confirmed: {
    label: '政策已确认',
    labelEn: 'Policy Confirmed',
    color: 'text-orange-600',
    bgColor: 'bg-orange-100',
    minScore: 60,
    maxScore: 79
  },
  implementation: {
    label: '实施中',
    labelEn: 'Implementation',
    color: 'text-red-600',
    bgColor: 'bg-red-100',
    minScore: 80,
    maxScore: 100
  },
  frozen: {
    label: '已冻结',
    labelEn: 'Frozen',
    color: 'text-gray-500',
    bgColor: 'bg-gray-200',
    minScore: null,
    maxScore: null
  }
}

/**
 * 获取评分对应的预期状态
 */
export function getExpectedState(score: number | null | undefined): PolicyState {
  if (score === null || score === undefined || !Number.isFinite(score)) {
    return 'frozen'
  }
  if (score >= 80) return 'implementation'
  if (score >= 60) return 'policy_confirmed'
  if (score >= 40) return 'policy_forming'
  if (score >= 20) return 'signal_detected'
  return 'monitoring'
}

/**
 * 政策状态显示组件 - 强制验证状态与评分一致性
 */
export function PolicyStateDisplay({ 
  state, 
  score,
  showWarning = true,
  className = ''
}: PolicyStateDisplayProps) {
  const config = STATE_CONFIG[state]
  const expectedState = getExpectedState(score)
  const isConsistent = state === expectedState
  
  return (
    <div className={`inline-flex items-center gap-2 ${className}`}>
      <span className={`px-2 py-1 rounded-full text-xs font-medium ${config.bgColor} ${config.color}`}>
        {config.label}
      </span>
      {showWarning && !isConsistent && (
        <div className="flex items-center gap-1 text-amber-500" title={`评分 ${score} 预期状态为 ${STATE_CONFIG[expectedState].label}`}>
          <AlertTriangle className="w-4 h-4" />
          <span className="text-xs">状态不一致</span>
        </div>
      )}
    </div>
  )
}

interface SignalValueProps {
  value: number | null | undefined
  previousValue?: number | null
  label?: string
  unit?: string
  showChange?: boolean
  className?: string
}

/**
 * 信号值显示组件 - 带变化指示
 */
export function SignalValue({
  value,
  previousValue,
  label,
  unit = '',
  showChange = true,
  className = ''
}: SignalValueProps) {
  const isValid = value !== null && value !== undefined && Number.isFinite(value)
  
  if (!isValid) {
    return (
      <div className={`flex flex-col ${className}`}>
        {label && <span className="text-xs text-gray-400">{label}</span>}
        <span className="text-gray-400">--</span>
      </div>
    )
  }
  
  return (
    <div className={`flex flex-col ${className}`}>
      {label && <span className="text-xs text-gray-400">{label}</span>}
      <div className="flex items-center gap-2">
        <span className="font-medium">{value.toFixed(2)}{unit}</span>
        {showChange && previousValue !== undefined && (
          <SafePercentChange 
            current={value} 
            previous={previousValue}
            className="text-xs"
          />
        )}
      </div>
    </div>
  )
}

interface LayerScoreDisplayProps {
  layerScores: {
    L0?: { score: number | null, evidenceCount: number, weight: number }
    L0_5?: { score: number | null, evidenceCount: number, weight: number }
    L1?: { score: number | null, evidenceCount: number, weight: number }
    L2?: { score: number | null, evidenceCount: number, weight: number }
  }
  className?: string
}

/**
 * 分层评分显示 - 显示各层级的贡献
 */
export function LayerScoreDisplay({ layerScores, className = '' }: LayerScoreDisplayProps) {
  const layers = [
    { key: 'L0', label: 'L0 (官方)', data: layerScores.L0 },
    { key: 'L0_5', label: 'L0.5 (央行/监管)', data: layerScores.L0_5 },
    { key: 'L1', label: 'L1 (一级媒体)', data: layerScores.L1 },
    { key: 'L2', label: 'L2 (二级来源)', data: layerScores.L2 },
  ]
  
  return (
    <div className={`space-y-2 ${className}`}>
      {layers.map(({ key, label, data }) => {
        if (!data) return null
        
        const hasScore = data.score !== null && Number.isFinite(data.score)
        const contributionPercent = (data.weight * 100).toFixed(0)
        
        return (
          <div key={key} className="flex items-center justify-between text-sm">
            <div className="flex items-center gap-2">
              <span className="font-medium">{label}</span>
              <span className="text-gray-400 text-xs">{data.evidenceCount} 条</span>
            </div>
            <div className="flex items-center gap-2">
              {hasScore ? (
                <>
                  <span className="font-medium">{data.score!.toFixed(0)}</span>
                  <span className="text-xs text-gray-400">权重 {contributionPercent}%</span>
                </>
              ) : (
                <span className="text-xs text-gray-400 italic">未参与计算（样本不足）</span>
              )}
            </div>
          </div>
        )
      })}
    </div>
  )
}

export default {
  SafeNumber,
  SafePercentChange,
  ScoreDisplay,
  PolicyStateDisplay,
  SignalValue,
  LayerScoreDisplay,
  getExpectedState
}
