import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'

type UserRole = 'trader' | 'admin' | 'viewer'

export interface AuthSession {
  username: string
  displayName: string
  role: UserRole
  token: string
  loggedInAt: string
}

interface LoginInput {
  username: string
  password: string
}

interface AuthContextValue {
  session: AuthSession | null
  isAuthenticated: boolean
  login: (input: LoginInput) => Promise<AuthSession>
  logout: () => void
}

const AUTH_STORAGE_KEY = 'pretrading.auth.session'

const AuthContext = createContext<AuthContextValue | null>(null)

function buildSession(username: string): AuthSession {
  const normalized = username.trim().toLowerCase()
  const role: UserRole = normalized.startsWith('admin') ? 'admin' : normalized === 'viewer' ? 'viewer' : 'trader'

  return {
    username: normalized,
    displayName:
      normalized === 'admin'
        ? '系统管理员'
        : normalized === 'admin2'
          ? '系统管理员 2'
          : normalized === 'admin3'
            ? '系统管理员 3'
            : normalized === 'viewer'
              ? '市场观察者'
              : '交易员',
    role,
    token: `local-${normalized}-${Date.now()}`,
    loggedInAt: new Date().toISOString(),
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [session, setSession] = useState<AuthSession | null>(null)

  useEffect(() => {
    const raw = localStorage.getItem(AUTH_STORAGE_KEY)
    if (!raw) {
      return
    }

    try {
      setSession(JSON.parse(raw) as AuthSession)
    } catch {
      localStorage.removeItem(AUTH_STORAGE_KEY)
    }
  }, [])

  const value = useMemo<AuthContextValue>(
    () => ({
      session,
      isAuthenticated: !!session,
      login: async ({ username, password }) => {
        const cleanUser = username.trim()
        const cleanPassword = password.trim()

        if (!cleanUser) {
          throw new Error('请输入用户名')
        }

        if (!cleanPassword) {
          throw new Error('请输入密码')
        }

        if (cleanPassword !== 'demo') {
          throw new Error('演示环境密码固定为 demo')
        }

        const nextSession = buildSession(cleanUser)
        localStorage.setItem(AUTH_STORAGE_KEY, JSON.stringify(nextSession))
        setSession(nextSession)
        return nextSession
      },
      logout: () => {
        localStorage.removeItem(AUTH_STORAGE_KEY)
        setSession(null)
      },
    }),
    [session],
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth() {
  const context = useContext(AuthContext)
  if (!context) {
    throw new Error('useAuth 必须在 AuthProvider 内使用')
  }
  return context
}
