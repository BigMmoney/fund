import { useCallback, useEffect, useMemo, useState } from 'react'
import { JsonPanel } from '@/components/JsonPanel'
import { Panel } from '@/components/Panel'
import { asList, asRecord, createExchangeApi, type AuthConfig, type JsonRecord } from '@/services/exchangeApi'

interface PageProps {
  auth: AuthConfig
  onNotice: (message: string) => void
}

function text(value: unknown, fallback = '-'): string {
  if (typeof value === 'string' && value.length > 0) return value
  if (typeof value === 'number') return String(value)
  if (typeof value === 'boolean') return String(value)
  return fallback
}

export function SystemPage({ auth, onNotice }: PageProps) {
  const api = useMemo(() => createExchangeApi(auth), [auth])
  const [loading, setLoading] = useState(false)
  const [health, setHealth] = useState<unknown>(null)
  const [ready, setReady] = useState<unknown>(null)
  const [version, setVersion] = useState<unknown>(null)
  const [partitions, setPartitions] = useState<unknown>(null)
  const [metrics, setMetrics] = useState<unknown>(null)
  const [prometheus, setPrometheus] = useState('')
  const [perfProfile, setPerfProfile] = useState<unknown>(null)
  const [perfSla, setPerfSla] = useState<unknown>(null)
  const [oncallStatus, setOncallStatus] = useState<unknown>(null)
  const [oncallEscalation, setOncallEscalation] = useState<unknown>(null)
  const [oncallRunbooks, setOncallRunbooks] = useState<JsonRecord[]>([])
  const [capacity, setCapacity] = useState<unknown>(null)
  const [capacityAlerts, setCapacityAlerts] = useState<JsonRecord[]>([])
  const [sentinelPosture, setSentinelPosture] = useState<unknown>(null)
  const [sentinelIncidents, setSentinelIncidents] = useState<JsonRecord[]>([])
  const [rollbackStatus, setRollbackStatus] = useState<unknown>(null)
  const [rollbackRunbook, setRollbackRunbook] = useState<unknown>(null)

  const load = useCallback(async () => {
    setLoading(true)
    const results = await Promise.allSettled([
      api.getHealth(),
      api.getReady(),
      api.getVersion(),
      api.getPartitions(),
      api.getMetrics(),
      api.getPrometheus(),
      api.getPerfProfile(),
      api.getPerfSla(),
      api.getOncallStatus(),
      api.getOncallEscalation(),
      api.getOncallRunbooks(),
      api.getCapacity(),
      api.getCapacityAlerts(),
      api.getSentinelPosture(),
      api.getSentinelIncidents(),
      api.getRollbackStatus(),
      api.getRollbackRunbook(),
    ])

    const [
      healthResult,
      readyResult,
      versionResult,
      partitionsResult,
      metricsResult,
      prometheusResult,
      perfProfileResult,
      perfSlaResult,
      oncallStatusResult,
      oncallEscalationResult,
      oncallRunbooksResult,
      capacityResult,
      alertsResult,
      sentinelPostureResult,
      sentinelIncidentsResult,
      rollbackStatusResult,
      rollbackRunbookResult,
    ] = results

    if (healthResult.status === 'fulfilled') setHealth(healthResult.value)
    if (readyResult.status === 'fulfilled') setReady(readyResult.value)
    if (versionResult.status === 'fulfilled') setVersion(versionResult.value)
    if (partitionsResult.status === 'fulfilled') setPartitions(partitionsResult.value)
    if (metricsResult.status === 'fulfilled') setMetrics(metricsResult.value)
    if (prometheusResult.status === 'fulfilled') setPrometheus(prometheusResult.value)
    if (perfProfileResult.status === 'fulfilled') setPerfProfile(perfProfileResult.value)
    if (perfSlaResult.status === 'fulfilled') setPerfSla(perfSlaResult.value)
    if (oncallStatusResult.status === 'fulfilled') setOncallStatus(oncallStatusResult.value)
    if (oncallEscalationResult.status === 'fulfilled') setOncallEscalation(oncallEscalationResult.value)
    if (oncallRunbooksResult.status === 'fulfilled') setOncallRunbooks(asList(oncallRunbooksResult.value))
    if (capacityResult.status === 'fulfilled') setCapacity(capacityResult.value)
    if (alertsResult.status === 'fulfilled') setCapacityAlerts(asList(alertsResult.value))
    if (sentinelPostureResult.status === 'fulfilled') setSentinelPosture(sentinelPostureResult.value)
    if (sentinelIncidentsResult.status === 'fulfilled') setSentinelIncidents(asList(sentinelIncidentsResult.value))
    if (rollbackStatusResult.status === 'fulfilled') setRollbackStatus(rollbackStatusResult.value)
    if (rollbackRunbookResult.status === 'fulfilled') setRollbackRunbook(rollbackRunbookResult.value)

    const failedCount = results.filter((item) => item.status === 'rejected').length
    onNotice(failedCount > 0 ? `System page refreshed with ${failedCount} backend call failures.` : 'System page refreshed from live backend.')
    setLoading(false)
  }, [api, onNotice])

  useEffect(() => {
    void load()
  }, [load])

  const healthRecord = asRecord(health)
  const readyRecord = asRecord(ready)
  const versionRecord = asRecord(version)
  const perfSlaRecord = asRecord(perfSla)

  return (
    <div className="page-grid">
      <section className="stat-grid">
        <div className="stat-card">
          <span>Health</span>
          <strong>{text(healthRecord.status, 'unknown')}</strong>
        </div>
        <div className="stat-card">
          <span>Ready</span>
          <strong>{text(readyRecord.status, 'unknown')}</strong>
        </div>
        <div className="stat-card">
          <span>Version</span>
          <strong>{text(versionRecord.version, text(versionRecord.build, 'unknown'))}</strong>
        </div>
        <div className="stat-card">
          <span>SLA</span>
          <strong>{text(perfSlaRecord.sla_compliant, 'unknown')}</strong>
        </div>
      </section>

      <Panel
        title="System Status"
        subtitle="Health, readiness, observability, incident posture, and rollback readiness."
        actions={
          <button type="button" className="button button-secondary" onClick={() => void load()}>
            {loading ? 'Refreshing...' : 'Refresh'}
          </button>
        }
      >
        <div className="three-column-grid">
          <div className="subcard">
            <h3>Core Health</h3>
            <div className="mini-list">
              <div className="mini-list-item">
                <strong>Health</strong>
                <span>{text(healthRecord.status)}</span>
              </div>
              <div className="mini-list-item">
                <strong>Ready</strong>
                <span>{text(readyRecord.status)}</span>
              </div>
              <div className="mini-list-item">
                <strong>Frontier Consistency</strong>
                <span>{text(readyRecord.frontier_consistency)}</span>
              </div>
            </div>
          </div>
          <div className="subcard">
            <h3>Performance</h3>
            <div className="mini-list">
              <div className="mini-list-item">
                <strong>Total Breaches</strong>
                <span>{text(perfSlaRecord.total_breaches)}</span>
              </div>
              <div className="mini-list-item">
                <strong>SLA Compliant</strong>
                <span>{text(perfSlaRecord.sla_compliant)}</span>
              </div>
            </div>
          </div>
          <div className="subcard">
            <h3>On-call</h3>
            <div className="mini-list">
              <div className="mini-list-item">
                <strong>Primary</strong>
                <span>{text(asRecord(oncallStatus).primary_oncall)}</span>
              </div>
              <div className="mini-list-item">
                <strong>Escalation</strong>
                <span>{text(asRecord(oncallEscalation).policy_name)}</span>
              </div>
            </div>
          </div>
        </div>
      </Panel>

      <div className="two-column-grid">
        <JsonPanel title="Health Snapshot" value={health ?? { info: 'No health payload loaded.' }} />
        <JsonPanel title="Readiness Snapshot" value={ready ?? { info: 'No ready payload loaded.' }} />
        <JsonPanel title="Partition Health" value={partitions ?? { info: 'No partition payload loaded.' }} />
        <JsonPanel title="Metrics Snapshot" value={metrics ?? { info: 'No metrics payload loaded.' }} />
      </div>

      <div className="two-column-grid">
        <JsonPanel title="Performance Profile" value={perfProfile ?? { info: 'No perf profile loaded.' }} />
        <JsonPanel title="Performance SLA" value={perfSla ?? { info: 'No SLA payload loaded.' }} />
        <JsonPanel title="On-call Status" value={oncallStatus ?? { info: 'No on-call status loaded.' }} />
        <JsonPanel title="On-call Escalation" value={oncallEscalation ?? { info: 'No escalation payload loaded.' }} />
      </div>

      <div className="two-column-grid">
        <JsonPanel title="Capacity Snapshot" value={capacity ?? { info: 'No capacity payload loaded.' }} />
        <JsonPanel title="Sentinel Posture" value={sentinelPosture ?? { info: 'No sentinel posture loaded.' }} />
        <JsonPanel title="Rollback Status" value={rollbackStatus ?? { info: 'No rollback status loaded.' }} />
        <JsonPanel title="Rollback Runbook" value={rollbackRunbook ?? { info: 'No rollback runbook loaded.' }} />
      </div>

      <Panel title="Active Alerts And Incidents" subtitle="Capacity alerts, sentinel incidents, and runbook inventory.">
        <div className="three-column-grid">
          <div className="subcard">
            <h3>Capacity Alerts</h3>
            <div className="mini-list">
              {capacityAlerts.slice(0, 8).map((item, index) => (
                <div key={`capacity-${index}`} className="mini-list-item">
                  <strong>{text(item.alert_type, 'alert')}</strong>
                  <span>{text(item.level, text(item.status))}</span>
                </div>
              ))}
            </div>
          </div>

          <div className="subcard">
            <h3>Sentinel Incidents</h3>
            <div className="mini-list">
              {sentinelIncidents.slice(0, 8).map((item, index) => (
                <div key={`incident-${index}`} className="mini-list-item">
                  <strong>{text(item.incident_id, text(item.id, 'incident'))}</strong>
                  <span>{text(item.status, text(item.origin))}</span>
                </div>
              ))}
            </div>
          </div>

          <div className="subcard">
            <h3>Runbooks</h3>
            <div className="mini-list">
              {oncallRunbooks.slice(0, 8).map((item, index) => (
                <div key={`runbook-${index}`} className="mini-list-item">
                  <strong>{text(item.name, text(item.id, 'runbook'))}</strong>
                  <span>{text(item.category, text(item.severity))}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </Panel>

      <Panel title="Prometheus Text" subtitle="Raw /metrics/prometheus output for debugging and quick inspection.">
        <div className="json-card">
          <div className="json-card-title">Prometheus</div>
          <pre>{prometheus || '# no metrics returned'}</pre>
        </div>
      </Panel>
    </div>
  )
}
