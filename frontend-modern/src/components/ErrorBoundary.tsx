import { Component, ReactNode } from 'react'
import { AlertTriangle, RefreshCw, Home, ChevronDown } from 'lucide-react'

interface ErrorBoundaryProps {
  children: ReactNode
  fallback?: ReactNode
  onError?: (error: Error, errorInfo: React.ErrorInfo) => void
}

interface ErrorBoundaryState {
  hasError: boolean
  error: Error | null
  errorInfo: React.ErrorInfo | null
  showDetails: boolean
}

/**
 * Error Boundary 组件
 * 捕获子组件的 JavaScript 错误，防止整个应用崩溃
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props)
    this.state = {
      hasError: false,
      error: null,
      errorInfo: null,
      showDetails: false
    }
  }

  static getDerivedStateFromError(error: Error): Partial<ErrorBoundaryState> {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    this.setState({ errorInfo })
    
    // 调用自定义错误处理函数
    if (this.props.onError) {
      this.props.onError(error, errorInfo)
    }
    
    // 记录错误日志
    console.error('[ErrorBoundary] Caught error:', error)
    console.error('[ErrorBoundary] Component stack:', errorInfo.componentStack)
  }

  handleRetry = () => {
    this.setState({ hasError: false, error: null, errorInfo: null })
  }

  handleGoHome = () => {
    window.location.hash = '#/'
    this.setState({ hasError: false, error: null, errorInfo: null })
  }

  handleRefreshPage = () => {
    window.location.reload()
  }

  toggleDetails = () => {
    this.setState(prev => ({ showDetails: !prev.showDetails }))
  }

  render() {
    if (this.state.hasError) {
      // 如果提供了自定义 fallback，使用它
      if (this.props.fallback) {
        return this.props.fallback
      }

      // 默认错误 UI
      return (
        <div className="min-h-screen bg-gray-50 dark:bg-gray-900 flex items-center justify-center p-4">
          <div className="max-w-lg w-full bg-white dark:bg-gray-800 rounded-2xl shadow-xl overflow-hidden">
            {/* Header */}
            <div className="bg-gradient-to-r from-red-500 to-orange-500 p-6">
              <div className="flex items-center gap-4">
                <div className="w-16 h-16 bg-white/20 rounded-xl flex items-center justify-center">
                  <AlertTriangle className="w-10 h-10 text-white" />
                </div>
                <div className="text-white">
                  <h1 className="text-xl font-bold">出现了一些问题</h1>
                  <p className="text-white/80 text-sm">Something went wrong</p>
                </div>
              </div>
            </div>

            {/* Content */}
            <div className="p-6 space-y-4">
              <p className="text-gray-600 dark:text-gray-300">
                应用程序遇到了一个错误。这可能是临时性的问题，请尝试以下操作：
              </p>

              {/* Action Buttons */}
              <div className="flex flex-col gap-2">
                <button
                  onClick={this.handleRetry}
                  className="flex items-center justify-center gap-2 w-full py-3 px-4 bg-blue-600 hover:bg-blue-700 text-white rounded-xl font-medium transition-colors"
                >
                  <RefreshCw className="w-5 h-5" />
                  重试 / Retry
                </button>
                <button
                  onClick={this.handleRefreshPage}
                  className="flex items-center justify-center gap-2 w-full py-3 px-4 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-200 rounded-xl font-medium transition-colors"
                >
                  <RefreshCw className="w-5 h-5" />
                  刷新页面 / Refresh Page
                </button>
                <button
                  onClick={this.handleGoHome}
                  className="flex items-center justify-center gap-2 w-full py-3 px-4 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-200 rounded-xl font-medium transition-colors"
                >
                  <Home className="w-5 h-5" />
                  返回首页 / Go Home
                </button>
              </div>

              {/* Error Details (Collapsible) */}
              <div className="border-t border-gray-200 dark:border-gray-700 pt-4">
                <button
                  onClick={this.toggleDetails}
                  className="flex items-center justify-between w-full text-left text-sm text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
                >
                  <span>技术详情 / Technical Details</span>
                  <ChevronDown className={`w-4 h-4 transition-transform ${this.state.showDetails ? 'rotate-180' : ''}`} />
                </button>
                
                {this.state.showDetails && (
                  <div className="mt-3 p-3 bg-gray-100 dark:bg-gray-900 rounded-lg overflow-auto max-h-48">
                    <div className="text-xs font-mono space-y-2">
                      <div>
                        <span className="text-red-500 font-semibold">Error: </span>
                        <span className="text-gray-700 dark:text-gray-300">{this.state.error?.message}</span>
                      </div>
                      {this.state.error?.stack && (
                        <div className="text-gray-500 dark:text-gray-400 whitespace-pre-wrap text-[10px]">
                          {this.state.error.stack.split('\n').slice(0, 5).join('\n')}
                        </div>
                      )}
                      {this.state.errorInfo?.componentStack && (
                        <div className="mt-2 pt-2 border-t border-gray-200 dark:border-gray-700">
                          <span className="text-orange-500 font-semibold">Component Stack: </span>
                          <div className="text-gray-500 dark:text-gray-400 whitespace-pre-wrap text-[10px]">
                            {this.state.errorInfo.componentStack.split('\n').slice(0, 5).join('\n')}
                          </div>
                        </div>
                      )}
                    </div>
                  </div>
                )}
              </div>

              {/* Timestamp */}
              <div className="text-center text-xs text-gray-400">
                Error occurred at: {new Date().toLocaleString()}
              </div>
            </div>
          </div>
        </div>
      )
    }

    return this.props.children
  }
}

/**
 * 用于页面级别的错误边界
 */
export class PageErrorBoundary extends ErrorBoundary {
  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    super.componentDidCatch(error, errorInfo)
    // 可以在这里添加页面级别的错误报告
    console.error('[PageErrorBoundary] Page error:', error.message)
  }
}

/**
 * 用于组件级别的错误边界（更简洁的 fallback）
 */
interface ComponentErrorBoundaryProps {
  children: ReactNode
  componentName?: string
}

interface ComponentErrorBoundaryState {
  hasError: boolean
  error: Error | null
}

export class ComponentErrorBoundary extends Component<ComponentErrorBoundaryProps, ComponentErrorBoundaryState> {
  constructor(props: ComponentErrorBoundaryProps) {
    super(props)
    this.state = { hasError: false, error: null }
  }

  static getDerivedStateFromError(error: Error): Partial<ComponentErrorBoundaryState> {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, _errorInfo: React.ErrorInfo) {
    console.error(`[ComponentErrorBoundary] Error in ${this.props.componentName || 'component'}:`, error)
  }

  handleRetry = () => {
    this.setState({ hasError: false, error: null })
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg">
          <div className="flex items-center gap-2 text-red-600 dark:text-red-400 mb-2">
            <AlertTriangle className="w-4 h-4" />
            <span className="text-sm font-medium">
              {this.props.componentName ? `${this.props.componentName} 加载失败` : '组件加载失败'}
            </span>
          </div>
          <p className="text-xs text-red-500 dark:text-red-400 mb-2">
            {this.state.error?.message}
          </p>
          <button
            onClick={this.handleRetry}
            className="text-xs px-3 py-1.5 bg-red-100 dark:bg-red-800 text-red-700 dark:text-red-200 rounded hover:bg-red-200 dark:hover:bg-red-700 transition-colors"
          >
            重试
          </button>
        </div>
      )
    }

    return this.props.children
  }
}

export default ErrorBoundary
