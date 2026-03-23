import { useCallback, useEffect, useMemo, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { AppShell } from '@/components/AppShell'
import { StatusBanner } from '@/components/StatusBanner'
import { useAuth } from '@/contexts/AuthContext'
import { exchangeAPI, type SystemEndpointStatus } from '@/services/exchangeAPI'

export function SystemStatus() {
  const { session } = useAuth()
  const [items, setItems] = useState<SystemEndpointStatus[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [statusFilter, setStatusFilter] = useState<'all' | 'running' | 'unavailable'>('all')
  const [selectedEndpointName, setSelectedEndpointName] = useState<string | null>(null)
  const workspaceSequence = [
    { label: '左侧', title: '先看总览', hint: '优先确认有多少服务不可用，再看本地会话与刷新时间。' },
    { label: '中间', title: '再看图与列表', hint: '通过延迟图和端点列表快速判断问题范围。' },
    { label: '右侧', title: '最后做诊断', hint: '锁定一个端点后，再依据严重度与建议动作处理。' },
  ]

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setItems(await exchangeAPI.getSystemStatus())
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : '系统状态拉取失败')
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const summary = useMemo(() => {
    const running = items.filter((item) => item.status === 'running').length
    const unavailable = items.length - running
    const latencyValues = items.map((item) => item.latencyMs).filter((item): item is number => typeof item === 'number')
    const avgLatency = latencyValues.length ? Math.round(latencyValues.reduce((sum, value) => sum + value, 0) / latencyValues.length) : 0
    const peakLatency = latencyValues.length ? Math.max(...latencyValues) : 0
    return { running, unavailable, avgLatency, peakLatency }
  }, [items])

  const latencyChartData = useMemo(() => items.map((item) => ({ name: item.name, latency: item.latencyMs ?? 0 })), [items])
  const visibleItems = useMemo(
    () => (statusFilter === 'all' ? items : items.filter((item) => item.status === statusFilter)),
    [items, statusFilter],
  )
  const statusBreakdown = useMemo(
    () => [
      { label: '运行中服务', value: String(summary.running), hint: '当前可响应', tone: 'positive' },
      { label: '不可用服务', value: String(summary.unavailable), hint: '需要排查', tone: summary.unavailable > 0 ? 'negative' : 'neutral' },
      { label: '平均延迟', value: `${summary.avgLatency} ms`, hint: '整体体验', tone: 'neutral' },
      { label: '峰值延迟', value: `${summary.peakLatency} ms`, hint: '最慢端点', tone: 'neutral' },
    ],
    [summary],
  )
  const selectedEndpoint = useMemo(
    () => visibleItems.find((item) => item.name === selectedEndpointName) ?? visibleItems[0] ?? null,
    [selectedEndpointName, visibleItems],
  )
  const selectedSeverity = useMemo(() => {
    if (!selectedEndpoint) return 'unknown'
    if (selectedEndpoint.status !== 'running') return 'critical'
    if ((selectedEndpoint.latencyMs ?? 0) > 800) return 'high'
    if ((selectedEndpoint.latencyMs ?? 0) > 250) return 'medium'
    return 'healthy'
  }, [selectedEndpoint])
  const highestRiskEndpoint = useMemo(() => {
    if (items.length === 0) return null
    return [...items].sort((left, right) => {
      const leftPenalty = left.status !== 'running' ? 10_000 : left.latencyMs ?? 0
      const rightPenalty = right.status !== 'running' ? 10_000 : right.latencyMs ?? 0
      return rightPenalty - leftPenalty
    })[0] ?? null
  }, [items])
  const severityFacts = useMemo(
    () => [
      {
        label: '优先级',
        value: selectedSeverity,
        hint:
          selectedSeverity === 'critical'
            ? '立即处理'
            : selectedSeverity === 'high'
              ? '优先关注'
              : selectedSeverity === 'medium'
                ? '持续观察'
                : '当前稳定',
      },
      {
        label: '当前端点',
        value: selectedEndpoint?.name ?? '未选择',
        hint: selectedEndpoint?.status === 'running' ? '服务可达' : '服务异常或未达',
      },
      {
        label: '下一动作',
        value: selectedEndpoint?.status === 'running' ? '看延迟波动' : '先恢复服务',
        hint: selectedEndpoint?.status === 'running' ? '继续核对峰值与平均值' : '再检查地址、端口、鉴权',
      },
    ],
    [selectedEndpoint, selectedSeverity],
  )

  useEffect(() => {
    if (!visibleItems.some((item) => item.name === selectedEndpointName)) {
      setSelectedEndpointName(visibleItems[0]?.name ?? null)
    }
  }, [selectedEndpointName, visibleItems])

  return (
    <AppShell title="系统状态" subtitle="把服务健康、延迟、会话上下文与逐项探测结果收束到同一页，帮助快速确认当前链路是否足够稳定。">
      <StatusBanner
        tone={error ? 'danger' : summary.unavailable > 0 ? 'warning' : 'success'}
        eyebrow="Health"
        title={error ? '探活异常' : loading ? '正在刷新状态' : '系统状态已更新'}
        message={error ?? (loading ? '正在重新探测各服务，请稍候。' : `当前共有 ${summary.running} 个服务可用，${summary.unavailable} 个服务不可用。`)}
        trailing={<button type="button" onClick={() => void load()} className="action-light px-4 py-2.5"><RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />刷新</button>}
      />

      <section className="surface-card p-6 md:p-7">
        <div className="workspace-lane-grid">
          {workspaceSequence.map((item) => (
            <div key={item.label} className="workspace-lane-item">
              <div className="workspace-lane-label">{item.label}</div>
              <div className="workspace-lane-title">{item.title}</div>
              <div className="workspace-lane-hint">{item.hint}</div>
            </div>
          ))}
        </div>
      </section>

      <section className="grid gap-6 xl:grid-cols-[340px_minmax(0,1fr)_360px]">
        <div className="sticky-rail space-y-6">
          <div className="surface-card p-7">
            <div className="eyebrow">Overview</div>
            <h2 className="mt-3 text-[28px] font-semibold tracking-[-0.05em] text-black">系统健康总览</h2>
            <p className="section-copy mt-3">把服务可用性、会话上下文与刷新状态放在同一个左侧锚点里，方便先判断“现在能不能安全继续操作”。</p>
            <div className="mt-5 grid gap-3">
              {statusBreakdown.map((item) => (
                <div key={item.label} className="surface-soft px-5 py-4">
                  <div className="flex items-center justify-between gap-4">
                    <div>
                      <div className="eyebrow">{item.label}</div>
                      <div className={`mt-2 text-[22px] font-semibold tracking-[-0.05em] ${item.tone === 'positive' ? 'signal-positive' : item.tone === 'negative' ? 'signal-negative' : 'text-black'}`}>{item.value}</div>
                    </div>
                    <div className="premium-micro">{item.hint}</div>
                  </div>
                </div>
              ))}
            </div>
          </div>

          <div className="surface-card p-7">
            <div className="eyebrow">Session</div>
            <div className="mt-5 space-y-3">
              <div className="stat-tile text-sm leading-7 text-neutral-700">
                <div>用户名：{session?.username ?? '-'}</div>
                <div className="mt-3">角色：{session?.role ?? '-'}</div>
                <div className="mt-3">本地 Token：{session?.token ?? '-'}</div>
                <div className="mt-3">上次刷新：{loading ? '刷新中…' : new Date().toLocaleString()}</div>
              </div>
              <div className="context-hint">
                <div className="context-hint-title">排查顺序</div>
                <div className="context-hint-copy">先看不可用服务，再看峰值延迟，最后对照右侧逐项端点说明定位问题。</div>
              </div>
            </div>
          </div>
        </div>

        <div className="space-y-6">
          <div className="surface-card p-7">
            <div className="flex items-end justify-between gap-4">
              <div>
                <div className="eyebrow">Latency Chart</div>
                <h2 className="mt-2 section-title">服务延迟图</h2>
              </div>
              <div className="premium-micro">按当前探测结果聚合</div>
            </div>
            {highestRiskEndpoint ? (
              <div className="selection-workbench mt-5">
                <div className="context-hint-title">当前最需要关注的端点</div>
                <div className="selection-workbench-grid mt-3">
                  <div className="selection-workbench-item">
                    <div className="selection-workbench-label">端点</div>
                    <div className="selection-workbench-value">{highestRiskEndpoint.name}</div>
                  </div>
                  <div className="selection-workbench-item">
                    <div className="selection-workbench-label">状态</div>
                    <div className="selection-workbench-value">{highestRiskEndpoint.status === 'running' ? '运行中' : '不可用'}</div>
                  </div>
                  <div className="selection-workbench-item">
                    <div className="selection-workbench-label">延迟</div>
                    <div className="selection-workbench-value">{highestRiskEndpoint.latencyMs ?? '-'} ms</div>
                  </div>
                  <div className="selection-workbench-item">
                    <div className="selection-workbench-label">建议</div>
                    <div className="selection-workbench-value">{highestRiskEndpoint.status === 'running' ? '继续观察波动' : '优先恢复服务'}</div>
                  </div>
                </div>
              </div>
            ) : null}
            <div className="chart-shell mt-5 h-72">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={latencyChartData} margin={{ top: 8, right: 8, left: -18, bottom: 0 }}>
                  <CartesianGrid vertical={false} stroke="#ececec" />
                  <XAxis dataKey="name" axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                  <YAxis axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                  <Tooltip cursor={{ fill: '#f5f5f5' }} contentStyle={{ borderRadius: 20, border: '1px solid #000', background: '#fff', boxShadow: '0 18px 36px rgba(17,17,17,0.08)' }} />
                  <Bar dataKey="latency" fill="#111111" radius={[10, 10, 0, 0]} />
                </BarChart>
              </ResponsiveContainer>
            </div>
          </div>

          <div className="surface-card p-7">
            <div className="flex items-end justify-between gap-4">
              <div>
                <div className="eyebrow">Endpoints</div>
                <h2 className="mt-2 section-title">当前探测结果</h2>
              </div>
              <div className="flex flex-wrap gap-2">
                {[
                  { value: 'all', label: '全部' },
                  { value: 'running', label: '运行中' },
                  { value: 'unavailable', label: '不可用' },
                ].map((option) => (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => setStatusFilter(option.value as 'all' | 'running' | 'unavailable')}
                    className={`rounded-full border px-3 py-2 text-xs transition ${statusFilter === option.value ? 'border-black bg-black text-white' : 'border-black bg-white text-black hover:bg-neutral-100'}`}
                  >
                    {option.label}
                  </button>
                ))}
              </div>
            </div>

            <div className="table-shell mt-6">
              <div className="table-head grid-cols-[1.2fr_0.55fr_0.45fr]">
                <div>服务</div>
                <div>状态</div>
                <div>延迟</div>
              </div>
              {visibleItems.map((item) => (
                <button
                  key={item.name}
                  type="button"
                  onClick={() => setSelectedEndpointName(item.name)}
                  className={`table-row table-row-interactive grid-cols-[1.2fr_0.55fr_0.45fr] text-left ${selectedEndpoint?.name === item.name ? 'table-row-active' : ''}`}
                >
                  <div className="min-w-0">
                    <div className="truncate font-medium text-black">{item.name}</div>
                    <div className="mt-1 truncate text-xs text-neutral-500">{item.url}</div>
                    <div className="mt-2 truncate text-sm leading-7 text-neutral-600">{item.details ?? '未返回额外说明。'}</div>
                  </div>
                  <div>
                    <div className={`inline-flex rounded-full border px-3 py-1 text-xs ${item.status === 'running' ? 'border-black bg-white text-black' : 'border-black bg-neutral-200 text-black'}`}>{item.status === 'running' ? '运行中' : '不可用'}</div>
                  </div>
                  <div className="data-mono font-medium text-black">{item.latencyMs ?? '-'} ms</div>
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="sticky-rail space-y-6">
          <div className="surface-card p-7">
            <div className="eyebrow">Endpoint Detail</div>
            <h3 className="mt-2 section-title">端点详情</h3>
            {selectedEndpoint ? (
              <>
                <div className="selection-workbench mt-5">
                  <div className="context-hint-title">处置摘要</div>
                  <div className="selection-workbench-grid mt-3">
                    {severityFacts.map((item) => (
                      <div key={item.label} className="selection-workbench-item">
                        <div className="selection-workbench-label">{item.label}</div>
                        <div className="selection-workbench-value">{item.value}</div>
                        <div className="mt-2 text-xs leading-6 text-neutral-500">{item.hint}</div>
                      </div>
                    ))}
                  </div>
                </div>
                <div className="mt-5 grid gap-3 sm:grid-cols-2 xl:grid-cols-1">
                  <div className="hero-stat">
                    <div className="hero-stat-label">当前端点</div>
                    <div className="hero-stat-value">{selectedEndpoint.name}</div>
                  </div>
                  <div className="hero-stat">
                    <div className="hero-stat-label">当前状态</div>
                    <div className="hero-stat-value">{selectedEndpoint.status === 'running' ? '运行中' : '不可用'}</div>
                  </div>
                </div>
                <div className="mt-4 flex flex-wrap gap-2">
                  <span className="mono-chip">{selectedEndpoint.url}</span>
                  <span className="mono-chip">{selectedEndpoint.latencyMs ?? '-'} ms</span>
                  <span className="mono-chip">严重度 {selectedSeverity}</span>
                </div>
                <div className="context-hint mt-5">
                  <div className="context-hint-title">诊断建议</div>
                  <div className="context-hint-copy">
                    {selectedEndpoint.status === 'running'
                      ? '当前端点可用；若体验仍然不稳定，优先对照平均延迟与峰值延迟，判断是否属于局部抖动。'
                      : '当前端点不可用；优先检查对应服务是否已启动，再核对网络地址、鉴权与本地端口占用。'}
                  </div>
                </div>
                <div className="surface-soft mt-5 p-4">
                  <div className="eyebrow">Detail</div>
                  <div className="mt-3 text-sm leading-7 text-neutral-700">{selectedEndpoint.details ?? '未返回额外说明。'}</div>
                </div>
              </>
            ) : (
              <StatusBanner compact tone="warning" eyebrow="Detail" title="暂无端点详情" message="当前过滤条件下没有可展示的端点。" />
            )}
          </div>
        </div>
      </section>
    </AppShell>
  )
}
