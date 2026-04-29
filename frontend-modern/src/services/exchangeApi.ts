export interface MarketSummary {
  id: string
  name: string
  state: string
  outcomes: number[]
  openOrders: number
  kind: 'spot' | 'margin' | 'perp' | 'future' | 'option' | 'otc' | 'earn'
  backendAvailable: boolean
  tradingEnabled: boolean
}

export interface BookLevel {
  price: number
  amount: number
  count?: number
}

export interface OrderBookSnapshot {
  marketId: string
  outcome: number
  bids: BookLevel[]
  asks: BookLevel[]
  timestamp: string
  source: 'api' | 'mock'
}

export interface TradeRecord {
  id: string
  marketId: string
  outcome: number
  price: number
  amount: number
  buyer?: string
  seller?: string
  timestamp: string
}

export interface BalanceRecord {
  userId: string
  asset: string
  available: number
  hold: number
  source: 'api' | 'mock'
}

export interface OpenOrderRecord {
  id: string
  marketId: string
  outcome: number
  side: string
  price: number
  amount: number
  filled: number
  remaining: number
  status: string
  createdAt: string
  source: 'api' | 'mock'
}

export interface OtcQuoteRecord {
  quoteId: string
  marketId: string
  settlementMarketId: string
  requesterUserId: string
  counterpartyUserId?: string
  side: string
  price: number
  amount: number
  outcome: number
  status: string
  createdAt: string
  acceptedAt?: string
}

export interface EarnPositionRecord {
  positionId: string
  userId: string
  productId: string
  asset: string
  principalAmount: number
  aprBps: number
  createdAt: string
  updatedAt: string
}

export interface SubmitOrderInput {
  userId: string
  marketId: string
  side: 'buy' | 'sell'
  price: number
  amount: number
  outcome?: number
  expiresIn?: number
}

export interface SubmitOrderResult {
  ok: boolean
  source: 'api' | 'mock'
  message: string
  intentId?: string
  raw?: unknown
}

export interface CancelOrderResult {
  ok: boolean
  source: 'api' | 'mock'
  message: string
  raw?: unknown
}

export interface SystemEndpointStatus {
  name: string
  url: string
  status: 'running' | 'unavailable'
  latencyMs?: number
  details?: string
}

export interface AdminActionResult {
  ok: boolean
  message: string
  raw?: unknown
}

export interface GovernanceActionRecord {
  action_id: string
  action_type: string
  requested_by: string
  status: string
  required_approvals?: number
  approvers?: string[]
  recorded_at?: string
  payload?: Record<string, unknown>
}

export interface Fill {
  id: string
  user: string
  side: 'Buy' | 'Sell'
  price: number
  amount: number
  timestamp: string
}

export interface Intent {
  user_id: string
  market_id: string
  side: 'Buy' | 'Sell'
  price: number
  amount: number
  outcome: number
}

type ApiSource = 'api' | 'mock'
type DemoActor = 'trader' | 'admin' | 'admin2' | 'admin3' | 'viewer'

function inferMarketKind(marketId: string): MarketSummary['kind'] {
  if (marketId.startsWith('margin:')) return 'margin'
  if (marketId.startsWith('perp:')) return 'perp'
  if (marketId.startsWith('future:')) return 'future'
  if (marketId.startsWith('option:')) return 'option'
  if (marketId.startsWith('otc:')) return 'otc'
  if (marketId.startsWith('earn:')) return 'earn'
  return 'spot'
}

function catalogMarkets(): MarketSummary[] {
  return [
    { id: 'btc-usdt', name: 'BTC/USDT Spot', state: 'normal', outcomes: [0], openOrders: 0, kind: 'spot', backendAvailable: true, tradingEnabled: true },
    { id: 'eth-usdt', name: 'ETH/USDT Spot', state: 'normal', outcomes: [0], openOrders: 0, kind: 'spot', backendAvailable: false, tradingEnabled: false },
    { id: 'margin:btc-usdt', name: 'BTC/USDT Margin', state: 'normal', outcomes: [0], openOrders: 0, kind: 'margin', backendAvailable: true, tradingEnabled: true },
    { id: 'perp:btc-usdt', name: 'BTC/USDT Perpetual', state: 'normal', outcomes: [0], openOrders: 0, kind: 'perp', backendAvailable: true, tradingEnabled: true },
    { id: 'future:btc-usdt:202606', name: 'BTC/USDT Quarterly Future', state: 'normal', outcomes: [0], openOrders: 0, kind: 'future', backendAvailable: true, tradingEnabled: true },
    { id: 'option:btc-usdt:call-70000:202606', name: 'BTC Call 70000 2026-06', state: 'normal', outcomes: [0], openOrders: 0, kind: 'option', backendAvailable: true, tradingEnabled: true },
    { id: 'otc:btc-usdt:block', name: 'BTC/USDT OTC Block', state: 'normal', outcomes: [0], openOrders: 0, kind: 'otc', backendAvailable: true, tradingEnabled: true },
    { id: 'earn:usdc:flex', name: 'USDC Flexible Earn', state: 'normal', outcomes: [0], openOrders: 0, kind: 'earn', backendAvailable: true, tradingEnabled: true },
  ]
}

