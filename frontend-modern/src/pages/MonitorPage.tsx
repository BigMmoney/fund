import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { JsonPanel } from '@/components/JsonPanel'
import { Panel } from '@/components/Panel'
import {
  ApiError,
  asList,
  asRecord,
  createExchangeApi,
  type AuthConfig,
  type JsonRecord,
} from '@/services/exchangeApi'

interface PageProps {
  auth: AuthConfig
  onNotice: (message: string) => void
}

const STAGE_OPTIONS = [
  '',
  'api_received',
  'api_validated',
  'api_rejected',
  'sequencer_accepted',
  'sequencer_persisted',
  'wal_appended',
  'matching_resting',
  'matching_partially_filled',
  'matching_filled',
  'matching_cancelled',
  'projection_updated',
  'ledger_settled',
  'recovery_replayed',
  'recovery_skipped_terminal',
  'recovery_completed',
] as const

const TERMINAL_OPTIONS = [
  { label: 'all', value: '' },
  { label: 'open', value: 'false' },
  { label: 'terminal', value: 'true' },
] as const

const REFRESH_INTERVALS_MS = [0, 2000, 5000, 15000] as const

function fmtIso(value: unknown): string {
  if (typeof value !== 'string') return '-'
  // Trim sub-second precision for a denser table.
  const t = value.replace(/\.\d+/, '').replace('Z', '')
  return t || '-'
}

function readText(value: unknown, fallback = '-'): string {
  if (typeof value === 'string' && value.length > 0) return value
  if (typeof value === 'number') return String(value)
  if (typeof value === 'boolean') return String(value)
  return fallback
}

