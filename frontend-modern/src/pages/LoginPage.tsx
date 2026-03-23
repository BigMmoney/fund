import { useEffect, useState } from 'react'
import { ArrowRight } from 'lucide-react'
import { Link, useNavigate } from 'react-router-dom'
import { useAuth } from '@/contexts/AuthContext'

const demoAccounts = [
  { label: '交易员', username: 'trader', note: '默认下单与查看持仓' },
  { label: '管理员 1', username: 'admin', note: '控制面与审批操作' },
  { label: '管理员 2', username: 'admin2', note: '双人治理审批演示' },
  { label: '管理员 3', username: 'admin3', note: '额外治理席位演示' },
  { label: '观察者', username: 'viewer', note: '只读查看系统与行情' },
]

export function LoginPage() {
  const navigate = useNavigate()
  const { isAuthenticated, login } = useAuth()
  const [username, setUsername] = useState('trader')
  const [password, setPassword] = useState('demo')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (isAuthenticated) navigate('/home', { replace: true })
  }, [isAuthenticated, navigate])

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setLoading(true)
    setError(null)
    try {
      await login({ username, password })
      navigate('/home', { replace: true })
    } catch (submissionError) {
      setError(submissionError instanceof Error ? submissionError.message : '登录失败，请稍后重试')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen bg-neutral-50 px-6 py-10 text-black">
      <div className="mx-auto grid max-w-[1400px] gap-8 lg:grid-cols-[1.08fr_0.92fr]">
        <section className="hero-panel p-10 md:p-12">
          <div className="eyebrow">Login</div>
          <h1 className="hero-title mt-4 max-w-3xl">
            进入本地演示环境，
            <br />
            保持最小动作与最高可读性。
          </h1>
          <p className="premium-copy mt-6 max-w-2xl">
            这版登录页只保留真正必要的操作：选择身份、输入凭证、进入系统。所有演示账户统一使用密码
            <span className="mx-1 font-medium text-black">demo</span>
            ，界面保持白黑极简，避免多余说明干扰首屏判断。
          </p>

          <div className="mt-10 grid gap-4 md:grid-cols-2">
            {demoAccounts.map((account) => (
              <button
                key={account.username}
                type="button"
                onClick={() => setUsername(account.username)}
                className={`rounded-[30px] border px-6 py-6 text-left transition ${username === account.username ? 'border-black bg-black text-white shadow-[0_18px_44px_rgba(17,17,17,0.12)]' : 'border-black bg-white text-black hover:bg-neutral-50'}`}
              >
                <div className="text-xl font-semibold tracking-[-0.04em]">{account.label}</div>
                <div className={`mt-2 text-sm ${username === account.username ? 'text-neutral-300' : 'text-neutral-500'}`}>{account.username}</div>
                <div className={`mt-4 text-sm leading-7 ${username === account.username ? 'text-neutral-200' : 'text-neutral-600'}`}>{account.note}</div>
              </button>
            ))}
          </div>

          <div className="surface-soft mt-10 p-6 text-sm leading-8 text-neutral-600">
            登录后可进入 <Link className="subtle-link" to="/home">首页</Link>、<Link className="subtle-link" to="/trading">交易页</Link>、<Link className="subtle-link" to="/system">系统页</Link>；管理员额外可见控制面。
          </div>
        </section>

        <section className="surface-card p-10 md:p-12">
          <div className="eyebrow">Access</div>
          <h2 className="mt-4 text-[34px] font-semibold tracking-[-0.06em] text-black">输入凭证</h2>
          <p className="premium-copy mt-3">用户名可直接通过左侧身份卡选取，密码固定为 demo。该页面仅服务于本地工作流，不接外部身份系统。</p>

          <div className="mt-8 grid gap-3 sm:grid-cols-3">
            <div className="hero-stat"><div className="hero-stat-label">账户数量</div><div className="hero-stat-value">{demoAccounts.length}</div></div>
            <div className="hero-stat"><div className="hero-stat-label">密码策略</div><div className="hero-stat-value">demo</div></div>
            <div className="hero-stat"><div className="hero-stat-label">访问模式</div><div className="hero-stat-value">本地演示</div></div>
          </div>

          <form className="mt-10 space-y-6" onSubmit={handleSubmit}>
            <label className="block">
              <span className="mb-2 block text-xs uppercase tracking-[0.18em] text-neutral-500">用户名</span>
              <input value={username} onChange={(event) => setUsername(event.target.value)} className="field-shell" placeholder="请输入用户名" />
            </label>
            <label className="block">
              <span className="mb-2 block text-xs uppercase tracking-[0.18em] text-neutral-500">密码</span>
              <input type="password" value={password} onChange={(event) => setPassword(event.target.value)} className="field-shell" placeholder="请输入密码" />
            </label>
            {error ? <div className="surface-soft px-4 py-3 text-sm text-black">{error}</div> : null}
            <button type="submit" disabled={loading} className="action-dark w-full disabled:cursor-not-allowed disabled:opacity-60">
              {loading ? '登录中…' : '进入系统'}
              <ArrowRight className="h-4 w-4" />
            </button>
          </form>
        </section>
      </div>
    </div>
  )
}
