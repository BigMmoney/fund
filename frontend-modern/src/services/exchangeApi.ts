export type AuthRole = 'user' | 'admin'

export interface AuthConfig {
  baseUrl: string
  secret: string
  subject: string
  role: AuthRole
  sessionId: string
}

export type JsonRecord = Record<string, unknown>

export class ApiError extends Error {
  status: number
  payload: unknown

  constructor(status: number, message: string, payload: unknown) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.payload = payload
  }
}

const encoder = new TextEncoder()

function toHex(buffer: ArrayBuffer): string {
  return Array.from(new Uint8Array(buffer))
    .map((byte) => byte.toString(16).padStart(2, '0'))
    .join('')
}

async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', encoder.encode(text))
  return toHex(digest)
}

async function hmacHex(secret: string, payload: string): Promise<string> {
  const key = await crypto.subtle.importKey('raw', encoder.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign'])
  const signature = await crypto.subtle.sign('HMAC', key, encoder.encode(payload))
  return toHex(signature)
}

function randomId(prefix: string): string {
  const value = typeof crypto.randomUUID === 'function' ? crypto.randomUUID() : `${Date.now()}-${Math.random().toString(16).slice(2)}`
  return `${prefix}-${value}`
}

function resolveRequestUrl(auth: AuthConfig, path: string) {
  const normalizedBase = auth.baseUrl.trim()
  if (normalizedBase.length === 0) {
    const absolute = new URL(path, window.location.origin)
    return {
      fetchUrl: `${absolute.pathname}${absolute.search}`,
      signaturePath: absolute.pathname,
      signatureQuery: absolute.search.startsWith('?') ? absolute.search.slice(1) : '',
    }
  }

  const absolute = new URL(path, normalizedBase.endsWith('/') ? normalizedBase : `${normalizedBase}/`)
  return {
    fetchUrl: absolute.toString(),
    signaturePath: absolute.pathname,
    signatureQuery: absolute.search.startsWith('?') ? absolute.search.slice(1) : '',
  }
}

async function buildInternalAuthHeaders(
  auth: AuthConfig,
  method: string,
  signaturePath: string,
  signatureQuery: string,
  bodyText: string,
) {
  const timestamp = Math.floor(Date.now() / 1000).toString()
  const requestId = randomId('ui')
  const bodyHash = await sha256Hex(bodyText)
  const payload = [
    method.toUpperCase(),
    signaturePath,
    signatureQuery,
    auth.subject,
    auth.role,
    auth.sessionId,
    timestamp,
    requestId,
  ].join('\n')
  const signature = await hmacHex(auth.secret, payload)

  return {
    'x-request-id': requestId,
    'x-internal-auth-subject': auth.subject,
    'x-internal-auth-role': auth.role,
    'x-internal-auth-session-id': auth.sessionId,
    'x-internal-auth-timestamp': timestamp,
    'x-internal-auth-signature': signature,
    'x-internal-auth-body-sha256': bodyHash,
  }
}

async function requestJson<T = unknown>(
  auth: AuthConfig,
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const method = (init.method ?? 'GET').toUpperCase()
  const { fetchUrl, signaturePath, signatureQuery } = resolveRequestUrl(auth, path)
  const bodyText = typeof init.body === 'string' ? init.body : ''
  const authHeaders = await buildInternalAuthHeaders(auth, method, signaturePath, signatureQuery, bodyText)
  const headers = new Headers(init.headers ?? {})

  Object.entries(authHeaders).forEach(([key, value]) => headers.set(key, value))
  if (method !== 'GET' && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }
  if (!headers.has('Accept')) {
    headers.set('Accept', 'application/json')
  }

  const response = await fetch(fetchUrl, {
    ...init,
    method,
    headers,
  })

  const text = await response.text()
  let payload: unknown = null
  try {
    payload = text.length > 0 ? (JSON.parse(text) as unknown) : null
  } catch {
    payload = text
  }

  if (!response.ok) {
    const message =
      typeof payload === 'object' && payload && 'error' in payload && typeof (payload as JsonRecord).error === 'string'
        ? String((payload as JsonRecord).error)
        : `HTTP ${response.status}`
    throw new ApiError(response.status, message, payload)
  }

  return payload as T
}

async function requestText(auth: AuthConfig, path: string): Promise<string> {
  const method = 'GET'
  const { fetchUrl, signaturePath, signatureQuery } = resolveRequestUrl(auth, path)
  const authHeaders = await buildInternalAuthHeaders(auth, method, signaturePath, signatureQuery, '')
  const headers = new Headers({ Accept: 'text/plain' })
  Object.entries(authHeaders).forEach(([key, value]) => headers.set(key, value))

  const response = await fetch(fetchUrl, { method, headers })
  const text = await response.text()
  if (!response.ok) {
    throw new ApiError(response.status, text || `HTTP ${response.status}`, text)
  }
  return text
}

export function asRecord(value: unknown): JsonRecord {
  return typeof value === 'object' && value !== null ? (value as JsonRecord) : {}
}

export function asList(
  value: unknown,
  keys: string[] = ['items', 'markets', 'orders', 'fills', 'balances', 'positions', 'trades', 'runbooks', 'incidents', 'alerts'],
): JsonRecord[] {
  if (Array.isArray(value)) {
    return value.filter((item): item is JsonRecord => typeof item === 'object' && item !== null)
  }

  const record = asRecord(value)
  for (const key of keys) {
    const candidate = record[key]
    if (Array.isArray(candidate)) {
      return candidate.filter((item): item is JsonRecord => typeof item === 'object' && item !== null)
    }
  }
  return []
}

export function createExchangeApi(auth: AuthConfig) {
  return {
    getMarketsSummary: () => requestJson(auth, '/markets/summary'),
    getMarkets: () => requestJson(auth, '/markets'),
    getRules: () => requestJson(auth, '/rules'),
    getBook: (marketId: string, depth = 20, outcome = 0) =>
      requestJson(auth, `/markets/${encodeURIComponent(marketId)}/book?depth=${depth}&outcome=${outcome}`),
    getTrades: (marketId: string, limit = 20, outcome = 0) =>
      requestJson(auth, `/markets/${encodeURIComponent(marketId)}/trades?limit=${limit}&outcome=${outcome}`),
    getTicker: (marketId: string) => requestJson(auth, `/markets/${encodeURIComponent(marketId)}/ticker`),
    getKlines: (marketId: string, interval = '1m', limit = 60, outcome = 0) =>
      requestJson(auth, `/markets/${encodeURIComponent(marketId)}/klines?interval=${encodeURIComponent(interval)}&limit=${limit}&outcome=${outcome}`),
    getOpenInterest: (marketId: string) => requestJson(auth, `/markets/${encodeURIComponent(marketId)}/open-interest`),
    getPublicFundingRate: (marketId: string) => requestJson(auth, `/markets/${encodeURIComponent(marketId)}/funding-rate`),
    getMarkPrice: (marketId: string) => requestJson(auth, `/markets/${encodeURIComponent(marketId)}/mark-price`),
    getMicrostructure: (marketId: string) => requestJson(auth, `/markets/${encodeURIComponent(marketId)}/microstructure`),
    getBalances: (userId: string) => requestJson(auth, `/balances/${encodeURIComponent(userId)}`),
    getPositions: (userId: string) => requestJson(auth, `/positions/${encodeURIComponent(userId)}`),
    getMargin: (userId: string, marketId: string, outcome = 0) =>
      requestJson(auth, `/margin/${encodeURIComponent(userId)}?market_id=${encodeURIComponent(marketId)}&outcome=${outcome}`),
    getPnl: (userId: string, marketId: string, outcome = 0) =>
      requestJson(auth, `/pnl/${encodeURIComponent(userId)}?market_id=${encodeURIComponent(marketId)}&outcome=${outcome}`),
    getOrders: (userId: string, marketId?: string) =>
      requestJson(auth, `/orders/${encodeURIComponent(userId)}${marketId ? `?market_id=${encodeURIComponent(marketId)}` : ''}`),
    getFills: (userId: string, marketId?: string) =>
      requestJson(auth, `/fills/${encodeURIComponent(userId)}${marketId ? `?market_id=${encodeURIComponent(marketId)}` : ''}`),
    getLedger: (userId: string) => requestJson(auth, `/ledger/${encodeURIComponent(userId)}`),
    submitOrder: (body: JsonRecord) => requestJson(auth, '/submit-order', { method: 'POST', body: JSON.stringify(body) }),
    cancelOrder: (body: JsonRecord) => requestJson(auth, '/cancel-order', { method: 'POST', body: JSON.stringify(body) }),
    replaceOrder: (body: JsonRecord) => requestJson(auth, '/replace-order', { method: 'POST', body: JSON.stringify(body) }),
    deposit: (body: JsonRecord) => requestJson(auth, '/deposit', { method: 'POST', body: JSON.stringify(body) }),
    getHealth: () => requestJson(auth, '/health'),
    getReady: () => requestJson(auth, '/ready'),
    getPartitions: () => requestJson(auth, '/health/partitions'),
    getMetrics: () => requestJson(auth, '/metrics'),
    getPrometheus: () => requestText(auth, '/metrics/prometheus'),
    getVersion: () => requestJson(auth, '/version'),
    getPlanes: () => requestJson(auth, '/admin/planes'),
    resetPlanes: (plane: 'data' | 'control' | 'all') => requestJson(auth, '/admin/planes/reset', { method: 'POST', body: JSON.stringify({ plane }) }),
    getOpsNode: () => requestJson(auth, '/admin/ops/node'),
    setDrain: (enable: boolean) => requestJson(auth, '/admin/ops/drain', { method: 'POST', body: JSON.stringify({ enable }) }),
    checkpoint: () => requestJson(auth, '/admin/ops/checkpoint', { method: 'POST', body: JSON.stringify({}) }),
    setKillSwitch: (enabled: boolean) =>
      requestJson(auth, '/admin/kill-switch', { method: 'POST', body: JSON.stringify({ enabled, request_id: randomId('kill') }) }),
    setMarketState: (marketId: string, state: string, outcome = 0) =>
      requestJson(auth, '/admin/market-state', {
        method: 'POST',
        body: JSON.stringify({ market_id: marketId, state, outcome, request_id: randomId('market-state') }),
      }),
    getMarketState: (marketId: string) => requestJson(auth, `/admin/market-state/${encodeURIComponent(marketId)}`),
    getFundingRates: (marketId?: string) =>
      requestJson(auth, `/admin/risk/funding-rates${marketId ? `?market_id=${encodeURIComponent(marketId)}` : ''}`),
    upsertFundingRate: (marketId: string, fundingRatePpm: number, outcome = 0) =>
      requestJson(auth, '/admin/risk/funding-rates', {
        method: 'POST',
        body: JSON.stringify({ market_id: marketId, funding_rate_ppm: Math.round(fundingRatePpm), outcome }),
      }),
    getRiskEvents: (limit = 20) => requestJson(auth, `/admin/risk/events?limit=${limit}`),
    getGovernanceActions: (limit = 20) => requestJson(auth, `/admin/risk/governance/actions?limit=${limit}`),
    approveGovernanceAction: (actionId: string) =>
      requestJson(auth, `/admin/risk/governance/actions/${encodeURIComponent(actionId)}/approve`, { method: 'POST', body: JSON.stringify({}) }),
    rejectGovernanceAction: (actionId: string) =>
      requestJson(auth, `/admin/risk/governance/actions/${encodeURIComponent(actionId)}/reject`, { method: 'POST', body: JSON.stringify({}) }),
    getAdminAudit: (limit = 20) => requestJson(auth, `/admin/audit/actions?limit=${limit}`),
    getBetaControlPlane: () => requestJson(auth, '/admin/beta/control-plane'),
    updateBetaControlPlane: (body: JsonRecord) =>
      requestJson(auth, '/admin/beta/control-plane', { method: 'POST', body: JSON.stringify(body) }),
    listBetaUsers: () => requestJson(auth, '/admin/beta/users'),
    updateBetaUser: (userId: string, body: JsonRecord) =>
      requestJson(auth, `/admin/beta/users/${encodeURIComponent(userId)}`, { method: 'POST', body: JSON.stringify(body) }),
    listBetaMarkets: () => requestJson(auth, '/admin/beta/markets'),
    updateBetaMarket: (marketId: string, body: JsonRecord) =>
      requestJson(auth, `/admin/beta/markets/${encodeURIComponent(marketId)}`, { method: 'POST', body: JSON.stringify(body) }),
    getTreasuryFeeCollector: () => requestJson(auth, '/admin/treasury/fee-collector'),
    getTreasuryInsuranceFunds: () => requestJson(auth, '/admin/treasury/insurance-funds'),
    getPerfProfile: () => requestJson(auth, '/admin/perf/profile'),
    getPerfSla: () => requestJson(auth, '/admin/perf/sla'),
    getOncallStatus: () => requestJson(auth, '/admin/oncall/status'),
    getOncallEscalation: () => requestJson(auth, '/admin/oncall/escalation'),
    getOncallRunbooks: () => requestJson(auth, '/admin/oncall/runbooks'),
    getCapacity: () => requestJson(auth, '/admin/capacity'),
    getCapacityAlerts: () => requestJson(auth, '/admin/capacity/alerts'),
    getSentinelPosture: () => requestJson(auth, '/admin/sentinel/posture'),
    getSentinelIncidents: () => requestJson(auth, '/admin/sentinel/incidents'),
    getRollbackStatus: () => requestJson(auth, '/admin/rollback/status'),
    getRollbackRunbook: () => requestJson(auth, '/admin/rollback/runbook'),
    getReleaseChecklist: () => requestJson(auth, '/admin/release/checklist'),
    getReleaseVersion: (target: string) => requestJson(auth, `/admin/release/version?target=${encodeURIComponent(target)}`),
    getReleaseFeatures: () => requestJson(auth, '/admin/release/features'),
    getFailpoints: () => requestJson(auth, '/admin/failpoints'),
    activateFailpoint: (body: JsonRecord) => requestJson(auth, '/admin/failpoints/activate', { method: 'POST', body: JSON.stringify(body) }),
    deactivateFailpoint: (name: string) =>
      requestJson(auth, '/admin/failpoints/deactivate', { method: 'POST', body: JSON.stringify({ name }) }),
    getFeeTiers: () => requestJson(auth, '/fee-tiers'),
    getUserFeeTier: (userId: string) => requestJson(auth, `/fee-tier/${encodeURIComponent(userId)}`),
    requestWithdrawal: (body: JsonRecord) => requestJson(auth, '/withdraw', { method: 'POST', body: JSON.stringify(body) }),
    getWithdrawals: (userId: string, status?: string, limit = 20) =>
      requestJson(
        auth,
        `/withdrawals/${encodeURIComponent(userId)}?limit=${limit}${status ? `&status=${encodeURIComponent(status)}` : ''}`,
      ),
    getPendingWithdrawals: () => requestJson(auth, '/admin/withdrawals/pending'),
    approveWithdrawal: (withdrawalId: string) =>
      requestJson(auth, '/admin/withdrawal/approve', { method: 'POST', body: JSON.stringify({ withdrawal_id: withdrawalId }) }),
    rejectWithdrawal: (withdrawalId: string) =>
      requestJson(auth, '/admin/withdrawal/reject', { method: 'POST', body: JSON.stringify({ withdrawal_id: withdrawalId }) }),
    getCustodyAudit: () => requestJson(auth, '/admin/custody/audit'),
    getCustodyAuditEvents: () => requestJson(auth, '/admin/custody/audit/events'),
    getCustodyBreaker: () => requestJson(auth, '/admin/custody/breaker'),
    resetCustodyBreaker: () => requestJson(auth, '/admin/custody/breaker/reset', { method: 'POST', body: JSON.stringify({}) }),
    listOtcQuotes: () => requestJson(auth, '/otc/quotes'),
    createOtcQuote: (body: JsonRecord) => requestJson(auth, '/otc/quotes', { method: 'POST', body: JSON.stringify(body) }),
    acceptOtcQuote: (quoteId: string) => requestJson(auth, `/otc/quotes/${encodeURIComponent(quoteId)}/accept`, { method: 'POST', body: JSON.stringify({}) }),
    getEarnPositions: (userId: string) => requestJson(auth, `/earn/positions/${encodeURIComponent(userId)}`),
    subscribeEarn: (body: JsonRecord) => requestJson(auth, '/earn/subscribe', { method: 'POST', body: JSON.stringify(body) }),
    redeemEarn: (body: JsonRecord) => requestJson(auth, '/earn/redeem', { method: 'POST', body: JSON.stringify(body) }),
  }
}
