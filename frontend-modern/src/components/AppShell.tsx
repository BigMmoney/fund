import type { Dispatch, ReactNode, SetStateAction } from 'react'
import { NavLink } from 'react-router-dom'
import type { AuthConfig, AuthRole } from '@/services/exchangeApi'

interface AppShellProps {
  auth: AuthConfig
  onChangeAuth: Dispatch<SetStateAction<AuthConfig>>
  notice: string
  onNotice: (message: string) => void
  children: ReactNode
}

function preset(role: AuthRole): AuthConfig {
  if (role === 'admin') {
    return {
      baseUrl: '',
      secret: 'dev-secret-change-me-to-32-chars-min!',
      subject: 'admin-001',
      role: 'admin',
      sessionId: '',
    }
  }

  return {
    baseUrl: '',
    secret: 'dev-secret-change-me-to-32-chars-min!',
    subject: 'trader-001',
    role: 'user',
    sessionId: '',
  }
}

export function AppShell({ auth, onChangeAuth, notice, onNotice, children }: AppShellProps) {
  const updateField = <K extends keyof AuthConfig>(key: K, value: AuthConfig[K]) => {
    onChangeAuth((current) => ({ ...current, [key]: value }))
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-block">
          <div className="brand-kicker">Exchange Workspace</div>
          <h1>Pre Trading Frontend Console</h1>
          <p>Business terminal, operator control surface, and system status aligned to the current backend.</p>
        </div>

        <nav className="nav-tabs">
          <NavLink to="/business" className={({ isActive }) => `nav-tab ${isActive ? 'nav-tab-active' : ''}`}>
            Business
          </NavLink>
          <NavLink to="/control" className={({ isActive }) => `nav-tab ${isActive ? 'nav-tab-active' : ''}`}>
            Control
          </NavLink>
          <NavLink to="/monitor" className={({ isActive }) => `nav-tab ${isActive ? 'nav-tab-active' : ''}`}>
            Monitor
          </NavLink>
          <NavLink to="/system" className={({ isActive }) => `nav-tab ${isActive ? 'nav-tab-active' : ''}`}>
            System
          </NavLink>
        </nav>
      </header>

      <section className="workspace-banner">
        <div>
          <strong>Status:</strong> {notice}
        </div>
        <button type="button" className="button button-secondary" onClick={() => onNotice('Workspace ready for live backend interaction.')}>
          Reset Notice
        </button>
      </section>

      <section className="settings-panel">
        <div className="section-heading">
          <div>
            <div className="section-kicker">Runtime Auth</div>
            <h2>Frontend Request Identity</h2>
          </div>
          <div className="button-row">
            <button type="button" className="button button-secondary" onClick={() => onChangeAuth(preset('user'))}>
              Trader Preset
            </button>
            <button type="button" className="button button-secondary" onClick={() => onChangeAuth(preset('admin'))}>
              Admin Preset
            </button>
          </div>
        </div>

        <div className="form-grid form-grid-auth">
          <label className="field">
            <span>Base URL</span>
            <input
              value={auth.baseUrl}
              onChange={(event) => updateField('baseUrl', event.target.value)}
              placeholder="Leave blank to use Vite proxy"
            />
          </label>
          <label className="field">
            <span>Subject</span>
            <input value={auth.subject} onChange={(event) => updateField('subject', event.target.value)} />
          </label>
          <label className="field">
            <span>Role</span>
            <select value={auth.role} onChange={(event) => updateField('role', event.target.value as AuthRole)}>
              <option value="user">user</option>
              <option value="admin">admin</option>
            </select>
          </label>
          <label className="field">
            <span>Session ID</span>
            <input value={auth.sessionId} onChange={(event) => updateField('sessionId', event.target.value)} placeholder="optional" />
          </label>
          <label className="field field-span-2">
            <span>Internal Auth Secret</span>
            <input value={auth.secret} onChange={(event) => updateField('secret', event.target.value)} />
          </label>
        </div>
      </section>

      <main className="page-stack">{children}</main>
    </div>
  )
}
