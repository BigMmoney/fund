import { Link, useLocation } from 'react-router-dom'
import { TrendingUp, PieChart, Settings, Terminal } from 'lucide-react'

export function Navigation() {
  const location = useLocation()

  const navItems = [
    {
      path: '/',
      icon: Terminal,
      labelEn: 'Trading Terminal',
      labelZh: '交易终端',
      color: 'text-emerald-500'
    },
    {
      path: '/prediction',
      icon: PieChart,
      labelEn: 'Prediction Market',
      labelZh: '预测市场',
      color: 'text-green-500'
    },
    {
      path: '/admin',
      icon: Settings,
      labelEn: 'Admin Dashboard',
      labelZh: '管理后台',
      color: 'text-purple-500'
    }
  ]

  return (
    <nav className="border-b border-border bg-card">
      <div className="container mx-auto px-4">
        <div className="flex items-center justify-between h-16">
          {/* Logo */}
          <Link to="/" className="flex items-center gap-2 font-bold text-xl">
            <TrendingUp className="h-6 w-6 text-primary" />
            <span>PreTrading Platform</span>
          </Link>

          {/* Navigation Items */}
          <div className="flex items-center gap-2">
            {navItems.map((item) => {
              const Icon = item.icon
              const isActive = location.pathname === item.path
              
              return (
                <Link
                  key={item.path}
                  to={item.path}
                  className={`flex items-center gap-2 px-4 py-2 rounded-lg transition-colors ${
                    isActive
                      ? 'bg-primary text-primary-foreground'
                      : 'hover:bg-accent'
                  }`}
                >
                  <Icon className={`h-5 w-5 ${isActive ? '' : item.color}`} />
                  <div className="flex flex-col leading-tight">
                    <span className="text-sm font-medium">{item.labelEn}</span>
                    <span className="text-xs opacity-70">{item.labelZh}</span>
                  </div>
                </Link>
              )
            })}
          </div>

          {/* Status Indicator */}
          <div className="flex items-center gap-2">
            <div className="h-2 w-2 rounded-full bg-success animate-pulse"></div>
            <span className="text-sm text-muted-foreground">
              Online / 在线
            </span>
          </div>
        </div>
      </div>
    </nav>
  )
}
