import { useCallback, useEffect, useMemo, useState } from 'react'
import { Bar, BarChart, CartesianGrid, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { AppShell } from '@/components/AppShell'
import { EmptyStatePanel } from '@/components/EmptyStatePanel'
import { StatusBanner } from '@/components/StatusBanner'
import { useAuth } from '@/contexts/AuthContext'
import { clearAnnouncement, readAnnouncement, subscribeAnnouncement, writeAnnouncement } from '@/services/announcementStore'
import { exchangeAPI, type GovernanceActionRecord } from '@/services/exchangeAPI'

function pretty(value: unknown) {
  try { return JSON.stringify(value, null, 2) } catch { return String(value) }
}

function parseResponseError(value: unknown) {
  if (!value || typeof value !== 'object') return null
  const record = value as Record<string, unknown>
  const message = typeof record.message === 'string' ? record.message : undefined
  const error = typeof record.error === 'string' ? record.error : undefined
  const code = typeof record.code === 'string' ? record.code : undefined
  const details = record.details ? pretty(record.details) : undefined
  if (!message && !error && !code && !details) return null
  return { code, message, error, details }
}

function readNumber(value: unknown) {
  if (typeof value === 'number') return value
  if (typeof value === 'string') {
    const n = Number(value)
    return Number.isFinite(n) ? n : 0
  }
  return 0
}

export function AdminControlPage() {
  const { session } = useAuth()
  const [loading, setLoading] = useState(false)
  const [instruments, setInstruments] = useState<Record<string, unknown>[]>([])
  const [fundingRates, setFundingRates] = useState<Record<string, unknown>[]>([])
  const [riskEvents, setRiskEvents] = useState<Record<string, unknown>[]>([])
  const [governanceActions, setGovernanceActions] = useState<GovernanceActionRecord[]>([])
  const [selectedActionId, setSelectedActionId] = useState<string | null>(null)
  const [actionHistory, setActionHistory] = useState<Array<{ id: string; title: string; status: 'ok' | 'error'; time: string; message: string }>>([])
  const [notice, setNotice] = useState('管理控制面已接入真实读链路，写操作会返回最新响应体。')
  const [lastResponse, setLastResponse] = useState<unknown>(null)
  const [marketId, setMarketId] = useState('btc-usdt')
  const [marketState, setMarketState] = useState('trading')
  const [fundingMarketId, setFundingMarketId] = useState('perp:btc-usdt')
  const [fundingRatePpm, setFundingRatePpm] = useState('125')
  const [governanceStatus, setGovernanceStatus] = useState<'all' | 'pending' | 'approved' | 'rejected'>('all')
  const [actionSearch, setActionSearch] = useState('')
  const [actionTypeFilter, setActionTypeFilter] = useState('all')
  const [announcementText, setAnnouncementText] = useState('')
  const [announcementEnabled, setAnnouncementEnabled] = useState(true)
  const [announcementUpdatedAt, setAnnouncementUpdatedAt] = useState('')
  const [announcementUpdatedBy, setAnnouncementUpdatedBy] = useState('system')
  const workspaceSequence = [
    { label: '左侧', title: '先做控制', hint: '先确认 kill-switch、市场状态和资金费率这些控制面动作。' },
    { label: '中间', title: '再看治理池', hint: '从治理列表里筛出当前最需要处理的动作。' },
    { label: '右侧', title: '最后核对审批', hint: '审批前后都在右侧确认 payload、审批进度与返回体。' },
  ]

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const [nextInstruments, nextFundingRates, nextRiskEvents, nextGovernanceActions] = await Promise.all([
        exchangeAPI.listAdminInstruments().catch(() => []),
        exchangeAPI.listFundingRates().catch(() => []),
        exchangeAPI.listRiskEvents(12).catch(() => []),
        exchangeAPI.listGovernanceActions(20, governanceStatus === 'all' ? undefined : governanceStatus).catch(() => []),
      ])
      setInstruments(nextInstruments)
      setFundingRates(nextFundingRates)
      setRiskEvents(nextRiskEvents)
      setGovernanceActions(nextGovernanceActions)
      setSelectedActionId((current) => current ?? nextGovernanceActions[0]?.action_id ?? null)
    } finally {
      setLoading(false)
    }
  }, [governanceStatus])

  const syncAnnouncementState = useCallback((current = readAnnouncement()) => {
    setAnnouncementText(current.text)
    setAnnouncementEnabled(current.enabled)
    setAnnouncementUpdatedAt(current.updatedAt)
    setAnnouncementUpdatedBy(current.updatedBy ?? 'system')
  }, [])

  useEffect(() => { void load() }, [load])

  useEffect(() => {
    syncAnnouncementState()
    return subscribeAnnouncement(syncAnnouncementState)
  }, [syncAnnouncementState])

  const selectedAction = useMemo(() => governanceActions.find((item) => item.action_id === selectedActionId) ?? null, [governanceActions, selectedActionId])
  const governanceChartData = useMemo(() => {
    const counts = new Map<string, number>()
    for (const action of governanceActions) counts.set(action.status, (counts.get(action.status) ?? 0) + 1)
    return Array.from(counts.entries()).map(([name, count]) => ({ name, count }))
  }, [governanceActions])
  const fundingChartData = useMemo(() => fundingRates.slice(0, 8).map((item, index) => ({ name: String(item.market_id ?? item.marketId ?? `M${index + 1}`), rate: readNumber(item.funding_rate_ppm ?? item.fundingRatePpm ?? item.rate_ppm ?? item.rate) / 10000 })), [fundingRates])
  const pendingCount = governanceActions.filter((item) => item.status === 'pending').length
  const parsedLastResponseError = useMemo(() => parseResponseError(lastResponse), [lastResponse])
  const controlSummary = useMemo(
    () => [
      { label: '待审批动作', value: String(pendingCount), hint: '优先处理' },
      { label: '风险事件', value: String(riskEvents.length), hint: '最近样本' },
      { label: '资金费率', value: String(fundingRates.length), hint: '当前条目' },
    ],
    [fundingRates.length, pendingCount, riskEvents.length],
  )
  const remainingApprovals = useMemo(() => {
    if (!selectedAction) return 0
    const required = selectedAction.required_approvals ?? 0
    const approved = selectedAction.approvers?.length ?? 0
    return Math.max(required - approved, 0)
  }, [selectedAction])
  const actionTypes = useMemo(() => ['all', ...Array.from(new Set(governanceActions.map((item) => item.action_type)))], [governanceActions])
  const visibleGovernanceActions = useMemo(
    () =>
      governanceActions.filter((action) => {
        const matchesType = actionTypeFilter === 'all' || action.action_type === actionTypeFilter
        const search = actionSearch.trim().toLowerCase()
        const matchesSearch =
          search.length === 0 ||
          action.action_type.toLowerCase().includes(search) ||
          action.action_id.toLowerCase().includes(search) ||
          action.requested_by.toLowerCase().includes(search)
        return matchesType && matchesSearch
      }),
    [actionSearch, actionTypeFilter, governanceActions],
  )
  const workflowSummary = useMemo(
    () => [
      { label: '当前筛选结果', value: String(visibleGovernanceActions.length), hint: '列表中可见' },
      { label: '待审批', value: String(visibleGovernanceActions.filter((item) => item.status === 'pending').length), hint: '优先处理' },
      { label: '已完成', value: String(visibleGovernanceActions.filter((item) => item.status !== 'pending').length), hint: '通过或拒绝' },
    ],
    [visibleGovernanceActions],
  )
  const governanceWorkbench = useMemo(
    () => [
      {
        label: '当前动作',
        value: selectedAction?.action_type ?? '未选择',
        hint: selectedAction?.action_id ?? '等待从中间列表选择',
      },
      {
        label: '申请人',
        value: selectedAction?.requested_by ?? '-',
        hint: selectedAction ? '发起治理动作的操作者' : '暂无上下文',
      },
      {
        label: '剩余审批',
        value: String(remainingApprovals),
        hint: remainingApprovals > 0 ? '仍需继续收集' : '已达到门槛',
      },
    ],
    [remainingApprovals, selectedAction],
  )
  const approvalSteps = useMemo(
    () => [
      {
        label: '创建动作',
        state: selectedAction ? 'done' : 'idle',
        detail: selectedAction ? '动作已进入治理列表。' : '等待选择动作。',
      },
      {
        label: '收集审批',
        state: selectedAction ? ((selectedAction.approvers?.length ?? 0) >= (selectedAction.required_approvals ?? 0) ? 'done' : 'active') : 'idle',
        detail: selectedAction ? `${selectedAction.approvers?.length ?? 0}/${selectedAction.required_approvals ?? 0} 已完成` : '暂无上下文',
      },
      {
        label: '核对返回',
        state: lastResponse ? 'active' : 'idle',
        detail: lastResponse ? '右侧已记录最近一次返回。' : '执行操作后在右侧确认返回体。',
      },
    ],
    [lastResponse, selectedAction],
  )

  useEffect(() => {
    if (!visibleGovernanceActions.some((item) => item.action_id === selectedActionId)) {
      setSelectedActionId(visibleGovernanceActions[0]?.action_id ?? null)
    }
  }, [selectedActionId, visibleGovernanceActions])

  async function runAction(task: Promise<{ ok: boolean; message: string; raw?: unknown }>, successNotice: string, actionTitle = '控制动作') {
    const result = await task
    const actionStatus: 'ok' | 'error' = result.ok ? 'ok' : 'error'
    setLastResponse(result.raw ?? result)
    setNotice(result.ok ? successNotice : result.message)
    setActionHistory((current) => [
      { id: `${Date.now()}-${Math.random()}`, title: actionTitle, status: actionStatus, time: new Date().toLocaleTimeString(), message: result.ok ? successNotice : result.message },
      ...current,
    ].slice(0, 8))
    await load()
  }

  function publishAnnouncement() {
    const next = {
      text: announcementText.trim(),
      enabled: announcementEnabled && announcementText.trim().length > 0,
      updatedAt: new Date().toISOString(),
      updatedBy: session?.displayName ?? session?.username ?? 'admin',
    }
    writeAnnouncement(next)
    setAnnouncementUpdatedAt(next.updatedAt)
    setAnnouncementUpdatedBy(next.updatedBy)
    setAnnouncementEnabled(next.enabled)
    setNotice(next.enabled ? '交易所公告已发布，顶部滚动条已更新。' : '公告内容为空，顶部滚动条已关闭。')
    setLastResponse({ status: 'ok', type: 'announcement_publish', announcement: next })
    setActionHistory((current) => [
      { id: `${Date.now()}-${Math.random()}`, title: '公告发布', status: 'ok' as const, time: new Date().toLocaleTimeString(), message: next.enabled ? '已发布顶部公告。' : '已关闭顶部公告。' },
      ...current,
    ].slice(0, 8))
  }

  function disableAnnouncement() {
    const next = {
      text: announcementText.trim(),
      enabled: false,
      updatedAt: new Date().toISOString(),
      updatedBy: session?.displayName ?? session?.username ?? 'admin',
    }
    writeAnnouncement(next)
    setAnnouncementUpdatedAt(next.updatedAt)
    setAnnouncementUpdatedBy(next.updatedBy)
    setAnnouncementEnabled(false)
    setNotice('顶部公告已停用。')
    setLastResponse({ status: 'ok', type: 'announcement_disable', announcement: next })
  }

  function resetAnnouncement() {
    clearAnnouncement()
    const current = readAnnouncement()
    syncAnnouncementState(current)
    setNotice('顶部公告已恢复默认内容。')
    setLastResponse({ status: 'ok', type: 'announcement_reset', announcement: current })
  }

  if (session?.role !== 'admin') {
    return (
      <AppShell title="管理控制台" subtitle="该页面仅向管理员开放，用于控制面动作、治理审批与风险观察。">
        <StatusBanner tone="warning" eyebrow="Access" title="当前账号无管理员权限" message="请切换到 admin / admin2 / admin3 演示账户后再访问此页面。" />
      </AppShell>
    )
  }

  return (
    <AppShell title="管理控制台" subtitle="控制面聚焦三件事：修改交易状态、执行治理动作、核对最近一次真实返回。布局继续向旗舰控制台收口。">
      <StatusBanner
        tone="neutral"
        eyebrow="Control Plane"
        title="当前提示"
        message={notice}
        trailing={<button type="button" onClick={() => void load()} className="action-light px-4 py-2.5">{loading ? '刷新中…' : '刷新数据'}</button>}
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

      <section className="grid gap-6 xl:grid-cols-[340px_minmax(0,1fr)_380px]">
        <div className="sticky-rail space-y-6">
          <div className="surface-card p-7">
            <div className="eyebrow">Control Summary</div>
            <h2 className="mt-3 text-[28px] font-semibold tracking-[-0.05em] text-black">管理员工作台</h2>
            <p className="section-copy mt-3">左侧先看全局压力与快捷动作，中间处理治理与图表，右侧核对审批详情与真实返回。</p>
            <div className="mt-5 grid gap-3">
              {controlSummary.map((item) => (
                <div key={item.label} className="surface-soft px-5 py-4">
                  <div className="flex items-center justify-between gap-4">
                    <div>
                      <div className="eyebrow">{item.label}</div>
                      <div className="mt-2 text-[22px] font-semibold tracking-[-0.05em] text-black">{item.value}</div>
                    </div>
                    <div className="premium-micro">{item.hint}</div>
                  </div>
                </div>
              ))}
            </div>
          </div>

          <div className="surface-card p-7">
            <div className="eyebrow">Quick Actions</div>
            <h3 className="mt-2 section-title">控制动作</h3>
            <div className="context-hint mt-4">
              <div className="context-hint-title">操作提示</div>
              <div className="context-hint-copy">先执行全局开关，再修改市场状态与资金费率；所有写动作的最新返回都会固定显示在右侧。</div>
            </div>
            <div className="mt-5 grid gap-3">
              <button type="button" onClick={() => void runAction(exchangeAPI.setKillSwitch(true), '已请求开启 Kill-Switch。', 'Kill-Switch 开启')} className="action-dark">开启 Kill-Switch</button>
              <button type="button" onClick={() => void runAction(exchangeAPI.setKillSwitch(false), '已请求关闭 Kill-Switch。', 'Kill-Switch 关闭')} className="action-light">关闭 Kill-Switch</button>
            </div>
          </div>

          <div className="surface-card p-7">
            <div className="eyebrow">Market State</div>
            <div className="mt-5 space-y-4">
              <input value={marketId} onChange={(event) => setMarketId(event.target.value)} className="field-shell" placeholder="市场 ID" />
              <select value={marketState} onChange={(event) => setMarketState(event.target.value)} className="field-shell">
                <option value="trading">trading</option>
                <option value="halted">halted</option>
                <option value="auction">auction</option>
                <option value="settlement">settlement</option>
              </select>
              <button type="button" onClick={() => void runAction(exchangeAPI.setMarketState(marketId, marketState, 0), `已请求将 ${marketId} 状态修改为 ${marketState}。`, '市场状态变更')} className="action-light w-full">提交市场状态变更</button>
            </div>
          </div>

          <div className="surface-card p-7">
            <div className="eyebrow">Funding Rate</div>
            <div className="mt-5 space-y-4">
              <input value={fundingMarketId} onChange={(event) => setFundingMarketId(event.target.value)} className="field-shell" placeholder="永续市场 ID" />
              <input value={fundingRatePpm} onChange={(event) => setFundingRatePpm(event.target.value)} className="field-shell" placeholder="资金费率 ppm" />
              <button type="button" onClick={() => void runAction(exchangeAPI.upsertFundingRate(fundingMarketId, Number(fundingRatePpm) || 0, 0), `已请求写入 ${fundingMarketId} 资金费率。`, '资金费率写入')} className="action-light w-full">写入资金费率</button>
            </div>
          </div>

          <div className="surface-card p-7">
            <div className="eyebrow">Announcement</div>
            <h3 className="mt-2 section-title">交易所公告推送</h3>
            <div className="context-hint mt-4">
              <div className="context-hint-title">推送说明</div>
              <div className="context-hint-copy">在这里直接编辑公告文案，点击发布后，前台顶部滚动公告会立即同步更新。适合紧急通知、系统维护提示和活动消息。</div>
            </div>
            <div className="mt-5 space-y-4">
              <textarea
                value={announcementText}
                onChange={(event) => setAnnouncementText(event.target.value)}
                className="field-shell min-h-[132px] resize-none"
                placeholder="输入要推送到交易所顶部滚动条的公告文字…"
              />
              <label className="inline-flex items-center gap-3 rounded-full border border-black bg-white px-4 py-3 text-sm text-black">
                <input
                  type="checkbox"
                  checked={announcementEnabled}
                  onChange={(event) => setAnnouncementEnabled(event.target.checked)}
                  className="h-4 w-4 accent-black"
                />
                <span>发布后立即启用顶部滚动公告</span>
              </label>
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="surface-soft px-4 py-4">
                  <div className="eyebrow">最后更新</div>
                  <div className="mt-2 text-sm font-medium text-black">{announcementUpdatedAt ? new Date(announcementUpdatedAt).toLocaleString('zh-CN', { hour12: false }) : '-'}</div>
                </div>
                <div className="surface-soft px-4 py-4">
                  <div className="eyebrow">更新人</div>
                  <div className="mt-2 text-sm font-medium text-black">{announcementUpdatedBy || '-'}</div>
                </div>
              </div>
              <div className="flex flex-wrap gap-2">
                <button type="button" onClick={publishAnnouncement} className="action-dark px-4 py-2 text-xs">发布公告</button>
                <button type="button" onClick={disableAnnouncement} className="action-light px-4 py-2 text-xs">停用公告</button>
                <button type="button" onClick={resetAnnouncement} className="action-light px-4 py-2 text-xs">恢复默认</button>
              </div>
            </div>
          </div>

          <div className="surface-card p-7">
            <div className="eyebrow">Operator</div>
            <div className="mt-5 stat-tile text-sm leading-7 text-neutral-700">
              <div>当前管理员：{session?.displayName ?? '-'}</div>
              <div className="mt-3">角色：{session?.role ?? '-'}</div>
              <div className="mt-3">治理动作总数：{governanceActions.length}</div>
              <div className="mt-3">本地最近刷新：{loading ? '刷新中…' : new Date().toLocaleString()}</div>
            </div>
          </div>
        </div>

        <div className="space-y-6">
          <div className="surface-card p-7">
            <div className="flex items-center justify-between gap-4">
              <div><div className="eyebrow">Governance</div><h2 className="mt-2 section-title">治理动作</h2></div>
              <div className="flex flex-wrap gap-2">
                <select value={governanceStatus} onChange={(event) => setGovernanceStatus(event.target.value as 'all' | 'pending' | 'approved' | 'rejected')} className="field-shell max-w-[180px] rounded-full px-4 py-2">
                  <option value="all">全部状态</option>
                  <option value="pending">待审批</option>
                  <option value="approved">已通过</option>
                  <option value="rejected">已拒绝</option>
                </select>
                <select value={actionTypeFilter} onChange={(event) => setActionTypeFilter(event.target.value)} className="field-shell max-w-[180px] rounded-full px-4 py-2">
                  {actionTypes.map((type) => (
                    <option key={type} value={type}>{type === 'all' ? '全部动作' : type}</option>
                  ))}
                </select>
              </div>
            </div>
            <div className="mt-4">
              <input value={actionSearch} onChange={(event) => setActionSearch(event.target.value)} className="field-shell" placeholder="搜索动作类型 / action id / 申请人" />
            </div>
            <div className="mt-4 grid gap-3 md:grid-cols-3">
              {workflowSummary.map((item) => (
                <div key={item.label} className="surface-soft px-4 py-4">
                  <div className="eyebrow">{item.label}</div>
                  <div className="mt-2 text-lg font-semibold tracking-[-0.04em] text-black">{item.value}</div>
                  <div className="mt-1 text-xs text-neutral-500">{item.hint}</div>
                </div>
              ))}
            </div>

            <div className="table-shell mt-6">
              <div className="table-head grid-cols-[0.95fr_0.5fr_0.65fr]">
                <div>动作</div>
                <div>状态</div>
                <div>审批进度</div>
              </div>
              {visibleGovernanceActions.length === 0 ? (
                <div className="p-6"><EmptyStatePanel title="暂无治理动作" description="当前过滤条件下没有可展示的数据。" /></div>
              ) : (
                visibleGovernanceActions.map((action) => (
                  <div key={action.action_id} className={`table-row table-row-interactive grid-cols-[0.95fr_0.5fr_0.65fr] ${selectedActionId === action.action_id ? 'table-row-active' : ''}`}>
                    <button type="button" onClick={() => setSelectedActionId(action.action_id)} className="min-w-0 text-left">
                      <div className="truncate text-base font-semibold text-black">{action.action_type}</div>
                      <div className="mt-1 truncate text-xs text-neutral-500">{action.action_id}</div>
                      <div className="mt-2 text-sm text-neutral-600">申请人：{action.requested_by}</div>
                    </button>
                    <div><div className="inline-flex rounded-full border border-black px-3 py-1 text-xs text-black">{action.status}</div></div>
                    <div className="text-sm text-neutral-700">{action.approvers?.length ?? 0}/{action.required_approvals ?? '-'}</div>
                  </div>
                ))
              )}
            </div>
          </div>

          <div className="grid gap-6 lg:grid-cols-2">
            <div className="surface-card p-7">
              <div className="flex items-end justify-between gap-4"><div><div className="eyebrow">Governance Chart</div><h3 className="mt-2 section-title">治理状态分布</h3></div><div className="premium-micro">按当前列表聚合</div></div>
              <div className="chart-shell mt-5 h-56">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={governanceChartData} margin={{ top: 8, right: 8, left: -18, bottom: 0 }}>
                    <CartesianGrid vertical={false} stroke="#ececec" />
                    <XAxis dataKey="name" axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                    <YAxis axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                    <Tooltip cursor={{ fill: '#f5f5f5' }} contentStyle={{ borderRadius: 20, border: '1px solid #000', background: '#fff', boxShadow: '0 18px 36px rgba(17,17,17,0.08)' }} />
                    <Bar dataKey="count" fill="#111111" radius={[10, 10, 0, 0]} />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            </div>

            <div className="surface-card p-7">
              <div className="flex items-end justify-between gap-4"><div><div className="eyebrow">Funding Trend</div><h3 className="mt-2 section-title">资金费率趋势</h3></div><div className="premium-micro">截取最近条目</div></div>
              <div className="chart-shell mt-5 h-56">
                <ResponsiveContainer width="100%" height="100%">
                  <LineChart data={fundingChartData} margin={{ top: 8, right: 8, left: -18, bottom: 0 }}>
                    <CartesianGrid vertical={false} stroke="#ececec" />
                    <XAxis dataKey="name" axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                    <YAxis axisLine={false} tickLine={false} fontSize={12} stroke="#525252" />
                    <Tooltip contentStyle={{ borderRadius: 20, border: '1px solid #000', background: '#fff', boxShadow: '0 18px 36px rgba(17,17,17,0.08)' }} />
                    <Line type="monotone" dataKey="rate" stroke="#111111" strokeWidth={2} dot={false} />
                  </LineChart>
                </ResponsiveContainer>
              </div>
            </div>
          </div>
        </div>

        <div className="sticky-rail space-y-6">
          <div className="surface-card p-7">
            <div className="eyebrow">Action Detail</div>
            <h3 className="mt-2 section-title">动作详情</h3>
            {selectedAction ? (
              <>
                <div className="selection-workbench mt-5">
                  <div className="context-hint-title">审批工作区</div>
                  <div className="selection-workbench-grid mt-3">
                    {governanceWorkbench.map((item) => (
                      <div key={item.label} className="selection-workbench-item">
                        <div className="selection-workbench-label">{item.label}</div>
                        <div className="selection-workbench-value">{item.value}</div>
                        <div className="mt-2 text-xs leading-6 text-neutral-500">{item.hint}</div>
                      </div>
                    ))}
                  </div>
                </div>
                <div className="mt-5 grid gap-3 sm:grid-cols-2">
                  <div className="hero-stat">
                    <div className="hero-stat-label">动作状态</div>
                    <div className="hero-stat-value">{selectedAction.status}</div>
                  </div>
                  <div className="hero-stat">
                    <div className="hero-stat-label">审批进度</div>
                    <div className="hero-stat-value">{selectedAction.approvers?.length ?? 0}/{selectedAction.required_approvals ?? 0}</div>
                  </div>
                </div>
                <div className="mt-4 flex flex-wrap gap-2">
                  <span className="mono-chip">{selectedAction.action_type}</span>
                  <span className="mono-chip">申请人 {selectedAction.requested_by}</span>
                  <span className="mono-chip">{selectedAction.recorded_at ?? '未记录时间'}</span>
                  <span className="mono-chip">剩余审批 {remainingApprovals}</span>
                </div>
                <div className="mt-5 grid gap-3 sm:grid-cols-2">
                  <div className="surface-soft px-4 py-4">
                    <div className="eyebrow">Approvers</div>
                    <div className="mt-3 flex flex-wrap gap-2">
                      {(selectedAction.approvers?.length ?? 0) > 0 ? (
                        selectedAction.approvers?.map((approver) => <span key={approver} className="mono-chip">{approver}</span>)
                      ) : (
                        <span className="text-sm text-neutral-500">尚无审批人</span>
                      )}
                    </div>
                  </div>
                  <div className="surface-soft px-4 py-4">
                    <div className="eyebrow">Decision State</div>
                    <div className="mt-3 text-sm leading-7 text-neutral-700">
                      {remainingApprovals > 0 ? `当前还需要 ${remainingApprovals} 次审批动作才满足门槛。` : '当前审批数量已达到要求，可重点核对最终状态与返回体。'}
                    </div>
                  </div>
                </div>
                <div className="mt-5 flex flex-wrap gap-2">
                  <button type="button" onClick={() => void runAction(exchangeAPI.approveGovernanceAction(selectedAction.action_id, 'admin2'), `已批准动作 ${selectedAction.action_id}。`, '治理动作批准')} className="action-dark px-4 py-2 text-xs">管理员 2 批准</button>
                  <button type="button" onClick={() => void runAction(exchangeAPI.rejectGovernanceAction(selectedAction.action_id, 'admin3'), `已拒绝动作 ${selectedAction.action_id}。`, '治理动作拒绝')} className="action-light px-4 py-2 text-xs">管理员 3 拒绝</button>
                </div>
                <div className="mt-5 grid gap-3">
                  <div className="surface-soft px-4 py-4">
                    <div className="eyebrow">Workflow</div>
                    <div className="mt-3 space-y-2 text-sm text-neutral-700">
                      <div className="flex items-center justify-between gap-3"><span>1. 动作创建</span><span className="mono-chip">已记录</span></div>
                      <div className="flex items-center justify-between gap-3"><span>2. 审批收集</span><span className="mono-chip">{selectedAction.approvers?.length ?? 0}/{selectedAction.required_approvals ?? 0}</span></div>
                      <div className="flex items-center justify-between gap-3"><span>3. 最终状态</span><span className="mono-chip">{selectedAction.status}</span></div>
                    </div>
                  </div>
                </div>
                <div className="workflow-ladder mt-5">
                  {approvalSteps.map((step, index) => (
                    <div key={step.label} className="workflow-ladder-item">
                      <div className={`workflow-ladder-index ${step.state === 'done' ? 'workflow-ladder-index-done' : step.state === 'active' ? 'workflow-ladder-index-active' : ''}`}>{index + 1}</div>
                      <div>
                        <div className="workflow-ladder-title">{step.label}</div>
                        <div className="workflow-ladder-copy">{step.detail}</div>
                      </div>
                    </div>
                  ))}
                </div>
                <div className="context-hint mt-5">
                  <div className="context-hint-title">审批节奏</div>
                  <div className="context-hint-copy">先确认状态与申请人，再执行批准或拒绝；完成后立刻在下方返回体与左侧动作记录里交叉核对。</div>
                </div>
                <div className="surface-soft mt-5 p-4">
                  <div className="eyebrow">Payload Snapshot</div>
                  <div className="mt-3 text-sm leading-7 text-neutral-700">
                    {selectedAction.payload ? '当前动作已带 payload，可在下方查看完整原始结构。' : '当前动作没有额外 payload 字段。'}
                  </div>
                </div>
                <pre className="mt-5 overflow-auto rounded-[24px] border border-black bg-neutral-50 p-4 text-xs leading-6 text-neutral-700">{pretty(selectedAction)}</pre>
              </>
            ) : (
              <EmptyStatePanel title="未选择动作" description="点击中间列表任一治理动作即可查看详情与审批操作。" />
            )}
          </div>

          <div className="surface-card p-7">
            <div className="eyebrow">Observability</div>
            <div className="mt-5 space-y-3 text-sm text-neutral-700">
              <div className="surface-soft px-4 py-3">注册表条目：{instruments.length}</div>
              <div className="surface-soft px-4 py-3">资金费率条目：{fundingRates.length}</div>
              <div className="surface-soft px-4 py-3">风险事件：{riskEvents.length}</div>
            </div>
          </div>

          <div className="surface-card p-7">
            <div className="eyebrow">Action History</div>
            <h3 className="mt-2 section-title">最近控制动作</h3>
            <div className="mt-5 space-y-3">
              {actionHistory.length === 0 ? (
                <EmptyStatePanel title="暂无控制动作" description="执行任意治理或控制操作后，这里会保留最近的控制台动作记录。" />
              ) : (
                actionHistory.map((item) => (
                  <div key={item.id} className="surface-soft p-4">
                    <div className="flex items-center justify-between gap-3">
                      <div className="text-sm font-medium text-black">{item.title}</div>
                      <span className="mono-chip">{item.time}</span>
                    </div>
                    <div className={`mt-2 text-sm leading-7 ${item.status === 'error' ? 'signal-negative' : 'signal-positive'}`}>{item.message}</div>
                  </div>
                ))
              )}
            </div>
          </div>

          <div className="surface-card p-7">
            <div className="eyebrow">Last Response</div>
            <h3 className="mt-2 section-title">最近一次返回</h3>
            {parsedLastResponseError ? (
              <div className="mt-5 space-y-3">
                <div className="surface-soft p-4">
                  <div className="eyebrow">Error Summary</div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <span className="mono-chip">{parsedLastResponseError.code ?? '未提供代码'}</span>
                  </div>
                  <div className="mt-3 text-sm leading-7 text-neutral-700">{parsedLastResponseError.message ?? parsedLastResponseError.error ?? '未提供错误信息'}</div>
                  {parsedLastResponseError.details ? <pre className="mt-3 overflow-auto rounded-[18px] border border-black bg-white p-3 text-xs leading-6 text-neutral-700">{parsedLastResponseError.details}</pre> : null}
                </div>
                {lastResponse ? <pre className="overflow-auto rounded-[24px] border border-black bg-neutral-50 p-4 text-xs leading-6 text-neutral-700">{pretty(lastResponse)}</pre> : null}
              </div>
            ) : lastResponse ? <pre className="mt-5 overflow-auto rounded-[24px] border border-black bg-neutral-50 p-4 text-xs leading-6 text-neutral-700">{pretty(lastResponse)}</pre> : <EmptyStatePanel title="暂无返回体" description="执行一次控制动作后，这里会显示最新响应内容。" />}
          </div>
        </div>
      </section>
    </AppShell>
  )
}
