import { useEffect, useMemo, useState } from 'react'
import { HashRouter, Navigate, Route, Routes } from 'react-router-dom'
import { AppShell } from '@/components/AppShell'
import { BusinessPage } from '@/pages/BusinessPage'
import { ControlPage } from '@/pages/ControlPage'
import { SystemPage } from '@/pages/SystemPage'
import type { AuthConfig } from '@/services/exchangeApi'

const STORAGE_KEY = 'pretrading.frontend.workspace.auth'

const defaultAuth: AuthConfig = {
  baseUrl: '',
  secret: 'dev-secret-change-me-to-32-chars-min!',
  subject: 'trader-001',
  role: 'user',
  sessionId: '',
}

function loadInitialAuth(): AuthConfig {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY)
    if (!raw) return defaultAuth
    const parsed = JSON.parse(raw) as Partial<AuthConfig>
    return {
      baseUrl: typeof parsed.baseUrl === 'string' ? parsed.baseUrl : defaultAuth.baseUrl,
      secret: typeof parsed.secret === 'string' ? parsed.secret : defaultAuth.secret,
      subject: typeof parsed.subject === 'string' ? parsed.subject : defaultAuth.subject,
      role: parsed.role === 'admin' ? 'admin' : 'user',
      sessionId: typeof parsed.sessionId === 'string' ? parsed.sessionId : defaultAuth.sessionId,
    }
  } catch {
    return defaultAuth
  }
}

function Workspace() {
  const [auth, setAuth] = useState<AuthConfig>(loadInitialAuth)
  const [notice, setNotice] = useState('A clean frontend has been rebuilt for the current exchange backend.')

  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(auth))
  }, [auth])

  const shellProps = useMemo(
    () => ({
      auth,
      onChangeAuth: setAuth,
      notice,
      onNotice: setNotice,
    }),
    [auth, notice],
  )

  return (
    <AppShell {...shellProps}>
      <Routes>
        <Route path="/" element={<Navigate replace to="/business" />} />
        <Route path="/business" element={<BusinessPage auth={auth} onNotice={setNotice} />} />
        <Route path="/control" element={<ControlPage auth={auth} onNotice={setNotice} />} />
        <Route path="/system" element={<SystemPage auth={auth} onNotice={setNotice} />} />
        <Route path="*" element={<Navigate replace to="/business" />} />
      </Routes>
    </AppShell>
  )
}

export default function App() {
  return (
    <HashRouter>
      <Workspace />
    </HashRouter>
  )
}
