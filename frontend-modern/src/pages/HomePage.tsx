import { ArrowRight, LineChart, ShieldCheck, Sparkles } from 'lucide-react'
import { Link } from 'react-router-dom'
import { Bar, BarChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { AppShell } from '@/components/AppShell'
import { useAuth } from '@/contexts/AuthContext'

const modules = [
  { title: '交易终端', description: '统一承载下单、盘口、成交、挂单、OTC 与理财入口，强调清晰反馈与专业密度。', to: '/trading', icon: LineChart },
  { title: '系统状态', description: '把前端、交易 API、鉴权接口与控制面健康状态放在同一视图里快速确认。', to: '/system', icon: ShieldCheck },
  { title: '管理控制台', description: 'Kill-Switch、市场状态、资金费率与治理动作收束到统一的控制平面。', to: '/admin', icon: Sparkles },
]

const principles = [
  '信息优先级必须一眼可读，首屏只保留真正影响判断的内容。',
  '交互反馈必须克制而明确：成功、失败、回退、健康状态都要稳定可见。',
  '视觉目标是更高级的专业终端，而不是组件堆叠：更少噪音、更强秩序、更稳节奏。',
]

export function HomePage() {
  const { session } = useAuth()
  const chartData = modules.map((item, index) => ({ name: item.title, score: [95, 90, 87][index] ?? 84 }))
  const launchSequence = [
    { label: '01', title: '进入交易', detail: '先进入交易终端确认市场、余额和下单链路。' },
    { label: '02', title: '核对系统', detail: '需要排查时，再进入系统页确认延迟和不可用端点。' },
    { label: '03', title: '执行管理', detail: '仅在需要控制动作或治理审批时再切到控制台。' },
  ]

  return (
    <AppShell title="首页" subtitle="这版首页不再追求铺满信息，而是先建立更高级的秩序：入口清晰、身份稳定、模块边界明确、视觉留白足够。">
      <section className="hero-panel grid gap-8 px-8 py-8 lg:grid-cols-[1.18fr_0.82fr] lg:px-10 lg:py-10">
        <div className="max-w-4xl">
          <div className="eyebrow">Overview</div>
          <h2 className="hero-title mt-4">
            用更少的界面噪音，
            <br />
            承载更高密度的交易与控制信息。
          </h2>
          <p className="premium-copy mt-6 max-w-3xl">
            当前首页的目标不是“展示更多模块”，而是先建立工作台秩序：入口要清晰、身份要稳定、关键指标要一眼可读，整体气质更接近高端专业终端，而不是传统后台拼装页。
          </p>
          <div className="mt-9 flex flex-wrap gap-3">
            <Link to="/trading" className="action-dark">进入交易终端<ArrowRight className="h-4 w-4" /></Link>
            <Link to="/system" className="action-light">查看系统状态</Link>
          </div>
          <div className="mt-8 grid gap-3 md:grid-cols-3">
            <div className="hero-stat"><div className="hero-stat-label">首选入口</div><div className="hero-stat-value">交易终端</div></div>
            <div className="hero-stat"><div className="hero-stat-label">视觉原则</div><div className="hero-stat-value">少而稳</div></div>
            <div className="hero-stat"><div className="hero-stat-label">当前身份</div><div className="hero-stat-value">{session?.role ?? '-'}</div></div>
          </div>
        </div>

        <div className="surface-soft p-6 md:p-7">
          <div className="eyebrow">Session</div>
          <div className="mt-5 grid gap-4">
            <div className="stat-tile">
              <div className="eyebrow">当前身份</div>
              <div className="mt-3 text-2xl font-semibold tracking-[-0.05em] text-black">{session?.displayName ?? '-'}</div>
            </div>
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="stat-tile"><div className="eyebrow">用户名</div><div className="data-mono mt-3 text-base font-medium text-black">{session?.username ?? '-'}</div></div>
              <div className="stat-tile"><div className="eyebrow">角色</div><div className="mt-3 text-base font-medium text-black">{session?.role ?? '-'}</div></div>
            </div>
            <div className="stat-tile"><div className="eyebrow">登录时间</div><div className="data-mono mt-3 text-base font-medium text-black">{session?.loggedInAt ? new Date(session.loggedInAt).toLocaleString() : '-'}</div></div>
          </div>
        </div>
      </section>

      <section className="grid gap-4 xl:grid-cols-4">
        <div className="surface-card p-6">
          <div className="eyebrow">Primary</div>
          <div className="mt-3 text-[24px] font-semibold tracking-[-0.05em] text-black">交易终端</div>
          <div className="mt-2 text-sm leading-7 text-neutral-600">首屏优先进入交易链路，减少跳转与寻找成本。</div>
        </div>
        <div className="surface-card p-6">
          <div className="eyebrow">Role</div>
          <div className="mt-3 text-[24px] font-semibold tracking-[-0.05em] text-black">{session?.role ?? '-'}</div>
          <div className="mt-2 text-sm leading-7 text-neutral-600">当前身份决定可见模块与控制范围。</div>
        </div>
        <div className="surface-card p-6">
          <div className="eyebrow">Style</div>
          <div className="mt-3 text-[24px] font-semibold tracking-[-0.05em] text-black">极简白黑</div>
          <div className="mt-2 text-sm leading-7 text-neutral-600">强调秩序、留白、边界和清晰信息密度。</div>
        </div>
        <div className="surface-card p-6">
          <div className="eyebrow">Focus</div>
          <div className="mt-3 text-[24px] font-semibold tracking-[-0.05em] text-black">少而准</div>
          <div className="mt-2 text-sm leading-7 text-neutral-600">优先保留真正影响判断和操作成功率的内容。</div>
        </div>
      </section>

      <section className="surface-card p-8 md:p-9">
        <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
          <div>
            <div className="eyebrow">Launch Sequence</div>
            <h3 className="mt-3 text-[30px] font-semibold tracking-[-0.06em] text-black">推荐工作流顺序</h3>
          </div>
          <div className="premium-micro">让首屏先告诉用户“下一步应该去哪”</div>
        </div>
        <div className="workspace-lane-grid mt-6">
          {launchSequence.map((item) => (
            <div key={item.label} className="workspace-lane-item">
              <div className="workspace-lane-label">{item.label}</div>
              <div className="workspace-lane-title">{item.title}</div>
              <div className="workspace-lane-hint">{item.detail}</div>
            </div>
          ))}
        </div>
      </section>

      <section className="grid gap-6 xl:grid-cols-3">
        {modules.map(({ title, description, to, icon: Icon }) => (
          <Link key={title} to={to} className="surface-card group flex h-full flex-col p-8 transition duration-200 hover:-translate-y-1 hover:shadow-[0_24px_60px_rgba(17,17,17,0.08)]">
            <div className="flex h-12 w-12 items-center justify-center rounded-[18px] border border-black bg-neutral-50"><Icon className="h-5 w-5" /></div>
            <h3 className="mt-7 text-[28px] font-semibold tracking-[-0.06em] text-black">{title}</h3>
            <p className="premium-copy mt-3">{description}</p>
            <div className="mt-auto pt-8 inline-flex items-center gap-2 text-sm font-medium text-black">打开模块<ArrowRight className="h-4 w-4 transition group-hover:translate-x-0.5" /></div>
          </Link>
        ))}
      </section>

      <section className="grid gap-6 xl:grid-cols-[0.92fr_1.08fr]">
        <div className="surface-card p-8 md:p-9">
          <div className="eyebrow">Design Goal</div>
          <h3 className="mt-4 text-[32px] font-semibold tracking-[-0.06em] text-black">当前升级方向，是更高级，而不是更复杂。</h3>
          <p className="section-copy mt-4">我们先提升真正决定观感上限的部分：字体层级、区块留白、控件边界、信息节奏与整体秩序。这样页面即使承载更多真实数据，也不会显得拥挤、杂乱或廉价。</p>
          <div className="mt-8 space-y-3">
            {principles.map((item) => <div key={item} className="surface-soft px-5 py-4 text-[15px] leading-8 text-neutral-700">{item}</div>)}
          </div>
        </div>

        <div className="surface-card p-8 md:p-9">
          <div className="eyebrow">Maturity</div>
          <div className="mt-4 flex items-end justify-between gap-4"><h3 className="text-[32px] font-semibold tracking-[-0.06em] text-black">当前模块成熟度</h3><div className="premium-micro">设计与交互可继续打磨</div></div>
          <p className="section-copy mt-3">这张图不是为了热闹，而是帮助快速判断当前哪些模块已经适合继续打磨体验与交互。</p>
          <div className="chart-shell mt-8 h-72">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={chartData} margin={{ top: 8, right: 10, left: -20, bottom: 0 }}>
                <XAxis dataKey="name" axisLine={false} tickLine={false} fontSize={12} stroke="#6b7280" />
                <YAxis axisLine={false} tickLine={false} fontSize={12} stroke="#6b7280" />
                <Tooltip cursor={{ fill: '#f5f5f4' }} contentStyle={{ borderRadius: 20, border: '1px solid #111', background: '#fff', boxShadow: '0 18px 36px rgba(17,17,17,0.08)' }} />
                <Bar dataKey="score" fill="#111111" radius={[12, 12, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </div>
      </section>
    </AppShell>
  )
}
