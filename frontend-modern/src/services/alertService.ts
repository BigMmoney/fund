/**
 * Alert Service - 政策情报警报系统
 * 
 * 功能：
 * 1. 实时监控新闻流，触发条件警报
 * 2. 多级别警报：Critical, High, Medium, Low
 * 3. 多通道通知：Browser Push, Email, Telegram, Webhook
 * 4. 警报规则引擎：基于关键词、实体、来源层级、评分阈值
 * 5. 智能降噪：基于 Noise Gate 规则过滤低质量警报
 */

// ============== Type Definitions ==============

export type AlertPriority = 'critical' | 'high' | 'medium' | 'low'
export type AlertChannel = 'browser' | 'email' | 'telegram' | 'webhook' | 'sms'
export type AlertStatus = 'active' | 'acknowledged' | 'dismissed' | 'expired'
export type RuleConditionType = 
  | 'keyword_match' 
  | 'entity_mention' 
  | 'source_level' 
  | 'score_threshold'
  | 'domain_match'
  | 'state_change'
  | 'sentiment_shift'
  | 'volume_spike'

export interface AlertRule {
  id: string
  name: string
  description: string
  enabled: boolean
  priority: AlertPriority
  conditions: RuleCondition[]
  conditionLogic: 'AND' | 'OR'  // 条件逻辑
  channels: AlertChannel[]
  cooldownMinutes: number  // 冷却时间，防止重复警报
  createdAt: string
  updatedAt: string
}

export interface RuleCondition {
  type: RuleConditionType
  operator: 'equals' | 'contains' | 'greater_than' | 'less_than' | 'in' | 'not_in'
  value: string | number | string[]
  field?: string  // 用于指定检查哪个字段
}

export interface Alert {
  id: string
  ruleId: string
  ruleName: string
  priority: AlertPriority
  status: AlertStatus
  title: string
  message: string
  sourceId?: string
  sourceLevel?: string
  domain?: string
  score?: number
  matchedConditions: string[]
  triggeredAt: string
  acknowledgedAt?: string
  acknowledgedBy?: string
  expiresAt: string
  metadata: Record<string, unknown>
}

export interface AlertStats {
  total: number
  byPriority: Record<AlertPriority, number>
  byStatus: Record<AlertStatus, number>
  last24Hours: number
  last7Days: number
}

export interface NotificationPayload {
  title: string
  body: string
  icon?: string
  url?: string
  data?: Record<string, unknown>
}

// ============== Default Alert Rules ==============

