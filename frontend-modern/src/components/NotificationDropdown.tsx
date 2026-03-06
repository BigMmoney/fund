import { useState, useRef, useEffect } from 'react'
import { Bell, X, Check, CheckCheck, Trash2, TrendingUp, DollarSign, AlertCircle, Info, CheckCircle } from 'lucide-react'
import { useNotifications, NotificationType } from '@/contexts/NotificationContext'

const getNotificationIcon = (type: NotificationType) => {
  switch (type) {
    case 'success': return <CheckCircle className="h-4 w-4 text-green-500" />
    case 'error': return <AlertCircle className="h-4 w-4 text-red-500" />
    case 'warning': return <AlertCircle className="h-4 w-4 text-yellow-500" />
    case 'trade': return <DollarSign className="h-4 w-4 text-blue-500" />
    case 'price': return <TrendingUp className="h-4 w-4 text-purple-500" />
    default: return <Info className="h-4 w-4 text-gray-500" />
  }
}

const formatTime = (date: Date) => {
  const now = new Date()
  const diff = now.getTime() - date.getTime()
  const minutes = Math.floor(diff / 60000)
  const hours = Math.floor(diff / 3600000)
  
  if (minutes < 1) return '刚刚 / Just now'
  if (minutes < 60) return `${minutes}分钟前 / ${minutes}m ago`
  if (hours < 24) return `${hours}小时前 / ${hours}h ago`
  return date.toLocaleDateString()
}

export function NotificationDropdown() {
  const [isOpen, setIsOpen] = useState(false)
  const dropdownRef = useRef<HTMLDivElement>(null)
  const { notifications, unreadCount, markAsRead, markAllAsRead, removeNotification, clearAll } = useNotifications()

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

  return (
    <div className="relative" ref={dropdownRef}>
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="relative p-2 rounded-lg hover:bg-accent transition-colors"
      >
        <Bell className="h-5 w-5" />
        {unreadCount > 0 && (
          <span className="absolute -top-1 -right-1 h-5 w-5 bg-red-500 text-white text-xs font-bold rounded-full flex items-center justify-center">
            {unreadCount > 9 ? '9+' : unreadCount}
          </span>
        )}
      </button>

      {isOpen && (
        <div className="absolute right-0 top-full mt-2 w-96 bg-card border border-border rounded-lg shadow-xl z-50 overflow-hidden">
          {/* Header */}
          <div className="px-4 py-3 border-b border-border flex items-center justify-between bg-accent/50">
            <div>
              <h3 className="font-semibold">通知 / Notifications</h3>
              <p className="text-xs text-muted-foreground">{unreadCount} 条未读</p>
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={markAllAsRead}
                className="p-1.5 rounded hover:bg-accent text-muted-foreground hover:text-foreground"
                title="全部已读"
              >
                <CheckCheck className="h-4 w-4" />
              </button>
              <button
                onClick={clearAll}
                className="p-1.5 rounded hover:bg-accent text-muted-foreground hover:text-foreground"
                title="清空全部"
              >
                <Trash2 className="h-4 w-4" />
              </button>
            </div>
          </div>

          {/* Notifications List */}
          <div className="max-h-80 overflow-y-auto">
            {notifications.length === 0 ? (
              <div className="py-8 text-center text-muted-foreground">
                <Bell className="h-8 w-8 mx-auto mb-2 opacity-50" />
                <p className="text-sm">暂无通知 / No notifications</p>
              </div>
            ) : (
              notifications.map(notification => (
                <div
                  key={notification.id}
                  className={`px-4 py-3 border-b border-border hover:bg-accent/50 transition-colors ${
                    !notification.read ? 'bg-primary/5' : ''
                  }`}
                >
                  <div className="flex items-start gap-3">
                    <div className="mt-0.5">
                      {getNotificationIcon(notification.type)}
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center justify-between gap-2">
                        <h4 className={`text-sm font-medium truncate ${!notification.read ? 'text-foreground' : 'text-muted-foreground'}`}>
                          {notification.title}
                        </h4>
                        <span className="text-xs text-muted-foreground shrink-0">
                          {formatTime(notification.timestamp)}
                        </span>
                      </div>
                      <p className="text-xs text-muted-foreground mt-0.5 line-clamp-2">
                        {notification.message}
                      </p>
                    </div>
                    <div className="flex items-center gap-1 shrink-0">
                      {!notification.read && (
                        <button
                          onClick={() => markAsRead(notification.id)}
                          className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground"
                          title="标记已读"
                        >
                          <Check className="h-3 w-3" />
                        </button>
                      )}
                      <button
                        onClick={() => removeNotification(notification.id)}
                        className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-red-500"
                        title="删除"
                      >
                        <X className="h-3 w-3" />
                      </button>
                    </div>
                  </div>
                </div>
              ))
            )}
          </div>

          {/* Footer */}
          {notifications.length > 0 && (
            <div className="px-4 py-2 border-t border-border bg-accent/30">
              <button className="w-full text-center text-xs text-primary hover:underline">
                查看全部通知 / View All
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
