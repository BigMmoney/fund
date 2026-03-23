import { useEffect, useState, type ReactNode } from 'react'
import { Activity, Bell, Home, LineChart, LogOut, Settings, ShieldCheck } from 'lucide-react'
import { Link, useLocation } from 'react-router-dom'
import { useAuth } from '@/contexts/AuthContext'
import { readAnnouncement, subscribeAnnouncement } from '@/services/announcementStore'

interface NavItem {
  to: string
  label: string
  icon: typeof Home
  roles?: Array<'trader' | 'admin' | 'viewer'>
}

const navItems: NavItem[] = [
  { to: '/home', label: '首页', icon: Home },
  { to: '/trading', label: '交易', icon: LineChart },
  { to: '/intel', label: '情报', icon: Bell },
  { to: '/system', label: '系统', icon: ShieldCheck },
  { to: '/admin', label: '管理', icon: Settings, roles: ['admin'] },
]

const roleLabel: Record<'trader' | 'admin' | 'viewer', string> = {
  trader: '交易账户',
  admin: '管理员',
  viewer: '观察账户',
}

export function AppShell({
  title,
  subtitle,
  children,
  mode = 'default',
}: {
  title: string
  subtitle?: string
  children: ReactNode
  mode?: 'default' | 'terminal'
}) {
  const location = useLocation()
  const { session, logout } = useAuth()
  const visibleItems = navItems.filter((item) => !item.roles || (session?.role ? item.roles.includes(session.role) : false))
  const [announcement, setAnnouncement] = useState(() => readAnnouncement())

  useEffect(() => {
    setAnnouncement(readAnnouncement())
    return subscribeAnnouncement(setAnnouncement)
  }, [])

  return (
    <div className="min-h-screen bg-neutral-50 text-black">
      <header className="sticky top-0 z-40 border-b border-black/80 bg-[rgba(250,250,248,0.94)] backdrop-blur-xl">
        <div className="shell-container">
          <div className="flex min-h-[88px] items-center justify-between gap-6">
            <div className="flex min-w-0 items-center gap-4">
              <div className="flex h-12 w-12 items-center justify-center rounded-[18px] border border-black bg-white shadow-[0_10px_24px_rgba(17,17,17,0.06)]">
                <Activity className="h-5 w-5" />
              </div>
              <div className="min-w-0">
                <div className="truncate text-[15px] font-semibold tracking-[-0.03em] text-black">Pre Trading Exchange</div>
                <div className="mt-1 truncate text-xs text-neutral-500">统一演示控制台 · 极简白黑交易工作台</div>
              </div>
            </div>

            <nav className="hidden items-center gap-2 lg:flex">
              {visibleItems.map(({ to, label, icon: Icon }) => {
                const active = location.pathname === to
                return (
                  <Link key={to} to={to} className={`nav-pill ${active ? 'nav-pill-active' : ''}`}>
                    <Icon className="h-4 w-4" />
                    {label}
                  </Link>
                )
              })}
            </nav>

            <div className="flex items-center gap-3">
              <div className="hidden items-center gap-3 rounded-full border border-black/85 bg-white px-4 py-2.5 text-sm text-neutral-700 xl:flex">
                <span className="h-2.5 w-2.5 rounded-full bg-black" />
                <span className="font-medium text-black">{session?.displayName ?? '未登录'}</span>
                <span className="text-neutral-300">/</span>
                <span>{session?.role ? roleLabel[session.role] : '-'}</span>
              </div>
              <button type="button" onClick={logout} className="action-light px-4 py-2.5">
                <LogOut className="h-4 w-4" />
                退出
              </button>
            </div>
          </div>

          <div className="flex gap-2 overflow-x-auto pb-4 lg:hidden">
            {visibleItems.map(({ to, label, icon: Icon }) => {
              const active = location.pathname === to
              return (
                <Link key={to} to={to} className={`nav-pill shrink-0 ${active ? 'nav-pill-active' : ''}`}>
                  <Icon className="h-4 w-4" />
                  {label}
                </Link>
              )
            })}
          </div>

          {announcement.enabled && announcement.text.trim().length > 0 ? (
            <div className="announcement-bar">
              <div className="announcement-bar-label">交易所公告</div>
              <div className="announcement-marquee">
                <div className="announcement-marquee-track">
                  <span>{announcement.text}</span>
                  <span aria-hidden="true">{announcement.text}</span>
                </div>
              </div>
              <div className="announcement-bar-meta">
                <span>{announcement.updatedBy ?? 'system'}</span>
                <span>·</span>
                <span>{new Date(announcement.updatedAt).toLocaleString('zh-CN', { hour12: false })}</span>
              </div>
            </div>
          ) : null}
        </div>
      </header>

      <main className={`shell-container flex flex-col ${mode === 'terminal' ? 'gap-4 py-4 pb-10 md:gap-5 md:py-5 md:pb-14' : 'gap-8 py-8 pb-12 md:gap-10 md:py-10 md:pb-16'}`}>
        {mode === 'terminal' ? (
          <section className="surface-card px-5 py-4 md:px-6">
            <div className="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
              <div className="min-w-0">
                <div className="eyebrow">Terminal</div>
                <h1 className="mt-1 text-[22px] font-semibold tracking-[-0.05em] text-black md:text-[26px]">{title}</h1>
                {subtitle ? <p className="mt-1 max-w-3xl text-xs leading-6 text-neutral-500 md:text-sm">{subtitle}</p> : null}
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <span className="mono-chip">{session?.displayName ?? '未登录'}</span>
                <span className="mono-chip">{session?.role ? roleLabel[session.role] : '未分配'}</span>
                <span className="mono-chip">终端模式</span>
              </div>
            </div>
          </section>
        ) : (
          <section className="hero-panel grid gap-5 px-8 py-8 md:px-10 md:py-10">
            <div className="flex flex-wrap items-center justify-between gap-4">
              <div className="eyebrow">Workspace</div>
              <div className="hidden lg:flex items-center gap-2">
                <span className="mono-chip">{session?.displayName ?? '未登录'}</span>
                <span className="mono-chip">{session?.role ? roleLabel[session.role] : '未分配'}</span>
              </div>
            </div>
            <div className="max-w-5xl">
              <h1 className="display-title">{title}</h1>
              {subtitle ? <p className="premium-copy mt-4 max-w-4xl">{subtitle}</p> : null}
            </div>
            <div className="workspace-rhythm-grid">
              <div className="workspace-rhythm-item">
                <div className="workspace-rhythm-label">当前页面</div>
                <div className="workspace-rhythm-value">{title}</div>
              </div>
              <div className="workspace-rhythm-item">
                <div className="workspace-rhythm-label">当前身份</div>
                <div className="workspace-rhythm-value">{session?.displayName ?? '未登录'}</div>
              </div>
              <div className="workspace-rhythm-item">
                <div className="workspace-rhythm-label">工作模式</div>
                <div className="workspace-rhythm-value">极简控制台</div>
              </div>
            </div>
          </section>
        )}
        {children}
      </main>
    </div>
  )
}