export const DEFAULT_ALERT_RULES: AlertRule[] = [
  {
    id: 'rule-trump-direct',
    name: '特朗普直接声明',
    description: '监控特朗普关于贸易、关税、制裁的直接言论',
    enabled: true,
    priority: 'critical',
    conditions: [
      { type: 'keyword_match', operator: 'contains', value: ['trump', 'white house', 'potus'] },
      { type: 'source_level', operator: 'in', value: ['L0', 'L0.5'] },
      { type: 'keyword_match', operator: 'contains', value: ['tariff', 'sanction', 'trade war', 'china', '关税', '制裁'] }
    ],
    conditionLogic: 'AND',
    channels: ['browser', 'telegram'],
    cooldownMinutes: 5,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  },
  {
    id: 'rule-ofac-sdn',
    name: 'OFAC SDN 名单变更',
    description: '监控 OFAC 制裁名单的添加和移除',
    enabled: true,
    priority: 'critical',
    conditions: [
      { type: 'entity_mention', operator: 'contains', value: ['OFAC', 'SDN', 'Treasury'] },
      { type: 'keyword_match', operator: 'contains', value: ['designated', 'added', 'removed', 'sanction', '指定', '添加', '移除'] }
    ],
    conditionLogic: 'AND',
    channels: ['browser', 'email', 'telegram'],
    cooldownMinutes: 1,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  },
  {
    id: 'rule-entity-list',
    name: 'BIS Entity List 变更',
    description: '监控 BIS 实体清单的变更',
    enabled: true,
    priority: 'critical',
    conditions: [
      { type: 'entity_mention', operator: 'contains', value: ['BIS', 'Entity List', 'Bureau of Industry'] },
      { type: 'keyword_match', operator: 'contains', value: ['added', 'removed', 'export control', '出口管制'] }
    ],
    conditionLogic: 'AND',
    channels: ['browser', 'email'],
    cooldownMinutes: 5,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  },
  {
    id: 'rule-fed-rate',
    name: '美联储利率决策',
    description: '监控联储利率相关声明',
    enabled: true,
    priority: 'high',
    conditions: [
      { type: 'entity_mention', operator: 'contains', value: ['Federal Reserve', 'FOMC', 'Powell', 'Fed'] },
      { type: 'keyword_match', operator: 'contains', value: ['rate', 'hike', 'cut', 'basis points', 'bps', '利率', '加息', '降息'] }
    ],
    conditionLogic: 'AND',
    channels: ['browser'],
    cooldownMinutes: 15,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  },
  {
    id: 'rule-high-score',
    name: '高分决策信号',
    description: '当决策评分超过 80 分时触发',
    enabled: true,
    priority: 'high',
    conditions: [
      { type: 'score_threshold', operator: 'greater_than', value: 80 }
    ],
    conditionLogic: 'AND',
    channels: ['browser', 'telegram'],
    cooldownMinutes: 30,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  },
  {
    id: 'rule-state-change',
    name: '政策状态转换',
    description: '监控政策从协商进入执行阶段',
    enabled: true,
    priority: 'medium',
    conditions: [
      { type: 'state_change', operator: 'equals', value: 'implementing' }
    ],
    conditionLogic: 'AND',
    channels: ['browser'],
    cooldownMinutes: 60,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  },
  {
    id: 'rule-sentiment-shift',
    name: '情绪剧烈波动',
    description: '24小时内情绪变化超过0.5',
    enabled: true,
    priority: 'medium',
    conditions: [
      { type: 'sentiment_shift', operator: 'greater_than', value: 0.5 }
    ],
    conditionLogic: 'AND',
    channels: ['browser'],
    cooldownMinutes: 120,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  },
  {
    id: 'rule-volume-spike',
    name: '新闻量激增',
    description: '某主题新闻量超过均值3倍',
    enabled: true,
    priority: 'low',
    conditions: [
      { type: 'volume_spike', operator: 'greater_than', value: 3 }
    ],
    conditionLogic: 'AND',
    channels: ['browser'],
    cooldownMinutes: 240,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  }
]

// ============== Alert Storage ==============

class AlertStorage {
  private readonly STORAGE_KEY = 'intel_alerts'
  private readonly RULES_KEY = 'intel_alert_rules'
  private readonly LAST_TRIGGER_KEY = 'intel_alert_last_trigger'

  getAlerts(): Alert[] {
    try {
      const stored = localStorage.getItem(this.STORAGE_KEY)
      return stored ? JSON.parse(stored) : []
    } catch {
      return []
    }
  }

  saveAlert(alert: Alert): void {
    const alerts = this.getAlerts()
    alerts.unshift(alert)
    // 保留最近 500 条
    const trimmed = alerts.slice(0, 500)
    localStorage.setItem(this.STORAGE_KEY, JSON.stringify(trimmed))
  }

  updateAlert(alertId: string, updates: Partial<Alert>): void {
    const alerts = this.getAlerts()
    const index = alerts.findIndex(a => a.id === alertId)
    if (index >= 0) {
      alerts[index] = { ...alerts[index], ...updates }
      localStorage.setItem(this.STORAGE_KEY, JSON.stringify(alerts))
    }
  }

  getRules(): AlertRule[] {
    try {
      const stored = localStorage.getItem(this.RULES_KEY)
      return stored ? JSON.parse(stored) : DEFAULT_ALERT_RULES
    } catch {
      return DEFAULT_ALERT_RULES
    }
  }

  saveRules(rules: AlertRule[]): void {
    localStorage.setItem(this.RULES_KEY, JSON.stringify(rules))
  }

  getLastTriggerTime(ruleId: string): number | null {
    try {
      const stored = localStorage.getItem(this.LAST_TRIGGER_KEY)
      const times: Record<string, number> = stored ? JSON.parse(stored) : {}
      return times[ruleId] || null
    } catch {
      return null
    }
  }

