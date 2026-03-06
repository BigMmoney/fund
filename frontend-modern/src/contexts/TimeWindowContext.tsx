/**
 * 时间窗口上下文
 * 统一所有页面使用相同的时间窗口
 */

import { createContext, useContext, useState, useCallback, ReactNode } from 'react'
import { getItem, setItem, STORAGE_KEYS } from '@/services/persistenceService'

export type TimeWindow = '1h' | '6h' | '24h' | '7d' | '30d'

interface TimeWindowContextType {
  window: TimeWindow
  setWindow: (window: TimeWindow) => void
  windowMs: number
  windowLabel: string
  windowLabelEn: string
}

const TIME_WINDOW_CONFIG: Record<TimeWindow, {
  ms: number
  label: string
  labelEn: string
}> = {
  '1h': { ms: 1 * 60 * 60 * 1000, label: '1小时', labelEn: '1 Hour' },
  '6h': { ms: 6 * 60 * 60 * 1000, label: '6小时', labelEn: '6 Hours' },
  '24h': { ms: 24 * 60 * 60 * 1000, label: '24小时', labelEn: '24 Hours' },
  '7d': { ms: 7 * 24 * 60 * 60 * 1000, label: '7天', labelEn: '7 Days' },
  '30d': { ms: 30 * 24 * 60 * 60 * 1000, label: '30天', labelEn: '30 Days' },
}

const DEFAULT_WINDOW: TimeWindow = '24h'

const TimeWindowContext = createContext<TimeWindowContextType>({
  window: DEFAULT_WINDOW,
  setWindow: () => {},
  windowMs: TIME_WINDOW_CONFIG[DEFAULT_WINDOW].ms,
  windowLabel: TIME_WINDOW_CONFIG[DEFAULT_WINDOW].label,
  windowLabelEn: TIME_WINDOW_CONFIG[DEFAULT_WINDOW].labelEn,
})

export function TimeWindowProvider({ children }: { children: ReactNode }) {
  const [window, setWindowState] = useState<TimeWindow>(() => {
    // 从 localStorage 恢复
    const saved = getItem(STORAGE_KEYS.TIME_WINDOW, null)
    if (saved && Object.keys(TIME_WINDOW_CONFIG).includes(saved)) {
      return saved as TimeWindow
    }
    return DEFAULT_WINDOW
  })

  const setWindow = useCallback((newWindow: TimeWindow) => {
    setWindowState(newWindow)
    setItem(STORAGE_KEYS.TIME_WINDOW, newWindow)
  }, [])

  const config = TIME_WINDOW_CONFIG[window]

  return (
    <TimeWindowContext.Provider
      value={{
        window,
        setWindow,
        windowMs: config.ms,
        windowLabel: config.label,
        windowLabelEn: config.labelEn,
      }}
    >
      {children}
    </TimeWindowContext.Provider>
  )
}

export function useTimeWindow() {
  return useContext(TimeWindowContext)
}

/**
 * 时间窗口选择器组件
 */
interface TimeWindowSelectorProps {
  className?: string
  showLabel?: boolean
  size?: 'sm' | 'md'
}

export function TimeWindowSelector({ 
  className = '', 
  showLabel = true,
  size = 'md'
}: TimeWindowSelectorProps) {
  const { window, setWindow } = useTimeWindow()
  
  const windows: TimeWindow[] = ['1h', '6h', '24h', '7d', '30d']
  
  const sizeClasses = {
    sm: 'text-[10px] px-1.5 py-0.5',
    md: 'text-xs px-2 py-1'
  }
  
  return (
    <div className={`flex items-center gap-1 ${className}`}>
      {showLabel && (
        <span className="text-xs text-gray-500 mr-1">窗口:</span>
      )}
      <div className="flex bg-gray-100 rounded-lg p-0.5">
        {windows.map(w => (
          <button
            key={w}
            onClick={() => setWindow(w)}
            className={`${sizeClasses[size]} rounded font-medium transition-colors ${
              window === w 
                ? 'bg-white shadow text-gray-900' 
                : 'text-gray-500 hover:text-gray-700'
            }`}
          >
            {w}
          </button>
        ))}
      </div>
    </div>
  )
}

/**
 * 获取时间窗口的开始时间
 */
export function getWindowStartTime(window: TimeWindow, asOf: Date = new Date()): Date {
  const config = TIME_WINDOW_CONFIG[window]
  return new Date(asOf.getTime() - config.ms)
}

/**
 * 检查时间戳是否在窗口内
 */
export function isWithinWindow(
  timestamp: Date | string, 
  window: TimeWindow, 
  asOf: Date = new Date()
): boolean {
  const t = typeof timestamp === 'string' ? new Date(timestamp) : timestamp
  const start = getWindowStartTime(window, asOf)
  return t >= start && t <= asOf
}

export default TimeWindowContext
