import { useState, useEffect, useMemo } from 'react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { 
  Users, DollarSign, Activity, BarChart3, Settings, Shield, 
  TrendingUp, TrendingDown, Search, Plus, Trash2,
  Eye, Ban, CheckCircle, RefreshCw, Download
} from 'lucide-react'
import { AreaChart, Area, XAxis, YAxis, ResponsiveContainer, Tooltip, PieChart, Pie, Cell } from 'recharts'
import { GlobalNavbar } from '@/components/GlobalNavbar'
import { useLanguage } from '@/contexts/LanguageContext'

// 安全格式化数字
const formatNumber = (num: number | undefined | null): string => {
  if (num === undefined || num === null || isNaN(num)) return '0'
  return num.toLocaleString()
}

const formatCurrency = (num: number | undefined | null): string => {
  if (num === undefined || num === null || isNaN(num)) return '$0'
  return '$' + num.toLocaleString()
}

const formatMillions = (num: number | undefined | null): string => {
  if (num === undefined || num === null || isNaN(num)) return '$0M'
  return '$' + (num / 1000000).toFixed(2) + 'M'
}

interface Stats {
  totalUsers: number
  totalVolume: number
  totalMarkets: number
  activeTrades: number
  dailyVolume: number
  dailyPnL: number
}

interface User {
  id: string
  name: string
  email: string
  balance: number
  trades: number
  pnl: number
  status: 'active' | 'suspended' | 'pending'
  joinDate: string
  lastActive: string
}

interface Trade {
  id: string
  user: string
  market: string
  outcome: string
  amount: number
  timestamp: string
  status: 'pending' | 'filled' | 'cancelled'
}

interface MarketData {
  id: string
  name: string
  volume: number
  participants: number
  state: 'open' | 'closed' | 'resolved'
  yesPrice: number
  createdAt: string
}

// 默认数据
const defaultStats: Stats = {
  totalUsers: 1247,
  totalVolume: 85400000,
  totalMarkets: 45,
  activeTrades: 234,
  dailyVolume: 3650000,
  dailyPnL: 125000,
}

const defaultUsers: User[] = [
  { id: 'U001', name: 'Alice Chen / 陈小明', email: 'alice@email.com', balance: 125000, trades: 145, pnl: 15600, status: 'active', joinDate: '2025-12-01', lastActive: '2026-01-16 10:30' },
  { id: 'U002', name: 'Bob Wang / 王大伟', email: 'bob@email.com', balance: 89000, trades: 87, pnl: -4500, status: 'active', joinDate: '2025-11-15', lastActive: '2026-01-16 09:45' },
  { id: 'U003', name: 'Carol Li / 李美玲', email: 'carol@email.com', balance: 234000, trades: 312, pnl: 45800, status: 'active', joinDate: '2025-10-20', lastActive: '2026-01-16 10:15' },
  { id: 'U004', name: 'David Zhang / 张强', email: 'david@email.com', balance: 56000, trades: 56, pnl: -2300, status: 'suspended', joinDate: '2026-01-05', lastActive: '2026-01-14 18:20' },
  { id: 'U005', name: 'Eva Liu / 刘思琪', email: 'eva@email.com', balance: 178000, trades: 198, pnl: 28900, status: 'active', joinDate: '2025-12-10', lastActive: '2026-01-16 10:28' },
  { id: 'U006', name: 'Frank Wu / 吴飞', email: 'frank@email.com', balance: 45000, trades: 34, pnl: 1200, status: 'pending', joinDate: '2026-01-15', lastActive: '2026-01-15 14:00' },
]

