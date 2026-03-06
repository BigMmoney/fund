import { HashRouter as Router, Routes, Route, Navigate } from 'react-router-dom'
import { TradingTerminal } from '@/pages/TradingTerminal'
import { PredictionMarket } from '@/pages/PredictionMarket'
import { AdminDashboard } from '@/pages/AdminDashboard'
import NewsIntelligence from '@/pages/NewsIntelligence'
import { AccountProvider } from '@/contexts/AccountContext'
import { MarketStatusProvider } from '@/contexts/MarketStatusContext'
import { ThemeProvider } from '@/contexts/ThemeContext'
import { LanguageProvider } from '@/contexts/LanguageContext'
import { NotificationProvider } from '@/contexts/NotificationContext'
import { TimeWindowProvider } from '@/contexts/TimeWindowContext'
import { ErrorBoundary, PageErrorBoundary } from '@/components/ErrorBoundary'

// 使用 HashRouter 解决 GitHub Pages SPA 刷新404问题
// HashRouter uses # in URL, which works with static hosting
function App() {
  return (
    <ErrorBoundary>
      <LanguageProvider>
        <ThemeProvider>
          <NotificationProvider>
            <TimeWindowProvider>
              <AccountProvider>
                <MarketStatusProvider>
                  <Router>
                    <div className="min-h-screen bg-[#0a0a0a]">
                      <Routes>
                        <Route path="/" element={
                          <PageErrorBoundary>
                            <TradingTerminal />
                          </PageErrorBoundary>
                        } />
                        <Route path="/prediction" element={
                          <PageErrorBoundary>
                            <PredictionMarket />
                          </PageErrorBoundary>
                        } />
                        <Route path="/news-intel" element={
                          <PageErrorBoundary>
                            <NewsIntelligence />
                          </PageErrorBoundary>
                        } />
                        <Route path="/admin" element={
                          <PageErrorBoundary>
                            <AdminDashboard />
                          </PageErrorBoundary>
                        } />
                        <Route path="*" element={<Navigate to="/" replace />} />
                      </Routes>
                    </div>
                  </Router>
                </MarketStatusProvider>
              </AccountProvider>
            </TimeWindowProvider>
          </NotificationProvider>
        </ThemeProvider>
      </LanguageProvider>
    </ErrorBoundary>
  )
}

export default App