  setLastTriggerTime(ruleId: string, timestamp: number): void {
    try {
      const stored = localStorage.getItem(this.LAST_TRIGGER_KEY)
      const times: Record<string, number> = stored ? JSON.parse(stored) : {}
      times[ruleId] = timestamp
      localStorage.setItem(this.LAST_TRIGGER_KEY, JSON.stringify(times))
    } catch {
      // Ignore storage errors
    }
  }

  clearExpiredAlerts(): void {
    const now = Date.now()
    const alerts = this.getAlerts().filter(a => new Date(a.expiresAt).getTime() > now)
    localStorage.setItem(this.STORAGE_KEY, JSON.stringify(alerts))
  }
}

// ============== Alert Evaluation Engine ==============

interface NewsItem {
  id: string
  title: string
  content: string
  sourceLevel: string
  domain?: string
  sentiment?: number
  score?: number
  publishedAt: string
  entities?: string[]
  currentState?: string
  previousState?: string
}

class AlertEvaluator {
  private storage: AlertStorage

  constructor(storage: AlertStorage) {
    this.storage = storage
  }

  evaluateCondition(condition: RuleCondition, newsItem: NewsItem): boolean {
    const fullText = `${newsItem.title} ${newsItem.content}`.toLowerCase()

    switch (condition.type) {
      case 'keyword_match': {
        const keywords = Array.isArray(condition.value) 
          ? condition.value 
          : [condition.value as string]
        
        if (condition.operator === 'contains') {
          return keywords.some(kw => fullText.includes(String(kw).toLowerCase()))
        }
        if (condition.operator === 'not_in') {
          return !keywords.some(kw => fullText.includes(String(kw).toLowerCase()))
        }
        return false
      }

      case 'entity_mention': {
        const entities = Array.isArray(condition.value)
          ? condition.value
          : [condition.value as string]
        
        const allEntities = [...(newsItem.entities || []), fullText]
        const entityText = allEntities.join(' ').toLowerCase()
        
        if (condition.operator === 'contains') {
          return entities.some(e => entityText.includes(String(e).toLowerCase()))
        }
        return false
      }

      case 'source_level': {
        const levels = Array.isArray(condition.value)
          ? condition.value
          : [condition.value as string]
        
        if (condition.operator === 'in') {
          return levels.includes(newsItem.sourceLevel)
        }
        if (condition.operator === 'equals') {
          return newsItem.sourceLevel === condition.value
        }
        return false
      }

      case 'score_threshold': {
        const threshold = Number(condition.value)
        const score = newsItem.score || 0
        
        if (condition.operator === 'greater_than') return score > threshold
        if (condition.operator === 'less_than') return score < threshold
        if (condition.operator === 'equals') return score === threshold
        return false
      }

      case 'domain_match': {
        const domains = Array.isArray(condition.value)
          ? condition.value
          : [condition.value as string]
        
        if (condition.operator === 'in') {
          return domains.includes(newsItem.domain || '')
        }
        if (condition.operator === 'equals') {
          return newsItem.domain === condition.value
        }
        return false
      }

      case 'state_change': {
        if (!newsItem.currentState) return false
        
        if (condition.operator === 'equals') {
          return newsItem.currentState === condition.value
        }
        return false
      }

      case 'sentiment_shift': {
        // 需要历史数据对比，这里简化处理
        const threshold = Number(condition.value)
        const sentiment = newsItem.sentiment || 0
        
        if (condition.operator === 'greater_than') {
          return Math.abs(sentiment) > threshold
        }
        return false
      }

      case 'volume_spike': {
        // 需要统计数据，这里返回 false 作为默认
        return false
      }

      default:
        return false
    }
  }

  evaluateRule(rule: AlertRule, newsItem: NewsItem): boolean {
    if (!rule.enabled) return false

    // 检查冷却时间
    const lastTrigger = this.storage.getLastTriggerTime(rule.id)
    if (lastTrigger) {
      const cooldownMs = rule.cooldownMinutes * 60 * 1000
      if (Date.now() - lastTrigger < cooldownMs) {
        return false
      }
    }

    // 评估所有条件
    const results = rule.conditions.map(c => this.evaluateCondition(c, newsItem))

    // 根据逻辑组合结果
    if (rule.conditionLogic === 'AND') {
      return results.every(r => r)
    } else {
      return results.some(r => r)
    }
  }

