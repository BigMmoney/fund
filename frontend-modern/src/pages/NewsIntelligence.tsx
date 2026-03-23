import { useMemo, useState } from 'react'
import { Bell, Bookmark, RefreshCw, ShieldAlert } from 'lucide-react'
import { Bar, BarChart, CartesianGrid, Cell, Pie, PieChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { AppShell } from '@/components/AppShell'
import { EmptyStatePanel } from '@/components/EmptyStatePanel'
import { StatusBanner } from '@/components/StatusBanner'

const palette = ['#111111', '#404040', '#737373', '#a3a3a3', '#d4d4d4']
const domainOptions = ['all', 'export_control', 'trade', 'sanction'] as const
const sourceLevelOptions = ['all', 'L0', 'L1'] as const
const alertStatusOptions = ['all', 'active', 'acknowledged'] as const

const sampleHeadlines = [
  {
    id: 'headline-1',
    title: '美国商务口径更新，半导体出口限制再次成为市场焦点',
    source: 'Reuters',
    sourceLevel: 'L1',
    domain: 'export_control',
    summary: '当前版本的情报页先提供稳定版读模型视图，帮助你快速查看主题、来源层级与影响方向。',
    publishedAt: '2026-03-14T05:30:00Z',
    confidence: 0.81,
  },
  {
    id: 'headline-2',
    title: '联邦公报新增贸易相关草案，评论窗口即将开启',
    source: 'Federal Register',
    sourceLevel: 'L0',
    domain: 'trade',
    summary: '页面会优先展示最需要人判断的几类信息：政策主题、告警优先级和文档入口。',
    publishedAt: '2026-03-14T04:10:00Z',
    confidence: 0.92,
  },
  {
    id: 'headline-3',
    title: '制裁名单更新预期抬升，跨市场风险偏好短时走弱',
    source: 'Bloomberg',
    sourceLevel: 'L1',
    domain: 'sanction',
    summary: '这一页已经从历史实验态改成轻量版工作台，重点放在“看清楚”和“能继续迭代”。',
    publishedAt: '2026-03-14T03:20:00Z',
    confidence: 0.77,
  },
]

const sampleAlerts = [
  {
    id: 'alert-1',
    title: '高优先级：出口管制主题热度上升',
    message: '来源层级混合出现 L0 与 L1，建议人工复核后再决定是否扩大监控范围。',
    priority: 'high',
    status: 'active',
  },
  {
    id: 'alert-2',
    title: '关键：制裁名单变动窗口临近',
    message: '建议将名单更新与受影响资产清单并排审阅，避免漏看跨市场联动。',
    priority: 'critical',
    status: 'active',
  },
]

const sampleDocuments = [
  {
    id: 'doc-1',
    title: 'Federal Register Draft on Trade Controls',
    summary: '汇总最新草案、窗口期和受影响行业，适合作为今日政策审阅入口。',
    source: 'federal_register',
    publishedAt: '2026-03-13T18:00:00Z',
    bookmarked: true,
  },
  {
    id: 'doc-2',
    title: 'Treasury Sanctions Briefing Note',
    summary: '适合用来串联风险台、法务台与交易台的共识说明。',
    source: 'treasury',
    publishedAt: '2026-03-13T15:30:00Z',
    bookmarked: false,
  },
]

const sampleFederalDocs = [
  {
    id: 'fed-1',
    title: 'Advance Notice of Proposed Rulemaking',
    type: 'Notice',
    abstractText: '围绕关键材料与出口控制的草案说明，适合做早盘政策复盘。',
    publishedAt: '2026-03-13T10:00:00Z',
  },
]

const sampleSdnUpdates = [
  {
    id: 'sdn-1',
    name: 'Example Entity Holdings',
    type: 'entity',
    country: 'CN',
    changeType: 'add',
    remarks: '名单条目为演示数据，用于说明后续这里会如何承载真实变更。',
    addedDate: '2026-03-13T09:00:00Z',
  },
]

function formatTime(value?: string | null) {
  if (!value) return '-'
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '-' : date.toLocaleString()
}

export function NewsIntelligence() {
  const [alerts, setAlerts] = useState(sampleAlerts)
  const [documents, setDocuments] = useState(sampleDocuments)
  const [isRefreshing, setIsRefreshing] = useState(false)
  const [domainFilter, setDomainFilter] = useState<(typeof domainOptions)[number]>('all')
  const [sourceLevelFilter, setSourceLevelFilter] = useState<(typeof sourceLevelOptions)[number]>('all')
  const [alertStatusFilter, setAlertStatusFilter] = useState<(typeof alertStatusOptions)[number]>('all')
  const [bookmarkOnly, setBookmarkOnly] = useState(false)

  const stats = {
    totalHeadlines: sampleHeadlines.length,
    totalDocuments: documents.length + sampleFederalDocs.length,
    totalAlerts: alerts.length,
    byDomain: { export_control: 1, trade: 1, sanction: 1 },
    bySource: { L0: 1, L1: 2 },
  }

  const alertStats = {
    total: alerts.length,
    byPriority: {
      critical: alerts.filter((item) => item.priority === 'critical').length,
      high: alerts.filter((item) => item.priority === 'high').length,
      medium: alerts.filter((item) => item.priority === 'medium').length,
      low: alerts.filter((item) => item.priority === 'low').length,
    },
    last24Hours: alerts.length,
  }

  const domainChartData = useMemo(
    () => Object.entries(stats.byDomain).map(([name, value]) => ({ name, value })),
    [],
  )

  const sourceChartData = useMemo(
    () => Object.entries(stats.bySource).map(([name, value]) => ({ name, value })),
    [],
  )

  const alertPriorityData = useMemo(
    () => Object.entries(alertStats.byPriority).map(([name, value]) => ({ name, value })),
    [alertStats.byPriority],
  )
  const filteredHeadlines = useMemo(
    () =>
      sampleHeadlines.filter(
        (headline) =>
          (domainFilter === 'all' || headline.domain === domainFilter) &&
          (sourceLevelFilter === 'all' || headline.sourceLevel === sourceLevelFilter),
      ),
    [domainFilter, sourceLevelFilter],
  )
  const filteredAlerts = useMemo(
    () => alerts.filter((alert) => alertStatusFilter === 'all' || alert.status === alertStatusFilter),
    [alertStatusFilter, alerts],
  )
  const filteredDocuments = useMemo(
    () => documents.filter((doc) => !bookmarkOnly || doc.bookmarked),
    [bookmarkOnly, documents],
  )

  function refresh() {
    setIsRefreshing(true)
    setTimeout(() => setIsRefreshing(false), 500)
  }

  function acknowledgeAlert(alertId: string) {
    setAlerts((current) => current.map((item) => (item.id === alertId ? { ...item, status: 'acknowledged' } : item)))
  }

  function dismissAlert(alertId: string) {
    setAlerts((current) => current.filter((item) => item.id !== alertId))
  }

  function toggleBookmark(docId: string) {
    setDocuments((current) => current.map((item) => (item.id === docId ? { ...item, bookmarked: !item.bookmarked } : item)))
  }

  return (
    <AppShell
      title="政策情报"
      subtitle="当前这里先作为稳定版前端工作台存在：重点展示情报结构、告警优先级和文档入口，不再承载旧实验页面的大量历史逻辑。"
    >
      <StatusBanner
        tone="success"
        eyebrow="Intelligence"
        title="情报页已完成轻量化收口"
        message="当前版本优先保证页面干净、可构建、可扩展；真实服务层的更深清理可以在下一轮单独继续。"
        trailing={
          <button
            type="button"
            onClick={refresh}
            className="inline-flex items-center gap-2 rounded-full border border-black bg-black px-4 py-2 text-sm text-white transition hover:bg-neutral-800"
          >
            <RefreshCw className={`h-4 w-4 ${isRefreshing ? 'animate-spin' : ''}`} />
            刷新
          </button>
        }
      />

      <section className="surface-card p-7">
        <div className="flex flex-col gap-5 xl:flex-row xl:items-end xl:justify-between">
          <div>
            <div className="eyebrow">Filters</div>
            <h2 className="mt-2 section-title">工作台筛选</h2>
          </div>
          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
            <select value={domainFilter} onChange={(event) => setDomainFilter(event.target.value as (typeof domainOptions)[number])} className="field-shell">
              <option value="all">全部主题</option>
              <option value="export_control">出口管制</option>
              <option value="trade">贸易规则</option>
              <option value="sanction">制裁主题</option>
            </select>
            <select value={sourceLevelFilter} onChange={(event) => setSourceLevelFilter(event.target.value as (typeof sourceLevelOptions)[number])} className="field-shell">
              <option value="all">全部来源层级</option>
              <option value="L0">仅 L0</option>
              <option value="L1">仅 L1</option>
            </select>
            <select value={alertStatusFilter} onChange={(event) => setAlertStatusFilter(event.target.value as (typeof alertStatusOptions)[number])} className="field-shell">
              <option value="all">全部告警状态</option>
              <option value="active">仅 active</option>
              <option value="acknowledged">仅 acknowledged</option>
            </select>
            <button type="button" onClick={() => setBookmarkOnly((current) => !current)} className={bookmarkOnly ? 'action-dark' : 'action-light'}>
              {bookmarkOnly ? '只看已收藏' : '显示全部文档'}
            </button>
          </div>
        </div>
      </section>

      <section className="hero-panel grid gap-6 px-8 py-8 xl:grid-cols-4">
        <div className="hero-stat">
          <div className="eyebrow">Headlines</div>
          <div className="mt-3 metric-number text-black">{filteredHeadlines.length}</div>
        </div>
        <div className="hero-stat">
          <div className="eyebrow">Documents</div>
          <div className="mt-3 metric-number text-black">{filteredDocuments.length + sampleFederalDocs.length}</div>
        </div>
        <div className="hero-stat">
          <div className="eyebrow">Alerts</div>
          <div className="mt-3 metric-number text-black">{filteredAlerts.length}</div>
        </div>
        <div className="hero-stat">
          <div className="eyebrow">Last 24h</div>
          <div className="mt-3 metric-number text-black">{alertStats.last24Hours}</div>
        </div>
      </section>

      <section className="grid gap-6 xl:grid-cols-3">
        <div className="surface-card p-7">
          <div className="eyebrow">Domain Mix</div>
          <h2 className="mt-2 section-title">领域分布</h2>
          <div className="mt-4 h-64">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={domainChartData} margin={{ top: 8, right: 8, left: -18, bottom: 0 }}>
                <CartesianGrid vertical={false} stroke="#e5e5e5" />
                <XAxis dataKey="name" axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                <YAxis axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                <Tooltip contentStyle={{ borderRadius: 20, border: '1px solid #000', background: '#fff', boxShadow: '0 18px 36px rgba(17,17,17,0.08)' }} />
                <Bar dataKey="value" fill="#111111" radius={[10, 10, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </div>

        <div className="surface-card p-7">
          <div className="eyebrow">Source Mix</div>
          <h2 className="mt-2 section-title">来源层级</h2>
          <div className="mt-4 h-64">
            <ResponsiveContainer width="100%" height="100%">
              <PieChart>
                <Pie data={sourceChartData} dataKey="value" nameKey="name" innerRadius={54} outerRadius={82} paddingAngle={2}>
                  {sourceChartData.map((_, index) => (
                    <Cell key={`source-cell-${index}`} fill={palette[index % palette.length]} />
                  ))}
                </Pie>
                <Tooltip contentStyle={{ borderRadius: 20, border: '1px solid #000', background: '#fff', boxShadow: '0 18px 36px rgba(17,17,17,0.08)' }} />
              </PieChart>
            </ResponsiveContainer>
          </div>
        </div>

        <div className="surface-card p-7">
          <div className="eyebrow">Alert Priority</div>
          <h2 className="mt-2 section-title">告警优先级</h2>
          <div className="mt-4 h-64">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={alertPriorityData} margin={{ top: 8, right: 8, left: -18, bottom: 0 }}>
                <CartesianGrid vertical={false} stroke="#e5e5e5" />
                <XAxis dataKey="name" axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                <YAxis axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                <Tooltip contentStyle={{ borderRadius: 20, border: '1px solid #000', background: '#fff', boxShadow: '0 18px 36px rgba(17,17,17,0.08)' }} />
                <Bar dataKey="value" fill="#111111" radius={[10, 10, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </div>
      </section>

      <section className="grid gap-6 xl:grid-cols-[1.1fr_0.9fr]">
        <div className="surface-card p-7">
          <div className="flex items-center gap-2">
            <Bell className="h-4 w-4" />
            <div className="eyebrow">Headlines</div>
          </div>
          <h2 className="mt-2 section-title">最新新闻</h2>
          <div className="mt-6 space-y-4">
            {filteredHeadlines.map((headline) => (
              <article key={headline.id} className="rounded-[24px] border border-black bg-neutral-50 p-5">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div className="text-sm font-semibold text-black">{headline.title}</div>
                  <div className="mono-chip">
                    {headline.sourceLevel} · {headline.domain}
                  </div>
                </div>
                <div className="mt-3 text-sm leading-7 text-neutral-600">{headline.summary}</div>
                <div className="mt-3 flex flex-wrap gap-3 text-xs text-neutral-500">
                  <span>{headline.source}</span>
                  <span>{formatTime(headline.publishedAt)}</span>
                  <span>置信度 {headline.confidence.toFixed(2)}</span>
                </div>
              </article>
            ))}
          </div>
        </div>

        <div className="surface-card p-7">
          <div className="flex items-center gap-2">
            <ShieldAlert className="h-4 w-4" />
            <div className="eyebrow">Alerts</div>
          </div>
          <h2 className="mt-2 section-title">最新告警</h2>
          <div className="mt-6 space-y-4">
            {filteredAlerts.length === 0 ? (
              <EmptyStatePanel title="暂无告警" description="当前没有新的告警触发。" />
            ) : (
              filteredAlerts.map((alert) => (
                <div key={alert.id} className="rounded-[24px] border border-black bg-neutral-50 p-5">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div className="text-sm font-semibold text-black">{alert.title}</div>
                  <div className="mono-chip">
                    {alert.priority} · {alert.status}
                  </div>
                </div>
                <div className="mt-3 text-sm leading-7 text-neutral-600">{alert.message}</div>
                <div className="mt-3 flex flex-wrap gap-2">
                    <button
                      type="button"
                      onClick={() => acknowledgeAlert(alert.id)}
                      className="action-light px-4 py-2 text-xs"
                    >
                      确认
                    </button>
                    <button
                      type="button"
                      onClick={() => dismissAlert(alert.id)}
                      className="action-light px-4 py-2 text-xs"
                    >
                      忽略
                    </button>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      </section>

      <section className="grid gap-6 xl:grid-cols-3">
        <div className="surface-card p-7">
          <div className="flex items-center gap-2">
            <Bookmark className="h-4 w-4" />
            <div className="eyebrow">Documents</div>
          </div>
          <h2 className="mt-2 section-title">最近文档</h2>
          <div className="mt-6 space-y-4">
            {filteredDocuments.map((doc) => (
              <div key={doc.id} className="rounded-[24px] border border-black bg-neutral-50 p-5">
                <div className="text-sm font-semibold text-black">{doc.title}</div>
                <div className="mt-2 text-sm leading-7 text-neutral-600">{doc.summary}</div>
                <div className="mt-3 flex items-center justify-between gap-3 text-xs text-neutral-500">
                  <span>{doc.source}</span>
                  <span>{formatTime(doc.publishedAt)}</span>
                </div>
                <button type="button" onClick={() => toggleBookmark(doc.id)} className="action-light mt-3 px-4 py-2 text-xs">
                  {doc.bookmarked ? '取消收藏' : '加入收藏'}
                </button>
              </div>
            ))}
          </div>
        </div>

        <div className="surface-card p-7">
          <div className="eyebrow">Federal Register</div>
          <h2 className="mt-2 section-title">联邦公报</h2>
          <div className="mt-6 space-y-4">
            {sampleFederalDocs.map((doc) => (
              <div key={doc.id} className="rounded-[24px] border border-black bg-neutral-50 p-5">
                <div className="text-sm font-semibold text-black">{doc.title}</div>
                <div className="mt-2 text-xs text-neutral-500">
                  {doc.type} · {formatTime(doc.publishedAt)}
                </div>
                <div className="mt-3 text-sm leading-7 text-neutral-600">{doc.abstractText}</div>
              </div>
            ))}
          </div>
        </div>

        <div className="surface-card p-7">
          <div className="eyebrow">Sanctions</div>
          <h2 className="mt-2 section-title">制裁更新</h2>
          <div className="mt-6 space-y-4">
            {sampleSdnUpdates.map((item) => (
              <div key={item.id} className="rounded-[24px] border border-black bg-neutral-50 p-5">
                <div className="flex items-center justify-between gap-3">
                  <div className="text-sm font-semibold text-black">{item.name}</div>
                  <div className="mono-chip">{item.changeType}</div>
                </div>
                <div className="mt-2 text-xs text-neutral-500">
                  {item.type} · {item.country} · {formatTime(item.addedDate)}
                </div>
                <div className="mt-3 text-sm text-neutral-600">{item.remarks}</div>
              </div>
            ))}
          </div>
        </div>
      </section>
    </AppShell>
  )
}

export default NewsIntelligence
