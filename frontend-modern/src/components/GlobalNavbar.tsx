import { useLocation, useNavigate, Link } from 'react-router-dom'
import { useState, useEffect } from 'react'
import { 
  Zap, Activity, PieChart, Settings, Sun, Moon, 
  Terminal, Menu, X, Newspaper
} from 'lucide-react'
import { useTheme } from '@/contexts/ThemeContext'
import { useLanguage } from '@/contexts/LanguageContext'
import { NotificationDropdown } from '@/components/NotificationDropdown'
import LanguageSelector from '@/components/LanguageSelector'
import { formatCurrency } from '@/lib/utils'

interface GlobalNavbarProps {
  /** 账户余额 */
  accountBalance?: number
  /** 日盈亏 */
  dailyPnL?: number
  /** 周盈亏 */
  weeklyPnL?: number
  /** 胜率 */
  winRate?: number
  /** 夏普比率 */
  sharpeRatio?: number
  /** 是否显示交易指标 */
  showMetrics?: boolean
  /** 是否简洁模式 */
  compact?: boolean
}

export function GlobalNavbar({
  accountBalance = 0,
  dailyPnL = 0,
  weeklyPnL = 0,
  winRate = 0,
  sharpeRatio = 0,
  showMetrics = true,
  compact = false
}: GlobalNavbarProps) {
  const location = useLocation()
  const navigate = useNavigate()
  const { theme, toggleTheme } = useTheme()
  const { t, language } = useLanguage()
  const [currentTime, setCurrentTime] = useState(new Date())
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const [connectionStatus] = useState<'online' | 'offline' | 'connecting'>('online')

  // 更新时间
  useEffect(() => {
    const timer = setInterval(() => setCurrentTime(new Date()), 1000)
    return () => clearInterval(timer)
  }, [])

  const navItems = [
    {
      path: '/',
      icon: Terminal,
      labelKey: 'nav.trading',
      color: 'text-emerald-500',
      hoverBg: 'hover:bg-emerald-500/10',
      activeBg: 'bg-emerald-500/20',
      borderColor: 'border-emerald-500'
    },
    {
      path: '/prediction',
      icon: PieChart,
      labelKey: 'nav.prediction',
      color: 'text-blue-500',
      hoverBg: 'hover:bg-blue-500/10',
      activeBg: 'bg-blue-500/20',
      borderColor: 'border-blue-500'
    },
    {
      path: '/news-intel',
      icon: Newspaper,
      labelKey: 'nav.news',
      color: 'text-amber-500',
      hoverBg: 'hover:bg-amber-500/10',
      activeBg: 'bg-amber-500/20',
      borderColor: 'border-amber-500'
    },
    {
      path: '/admin',
      icon: Settings,
      labelKey: 'nav.admin',
      color: 'text-purple-500',
      hoverBg: 'hover:bg-purple-500/10',
      activeBg: 'bg-purple-500/20',
      borderColor: 'border-purple-500'
    }
  ]

  const getStatusColor = () => {
    switch (connectionStatus) {
      case 'online': return 'text-green-500'
      case 'offline': return 'text-red-500'
      case 'connecting': return 'text-yellow-500'
    }
  }

  const getStatusText = () => {
    switch (connectionStatus) {
      case 'online': return t('common.live')
      case 'offline': return t('common.offline')
      case 'connecting': return t('common.loading')
    }
  }

  return (
    <nav className={`bg-[#0f0f0f] border-b border-[#1a1a1a] ${compact ? 'h-10' : 'h-12'}`}>
      <div className="h-full px-4 flex items-center justify-between">
        {/* 左侧: Logo + 导航 */}
        <div className="flex items-center gap-6">
          {/* Logo */}
          <Link to="/" className="flex items-center gap-2 font-bold">
            <Zap className={`${compact ? 'h-4 w-4' : 'h-5 w-5'} text-emerald-500`} />
            <span className={`text-emerald-500 ${compact ? 'text-sm' : 'text-base'} hidden sm:inline`}>
              PRE<span className="text-white">TRADING</span>
            </span>
          </Link>

          {/* 桌面导航 */}
          <div className="hidden md:flex items-center gap-1">
            {navItems.map((item) => {
              const Icon = item.icon
              const isActive = location.pathname === item.path
              
              return (
                <button
                  key={item.path}
                  onClick={() => navigate(item.path)}
                  className={`
                    flex items-center gap-2 px-3 py-1.5 rounded-md transition-all duration-200 cursor-pointer
                    ${isActive 
                      ? `${item.activeBg} border-b-2 ${item.borderColor}` 
                      : `${item.hoverBg} border-b-2 border-transparent`
                    }
                  `}
                >
                  <Icon className={`${compact ? 'h-3.5 w-3.5' : 'h-4 w-4'} ${item.color}`} />
                  <span className={`${compact ? 'text-[10px]' : 'text-xs'} font-medium text-white`}>
                    {t(item.labelKey)}
                  </span>
                </button>
              )
            })}
          </div>

          {/* 分隔符 */}
          <div className="hidden md:block h-5 w-px bg-[#2a2a2a]" />

          {/* 连接状态 */}
          <div className="hidden md:flex items-center gap-2">
            <Activity className={`h-3.5 w-3.5 ${getStatusColor()} ${connectionStatus === 'online' ? 'animate-pulse' : ''}`} />
            <span className={`text-xs ${getStatusColor()}`}>
              {getStatusText()}
            </span>
          </div>

          {/* 性能指标 */}
          {showMetrics && !compact && (
            <div className="hidden xl:flex items-center gap-4 text-xs">
              <span className="text-gray-500">LAT: <span className="text-green-500">0.8ms</span></span>
              <span className="text-gray-500">FILL: <span className="text-green-500">99.7%</span></span>
              {sharpeRatio > 0 && (
                <span className="text-gray-500">SHARPE: <span className="text-blue-400">{sharpeRatio.toFixed(2)}</span></span>
              )}
              {winRate > 0 && (
                <span className="text-gray-500">WIN: <span className="text-blue-400">{winRate.toFixed(1)}%</span></span>
              )}
            </div>
          )}
        </div>

        {/* 右侧: 指标 + 工具 */}
        <div className="flex items-center gap-4">
          {/* 盈亏指标 */}
          {showMetrics && (
            <div className="hidden lg:flex items-center gap-4 text-xs">
              {dailyPnL !== 0 && (
                <div className="flex items-center gap-1">
                  <span className="text-gray-500">DAY:</span>
                  <span className={dailyPnL >= 0 ? 'text-green-500' : 'text-red-500'}>
                    {dailyPnL >= 0 ? '+' : ''}{formatCurrency(dailyPnL)}
                  </span>
                </div>
              )}
              {weeklyPnL !== 0 && (
                <div className="flex items-center gap-1">
                  <span className="text-gray-500">WEEK:</span>
                  <span className={weeklyPnL >= 0 ? 'text-green-500' : 'text-red-500'}>
                    {weeklyPnL >= 0 ? '+' : ''}{formatCurrency(weeklyPnL)}
                  </span>
                </div>
              )}
              {accountBalance > 0 && (
                <div className="flex items-center gap-1">
                  <span className="text-gray-500">EQUITY:</span>
                  <span className="text-white font-bold">{formatCurrency(accountBalance)}</span>
                </div>
              )}
            </div>
          )}

          {/* 分隔符 */}
          <div className="hidden sm:block h-5 w-px bg-[#2a2a2a]" />

          {/* 通知 */}
          <NotificationDropdown />

          {/* 语言选择 */}
          <LanguageSelector variant="dropdown" />

          {/* 主题切换 */}
          <button
            onClick={toggleTheme}
            className="p-1.5 rounded-md hover:bg-[#1a1a1a] transition-colors"
            title={theme === 'dark' ? t('common.show') : t('common.hide')}
          >
            {theme === 'dark' ? (
              <Sun className="h-4 w-4 text-yellow-500" />
            ) : (
              <Moon className="h-4 w-4 text-gray-400" />
            )}
          </button>

          {/* 时间 */}
          <div className="hidden sm:flex flex-col items-end text-xs leading-tight">
            <span className="text-white font-mono">
              {currentTime.toLocaleTimeString('en-US', { hour12: false })}
            </span>
            <span className="text-gray-500 text-[10px]">
              {currentTime.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' })}
            </span>
          </div>

          {/* 移动端菜单按钮 */}
          <button
            onClick={() => setMobileMenuOpen(!mobileMenuOpen)}
            className="md:hidden p-1.5 rounded-md hover:bg-[#1a1a1a] transition-colors"
          >
            {mobileMenuOpen ? (
              <X className="h-5 w-5 text-gray-400" />
            ) : (
              <Menu className="h-5 w-5 text-gray-400" />
            )}
          </button>
        </div>
      </div>

      {/* 移动端菜单 */}
      {mobileMenuOpen && (
        <div className="md:hidden absolute top-12 left-0 right-0 bg-[#0f0f0f] border-b border-[#1a1a1a] z-50">
          <div className="px-4 py-2 space-y-1">
            {navItems.map((item) => {
              const Icon = item.icon
              const isActive = location.pathname === item.path
              
              return (
                <button
                  key={item.path}
                  onClick={() => {
                    navigate(item.path)
                    setMobileMenuOpen(false)
                  }}
                  className={`
                    w-full flex items-center gap-3 px-3 py-2 rounded-md transition-all cursor-pointer
                    ${isActive ? item.activeBg : item.hoverBg}
                  `}
                >
                  <Icon className={`h-5 w-5 ${item.color}`} />
                  <span className="text-sm font-medium text-white">{t(item.labelKey)}</span>
                </button>
              )
            })}
            
            {/* 移动端语言选择 */}
            <div className="pt-2 mt-2 border-t border-[#1a1a1a] px-3 py-2">
              <LanguageSelector variant="compact" />
            </div>
            
            {/* 移动端指标 */}
            {showMetrics && accountBalance > 0 && (
              <div className="pt-2 mt-2 border-t border-[#1a1a1a]">
                <div className="flex justify-between text-xs px-3 py-1">
                  <span className="text-gray-500">{t('trading.balance')}:</span>
                  <span className="text-white font-bold">{formatCurrency(accountBalance)}</span>
                </div>
                {dailyPnL !== 0 && (
                  <div className="flex justify-between text-xs px-3 py-1">
                    <span className="text-gray-500">Day P&L:</span>
                    <span className={dailyPnL >= 0 ? 'text-green-500' : 'text-red-500'}>
                      {dailyPnL >= 0 ? '+' : ''}{formatCurrency(dailyPnL)}
                    </span>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      )}
    </nav>
  )
}
