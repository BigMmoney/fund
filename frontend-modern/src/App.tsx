import { HashRouter, Navigate, Route, Routes } from 'react-router-dom'
import { AuthProvider, useAuth } from '@/contexts/AuthContext'
import { ProtectedRoute } from '@/components/ProtectedRoute'
import { LoginPage } from '@/pages/LoginPage'
import { HomePage } from '@/pages/HomePage'
import { TradingTerminal } from '@/pages/TradingTerminal'
import { NewsIntelligence } from '@/pages/NewsIntelligence'
import { SystemStatus } from '@/pages/SystemStatus'
import { AdminControlPage } from '@/pages/AdminControlPage'

function RootRoute() {
  const { isAuthenticated } = useAuth()
  return <Navigate replace to={isAuthenticated ? '/home' : '/login'} />
}

function App() {
  return (
    <AuthProvider>
      <HashRouter>
        <div className="min-h-screen bg-neutral-50 text-black">
          <Routes>
            <Route element={<RootRoute />} path="/" />
            <Route element={<LoginPage />} path="/login" />
            <Route
              element={
                <ProtectedRoute>
                  <HomePage />
                </ProtectedRoute>
              }
              path="/home"
            />
            <Route
              element={
                <ProtectedRoute>
                  <TradingTerminal />
                </ProtectedRoute>
              }
              path="/trading"
            />
            <Route
              element={
                <ProtectedRoute>
                  <NewsIntelligence />
                </ProtectedRoute>
              }
              path="/intel"
            />
            <Route
              element={
                <ProtectedRoute>
                  <SystemStatus />
                </ProtectedRoute>
              }
              path="/system"
            />
            <Route
              element={
                <ProtectedRoute>
                  <AdminControlPage />
                </ProtectedRoute>
              }
              path="/admin"
            />
            <Route element={<Navigate replace to="/" />} path="*" />
          </Routes>
        </div>
      </HashRouter>
    </AuthProvider>
  )
}

export default App
