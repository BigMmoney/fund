/**
 * 稳定列表渲染 Hook
 * 解决：列表 key 不稳定导致的组件 unmount/mount 闪烁
 */

import { useRef, useMemo, useCallback } from 'react'
import { shallowEqual } from '@/lib/realtimeBuffer'

// ============== 类型定义 ==============

export interface StableItem<T> {
  key: string           // 稳定的 React key
  data: T               // 实际数据
  isNew: boolean        // 是否为新增项
  isUpdated: boolean    // 是否有更新
  insertTime: number    // 插入时间
}

export interface UseStableListOptions<T> {
  idKey: keyof T                     // ID 字段
  maxItems?: number                  // 最大条数
  newItemDuration?: number           // 新项标记持续时间 (ms)
  compareFields?: (keyof T)[]        // 用于比较的字段
}

// ============== Hook ==============

export function useStableList<T extends Record<string, any>>(
  items: T[],
  options: UseStableListOptions<T>
): StableItem<T>[] {
  const { 
    idKey, 
    maxItems = 100, 
    newItemDuration = 3000,
    compareFields 
  } = options

  // 持久化存储
  const itemMapRef = useRef<Map<string, { data: T; insertTime: number }>>(new Map())
  const keyCounterRef = useRef(0)
  const stableKeyMapRef = useRef<Map<string, string>>(new Map())

  return useMemo(() => {
    const now = Date.now()
    const itemMap = itemMapRef.current
    const stableKeyMap = stableKeyMapRef.current
    const result: StableItem<T>[] = []
    const seenIds = new Set<string>()

    for (const item of items) {
      const id = String(item[idKey])
      seenIds.add(id)

      // 获取或创建稳定 key
      if (!stableKeyMap.has(id)) {
        stableKeyMap.set(id, `stable-${keyCounterRef.current++}`)
      }
      const stableKey = stableKeyMap.get(id)!

      // 检查是否为新项或更新
      const existing = itemMap.get(id)
      const isNew = !existing
      let isUpdated = false

      if (existing) {
        // 检查是否有更新
        if (compareFields) {
          isUpdated = compareFields.some(field => existing.data[field] !== item[field])
        } else {
          isUpdated = !shallowEqual(existing.data, item)
        }
      }

      // 更新存储
      itemMap.set(id, {
        data: item,
        insertTime: isNew ? now : existing!.insertTime
      })

      const insertTime = itemMap.get(id)!.insertTime
      const isStillNew = now - insertTime < newItemDuration

      result.push({
        key: stableKey,
        data: item,
        isNew: isStillNew,
        isUpdated,
        insertTime
      })
    }

    // 清理已移除的项
    for (const [id] of itemMap) {
      if (!seenIds.has(id)) {
        itemMap.delete(id)
        // 保留 stableKey 映射以防项目重新出现
      }
    }

    // 限制数量
    if (result.length > maxItems) {
      const removed = result.splice(maxItems)
      for (const item of removed) {
        const id = String(item.data[idKey])
        itemMap.delete(id)
      }
    }

    return result
  }, [items, idKey, maxItems, newItemDuration, compareFields])
}

// ============== 稳定排序 Hook ==============

export interface UseSortedListOptions<T> {
  sortKey: keyof T
  direction: 'asc' | 'desc'
  stableSort?: boolean  // 相等时保持原顺序
}

export function useStableSortedList<T extends Record<string, any>>(
  items: T[],
  options: UseSortedListOptions<T>
): T[] {
  const { sortKey, direction, stableSort = true } = options
  const orderMapRef = useRef<Map<any, number>>(new Map())

  return useMemo(() => {
    // 记录首次出现顺序
    items.forEach((item, index) => {
      const id = item[sortKey]
      if (!orderMapRef.current.has(id)) {
        orderMapRef.current.set(id, index)
      }
    })

    return [...items].sort((a, b) => {
      const aVal = a[sortKey]
      const bVal = b[sortKey]

      if (aVal === bVal && stableSort) {
        // 相等时按首次出现顺序
        const aOrder = orderMapRef.current.get(a[sortKey]) ?? 0
        const bOrder = orderMapRef.current.get(b[sortKey]) ?? 0
        return aOrder - bOrder
      }

      if (typeof aVal === 'number' && typeof bVal === 'number') {
        return direction === 'asc' ? aVal - bVal : bVal - aVal
      }

      const aStr = String(aVal)
      const bStr = String(bVal)
      return direction === 'asc' 
        ? aStr.localeCompare(bStr) 
        : bStr.localeCompare(aStr)
    })
  }, [items, sortKey, direction, stableSort])
}

// ============== 分组列表 Hook ==============

export interface GroupedItem<T> {
  groupKey: string
  groupLabel: string
  items: StableItem<T>[]
  isCollapsed: boolean
}

export function useGroupedList<T extends Record<string, any>>(
  items: T[],
  groupKey: keyof T,
  idKey: keyof T
): { groups: GroupedItem<T>[]; toggleGroup: (key: string) => void } {
  const collapsedRef = useRef<Set<string>>(new Set())
  
  const stableItems = useStableList(items, { idKey })

  const groups = useMemo(() => {
    const groupMap = new Map<string, StableItem<T>[]>()

    for (const item of stableItems) {
      const key = String(item.data[groupKey])
      if (!groupMap.has(key)) {
        groupMap.set(key, [])
      }
      groupMap.get(key)!.push(item)
    }

    return Array.from(groupMap.entries()).map(([key, groupItems]) => ({
      groupKey: key,
      groupLabel: key,
      items: groupItems,
      isCollapsed: collapsedRef.current.has(key)
    }))
  }, [stableItems, groupKey])

  const toggleGroup = useCallback((key: string) => {
    if (collapsedRef.current.has(key)) {
      collapsedRef.current.delete(key)
    } else {
      collapsedRef.current.add(key)
    }
  }, [])

  return { groups, toggleGroup }
}

// ============== 虚拟化支持 ==============

export interface VirtualListItem<T> {
  key: string
  data: T
  index: number
  offsetTop: number
  height: number
}

export function useVirtualList<T extends Record<string, any>>(
  items: T[],
  options: {
    idKey: keyof T
    itemHeight: number
    containerHeight: number
    overscan?: number
  }
): {
  virtualItems: VirtualListItem<T>[]
  totalHeight: number
  scrollTo: (index: number) => void
} {
  const { idKey, itemHeight, containerHeight, overscan = 3 } = options
  const scrollTopRef = useRef(0)

  const stableItems = useStableList(items, { idKey })

  const virtualItems = useMemo(() => {
    const scrollTop = scrollTopRef.current
    const startIndex = Math.max(0, Math.floor(scrollTop / itemHeight) - overscan)
    const endIndex = Math.min(
      stableItems.length - 1,
      Math.ceil((scrollTop + containerHeight) / itemHeight) + overscan
    )

    const result: VirtualListItem<T>[] = []
    for (let i = startIndex; i <= endIndex; i++) {
      const item = stableItems[i]
      if (item) {
        result.push({
          key: item.key,
          data: item.data,
          index: i,
          offsetTop: i * itemHeight,
          height: itemHeight
        })
      }
    }
    return result
  }, [stableItems, itemHeight, containerHeight, overscan])

  const scrollTo = useCallback((index: number) => {
    scrollTopRef.current = index * itemHeight
  }, [itemHeight])

  return {
    virtualItems,
    totalHeight: stableItems.length * itemHeight,
    scrollTo
  }
}