function mergeMarkets(apiMarkets: MarketSummary[]): MarketSummary[] {
  const merged = new Map<string, MarketSummary>()

  for (const item of catalogMarkets()) {
    merged.set(item.id, item)
  }

  for (const item of apiMarkets) {
    merged.set(item.id, {
      ...merged.get(item.id),
      ...item,
      kind: item.kind,
      backendAvailable: true,
      tradingEnabled: true,
    })
  }

  return Array.from(merged.values())
}

interface StoredSession {
  username: string
  role?: string
  token?: string
}

class ApiRequestError extends Error {
  readonly status: number
  readonly payload?: unknown

  constructor(status: number, message: string, payload?: unknown) {
    super(message)
    this.name = 'ApiRequestError'
    this.status = status
    this.payload = payload
  }
}

const AUTH_STORAGE_KEY = 'pretrading.auth.session'
const ADMIN_API_BASE = ''
const DEMO_TOKENS: Record<DemoActor, string> = {
  trader: 'demo-trader-token',
  admin: 'demo-admin-token',
  admin2: 'demo-admin2-token',
  admin3: 'demo-admin3-token',
  viewer: 'demo-viewer-token',
}

function parseNumber(value: unknown, fallback = 0): number {
  const numeric = typeof value === 'string' ? Number(value) : typeof value === 'number' ? value : Number.NaN
  return Number.isFinite(numeric) ? numeric : fallback
}

function readSession(): StoredSession | null {
  try {
    const raw = window.localStorage.getItem(AUTH_STORAGE_KEY)
    return raw ? (JSON.parse(raw) as StoredSession) : null
  } catch {
    return null
  }
}

function normalizeActor(actor?: string): DemoActor | undefined {
  if (actor === 'trader' || actor === 'admin' || actor === 'admin2' || actor === 'admin3' || actor === 'viewer') {
    return actor
  }
  return undefined
}

function buildAuthHeaders(actor?: string): Record<string, string> {
  const normalizedActor = normalizeActor(actor)
  if (normalizedActor) {
    return {
      Authorization: `Bearer ${DEMO_TOKENS[normalizedActor]}`,
      'X-Session-Id': `session-${normalizedActor}`,
    }
  }

  const session = readSession()
  const sessionActor = normalizeActor(session?.username)
  if (!sessionActor) {
    return {}
  }

  return {
    Authorization: `Bearer ${DEMO_TOKENS[sessionActor]}`,
    'X-Session-Id': session?.token ?? `session-${sessionActor}`,
  }
}

function formatApiError(payload: unknown, fallback: string): string {
  if (!payload || typeof payload !== 'object') {
    return fallback
  }

  const record = payload as Record<string, unknown>
  const message = typeof record.message === 'string' ? record.message : undefined
  const error = typeof record.error === 'string' ? record.error : undefined
  const code = typeof record.code === 'string' ? record.code : undefined

  if (message && error) {
    return `${message}: ${error}`
  }
  if (message) {
    return message
  }
  if (error && code) {
    return `${code}: ${error}`
  }
  if (error) {
    return error
  }
  if (code) {
    return code
  }
  return fallback
}

function unwrapArrayPayload(payload: unknown): Record<string, unknown>[] {
  if (Array.isArray(payload)) {
    return payload.filter((item): item is Record<string, unknown> => typeof item === 'object' && item !== null)
  }

  if (payload && typeof payload === 'object') {
    const record = payload as Record<string, unknown>
    if (Array.isArray(record.items)) {
      return record.items.filter((item): item is Record<string, unknown> => typeof item === 'object' && item !== null)
    }
    if (Array.isArray(record.value)) {
      return record.value.filter((item): item is Record<string, unknown> => typeof item === 'object' && item !== null)
    }
  }

  return []
}