const defaultTrades: Trade[] = [
  { id: 'T001', user: 'Alice Chen', market: 'BTC $150K', outcome: 'YES', amount: 5000, timestamp: '2026-01-16 10:30:15', status: 'filled' },
  { id: 'T002', user: 'Bob Wang', market: 'Trump 2028', outcome: 'NO', amount: 3000, timestamp: '2026-01-16 10:25:42', status: 'filled' },
  { id: 'T003', user: 'Carol Li', market: 'ETH $10K', outcome: 'YES', amount: 8000, timestamp: '2026-01-16 10:20:18', status: 'pending' },
  { id: 'T004', user: 'David Zhang', market: 'AGI 2026', outcome: 'YES', amount: 2000, timestamp: '2026-01-16 10:15:33', status: 'filled' },
  { id: 'T005', user: 'Eva Liu', market: 'Fed Rate Cut', outcome: 'YES', amount: 6000, timestamp: '2026-01-16 10:10:55', status: 'filled' },
  { id: 'T006', user: 'Frank Wu', market: 'NVDA $800', outcome: 'NO', amount: 1500, timestamp: '2026-01-16 10:05:22', status: 'cancelled' },
]

const defaultMarkets: MarketData[] = [
  { id: 'M001', name: 'BTC突破$150K (2026年底前)', volume: 8540000, participants: 2345, state: 'open', yesPrice: 0.65, createdAt: '2025-12-01' },
  { id: 'M002', name: 'ETH突破$10K', volume: 5230000, participants: 1876, state: 'open', yesPrice: 0.42, createdAt: '2025-11-15' },
  { id: 'M003', name: 'Trump 2028 连任', volume: 12500000, participants: 4532, state: 'open', yesPrice: 0.38, createdAt: '2025-11-01' },
  { id: 'M004', name: 'AGI实现 (2027年前)', volume: 3450000, participants: 1234, state: 'open', yesPrice: 0.18, createdAt: '2025-10-20' },
  { id: 'M005', name: 'BTC $100K EOY 2025', volume: 15600000, participants: 5678, state: 'resolved', yesPrice: 0.92, createdAt: '2025-01-01' },
]

const volumeData = Array.from({ length: 30 }, (_, i) => ({
  day: `${i + 1}`,
  volume: Math.floor(Math.random() * 2000000) + 500000,
  trades: Math.floor(Math.random() * 500) + 100,
}))

const categoryData = [
  { name: 'Crypto', value: 45, color: '#22c55e' },
  { name: 'Politics', value: 25, color: '#3b82f6' },
  { name: 'Tech', value: 15, color: '#a855f7' },
  { name: 'Finance', value: 10, color: '#eab308' },
  { name: 'Other', value: 5, color: '#6b7280' },
]

