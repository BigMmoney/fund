import { useEffect, useCallback, useRef } from 'react'

/**
 * 键盘快捷键定义
 */
export interface KeyboardShortcut {
  key: string                    // 主键，如 'r', '1', 'Escape'
  ctrlKey?: boolean              // 是否需要 Ctrl
  shiftKey?: boolean             // 是否需要 Shift
  altKey?: boolean               // 是否需要 Alt
  metaKey?: boolean              // 是否需要 Meta (Cmd on Mac)
  action: () => void             // 执行的动作
  description?: string           // 快捷键描述
  preventDefault?: boolean       // 是否阻止默认行为
  disabled?: boolean             // 是否禁用
}

/**
 * 快捷键组合字符串转为对象
 * 例如: 'Ctrl+Shift+R' => { key: 'r', ctrlKey: true, shiftKey: true }
 */
export function parseShortcut(shortcut: string): Partial<KeyboardShortcut> {
  const parts = shortcut.toLowerCase().split('+')
  const result: Partial<KeyboardShortcut> = {}
  
  parts.forEach(part => {
    switch (part) {
      case 'ctrl':
        result.ctrlKey = true
        break
      case 'shift':
        result.shiftKey = true
        break
      case 'alt':
        result.altKey = true
        break
      case 'meta':
      case 'cmd':
        result.metaKey = true
        break
      default:
        result.key = part
    }
  })
  
  return result
}

/**
 * 检查键盘事件是否匹配快捷键
 */
function matchesShortcut(event: KeyboardEvent, shortcut: KeyboardShortcut): boolean {
  const key = event.key.toLowerCase()
  const expectedKey = shortcut.key.toLowerCase()
  
  // 检查主键是否匹配
  if (key !== expectedKey) {
    // 也检查数字键
    if (event.code === `Digit${shortcut.key}` || event.code === `Numpad${shortcut.key}`) {
      // 继续检查修饰键
    } else {
      return false
    }
  }
  
  // 检查修饰键
  if (shortcut.ctrlKey && !event.ctrlKey) return false
  if (shortcut.shiftKey && !event.shiftKey) return false
  if (shortcut.altKey && !event.altKey) return false
  if (shortcut.metaKey && !event.metaKey) return false
  
  // 如果快捷键不需要修饰键，但用户按了修饰键，不匹配
  if (!shortcut.ctrlKey && event.ctrlKey) return false
  if (!shortcut.shiftKey && event.shiftKey) return false
  if (!shortcut.altKey && event.altKey) return false
  if (!shortcut.metaKey && event.metaKey) return false
  
  return true
}

/**
 * 检查是否在输入框中
 */
function isInputFocused(): boolean {
  const activeElement = document.activeElement
  if (!activeElement) return false
  
  const tagName = activeElement.tagName.toLowerCase()
  if (tagName === 'input' || tagName === 'textarea' || tagName === 'select') {
    return true
  }
  
  if (activeElement.getAttribute('contenteditable') === 'true') {
    return true
  }
  
  return false
}

/**
 * 键盘快捷键 Hook
 * @param shortcuts 快捷键列表
 * @param enabled 是否启用
 */
export function useKeyboardShortcuts(
  shortcuts: KeyboardShortcut[],
  enabled: boolean = true
): void {
  const shortcutsRef = useRef(shortcuts)
  shortcutsRef.current = shortcuts
  
  const handleKeyDown = useCallback((event: KeyboardEvent) => {
    // 如果正在输入，不处理快捷键（除了 Escape）
    if (isInputFocused() && event.key !== 'Escape') {
      return
    }
    
    for (const shortcut of shortcutsRef.current) {
      if (shortcut.disabled) continue
      
      if (matchesShortcut(event, shortcut)) {
        if (shortcut.preventDefault !== false) {
          event.preventDefault()
        }
        shortcut.action()
        break
      }
    }
  }, [])
  
  useEffect(() => {
    if (!enabled) return
    
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [enabled, handleKeyDown])
}

/**
 * 预定义的快捷键常量
 */
export const SHORTCUT_KEYS = {
  REFRESH: 'r',
  ESCAPE: 'Escape',
  HELP: '?',
  SEARCH: '/',
  TAB_1: '1',
  TAB_2: '2',
  TAB_3: '3',
  TAB_4: '4',
  TAB_5: '5',
  TAB_6: '6',
  TAB_7: '7',
  TAB_8: '8',
  TAB_9: '9',
  NEXT: 'ArrowRight',
  PREV: 'ArrowLeft',
  UP: 'ArrowUp',
  DOWN: 'ArrowDown',
  ENTER: 'Enter',
} as const

/**
 * 创建 Tab 切换快捷键
 */
export function createTabShortcuts(
  tabs: string[],
  setActiveTab: (tab: string) => void,
  currentTab: string
): KeyboardShortcut[] {
  return tabs.slice(0, 9).map((tab, index) => ({
    key: String(index + 1),
    action: () => setActiveTab(tab),
    description: `Switch to ${tab} tab`,
    disabled: currentTab === tab
  }))
}

/**
 * 快捷键帮助面板组件使用的数据
 */
export interface ShortcutHelpItem {
  key: string
  description: string
  category?: string
}

export const NEWS_INTELLIGENCE_SHORTCUTS: ShortcutHelpItem[] = [
  { key: '1-9', description: '切换标签页', category: '导航' },
  { key: 'R', description: '刷新数据', category: '操作' },
  { key: 'Esc', description: '关闭弹窗', category: '操作' },
  { key: '?', description: '显示快捷键帮助', category: '帮助' },
  { key: '/', description: '聚焦搜索框', category: '搜索' },
]

export const TRADING_TERMINAL_SHORTCUTS: ShortcutHelpItem[] = [
  { key: 'B', description: '买入', category: '交易' },
  { key: 'S', description: '卖出', category: '交易' },
  { key: 'R', description: '刷新数据', category: '操作' },
  { key: 'Esc', description: '取消操作', category: '操作' },
]

export default useKeyboardShortcuts