async function fetchJSON<T>(url: string, init?: RequestInit, actor?: string): Promise<T> {
  const response = await fetch(url, {
    ...init,
    headers: {
      Accept: 'application/json',
      ...buildAuthHeaders(actor),
      ...(init?.headers ?? {}),
    },
  })

  const text = await response.text().catch(() => '')
  const payload = text ? safeParseJson(text) : undefined

  if (!response.ok) {
    const fallbackMessage = text || (response.status === 409 ? 'Request rejected by backend (HTTP 409)' : `HTTP ${response.status}`)
    throw new ApiRequestError(response.status, formatApiError(payload, fallbackMessage), payload)
  }

  return (payload as T | undefined) ?? ({} as T)
}

function safeParseJson(text: string): unknown {
  try {
    return JSON.parse(text) as unknown
  } catch {
    return text
  }
}

function isNetworkError(error: unknown): boolean {
  return !(error instanceof ApiRequestError)
}

function mockMarkets(): MarketSummary[] {
  return mergeMarkets([])
}

function mockBook(marketId: string): OrderBookSnapshot {
  const mid = marketId === 'btc-usdt' ? 67250 : marketId === 'eth-usdt' ? 3450 : 67280
  const step = marketId === 'eth-usdt' ? 2 : 5

  return {
    marketId,
    outcome: 0,
    bids: Array.from({ length: 12 }, (_, index) => ({
      price: Number((mid - index * step).toFixed(2)),
      amount: Number((0.6 + index * 0.23).toFixed(3)),
      count: index + 1,
    })),
    asks: Array.from({ length: 12 }, (_, index) => ({
      price: Number((mid + index * step).toFixed(2)),
      amount: Number((0.5 + index * 0.21).toFixed(3)),
      count: index + 1,
    })),
    timestamp: new Date().toISOString(),
    source: 'mock',
  }
}

function mockTrades(marketId: string): TradeRecord[] {
  return Array.from({ length: 10 }, (_, index) => ({
    id: `${marketId}-${index}`,
    marketId,
    outcome: 0,
    price: marketId === 'btc-usdt' ? 67210 + index * 4 : marketId === 'eth-usdt' ? 3440 + index : 67220 + index * 5,
    amount: Number((0.2 + index * 0.13).toFixed(3)),
    buyer: index % 2 === 0 ? 'trader' : 'viewer',
    seller: index % 2 === 0 ? 'viewer' : 'trader',
    timestamp: new Date(Date.now() - index * 45_000).toISOString(),
  }))
}

function mockBalances(userId: string): BalanceRecord[] {
  return [
    { userId, asset: 'USDC', available: 1_000_000, hold: 2_500, source: 'mock' },
    { userId, asset: 'BTC-SPOT', available: 25, hold: 2, source: 'mock' },
  ]
}

function mockOrders(marketId?: string): OpenOrderRecord[] {
  return [
    {
      id: 'mock-order-1',
      marketId: marketId ?? 'btc-usdt',
      outcome: 0,
      side: 'buy',
      price: 67120,
      amount: 2,
      filled: 0,
      remaining: 2,
      status: 'open',
      createdAt: new Date().toISOString(),
      source: 'mock',
    },
  ]
}

export class ExchangeAPI {
  currentUserId(): string {
    return readSession()?.username ?? 'trader'
  }

  async getMarkets(): Promise<{ items: MarketSummary[]; source: ApiSource }> {
    try {
      const payload = await fetchJSON<unknown>('/v1/markets')
      const items = unwrapArrayPayload(payload).map((entry) => ({
        id: String(entry.id ?? entry.market_id ?? entry.name ?? 'unknown'),
        name: String(entry.name ?? entry.market_id ?? entry.id ?? 'unknown'),
        state: String(entry.state ?? 'Unknown'),
        outcomes: Array.isArray(entry.outcomes) ? entry.outcomes.map((value) => parseNumber(value)) : [0],
        openOrders: parseNumber(entry.open_orders),
        kind: inferMarketKind(String(entry.id ?? entry.market_id ?? entry.name ?? 'unknown')),
        backendAvailable: true,
        tradingEnabled: true,
      }))
      return { items: mergeMarkets(items), source: items.length > 0 ? 'api' : 'mock' }
    } catch {
      return { items: mockMarkets(), source: 'mock' }
    }
  }