export function MonitorPage({ auth, onNotice }: PageProps) {
  const api = useMemo(() => createExchangeApi(auth), [auth])

  // List + filters
  const [orders, setOrders] = useState<JsonRecord[]>([])
  const [meta, setMeta] = useState<{ total_returned?: number }>({})
  const [filterUserId, setFilterUserId] = useState('')
  const [filterMarketId, setFilterMarketId] = useState('')
  const [filterStage, setFilterStage] = useState('')
  const [filterTerminal, setFilterTerminal] = useState('')
  const [filterLimit, setFilterLimit] = useState('100')
  const [refreshMs, setRefreshMs] = useState<number>(2000)

  // Selected order + timeline
  const [selectedOrderId, setSelectedOrderId] = useState('')
  const [orderSummary, setOrderSummary] = useState<JsonRecord | null>(null)
  const [timeline, setTimeline] = useState<JsonRecord[]>([])
  const [timelineMeta, setTimelineMeta] = useState<JsonRecord | null>(null)

  const [busy, setBusy] = useState(false)
  const [lastError, setLastError] = useState<string | null>(null)
  const refreshTimer = useRef<number | null>(null)

  const fetchOrders = useCallback(async () => {
    try {
      setBusy(true)
      setLastError(null)
      const resp = await api.listMonitorOrders({
        userId: filterUserId.trim() || undefined,
        marketId: filterMarketId.trim() || undefined,
        stage: filterStage || undefined,
        terminal:
          filterTerminal === '' ? undefined : filterTerminal === 'true',
        limit: Number.isFinite(Number(filterLimit)) ? Number(filterLimit) : undefined,
      })
      const record = asRecord(resp)
      setOrders(asList(record.orders))
      setMeta({
        total_returned:
          typeof record.total_returned === 'number' ? record.total_returned : undefined,
      })
    } catch (error) {
      if (error instanceof ApiError) {
        setLastError(`HTTP ${error.status}: ${error.message}`)
      } else {
        setLastError(String(error))
      }
    } finally {
      setBusy(false)
    }
  }, [api, filterUserId, filterMarketId, filterStage, filterTerminal, filterLimit])

  const fetchTimeline = useCallback(
    async (orderId: string) => {
      if (!orderId) return
      try {
        setBusy(true)
        setLastError(null)
        const [summaryResp, timelineResp] = await Promise.all([
          api.getMonitorOrder(orderId),
          api.getMonitorTimeline(orderId, undefined, 200),
        ])
        setOrderSummary(asRecord(summaryResp))
        const tlRecord = asRecord(timelineResp)
        setTimeline(asList(tlRecord.timeline))
        setTimelineMeta(tlRecord)
      } catch (error) {
        if (error instanceof ApiError) {
          if (error.status === 404) {
            // The order may have been evicted from the projector ring.
            setOrderSummary(null)
            setTimeline([])
            setTimelineMeta(null)
            setLastError(`order ${orderId} not found (may be evicted from monitor ring)`)
          } else {
            setLastError(`HTTP ${error.status}: ${error.message}`)
          }
        } else {
          setLastError(String(error))
        }
      } finally {
        setBusy(false)
      }
    },
    [api],
  )

  useEffect(() => {
    void fetchOrders()
  }, [fetchOrders])

  useEffect(() => {
    if (refreshMs <= 0) {
      if (refreshTimer.current !== null) {
        window.clearInterval(refreshTimer.current)
        refreshTimer.current = null
      }
      return
    }
    refreshTimer.current = window.setInterval(() => {
      void fetchOrders()
      if (selectedOrderId) {
        void fetchTimeline(selectedOrderId)
      }
    }, refreshMs)
    return () => {
      if (refreshTimer.current !== null) {
        window.clearInterval(refreshTimer.current)
        refreshTimer.current = null
      }
    }
  }, [refreshMs, fetchOrders, fetchTimeline, selectedOrderId])

  const handleSelectOrder = useCallback(
    (orderId: string) => {
      setSelectedOrderId(orderId)
      onNotice(`Loading timeline for ${orderId}`)
      void fetchTimeline(orderId)
    },
    [fetchTimeline, onNotice],
  )

  return (
    <div className="page-stack">
      <Panel
        title="Order Flow Monitor"
        subtitle="Live view of orders flowing through the backend (docs/MONITOR_DESIGN.md). Polling-only; WebSocket streaming is a future step."
        actions={
          <>
            <button
              type="button"
              className="button button-secondary"
              onClick={() => void fetchOrders()}
              disabled={busy}
            >
              Refresh
            </button>
            <select
              value={String(refreshMs)}
              onChange={(event) => setRefreshMs(Number(event.target.value))}
              className="button button-secondary"
            >
              {REFRESH_INTERVALS_MS.map((ms) => (
                <option key={ms} value={ms}>
                  {ms === 0 ? 'manual' : `auto ${ms / 1000}s`}
                </option>
              ))}
            </select>
          </>
        }
      >
        <div className="form-grid">
          <label className="field">
            <span>User ID</span>
            <input
              value={filterUserId}
              onChange={(event) => setFilterUserId(event.target.value)}
              placeholder={auth.role === 'admin' ? '(any user)' : `${auth.subject} (forced)`}
              disabled={auth.role !== 'admin'}
            />
          </label>
          <label className="field">
            <span>Market ID</span>
            <input
              value={filterMarketId}
              onChange={(event) => setFilterMarketId(event.target.value)}
              placeholder="btc-usdt"
            />
          </label>
          <label className="field">
            <span>Stage</span>
            <select
              value={filterStage}
              onChange={(event) => setFilterStage(event.target.value)}
            >
              {STAGE_OPTIONS.map((stage) => (
                <option key={stage} value={stage}>
                  {stage === '' ? 'any stage' : stage}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>Terminal</span>
            <select
              value={filterTerminal}
              onChange={(event) => setFilterTerminal(event.target.value)}
            >
              {TERMINAL_OPTIONS.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>Limit</span>
            <input
              value={filterLimit}
              onChange={(event) => setFilterLimit(event.target.value)}
              placeholder="100"
            />
          </label>
        </div>

        {lastError ? (
          <div className="error-banner" role="alert">
            {lastError}
          </div>
        ) : null}

        <div className="table-wrap">
          <table className="data-table">
            <thead>
              <tr>
                <th>order_id</th>
                <th>user</th>
                <th>market</th>
                <th>stage</th>
                <th>cmd_seq</th>
                <th>remaining</th>
                <th>fills</th>
                <th>terminal</th>
                <th>last_updated</th>
              </tr>
            </thead>
            <tbody>
              {orders.length === 0 ? (
                <tr>
                  <td colSpan={9} className="muted">
                    {busy ? 'loading…' : 'no orders match the current filters'}
                  </td>
                </tr>
              ) : (
                orders.map((order) => {
                  const orderId = readText(order.order_id)
                  const isSelected = orderId === selectedOrderId
                  return (
                    <tr
                      key={orderId}
                      onClick={() => handleSelectOrder(orderId)}
                      className={isSelected ? 'row-selected' : undefined}
                      style={{ cursor: 'pointer' }}
                    >
                      <td>{orderId}</td>
                      <td>{readText(order.user_id)}</td>
                      <td>{readText(order.market_id)}</td>
                      <td>{readText(order.current_stage)}</td>
                      <td>{readText(order.command_seq)}</td>
                      <td>{readText(order.remaining_amount)}</td>
                      <td>{readText(order.fill_count)}</td>
                      <td>{readText(order.terminal)}</td>
                      <td>{fmtIso(order.last_updated_at)}</td>
                    </tr>
                  )
                })
              )}
            </tbody>
          </table>
        </div>
        <div className="muted">
          total_returned={readText(meta.total_returned)} · auto-refresh{' '}
          {refreshMs === 0 ? 'off' : `${refreshMs / 1000}s`}
          {auth.role !== 'admin' ? ' · non-admin: filter forced to your subject' : ''}
        </div>
      </Panel>

      <Panel
        title={selectedOrderId ? `Timeline · ${selectedOrderId}` : 'Timeline'}
        subtitle={
          selectedOrderId
            ? 'Click another row above to switch orders. Auto-refresh updates the selected timeline too.'
            : 'Click an order in the table above to load its full per-stage timeline.'
        }
      >
        {!selectedOrderId ? (
          <div className="muted">No order selected.</div>
        ) : orderSummary === null ? (
          <div className="muted">{busy ? 'loading…' : 'order not available'}</div>
        ) : (
          <>
            <div className="form-grid">
              <JsonPanel title="summary" value={orderSummary} />
              <JsonPanel title="timeline meta" value={timelineMeta} />
            </div>
            <div className="table-wrap">
              <table className="data-table">
                <thead>
                  <tr>
                    <th>recorded_at</th>
                    <th>stage</th>
                    <th>cmd_seq</th>
                    <th>side</th>
                    <th>price</th>
                    <th>amount</th>
                    <th>remaining</th>
                    <th>filled</th>
                    <th>fee</th>
                    <th>reject</th>
                  </tr>
                </thead>
                <tbody>
                  {timeline.length === 0 ? (
                    <tr>
                      <td colSpan={10} className="muted">
                        no timeline events
                      </td>
                    </tr>
                  ) : (
                    timeline.map((ev) => (
                      <tr key={readText(ev.event_id)}>
                        <td>{fmtIso(ev.recorded_at)}</td>
                        <td>{readText(ev.stage)}</td>
                        <td>{readText(ev.command_seq)}</td>
                        <td>{readText(ev.side)}</td>
                        <td>{readText(ev.price)}</td>
                        <td>{readText(ev.amount)}</td>
                        <td>{readText(ev.remaining_amount)}</td>
                        <td>{readText(ev.filled_amount)}</td>
                        <td>{readText(ev.fee)}</td>
                        <td>
                          {readText(ev.reject_code) !== '-'
                            ? `${readText(ev.reject_code)}: ${readText(ev.reject_message)}`
                            : '-'}
                        </td>
                      </tr>
                    ))
                  )}
                </tbody>
              </table>
            </div>
          </>
        )}
      </Panel>
    </div>
  )
}