  createAlert(rule: AlertRule, newsItem: NewsItem): Alert {
    const matchedConditions = rule.conditions
      .filter(c => this.evaluateCondition(c, newsItem))
      .map(c => `${c.type}: ${JSON.stringify(c.value)}`)

    return {
      id: `alert-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      ruleId: rule.id,
      ruleName: rule.name,
      priority: rule.priority,
      status: 'active',
      title: `[${rule.priority.toUpperCase()}] ${rule.name}`,
      message: newsItem.title,
      sourceId: newsItem.id,
      sourceLevel: newsItem.sourceLevel,
      domain: newsItem.domain,
      score: newsItem.score,
      matchedConditions,
      triggeredAt: new Date().toISOString(),
      expiresAt: new Date(Date.now() + 7 * 24 * 60 * 60 * 1000).toISOString(), // 7天后过期
      metadata: {
        content: newsItem.content.substring(0, 500),
        publishedAt: newsItem.publishedAt
      }
    }
  }
}

// ============== Notification Dispatcher ==============

class NotificationDispatcher {
  private browserPermission: NotificationPermission = 'default'

  async requestBrowserPermission(): Promise<boolean> {
    if (!('Notification' in window)) {
      console.warn('Browser does not support notifications')
      return false
    }

    if (Notification.permission === 'granted') {
      this.browserPermission = 'granted'
      return true
    }

    const permission = await Notification.requestPermission()
    this.browserPermission = permission
    return permission === 'granted'
  }

  async sendBrowserNotification(payload: NotificationPayload): Promise<void> {
    if (this.browserPermission !== 'granted') {
      const granted = await this.requestBrowserPermission()
      if (!granted) return
    }

    const notification = new Notification(payload.title, {
      body: payload.body,
      icon: payload.icon || '/favicon.ico',
      data: payload.data,
      tag: payload.data?.ruleId as string // 防止重复通知
    })

    notification.onclick = () => {
      if (payload.url) {
        window.open(payload.url, '_blank')
      }
      notification.close()
    }
  }

  async sendWebhook(webhookUrl: string, payload: Record<string, unknown>): Promise<void> {
    try {
      await fetch(webhookUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      })
    } catch (error) {
      console.error('Webhook notification failed:', error)
    }
  }

  async sendTelegram(botToken: string, chatId: string, message: string): Promise<void> {
    try {
      const url = `https://api.telegram.org/bot${botToken}/sendMessage`
      await fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          chat_id: chatId,
          text: message,
          parse_mode: 'Markdown'
        })
      })
    } catch (error) {
      console.error('Telegram notification failed:', error)
    }
  }

  async dispatch(alert: Alert, channels: AlertChannel[]): Promise<void> {
    const priorityEmoji: Record<AlertPriority, string> = {
      critical: '🔴',
      high: '🟠',
      medium: '🟡',
      low: '🟢'
    }

    const emoji = priorityEmoji[alert.priority]

    for (const channel of channels) {
      switch (channel) {
        case 'browser':
          await this.sendBrowserNotification({
            title: `${emoji} ${alert.title}`,
            body: alert.message,
            data: { alertId: alert.id, ruleId: alert.ruleId }
          })
          break

        case 'webhook':
          // Webhook URL should be configured elsewhere
          // await this.sendWebhook(webhookUrl, { alert })
          break

        case 'telegram':
          // Telegram config should be stored elsewhere
          // await this.sendTelegram(botToken, chatId, `${emoji} *${alert.title}*\n\n${alert.message}`)
          break

        default:
          console.log(`Channel ${channel} not implemented yet`)
      }
    }
  }
}

// ============== Main Alert Service ==============

export class AlertService {
  private storage: AlertStorage
  private evaluator: AlertEvaluator
  private dispatcher: NotificationDispatcher
  private listeners: ((alert: Alert) => void)[] = []

  constructor() {
    this.storage = new AlertStorage()
    this.evaluator = new AlertEvaluator(this.storage)
    this.dispatcher = new NotificationDispatcher()
    
    // 定期清理过期警报
    setInterval(() => this.storage.clearExpiredAlerts(), 60 * 60 * 1000)
  }

  // 订阅新警报
  subscribe(callback: (alert: Alert) => void): () => void {
    this.listeners.push(callback)
    return () => {
      this.listeners = this.listeners.filter(l => l !== callback)
    }
  }