  async getOrderBook(marketId: string, outcome = 0): Promise<OrderBookSnapshot> {
    try {
      const payload = await fetchJSON<Record<string, unknown>>(`/v1/markets/${encodeURIComponent(marketId)}/book?outcome=${outcome}&depth=12`)
      return {
        marketId: String(payload.market_id ?? marketId),
        outcome: parseNumber(payload.outcome),
        bids: Array.isArray(payload.bids)
          ? payload.bids.map((entry) => {
              const item = entry as Record<string, unknown>
              return {
                price: parseNumber(item.price),
                amount: parseNumber(item.amount),
                count: parseNumber(item.count),
              }
            })
          : [],
        asks: Array.isArray(payload.asks)
          ? payload.asks.map((entry) => {
              const item = entry as Record<string, unknown>
              return {
                price: parseNumber(item.price),
                amount: parseNumber(item.amount),
                count: parseNumber(item.count),
              }
            })
          : [],
        timestamp: String(payload.timestamp ?? new Date().toISOString()),
        source: 'api',
      }
    } catch {
      return mockBook(marketId)
    }
  }

  async getTrades(marketId: string, limit = 20): Promise<{ items: TradeRecord[]; source: ApiSource }> {
    try {
      const payload = await fetchJSON<unknown>(`/v1/trades?market_id=${encodeURIComponent(marketId)}&limit=${limit}`)
      const items = unwrapArrayPayload(payload).map((entry) => ({
        id: String(entry.id ?? `${Date.now()}-${Math.random()}`),
        marketId: String(entry.market_id ?? marketId),
        outcome: parseNumber(entry.outcome),
        price: parseNumber(entry.price),
        amount: parseNumber(entry.amount),
        buyer: entry.buyer ? String(entry.buyer) : undefined,
        seller: entry.seller ? String(entry.seller) : undefined,
        timestamp: String(entry.timestamp ?? new Date().toISOString()),
      }))
      return { items: items.length > 0 ? items : mockTrades(marketId), source: items.length > 0 ? 'api' : 'mock' }
    } catch {
      return { items: mockTrades(marketId), source: 'mock' }
    }
  }

  async getBalances(userId = this.currentUserId(), actor?: string): Promise<BalanceRecord[]> {
    try {
      const payload = await fetchJSON<unknown>(`/v1/balances?user_id=${encodeURIComponent(userId)}`, undefined, actor)
      return unwrapArrayPayload(payload).map((entry) => ({
        userId: String(entry.user_id ?? userId),
        asset: String(entry.asset ?? 'USDC'),
        available: parseNumber(entry.available),
        hold: parseNumber(entry.hold),
        source: 'api',
      }))
    } catch {
      return mockBalances(userId)
    }
  }

  async listOtcQuotes(actor?: string): Promise<OtcQuoteRecord[]> {
    try {
      const payload = await fetchJSON<unknown>('/v1/otc/quotes', undefined, actor)
      return unwrapArrayPayload(payload).map((entry) => ({
        quoteId: String(entry.quote_id ?? entry.id ?? ''),
        marketId: String(entry.market_id ?? 'otc:btc-usdt:block'),
        settlementMarketId: String(entry.settlement_market_id ?? ''),
        requesterUserId: String(entry.requester_user_id ?? ''),
        counterpartyUserId: entry.counterparty_user_id ? String(entry.counterparty_user_id) : undefined,
        side: String(entry.side ?? 'sell').toLowerCase(),
        price: parseNumber(entry.price),
        amount: parseNumber(entry.amount),
        outcome: parseNumber(entry.outcome),
        status: String(entry.status ?? 'open'),
        createdAt: String(entry.created_at ?? new Date().toISOString()),
        acceptedAt: entry.accepted_at ? String(entry.accepted_at) : undefined,
      }))
    } catch {
      return []
    }
  }

