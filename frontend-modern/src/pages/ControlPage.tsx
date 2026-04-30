import { useCallback, useEffect, useMemo, useState } from 'react'
import { JsonPanel } from '@/components/JsonPanel'
import { Panel } from '@/components/Panel'
import { ApiError, asList, asRecord, createExchangeApi, type AuthConfig, type JsonRecord } from '@/services/exchangeApi'

interface PageProps {
  auth: AuthConfig
  onNotice: (message: string) => void
}

function parseNumber(value: string): number | null {
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : null
}

function text(value: unknown, fallback = '-'): string {
  if (typeof value === 'string' && value.length > 0) return value
  if (typeof value === 'number') return String(value)
  if (typeof value === 'boolean') return String(value)
  return fallback
}

export function ControlPage({ auth, onNotice }: PageProps) {
  const api = useMemo(() => createExchangeApi(auth), [auth])
  const [loading, setLoading] = useState(false)

  const [planes, setPlanes] = useState<unknown>(null)
  const [opsNode, setOpsNode] = useState<unknown>(null)
  const [betaControl, setBetaControl] = useState<unknown>(null)
  const [betaUsers, setBetaUsers] = useState<JsonRecord[]>([])
  const [betaMarkets, setBetaMarkets] = useState<JsonRecord[]>([])
  const [riskEvents, setRiskEvents] = useState<JsonRecord[]>([])
  const [governanceActions, setGovernanceActions] = useState<JsonRecord[]>([])
  const [adminAudit, setAdminAudit] = useState<JsonRecord[]>([])
  const [treasuryFeeCollector, setTreasuryFeeCollector] = useState<unknown>(null)
  const [insuranceFunds, setInsuranceFunds] = useState<unknown>(null)
  const [pendingWithdrawals, setPendingWithdrawals] = useState<JsonRecord[]>([])
  const [custodyAudit, setCustodyAudit] = useState<unknown>(null)
  const [custodyAuditEvents, setCustodyAuditEvents] = useState<JsonRecord[]>([])
  const [custodyBreaker, setCustodyBreaker] = useState<unknown>(null)
  const [releaseChecklist, setReleaseChecklist] = useState<unknown>(null)
  const [releaseVersion, setReleaseVersion] = useState<unknown>(null)
  const [releaseFeatures, setReleaseFeatures] = useState<unknown>(null)
  const [failpoints, setFailpoints] = useState<unknown>(null)
  const [lastResponse, setLastResponse] = useState<unknown>(null)

  const [marketId, setMarketId] = useState('btc-usdt')
  const [marketState, setMarketState] = useState('trading')
  const [fundingMarketId, setFundingMarketId] = useState('perp:btc-usdt')
  const [fundingRatePpm, setFundingRatePpm] = useState('125')
  const [depositUser, setDepositUser] = useState('trader-001')
  const [depositAmount, setDepositAmount] = useState('100000000')
  const [betaEnabled, setBetaEnabled] = useState(true)
  const [betaRequireWhitelist, setBetaRequireWhitelist] = useState(true)
  const [betaUserId, setBetaUserId] = useState('trader-001')
  const [betaUserWhitelisted, setBetaUserWhitelisted] = useState(true)
  const [betaUserMaxCash, setBetaUserMaxCash] = useState('100000000')
  const [betaUserMaxOrders, setBetaUserMaxOrders] = useState('50')
  const [betaMarketId, setBetaMarketId] = useState('btc-usdt')
  const [betaMarketMaxNotional, setBetaMarketMaxNotional] = useState('500000000')
  const [betaMarketMaxLeverage, setBetaMarketMaxLeverage] = useState('10')
  const [approvalWithdrawalId, setApprovalWithdrawalId] = useState('')
  const [releaseTargetVersion, setReleaseTargetVersion] = useState('0.1.0')
  const [failpointName, setFailpointName] = useState('match_timeout')
  const [failpointMode, setFailpointMode] = useState('delay')
  const [failpointDelayUs, setFailpointDelayUs] = useState('1000')
  const [failpointMessage, setFailpointMessage] = useState('ui injected failure')

  const load = useCallback(async () => {
    setLoading(true)
    const results = await Promise.allSettled([
      api.getPlanes(),
      api.getOpsNode(),
      api.getBetaControlPlane(),
      api.listBetaUsers(),
      api.listBetaMarkets(),
      api.getRiskEvents(20),
      api.getGovernanceActions(20),
      api.getAdminAudit(20),
      api.getTreasuryFeeCollector(),
      api.getTreasuryInsuranceFunds(),
      api.getPendingWithdrawals(),
      api.getCustodyAudit(),
      api.getCustodyAuditEvents(),
      api.getCustodyBreaker(),
      api.getReleaseChecklist(),
      api.getReleaseVersion(releaseTargetVersion),
      api.getReleaseFeatures(),
      api.getFailpoints(),
    ])

    const [
      planesResult,
      opsNodeResult,
      betaControlResult,
      betaUsersResult,
      betaMarketsResult,
      riskEventsResult,
      governanceResult,
      auditResult,
      feeResult,
      insuranceResult,
      pendingWithdrawalsResult,
      custodyAuditResult,
      custodyAuditEventsResult,
      custodyBreakerResult,
      releaseChecklistResult,
      releaseVersionResult,
      releaseFeaturesResult,
      failpointsResult,
    ] = results

    if (planesResult.status === 'fulfilled') setPlanes(planesResult.value)
    if (opsNodeResult.status === 'fulfilled') setOpsNode(opsNodeResult.value)
    if (betaControlResult.status === 'fulfilled') {
      setBetaControl(betaControlResult.value)
      const betaRecord = asRecord(betaControlResult.value)
      const controlPlane = asRecord(betaRecord.control_plane)
      if (typeof controlPlane.enabled === 'boolean') setBetaEnabled(controlPlane.enabled)
      if (typeof controlPlane.require_whitelist === 'boolean') setBetaRequireWhitelist(controlPlane.require_whitelist)
    }
    if (betaUsersResult.status === 'fulfilled') setBetaUsers(asList(betaUsersResult.value))
    if (betaMarketsResult.status === 'fulfilled') setBetaMarkets(asList(betaMarketsResult.value))
    if (riskEventsResult.status === 'fulfilled') setRiskEvents(asList(riskEventsResult.value))
    if (governanceResult.status === 'fulfilled') setGovernanceActions(asList(governanceResult.value))
    if (auditResult.status === 'fulfilled') setAdminAudit(asList(auditResult.value))
    if (feeResult.status === 'fulfilled') setTreasuryFeeCollector(feeResult.value)
    if (insuranceResult.status === 'fulfilled') setInsuranceFunds(insuranceResult.value)
    if (pendingWithdrawalsResult.status === 'fulfilled') setPendingWithdrawals(asList(pendingWithdrawalsResult.value))
    if (custodyAuditResult.status === 'fulfilled') setCustodyAudit(custodyAuditResult.value)
    if (custodyAuditEventsResult.status === 'fulfilled') setCustodyAuditEvents(asList(custodyAuditEventsResult.value))
    if (custodyBreakerResult.status === 'fulfilled') setCustodyBreaker(custodyBreakerResult.value)
    if (releaseChecklistResult.status === 'fulfilled') setReleaseChecklist(releaseChecklistResult.value)
    if (releaseVersionResult.status === 'fulfilled') setReleaseVersion(releaseVersionResult.value)
    if (releaseFeaturesResult.status === 'fulfilled') setReleaseFeatures(releaseFeaturesResult.value)
    if (failpointsResult.status === 'fulfilled') setFailpoints(failpointsResult.value)

    const failedCount = results.filter((item) => item.status === 'rejected').length
    onNotice(failedCount > 0 ? `Control page refreshed with ${failedCount} backend call failures.` : 'Control page refreshed from live backend.')
    setLoading(false)
  }, [api, onNotice, releaseTargetVersion])

  useEffect(() => {
    void load()
  }, [load])

  async function run(task: Promise<unknown>, successMessage: string) {
    try {
      const response = await task
      setLastResponse(response)
      onNotice(successMessage)
      await load()
    } catch (error) {
      const message = error instanceof ApiError ? `${error.message} (${error.status})` : error instanceof Error ? error.message : 'Admin action failed'
      onNotice(message)
    }
  }

  const planeRecord = asRecord(planes)
  const opsRecord = asRecord(opsNode)
  const betaRecord = asRecord(asRecord(betaControl).control_plane)
  const failpointRecord = asRecord(failpoints)
  const failpointList = asList(failpointRecord.failpoints)

  return (
    <div className="page-grid">
      <section className="stat-grid">
        <div className="stat-card">
          <span>Kill switch</span>
          <strong>{text(planeRecord.kill_switch_status, 'unknown')}</strong>
        </div>
        <div className="stat-card">
          <span>Drain mode</span>
          <strong>{text(opsRecord.drain_mode, 'unknown')}</strong>
        </div>
        <div className="stat-card">
          <span>Beta enabled</span>
          <strong>{text(betaRecord.enabled, 'unknown')}</strong>
        </div>
        <div className="stat-card">
          <span>Pending withdrawals</span>
          <strong>{pendingWithdrawals.length}</strong>
        </div>
      </section>

      <div className="two-column-grid">
        <Panel title="Operator Controls" subtitle="Traffic posture, plane reset, checkpoints, and basic market operations.">
          <div className="button-row button-row-wrap">
            <button type="button" className="button button-primary" onClick={() => void run(api.setKillSwitch(true), 'Kill switch enable request submitted.')}>
              Enable Kill Switch
            </button>
            <button type="button" className="button button-secondary" onClick={() => void run(api.setKillSwitch(false), 'Kill switch disable request submitted.')}>
              Disable Kill Switch
            </button>
            <button type="button" className="button button-secondary" onClick={() => void run(api.setDrain(true), 'Drain mode enabled.')}>
              Enable Drain
            </button>
            <button type="button" className="button button-secondary" onClick={() => void run(api.setDrain(false), 'Drain mode disabled.')}>
              Disable Drain
            </button>
            <button type="button" className="button button-secondary" onClick={() => void run(api.checkpoint(), 'Checkpoint triggered.')}>
              Checkpoint
            </button>
            <button type="button" className="button button-secondary" onClick={() => void run(api.resetPlanes('data'), 'Data plane reset submitted.')}>
              Reset Data Plane
            </button>
            <button type="button" className="button button-secondary" onClick={() => void run(api.resetPlanes('control'), 'Control plane reset submitted.')}>
              Reset Control Plane
            </button>
          </div>

          <div className="form-grid stacked-gap">
            <label className="field">
              <span>Market ID</span>
              <input value={marketId} onChange={(event) => setMarketId(event.target.value)} />
            </label>
            <label className="field">
              <span>Market State</span>
              <input value={marketState} onChange={(event) => setMarketState(event.target.value)} />
            </label>
            <label className="field field-span-2">
              <span>Funding Market</span>
              <input value={fundingMarketId} onChange={(event) => setFundingMarketId(event.target.value)} />
            </label>
            <label className="field">
              <span>Funding Rate PPM</span>
              <input value={fundingRatePpm} onChange={(event) => setFundingRatePpm(event.target.value)} />
            </label>
            <label className="field">
              <span>Deposit User</span>
              <input value={depositUser} onChange={(event) => setDepositUser(event.target.value)} />
            </label>
            <label className="field">
              <span>Deposit Amount</span>
              <input value={depositAmount} onChange={(event) => setDepositAmount(event.target.value)} />
            </label>
          </div>

          <div className="button-row button-row-wrap">
            <button type="button" className="button button-primary" onClick={() => void run(api.setMarketState(marketId, marketState), `Market state update submitted for ${marketId}.`)}>
              Update Market State
            </button>
            <button
              type="button"
              className="button button-secondary"
              onClick={() => {
                const ppm = parseNumber(fundingRatePpm)
                if (ppm === null) {
                  onNotice('Funding rate must be a number.')
                  return
                }
                void run(api.upsertFundingRate(fundingMarketId, ppm), `Funding rate update submitted for ${fundingMarketId}.`)
              }}
            >
              Upsert Funding Rate
            </button>
            <button
              type="button"
              className="button button-secondary"
              onClick={() => {
                const amount = parseNumber(depositAmount)
                if (amount === null) {
                  onNotice('Deposit amount must be a number.')
                  return
                }
                void run(api.deposit({ user_id: depositUser, amount, op_id: `ui-${Date.now()}` }), `Deposit request submitted for ${depositUser}.`)
              }}
            >
              Deposit User Cash
            </button>
          </div>
        </Panel>

        <Panel title="Closed Beta Controls" subtitle="Global gate plus user and market overlays for the closed beta program.">
          <div className="form-grid">
            <label className="field">
              <span>Beta Enabled</span>
              <select value={String(betaEnabled)} onChange={(event) => setBetaEnabled(event.target.value === 'true')}>
                <option value="true">true</option>
                <option value="false">false</option>
              </select>
            </label>
            <label className="field">
              <span>Require Whitelist</span>
              <select value={String(betaRequireWhitelist)} onChange={(event) => setBetaRequireWhitelist(event.target.value === 'true')}>
                <option value="true">true</option>
                <option value="false">false</option>
              </select>
            </label>
          </div>
          <div className="button-row">
            <button
              type="button"
              className="button button-primary"
              onClick={() => void run(api.updateBetaControlPlane({ enabled: betaEnabled, require_whitelist: betaRequireWhitelist }), 'Closed beta control plane updated.')}
            >
              Save Global Beta Settings
            </button>
          </div>

          <div className="three-column-grid stacked-gap">
            <div className="subcard">
              <h3>User Policy</h3>
              <div className="form-grid">
                <label className="field">
                  <span>User ID</span>
                  <input value={betaUserId} onChange={(event) => setBetaUserId(event.target.value)} />
                </label>
                <label className="field">
                  <span>Whitelisted</span>
                  <select value={String(betaUserWhitelisted)} onChange={(event) => setBetaUserWhitelisted(event.target.value === 'true')}>
                    <option value="true">true</option>
                    <option value="false">false</option>
                  </select>
                </label>
                <label className="field">
                  <span>Max Cash</span>
                  <input value={betaUserMaxCash} onChange={(event) => setBetaUserMaxCash(event.target.value)} />
                </label>
                <label className="field">
                  <span>Max Open Orders</span>
                  <input value={betaUserMaxOrders} onChange={(event) => setBetaUserMaxOrders(event.target.value)} />
                </label>
              </div>
              <button
                type="button"
                className="button button-secondary"
                onClick={() =>
                  void run(
                    api.updateBetaUser(betaUserId, {
                      whitelisted: betaUserWhitelisted,
                      max_cash_balance: parseNumber(betaUserMaxCash),
                      max_open_orders: parseNumber(betaUserMaxOrders),
                    }),
                    `Beta user policy updated for ${betaUserId}.`,
                  )
                }
              >
                Save User Policy
              </button>
            </div>

            <div className="subcard">
              <h3>Market Policy</h3>
              <div className="form-grid">
                <label className="field">
                  <span>Market ID</span>
                  <input value={betaMarketId} onChange={(event) => setBetaMarketId(event.target.value)} />
                </label>
                <label className="field">
                  <span>Max Notional</span>
                  <input value={betaMarketMaxNotional} onChange={(event) => setBetaMarketMaxNotional(event.target.value)} />
                </label>
                <label className="field">
                  <span>Max Leverage</span>
                  <input value={betaMarketMaxLeverage} onChange={(event) => setBetaMarketMaxLeverage(event.target.value)} />
                </label>
              </div>
              <button
                type="button"
                className="button button-secondary"
                onClick={() =>
                  void run(
                    api.updateBetaMarket(betaMarketId, {
                      max_order_notional: parseNumber(betaMarketMaxNotional),
                      max_leverage: parseNumber(betaMarketMaxLeverage),
                    }),
                    `Beta market policy updated for ${betaMarketId}.`,
                  )
                }
              >
                Save Market Policy
              </button>
            </div>

            <div className="subcard">
              <h3>Configured Policies</h3>
              <div className="mini-list">
                {betaUsers.slice(0, 5).map((item, index) => (
                  <div key={`beta-user-${index}`} className="mini-list-item">
                    <strong>{text(item.user_id)}</strong>
                    <span>{text(item.whitelisted)}</span>
                  </div>
                ))}
                {betaMarkets.slice(0, 5).map((item, index) => (
                  <div key={`beta-market-${index}`} className="mini-list-item">
                    <strong>{text(item.market_id)}</strong>
                    <span>{text(item.max_order_notional)}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </Panel>
      </div>

      <div className="two-column-grid">
        <Panel title="Withdrawal Approval And Custody" subtitle="Approve or reject pending withdrawals, inspect custody audit, and reset breaker when needed.">
          <div className="form-grid">
            <label className="field field-span-2">
              <span>Withdrawal ID</span>
              <input value={approvalWithdrawalId} onChange={(event) => setApprovalWithdrawalId(event.target.value)} />
            </label>
          </div>
          <div className="button-row button-row-wrap">
            <button type="button" className="button button-primary" onClick={() => void run(api.approveWithdrawal(approvalWithdrawalId), `Withdrawal ${approvalWithdrawalId} approved.`)}>
              Approve Withdrawal
            </button>
            <button type="button" className="button button-secondary" onClick={() => void run(api.rejectWithdrawal(approvalWithdrawalId), `Withdrawal ${approvalWithdrawalId} rejected.`)}>
              Reject Withdrawal
            </button>
            <button type="button" className="button button-secondary" onClick={() => void run(api.resetCustodyBreaker(), 'Custody breaker reset requested.')}>
              Reset Custody Breaker
            </button>
          </div>

          <div className="mini-list stacked-gap">
            {pendingWithdrawals.slice(0, 8).map((item, index) => (
              <div key={`pending-wd-${index}`} className="mini-list-item">
                <strong>{text(item.withdrawal_id)}</strong>
                <span>{text(item.status)} / {text(item.amount)} / approvals left {text(item.approvals_remaining)}</span>
              </div>
            ))}
          </div>
        </Panel>

        <Panel title="Release And Failpoints" subtitle="Pre-flight release visibility and controlled failure injection tooling.">
          <div className="form-grid">
            <label className="field">
              <span>Release Target Version</span>
              <input value={releaseTargetVersion} onChange={(event) => setReleaseTargetVersion(event.target.value)} />
            </label>
            <label className="field">
              <span>Failpoint Name</span>
              <input value={failpointName} onChange={(event) => setFailpointName(event.target.value)} />
            </label>
            <label className="field">
              <span>Failpoint Mode</span>
              <select value={failpointMode} onChange={(event) => setFailpointMode(event.target.value)}>
                <option value="delay">delay</option>
                <option value="error">error</option>
                <option value="probabilistic">probabilistic</option>
                <option value="panic">panic</option>
              </select>
            </label>
            <label className="field">
              <span>Delay (us)</span>
              <input value={failpointDelayUs} onChange={(event) => setFailpointDelayUs(event.target.value)} />
            </label>
            <label className="field field-span-2">
              <span>Message</span>
              <input value={failpointMessage} onChange={(event) => setFailpointMessage(event.target.value)} />
            </label>
          </div>
          <div className="button-row button-row-wrap">
            <button type="button" className="button button-primary" onClick={() => void load()}>
              Refresh Release Data
            </button>
            <button
              type="button"
              className="button button-secondary"
              onClick={() =>
                void run(
                  api.activateFailpoint({
                    name: failpointName,
                    mode: failpointMode,
                    delay_us: parseNumber(failpointDelayUs),
                    message: failpointMessage,
                  }),
                  `Failpoint ${failpointName} activation requested.`,
                )
              }
            >
              Activate Failpoint
            </button>
            <button type="button" className="button button-secondary" onClick={() => void run(api.deactivateFailpoint(failpointName), `Failpoint ${failpointName} deactivated.`)}>
              Deactivate Failpoint
            </button>
          </div>

          <div className="mini-list stacked-gap">
            {failpointList.slice(0, 6).map((item, index) => (
              <div key={`fp-${index}`} className="mini-list-item">
                <strong>{text(item.name)}</strong>
                <span>{text(item.active)} / triggers {text(item.trigger_count)}</span>
              </div>
            ))}
          </div>
        </Panel>
      </div>

      <div className="two-column-grid">
        <Panel title="Governance And Audit" subtitle="Pending governance approvals, recent risk events, and admin action audit records.">
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>Action</th>
                  <th>Status</th>
                  <th>Actor</th>
                  <th>Operate</th>
                </tr>
              </thead>
              <tbody>
                {governanceActions.slice(0, 12).map((item, index) => {
                  const actionId = text(item.action_id, `row-${index}`)
                  return (
                    <tr key={actionId}>
                      <td>{text(item.action_type, actionId)}</td>
                      <td>{text(item.status)}</td>
                      <td>{text(item.requested_by)}</td>
                      <td>
                        <div className="table-actions">
                          <button type="button" className="link-button" onClick={() => void run(api.approveGovernanceAction(actionId), `Approved ${actionId}.`)}>
                            Approve
                          </button>
                          <button type="button" className="link-button" onClick={() => void run(api.rejectGovernanceAction(actionId), `Rejected ${actionId}.`)}>
                            Reject
                          </button>
                        </div>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>

          <div className="three-column-grid stacked-gap">
            <div className="subcard">
              <h3>Risk Events</h3>
              <div className="mini-list">
                {riskEvents.slice(0, 6).map((item, index) => (
                  <div key={`risk-${index}`} className="mini-list-item">
                    <strong>{text(item.event_type, text(item.kind, 'event'))}</strong>
                    <span>{text(item.recorded_at, text(item.timestamp))}</span>
                  </div>
                ))}
              </div>
            </div>
            <div className="subcard">
              <h3>Admin Audit</h3>
              <div className="mini-list">
                {adminAudit.slice(0, 6).map((item, index) => (
                  <div key={`audit-${index}`} className="mini-list-item">
                    <strong>{text(item.action)}</strong>
                    <span>{text(item.subject)}</span>
                  </div>
                ))}
              </div>
            </div>
            <div className="subcard">
              <h3>Treasury</h3>
              <div className="mini-list">
                <div className="mini-list-item">
                  <strong>Fee collector</strong>
                  <span>{text(asRecord(treasuryFeeCollector).balance, 'inspect JSON')}</span>
                </div>
                <div className="mini-list-item">
                  <strong>Insurance</strong>
                  <span>{text(asRecord(insuranceFunds).global_balance, 'inspect JSON')}</span>
                </div>
              </div>
            </div>
          </div>
        </Panel>

        <div className="json-grid">
          <JsonPanel title="Plane Snapshot" value={planes ?? { info: 'No plane payload loaded.' }} />
          <JsonPanel title="Ops Node" value={opsNode ?? { info: 'No node payload loaded.' }} />
          <JsonPanel title="Custody Breaker" value={custodyBreaker ?? { info: 'No breaker payload loaded.' }} />
          <JsonPanel title="Custody Audit" value={custodyAudit ?? { info: 'No custody audit payload loaded.' }} />
          <JsonPanel title="Custody Audit Events" value={custodyAuditEvents} />
          <JsonPanel title="Release Checklist" value={releaseChecklist ?? { info: 'No release checklist loaded.' }} />
          <JsonPanel title="Release Version" value={releaseVersion ?? { info: 'No release version payload loaded.' }} />
          <JsonPanel title="Release Features" value={releaseFeatures ?? { info: 'No release features payload loaded.' }} />
          <JsonPanel title="Failpoints" value={failpoints ?? { info: 'No failpoint payload loaded.' }} />
          <JsonPanel title="Last Backend Response" value={lastResponse ?? { info: 'No control mutation submitted yet.' }} />
        </div>
      </div>
    </div>
  )
}