  private notifyListeners(alert: Alert): void {
    this.listeners.forEach(callback => callback(alert))
  }

  // 处理新闻项，检查是否触发警报
  async processNewsItem(newsItem: NewsItem): Promise<Alert[]> {
    const rules = this.storage.getRules()
    const triggeredAlerts: Alert[] = []

    for (const rule of rules) {
      if (this.evaluator.evaluateRule(rule, newsItem)) {
        const alert = this.evaluator.createAlert(rule, newsItem)
        this.storage.saveAlert(alert)
        this.storage.setLastTriggerTime(rule.id, Date.now())
        
        await this.dispatcher.dispatch(alert, rule.channels)
        this.notifyListeners(alert)
        
        triggeredAlerts.push(alert)
      }
    }

    return triggeredAlerts
  }

  // 获取所有警报
  getAlerts(filter?: {
    priority?: AlertPriority
    status?: AlertStatus
    startDate?: Date
    endDate?: Date
  }): Alert[] {
    let alerts = this.storage.getAlerts()

    if (filter) {
      if (filter.priority) {
        alerts = alerts.filter(a => a.priority === filter.priority)
      }
      if (filter.status) {
        alerts = alerts.filter(a => a.status === filter.status)
      }
      if (filter.startDate) {
        alerts = alerts.filter(a => new Date(a.triggeredAt) >= filter.startDate!)
      }
      if (filter.endDate) {
        alerts = alerts.filter(a => new Date(a.triggeredAt) <= filter.endDate!)
      }
    }

    return alerts
  }

  // 获取警报统计
  getStats(): AlertStats {
    const alerts = this.storage.getAlerts()
    const now = Date.now()
    const day = 24 * 60 * 60 * 1000

    return {
      total: alerts.length,
      byPriority: {
        critical: alerts.filter(a => a.priority === 'critical').length,
        high: alerts.filter(a => a.priority === 'high').length,
        medium: alerts.filter(a => a.priority === 'medium').length,
        low: alerts.filter(a => a.priority === 'low').length
      },
      byStatus: {
        active: alerts.filter(a => a.status === 'active').length,
        acknowledged: alerts.filter(a => a.status === 'acknowledged').length,
        dismissed: alerts.filter(a => a.status === 'dismissed').length,
        expired: alerts.filter(a => a.status === 'expired').length
      },
      last24Hours: alerts.filter(a => now - new Date(a.triggeredAt).getTime() < day).length,
      last7Days: alerts.filter(a => now - new Date(a.triggeredAt).getTime() < 7 * day).length
    }
  }

  // 确认警报
  acknowledgeAlert(alertId: string, userId?: string): void {
    this.storage.updateAlert(alertId, {
      status: 'acknowledged',
      acknowledgedAt: new Date().toISOString(),
      acknowledgedBy: userId
    })
  }

  // 忽略警报
  dismissAlert(alertId: string): void {
    this.storage.updateAlert(alertId, { status: 'dismissed' })
  }

  // 获取规则
  getRules(): AlertRule[] {
    return this.storage.getRules()
  }

  // 保存规则
  saveRule(rule: AlertRule): void {
    const rules = this.storage.getRules()
    const index = rules.findIndex(r => r.id === rule.id)
    
    if (index >= 0) {
      rules[index] = { ...rule, updatedAt: new Date().toISOString() }
    } else {
      rules.push({ ...rule, createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() })
    }
    
    this.storage.saveRules(rules)
  }

  // 删除规则
  deleteRule(ruleId: string): void {
    const rules = this.storage.getRules().filter(r => r.id !== ruleId)
    this.storage.saveRules(rules)
  }

  // 启用/禁用规则
  toggleRule(ruleId: string, enabled: boolean): void {
    const rules = this.storage.getRules()
    const rule = rules.find(r => r.id === ruleId)
    if (rule) {
      rule.enabled = enabled
      rule.updatedAt = new Date().toISOString()
      this.storage.saveRules(rules)
    }
  }

  // 请求浏览器通知权限
  async requestNotificationPermission(): Promise<boolean> {
    return this.dispatcher.requestBrowserPermission()
  }

  // 重置为默认规则
  resetToDefaultRules(): void {
    this.storage.saveRules(DEFAULT_ALERT_RULES)
  }
}

// 导出单例
export const alertService = new AlertService()

export default alertService