// StatCard 组件
function StatCard({ title, value, icon: Icon, change, color }: { 
  title: string
  value: string
  icon: React.ComponentType<{ className?: string }>
  change?: number
  color: string 
}) {
  return (
    <Card className="bg-[#111] border-[#222]">
      <CardContent className="pt-6">
        <div className="flex items-center justify-between">
          <div>
            <p className="text-sm text-gray-400">{title}</p>
            <p className="text-2xl font-bold mt-1 text-white">{value || '0'}</p>
            {change !== undefined && (
              <p className={`text-xs mt-1 flex items-center gap-1 ${change >= 0 ? 'text-green-500' : 'text-red-500'}`}>
                {change >= 0 ? <TrendingUp className="h-3 w-3" /> : <TrendingDown className="h-3 w-3" />}
                {change >= 0 ? '+' : ''}{change}% vs yesterday
              </p>
            )}
          </div>
          <div className={`p-3 rounded-lg ${color}`}>
            <Icon className="h-6 w-6 text-white" />
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

export function AdminDashboard() {
  const { t } = useLanguage()
  const [stats] = useState<Stats>(defaultStats)
  const [users, setUsers] = useState<User[]>(defaultUsers)
  const [trades] = useState<Trade[]>(defaultTrades)
  const [markets] = useState<MarketData[]>(defaultMarkets)
  const [searchQuery, setSearchQuery] = useState('')
  const [activeTab, setActiveTab] = useState('overview')
  const [isReady, setIsReady] = useState(false)

  useEffect(() => {
    // 组件挂载后标记为就绪
    setIsReady(true)
  }, [])

  const toggleUserStatus = (userId: string) => {
    setUsers(prev => prev.map(u => {
      if (u.id === userId) {
        return { ...u, status: u.status === 'active' ? 'suspended' : 'active' as const }
      }
      return u
    }))
  }

  const filteredUsers = useMemo(() => {
    if (!users || !Array.isArray(users)) return []
    return users.filter(u => {
      if (!u) return false
      const name = u.name?.toLowerCase() || ''
      const email = u.email?.toLowerCase() || ''
      const query = searchQuery.toLowerCase()
      return name.includes(query) || email.includes(query)
    })
  }, [users, searchQuery])

  const sortedMarkets = useMemo(() => {
    if (!markets || !Array.isArray(markets)) return []
    return [...markets].sort((a, b) => (b?.volume || 0) - (a?.volume || 0)).slice(0, 5)
  }, [markets])

  const safeStats = stats || defaultStats

  if (!isReady) {
    return (
      <div className="min-h-screen bg-[#0a0a0a] text-white flex items-center justify-center">
        <div className="text-gray-400">Loading... / 加载中...</div>
      </div>
    )
  }

  return (
    <div className="min-h-screen bg-[#0a0a0a] text-white">
      <GlobalNavbar showMetrics={false} />

      {/* 页面标题栏 */}
      <div className="border-b border-[#1a1a1a] bg-[#0d0d0d]">
        <div className="container mx-auto px-4 py-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <Shield className="h-6 w-6 text-purple-500" />
              <div>
                <h1 className="text-lg font-bold">Admin Dashboard / 管理后台</h1>
                <p className="text-xs text-gray-500">Platform management & analytics / 平台管理与分析</p>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Button variant="outline" size="sm" className="bg-[#1a1a1a] border-[#2a2a2a] hover:bg-[#2a2a2a] text-white">
                <RefreshCw className="h-4 w-4 mr-2" />
                Refresh / 刷新
              </Button>
              <Button variant="outline" size="sm" className="bg-[#1a1a1a] border-[#2a2a2a] hover:bg-[#2a2a2a] text-white">
                <Download className="h-4 w-4 mr-2" />
                Export / 导出
              </Button>
            </div>
          </div>
        </div>
      </div>

      <div className="container mx-auto px-4 py-6">
        {/* Stats Overview */}
        <div className="grid grid-cols-6 gap-4 mb-6">
          <StatCard title="Users / 总用户" value={formatNumber(safeStats.totalUsers)} icon={Users} change={5.2} color="bg-blue-500" />
          <StatCard title="Volume / 总交易量" value={formatMillions(safeStats.totalVolume)} icon={DollarSign} change={12.8} color="bg-green-500" />
          <StatCard title="Markets / 活跃市场" value={formatNumber(safeStats.totalMarkets)} icon={BarChart3} change={2} color="bg-purple-500" />
          <StatCard title="Trades / 今日交易" value={formatNumber(safeStats.activeTrades)} icon={Activity} change={-3.5} color="bg-orange-500" />
          <StatCard title="Daily / 今日交易量" value={formatMillions(safeStats.dailyVolume)} icon={TrendingUp} change={8.3} color="bg-cyan-500" />
          <StatCard title="P&L / 平台盈亏" value={`$${formatNumber(Math.floor((safeStats.dailyPnL || 0) / 1000))}K`} icon={DollarSign} change={15.2} color="bg-emerald-500" />
        </div>

        <Tabs value={activeTab} onValueChange={setActiveTab}>
          <TabsList className="mb-6 bg-[#111] border border-[#222]">
            <TabsTrigger value="overview" className="data-[state=active]:bg-[#222] data-[state=active]:text-white text-gray-400">Overview / 概览</TabsTrigger>
            <TabsTrigger value="users" className="data-[state=active]:bg-[#222] data-[state=active]:text-white text-gray-400">Users / 用户管理</TabsTrigger>
            <TabsTrigger value="markets" className="data-[state=active]:bg-[#222] data-[state=active]:text-white text-gray-400">Markets / 市场管理</TabsTrigger>
            <TabsTrigger value="trades" className="data-[state=active]:bg-[#222] data-[state=active]:text-white text-gray-400">Trades / 交易记录</TabsTrigger>
            <TabsTrigger value="settings" className="data-[state=active]:bg-[#222] data-[state=active]:text-white text-gray-400">Settings / 系统设置</TabsTrigger>
          </TabsList>

          <TabsContent value="overview">
            <div className="grid grid-cols-12 gap-6">
              {/* Volume Chart */}
              <Card className="col-span-8 bg-[#111] border-[#222]">
                <CardHeader>
                  <CardTitle className="text-white">Volume Trend / 交易量趋势 (30 Days)</CardTitle>
                </CardHeader>
                <CardContent>
                  <div className="h-72">
                    <ResponsiveContainer width="100%" height="100%">
                      <AreaChart data={volumeData}>
                        <defs>
                          <linearGradient id="volumeGradient" x1="0" y1="0" x2="0" y2="1">
                            <stop offset="5%" stopColor="#3b82f6" stopOpacity={0.3}/>
                            <stop offset="95%" stopColor="#3b82f6" stopOpacity={0}/>
                          </linearGradient>
                        </defs>
                        <XAxis dataKey="day" tick={{ fontSize: 10, fill: '#888' }} axisLine={false} tickLine={false} />
                        <YAxis tick={{ fontSize: 10, fill: '#888' }} axisLine={false} tickLine={false} tickFormatter={(v) => `$${(v / 1000000).toFixed(1)}M`} />
                        <Tooltip contentStyle={{ background: '#1a1a2e', border: 'none', borderRadius: 8, color: '#fff' }} formatter={(value: number) => [formatCurrency(value), 'Volume']} />
                        <Area type="monotone" dataKey="volume" stroke="#3b82f6" fill="url(#volumeGradient)" strokeWidth={2} />
                      </AreaChart>
                    </ResponsiveContainer>
                  </div>
                </CardContent>
              </Card>

              {/* Category Distribution */}
              <Card className="col-span-4 bg-[#111] border-[#222]">
                <CardHeader>
                  <CardTitle className="text-white">Market Distribution / 市场分布</CardTitle>
                </CardHeader>
                <CardContent>
                  <div className="h-72 flex items-center justify-center">
                    <ResponsiveContainer width="100%" height="100%">
                      <PieChart>
                        <Pie data={categoryData} cx="50%" cy="50%" innerRadius={60} outerRadius={100} dataKey="value" label={({ name, percent }) => `${name} ${((percent || 0) * 100).toFixed(0)}%`}>
                          {categoryData.map((entry, i) => (<Cell key={i} fill={entry.color} />))}
                        </Pie>
                        <Tooltip contentStyle={{ background: '#1a1a2e', border: 'none', borderRadius: 8, color: '#fff' }} />
                      </PieChart>
                    </ResponsiveContainer>
                  </div>
                </CardContent>
              </Card>

              {/* Recent Trades */}
              <Card className="col-span-6 bg-[#111] border-[#222]">
                <CardHeader>
                  <CardTitle className="text-white">Recent Trades / 最近交易</CardTitle>
                </CardHeader>
                <CardContent>
                  <div className="space-y-3">
                    {(trades || []).slice(0, 5).map(trade => trade && (
                      <div key={trade.id} className="flex items-center justify-between p-3 bg-[#1a1a1a] rounded-lg">
                        <div>
                          <div className="font-medium text-white">{trade.user || 'Unknown'}</div>
                          <div className="text-sm text-gray-400">{trade.market || ''} • {trade.outcome || ''}</div>
                        </div>
                        <div className="text-right">
                          <div className="font-bold text-white">{formatCurrency(trade.amount)}</div>
                          <div className={`text-xs ${trade.status === 'filled' ? 'text-green-500' : trade.status === 'pending' ? 'text-yellow-500' : 'text-red-500'}`}>
                            {(trade.status || 'unknown').toUpperCase()}
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </CardContent>
              </Card>

              {/* Top Markets */}
              <Card className="col-span-6 bg-[#111] border-[#222]">
                <CardHeader>
                  <CardTitle className="text-white">Top Markets / 热门市场</CardTitle>
                </CardHeader>
                <CardContent>
                  <div className="space-y-3">
                    {sortedMarkets.map((market, i) => market && (
                      <div key={market.id} className="flex items-center gap-4 p-3 bg-[#1a1a1a] rounded-lg">
                        <span className="text-2xl font-bold text-gray-500 w-8">#{i + 1}</span>
                        <div className="flex-1 min-w-0">
                          <div className="font-medium truncate text-white">{market.name || 'Unknown'}</div>
                          <div className="text-sm text-gray-400">{formatNumber(market.participants)} participants</div>
                        </div>
                        <div className="text-right">
                          <div className="font-bold text-white">{formatMillions(market.volume)}</div>
                          <div className="text-sm text-green-500">{((market.yesPrice || 0) * 100).toFixed(0)}% YES</div>
                        </div>
                      </div>
                    ))}
                  </div>
                </CardContent>
              </Card>
            </div>
          </TabsContent>

          <TabsContent value="users">
            <Card className="bg-[#111] border-[#222]">
              <CardHeader>
                <div className="flex items-center justify-between">
                  <CardTitle className="text-white">User Management / 用户管理</CardTitle>
                  <div className="flex items-center gap-2">
                    <div className="relative">
                      <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-gray-500" />
                      <Input placeholder="Search users... / 搜索用户..." value={searchQuery} onChange={(e) => setSearchQuery(e.target.value)} className="pl-10 w-64 bg-[#0a0a0a] border-[#222] text-white" />
                    </div>
                    <Button size="sm" className="bg-emerald-600 hover:bg-emerald-700 text-white"><Plus className="h-4 w-4 mr-2" />Add User / 添加用户</Button>
                  </div>
                </div>
              </CardHeader>
              <CardContent>
                <div className="overflow-x-auto">
                  <table className="w-full">
                    <thead>
                      <tr className="text-left text-sm text-gray-400 border-b border-[#222]">
                        <th className="pb-3 font-medium">User / 用户</th>
                        <th className="pb-3 font-medium">Email / 邮箱</th>
                        <th className="pb-3 font-medium text-right">Balance / 余额</th>
                        <th className="pb-3 font-medium text-right">Trades / 交易</th>
                        <th className="pb-3 font-medium text-right">P&L / 盈亏</th>
                        <th className="pb-3 font-medium">Status / 状态</th>
                        <th className="pb-3 font-medium">Last Active / 最后活跃</th>
                        <th className="pb-3 font-medium">Actions / 操作</th>
                      </tr>
                    </thead>
                    <tbody>
                      {filteredUsers.map(user => user && (
                        <tr key={user.id} className="border-b border-[#222] hover:bg-[#1a1a1a]">
                          <td className="py-3 font-medium text-white">{user.name || ''}</td>
                          <td className="py-3 text-gray-400">{user.email || ''}</td>
                          <td className="py-3 text-right font-bold text-white">{formatCurrency(user.balance)}</td>
                          <td className="py-3 text-right text-gray-300">{formatNumber(user.trades)}</td>
                          <td className={`py-3 text-right font-bold ${(user.pnl || 0) >= 0 ? 'text-green-500' : 'text-red-500'}`}>
                            {(user.pnl || 0) >= 0 ? '+' : ''}{formatCurrency(user.pnl)}
                          </td>
                          <td className="py-3">
                            <span className={`px-2 py-1 rounded-full text-xs ${
                              user.status === 'active' ? 'bg-green-500/20 text-green-500' :
                              user.status === 'suspended' ? 'bg-red-500/20 text-red-500' :
                              'bg-yellow-500/20 text-yellow-500'
                            }`}>
                              {user.status || 'unknown'}
                            </span>
                          </td>
                          <td className="py-3 text-sm text-gray-400">{user.lastActive || ''}</td>
                          <td className="py-3">
                            <div className="flex items-center gap-1">
                              <Button variant="ghost" size="icon" className="h-8 w-8 hover:bg-[#222]"><Eye className="h-4 w-4 text-gray-400" /></Button>
                              <Button variant="ghost" size="icon" className="h-8 w-8 hover:bg-[#222]" onClick={() => toggleUserStatus(user.id)}>
                                {user.status === 'active' ? <Ban className="h-4 w-4 text-red-500" /> : <CheckCircle className="h-4 w-4 text-green-500" />}
                              </Button>
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </CardContent>
            </Card>
          </TabsContent>

          <TabsContent value="markets">
            <Card className="bg-[#111] border-[#222]">
              <CardHeader>
                <div className="flex items-center justify-between">
                  <CardTitle className="text-white">Market Management / 市场管理</CardTitle>
                  <Button size="sm" className="bg-emerald-600 hover:bg-emerald-700 text-white"><Plus className="h-4 w-4 mr-2" />Create Market / 创建市场</Button>
                </div>
              </CardHeader>
              <CardContent>
                <div className="overflow-x-auto">
                  <table className="w-full">
                    <thead>
                      <tr className="text-left text-sm text-gray-400 border-b border-[#222]">
                        <th className="pb-3 font-medium">Market / 市场名称</th>
                        <th className="pb-3 font-medium text-right">Volume / 交易量</th>
                        <th className="pb-3 font-medium text-right">Participants / 参与者</th>
                        <th className="pb-3 font-medium text-right">YES Price / YES价格</th>
                        <th className="pb-3 font-medium">Status / 状态</th>
                        <th className="pb-3 font-medium">Created / 创建日期</th>
                        <th className="pb-3 font-medium">Actions / 操作</th>
                      </tr>
                    </thead>
                    <tbody>
                      {(markets || []).map(market => market && (
                        <tr key={market.id} className="border-b border-[#222] hover:bg-[#1a1a1a]">
                          <td className="py-3 font-medium max-w-xs truncate text-white">{market.name || ''}</td>
                          <td className="py-3 text-right font-bold text-white">{formatMillions(market.volume)}</td>
                          <td className="py-3 text-right text-gray-300">{formatNumber(market.participants)}</td>
                          <td className="py-3 text-right">
                            <span className="text-green-500 font-bold">{((market.yesPrice || 0) * 100).toFixed(1)}%</span>
                          </td>
                          <td className="py-3">
                            <span className={`px-2 py-1 rounded-full text-xs ${
                              market.state === 'open' ? 'bg-green-500/20 text-green-500' :
                              market.state === 'resolved' ? 'bg-blue-500/20 text-blue-500' :
                              'bg-gray-500/20 text-gray-500'
                            }`}>
                              {market.state || 'unknown'}
                            </span>
                          </td>
                          <td className="py-3 text-sm text-gray-400">{market.createdAt || ''}</td>
                          <td className="py-3">
                            <div className="flex items-center gap-1">
                              <Button variant="ghost" size="icon" className="h-8 w-8 hover:bg-[#222]"><Eye className="h-4 w-4 text-gray-400" /></Button>
                              <Button variant="ghost" size="icon" className="h-8 w-8 hover:bg-[#222]"><Settings className="h-4 w-4 text-gray-400" /></Button>
                              <Button variant="ghost" size="icon" className="h-8 w-8 hover:bg-[#222]"><Trash2 className="h-4 w-4 text-red-500" /></Button>
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </CardContent>
            </Card>
          </TabsContent>

          <TabsContent value="trades">
            <Card className="bg-[#111] border-[#222]">
              <CardHeader>
                <CardTitle className="text-white">Trade History / 交易记录</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="overflow-x-auto">
                  <table className="w-full">
                    <thead>
                      <tr className="text-left text-sm text-gray-400 border-b border-[#222]">
                        <th className="pb-3 font-medium">ID</th>
                        <th className="pb-3 font-medium">User / 用户</th>
                        <th className="pb-3 font-medium">Market / 市场</th>
                        <th className="pb-3 font-medium">Outcome / 结果</th>
                        <th className="pb-3 font-medium text-right">Amount / 金额</th>
                        <th className="pb-3 font-medium">Time / 时间</th>
                        <th className="pb-3 font-medium">Status / 状态</th>
                      </tr>
                    </thead>
                    <tbody>
                      {(trades || []).map(trade => trade && (
                        <tr key={trade.id} className="border-b border-[#222] hover:bg-[#1a1a1a]">
                          <td className="py-3 font-mono text-sm text-gray-300">{trade.id || ''}</td>
                          <td className="py-3 text-white">{trade.user || ''}</td>
                          <td className="py-3 text-gray-300">{trade.market || ''}</td>
                          <td className="py-3">
                            <span className={`px-2 py-1 rounded text-xs ${trade.outcome === 'YES' ? 'bg-green-500/20 text-green-500' : 'bg-red-500/20 text-red-500'}`}>
                              {trade.outcome || ''}
                            </span>
                          </td>
                          <td className="py-3 text-right font-bold text-white">{formatCurrency(trade.amount)}</td>
                          <td className="py-3 text-sm text-gray-400">{trade.timestamp || ''}</td>
                          <td className="py-3">
                            <span className={`px-2 py-1 rounded-full text-xs ${
                              trade.status === 'filled' ? 'bg-green-500/20 text-green-500' :
                              trade.status === 'pending' ? 'bg-yellow-500/20 text-yellow-500' :
                              'bg-red-500/20 text-red-500'
                            }`}>
                              {trade.status || 'unknown'}
                            </span>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </CardContent>
            </Card>
          </TabsContent>

          <TabsContent value="settings">
            <div className="grid grid-cols-2 gap-6">
              <Card className="bg-[#111] border-[#222]">
                <CardHeader>
                  <CardTitle className="text-white">System Settings / 系统配置</CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                  <div>
                    <label className="text-sm font-medium mb-2 block text-gray-300">Platform Fee / 平台手续费 (%)</label>
                    <Input type="number" defaultValue="2" className="bg-[#0a0a0a] border-[#222] text-white" />
                  </div>
                  <div>
                    <label className="text-sm font-medium mb-2 block text-gray-300">Min Bet / 最小下注金额 ($)</label>
                    <Input type="number" defaultValue="10" className="bg-[#0a0a0a] border-[#222] text-white" />
                  </div>
                  <div>
                    <label className="text-sm font-medium mb-2 block text-gray-300">Max Bet / 最大下注金额 ($)</label>
                    <Input type="number" defaultValue="100000" className="bg-[#0a0a0a] border-[#222] text-white" />
                  </div>
                  <Button className="w-full bg-emerald-600 hover:bg-emerald-700 text-white">Save Settings / 保存设置</Button>
                </CardContent>
              </Card>

              <Card className="bg-[#111] border-[#222]">
                <CardHeader>
                  <CardTitle className="text-white">Risk Control / 风险控制</CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                  <div>
                    <label className="text-sm font-medium mb-2 block text-gray-300">Max Exposure per Market / 单市场最大敞口 ($)</label>
                    <Input type="number" defaultValue="1000000" className="bg-[#0a0a0a] border-[#222] text-white" />
                  </div>
                  <div>
                    <label className="text-sm font-medium mb-2 block text-gray-300">Max Position per User / 单用户最大持仓 ($)</label>
                    <Input type="number" defaultValue="50000" className="bg-[#0a0a0a] border-[#222] text-white" />
                  </div>
                  <div>
                    <label className="text-sm font-medium mb-2 block text-gray-300">Auto-halt Threshold / 自动停盘阈值 (%)</label>
                    <Input type="number" defaultValue="20" className="bg-[#0a0a0a] border-[#222] text-white" />
                  </div>
                  <Button className="w-full bg-emerald-600 hover:bg-emerald-700 text-white">Update Risk Controls / 更新风控</Button>
                </CardContent>
              </Card>
            </div>
          </TabsContent>
        </Tabs>
      </div>
    </div>
  )
}
