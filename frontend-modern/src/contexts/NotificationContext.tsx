import { createContext, useContext, useState, useCallback, ReactNode } from 'react'

export type NotificationType = 'success' | 'error' | 'warning' | 'info' | 'trade' | 'price'

export interface Notification {
  id: string
  type: NotificationType
  title: string
  message: string
  timestamp: Date
  read: boolean
  data?: any
}

interface NotificationContextType {
  notifications: Notification[]
  unreadCount: number
  addNotification: (type: NotificationType, title: string, message: string, data?: any) => void
  removeNotification: (id: string) => void
  markAsRead: (id: string) => void
  markAllAsRead: () => void
  clearAll: () => void
}

const NotificationContext = createContext<NotificationContextType | undefined>(undefined)

export function NotificationProvider({ children }: { children: ReactNode }) {
  const [notifications, setNotifications] = useState<Notification[]>([
    // 初始化一些示例通知
    {
      id: 'n1',
      type: 'trade',
      title: '订单成交 / Order Filled',
      message: 'BUY 0.5 BTC @ $95,150 已成交',
      timestamp: new Date(Date.now() - 60000),
      read: false,
      data: { symbol: 'BTC', side: 'BUY', qty: 0.5, price: 95150 }
    },
    {
      id: 'n2',
      type: 'price',
      title: '价格提醒 / Price Alert',
      message: 'ETH 突破 $3,500',
      timestamp: new Date(Date.now() - 120000),
      read: false,
      data: { symbol: 'ETH', price: 3500 }
    },
    {
      id: 'n3',
      type: 'success',
      title: '系统通知 / System',
      message: '平台连接成功，实时数据已启动',
      timestamp: new Date(Date.now() - 300000),
      read: true,
    }
  ])

  const addNotification = useCallback((type: NotificationType, title: string, message: string, data?: any) => {
    const newNotification: Notification = {
      id: `n_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`,
      type,
      title,
      message,
      timestamp: new Date(),
      read: false,
      data
    }
    setNotifications(prev => [newNotification, ...prev].slice(0, 50)) // 最多保留50条
  }, [])

  const removeNotification = useCallback((id: string) => {
    setNotifications(prev => prev.filter(n => n.id !== id))
  }, [])

  const markAsRead = useCallback((id: string) => {
    setNotifications(prev => prev.map(n => n.id === id ? { ...n, read: true } : n))
  }, [])

  const markAllAsRead = useCallback(() => {
    setNotifications(prev => prev.map(n => ({ ...n, read: true })))
  }, [])

  const clearAll = useCallback(() => {
    setNotifications([])
  }, [])

  const unreadCount = notifications.filter(n => !n.read).length

  return (
    <NotificationContext.Provider value={{
      notifications,
      unreadCount,
      addNotification,
      removeNotification,
      markAsRead,
      markAllAsRead,
      clearAll
    }}>
      {children}
    </NotificationContext.Provider>
  )
}

export function useNotifications() {
  const context = useContext(NotificationContext)
  if (context === undefined) {
    throw new Error('useNotifications must be used within a NotificationProvider')
  }
  return context
}