  async acceptOtcQuote(quoteId: string, actor?: string): Promise<SubmitOrderResult> {
    try {
      const payload = await fetchJSON<Record<string, unknown>>(
        `/v1/otc/quotes/${encodeURIComponent(quoteId)}/accept`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
          },
          body: '{}',
        },
        actor,
      )
      return {
        ok: true,
        source: 'api',
        message: 'OTC quote accepted by backend',
        intentId: payload.quote_id ? String(payload.quote_id) : quoteId,
        raw: payload,
      }
    } catch (error) {
      if (!isNetworkError(error)) {
        const apiError = error as ApiRequestError
        return {
          ok: false,
          source: 'api',
          message: apiError.message,
          raw: apiError.payload,
        }
      }
      return {
        ok: false,
        source: 'mock',
        message: error instanceof Error ? error.message : 'OTC accept failed',
      }
    }
  }

  async getEarnPositions(userId = this.currentUserId(), actor?: string): Promise<EarnPositionRecord[]> {
    try {
      const payload = await fetchJSON<unknown>(`/v1/earn/positions?user_id=${encodeURIComponent(userId)}`, undefined, actor)
      return unwrapArrayPayload(payload).map((entry) => ({
        positionId: String(entry.position_id ?? ''),
        userId: String(entry.user_id ?? userId),
        productId: String(entry.product_id ?? 'earn:usdc:flex'),
        asset: String(entry.asset ?? 'USDC'),
        principalAmount: parseNumber(entry.principal_amount),
        aprBps: parseNumber(entry.apr_bps),
        createdAt: String(entry.created_at ?? new Date().toISOString()),
        updatedAt: String(entry.updated_at ?? new Date().toISOString()),
      }))
    } catch {
      return []
    }
  }

  async getOrders(userId = this.currentUserId(), marketId?: string, actor?: string): Promise<OpenOrderRecord[]> {
    try {
      const query = new URLSearchParams({ user_id: userId })
      if (marketId) {
        query.set('market_id', marketId)
      }
      const payload = await fetchJSON<unknown>(`/v1/orders?${query.toString()}`, undefined, actor)
      return unwrapArrayPayload(payload).map((entry) => ({
        id: String(entry.id ?? ''),
        marketId: String(entry.market_id ?? marketId ?? 'unknown'),
        outcome: parseNumber(entry.outcome),
        side: String(entry.side ?? 'buy').toLowerCase(),
        price: parseNumber(entry.price),
        amount: parseNumber(entry.amount),
        filled: parseNumber(entry.filled),
        remaining: parseNumber(entry.remaining),
        status: String(entry.status ?? 'open'),
        createdAt: String(entry.created_at ?? new Date().toISOString()),
        source: 'api',
      }))
    } catch {
      return mockOrders(marketId)
    }
  }

  async submitOrder(input: SubmitOrderInput, actor?: string): Promise<SubmitOrderResult> {
    try {
      let path = '/v1/intents'
      let successMessage = 'Real order accepted by backend'
      let body: Record<string, unknown> = {
        user_id: input.userId,
        market_id: input.marketId,
        side: input.side,
        price: Math.round(input.price),
        amount: Math.round(input.amount),
        outcome: input.outcome ?? 0,
        expires_in: input.expiresIn ?? 0,
      }

      if (input.marketId.startsWith('otc:')) {
        path = '/v1/otc/quotes'
        successMessage = 'OTC quote request accepted by backend'
        body = {
          user_id: input.userId,
          market_id: input.marketId,
          side: input.side,
          price: Math.round(input.price),
          amount: Math.round(input.amount),
          outcome: input.outcome ?? 0,
        }
      } else if (input.marketId.startsWith('earn:')) {
        path = input.side === 'buy' ? '/v1/earn/subscribe' : '/v1/earn/redeem'
        successMessage = input.side === 'buy' ? 'Earn subscribe accepted by backend' : 'Earn redeem accepted by backend'
        body = {
          user_id: input.userId,
          product_id: input.marketId,
          amount: Math.round(input.amount),
        }
      }

      const payload = await fetchJSON<Record<string, unknown>>(
        path,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
          },
          body: JSON.stringify(body),
        },
        actor,
      )

      return {
        ok: true,
        source: 'api',
        message: successMessage,
        intentId: payload.intent_id
          ? String(payload.intent_id)
          : payload.order_id
            ? String(payload.order_id)
            : payload.quote_id
              ? String(payload.quote_id)
              : payload.position_id
                ? String(payload.position_id)
                : undefined,
        raw: payload,
      }
    } catch (error) {
      if (!isNetworkError(error)) {
        const apiError = error as ApiRequestError
        return {
          ok: false,
          source: 'api',
          message: apiError.message,
          raw: apiError.payload,
        }
      }

      return {
        ok: false,
        source: 'mock',
        message: error instanceof Error ? `Backend unavailable, local mock was not submitted: ${error.message}` : 'Backend unavailable, local mock was not submitted',
      }
    }
  }

  async cancelOrder(orderId: string, marketId: string, outcome = 0, actor?: string): Promise<CancelOrderResult> {
    try {
      const payload = await fetchJSON<Record<string, unknown>>(
        `/v1/orders/${encodeURIComponent(orderId)}/cancel?market_id=${encodeURIComponent(marketId)}&outcome=${outcome}`,
        { method: 'POST' },
        actor,
      )
      return {
        ok: true,
        source: 'api',
        message: 'Real cancel accepted by backend',
        raw: payload,
      }
    } catch (error) {
      if (!isNetworkError(error)) {
        const apiError = error as ApiRequestError
        return {
          ok: false,
          source: 'api',
          message: apiError.message,
          raw: apiError.payload,
        }
      }

      return {
        ok: false,
        source: 'mock',
        message: error instanceof Error ? `Backend unavailable, local mock cancel was not executed: ${error.message}` : 'Backend unavailable, local mock cancel was not executed',
      }
    }
  }

  async getSystemStatus(): Promise<SystemEndpointStatus[]> {
    const currentUser = this.currentUserId()
    const endpoints = [
      { name: 'Frontend', url: window.location.origin, probe: Promise.resolve({ ok: true, details: 'vite dev server' }) },
      { name: 'Go API', url: '/health', probe: this.probe('/health') },
      { name: 'Markets API', url: '/v1/markets', probe: this.probe('/v1/markets') },
      { name: 'Authenticated Read', url: `/v1/balances?user_id=${encodeURIComponent(currentUser)}`, probe: this.probe(`/v1/balances?user_id=${encodeURIComponent(currentUser)}`) },
      { name: 'Rust Admin API', url: '/admin/instruments', probe: this.probe('/admin/instruments', 'admin') },
    ]

    return Promise.all(
      endpoints.map(async (endpoint) => {
        const startedAt = performance.now()
        try {
          const result = await endpoint.probe
          return {
            name: endpoint.name,
            url: endpoint.url,
            status: result.ok ? 'running' : 'unavailable',
            latencyMs: Math.round(performance.now() - startedAt),
            details: result.details,
          } satisfies SystemEndpointStatus
        } catch (error) {
          return {
            name: endpoint.name,
            url: endpoint.url,
            status: 'unavailable',
            latencyMs: Math.round(performance.now() - startedAt),
            details: error instanceof Error ? error.message : 'probe failed',
          } satisfies SystemEndpointStatus
        }
      }),
    )
  }

  async listAdminInstruments(): Promise<Record<string, unknown>[]> {
    const payload = await fetchJSON<Record<string, unknown>>(`${ADMIN_API_BASE}/admin/instruments`, undefined, 'admin')
    return Array.isArray(payload.items) ? (payload.items as Record<string, unknown>[]) : []
  }

  async setKillSwitch(enabled: boolean): Promise<AdminActionResult> {
    return this.postAdmin('/admin/kill-switch', { enabled, request_id: `ui-kill-${Date.now()}` })
  }

  async setMarketState(marketId: string, state: string, outcome = 0): Promise<AdminActionResult> {
    return this.postAdmin('/admin/market-state', {
      request_id: `ui-market-state-${Date.now()}`,
      market_id: marketId,
      outcome,
      state,
    })
  }

  async listFundingRates(marketId?: string): Promise<Record<string, unknown>[]> {
    const query = new URLSearchParams()
    if (marketId) {
      query.set('market_id', marketId)
    }
    const suffix = query.size > 0 ? `?${query.toString()}` : ''
    const payload = await fetchJSON<Record<string, unknown>>(`${ADMIN_API_BASE}/admin/risk/funding-rates${suffix}`, undefined, 'admin')
    return Array.isArray(payload.items) ? (payload.items as Record<string, unknown>[]) : []
  }

  async upsertFundingRate(marketId: string, fundingRatePpm: number, outcome = 0): Promise<AdminActionResult> {
    return this.postAdmin('/admin/risk/funding-rates', {
      market_id: marketId,
      outcome,
      funding_rate_ppm: Math.round(fundingRatePpm),
    })
  }

  async listRiskEvents(limit = 20): Promise<Record<string, unknown>[]> {
    const payload = await fetchJSON<Record<string, unknown>>(`${ADMIN_API_BASE}/admin/risk/events?limit=${limit}`, undefined, 'admin')
    return Array.isArray(payload.items) ? (payload.items as Record<string, unknown>[]) : []
  }

  async listGovernanceActions(limit = 20, status?: string): Promise<GovernanceActionRecord[]> {
    const query = new URLSearchParams({ limit: String(limit) })
    if (status) {
      query.set('status', status)
    }
    const payload = await fetchJSON<Record<string, unknown>>(`${ADMIN_API_BASE}/admin/risk/governance/actions?${query.toString()}`, undefined, 'admin')
    return Array.isArray(payload.items) ? (payload.items as GovernanceActionRecord[]) : []
  }

  async approveGovernanceAction(actionId: string, actor = 'admin'): Promise<AdminActionResult> {
    try {
      const payload = await fetchJSON<Record<string, unknown>>(
        `${ADMIN_API_BASE}/admin/risk/governance/actions/${encodeURIComponent(actionId)}/approve`,
        { method: 'POST' },
        actor,
      )
      return { ok: true, message: 'Governance action approved', raw: payload }
    } catch (error) {
      if (error instanceof ApiRequestError) {
        return { ok: false, message: error.message, raw: error.payload }
      }
      return { ok: false, message: error instanceof Error ? error.message : 'Governance approve failed' }
    }
  }

  async rejectGovernanceAction(actionId: string, actor = 'admin'): Promise<AdminActionResult> {
    try {
      const payload = await fetchJSON<Record<string, unknown>>(
        `${ADMIN_API_BASE}/admin/risk/governance/actions/${encodeURIComponent(actionId)}/reject`,
        { method: 'POST' },
        actor,
      )
      return { ok: true, message: 'Governance action rejected', raw: payload }
    } catch (error) {
      if (error instanceof ApiRequestError) {
        return { ok: false, message: error.message, raw: error.payload }
      }
      return { ok: false, message: error instanceof Error ? error.message : 'Governance reject failed' }
    }
  }

  async getBalance(userId: string): Promise<number> {
    const balances = await this.getBalances(userId)
    return balances.reduce((total, item) => total + item.available, 0)
  }

  async getMarketData(marketId: string): Promise<{ price: number; volume: number }> {
    const [book, trades] = await Promise.all([this.getOrderBook(marketId), this.getTrades(marketId, 20)])
    const price = book.asks[0]?.price ?? book.bids[0]?.price ?? 0
    const volume = trades.items.reduce((total, item) => total + item.amount, 0)
    return { price, volume }
  }

  async getFills(userId = this.currentUserId()): Promise<Fill[]> {
    const trades = await this.getTrades('btc-usdt', 12)
    return trades.items.map((trade, index) => ({
      id: trade.id,
      user: trade.buyer === userId ? userId : trade.seller === userId ? userId : index % 2 === 0 ? userId : 'counterparty',
      side: trade.buyer === userId ? 'Buy' : 'Sell',
      price: trade.price,
      amount: trade.amount,
      timestamp: trade.timestamp,
    }))
  }

  async submitIntent(intent: Intent): Promise<boolean> {
    const result = await this.submitOrder(
      {
        userId: intent.user_id,
        marketId: intent.market_id,
        side: intent.side.toLowerCase() as 'buy' | 'sell',
        price: intent.price,
        amount: intent.amount,
        outcome: intent.outcome,
      },
      intent.user_id,
    )
    return result.ok
  }

  private async postAdmin(path: string, body: Record<string, unknown>): Promise<AdminActionResult> {
    try {
      const payload = await fetchJSON<Record<string, unknown>>(
        `${ADMIN_API_BASE}${path}`,
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        },
        'admin',
      )
      return { ok: true, message: 'Admin action accepted', raw: payload }
    } catch (error) {
      if (error instanceof ApiRequestError) {
        return { ok: false, message: error.message, raw: error.payload }
      }
      return { ok: false, message: error instanceof Error ? error.message : 'Admin request failed' }
    }
  }

  private async probe(url: string, actor?: string): Promise<{ ok: boolean; details?: string }> {
    const response = await fetch(url, { headers: { Accept: 'application/json', ...buildAuthHeaders(actor) } })
    if (!response.ok) {
      return { ok: false, details: `HTTP ${response.status}` }
    }
    return { ok: true }
  }
}

export const exchangeAPI = new ExchangeAPI()
