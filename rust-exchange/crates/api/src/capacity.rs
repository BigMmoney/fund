#![allow(dead_code)]
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Capacity Planning — Resource Tracking, Alerts & Auto-Scaling Hooks
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Supplements the existing backpressure system (partitioned.rs) and
// connection limits (websocket.rs) with:
//
//  - CapacityTracker — unified view of all resource dimensions
//  - CapacityAlert — threshold-based alert generation
//  - ResourceDimension — memory, FD, disk, queue, connections
//  - Auto-scaling readiness hooks (HPA target reporting)
//  - `/admin/capacity`        — current capacity snapshot
//  - `/admin/capacity/alerts` — active capacity alerts

use super::*;

// ── Resource dimensions ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ResourceDimension {
    QueueUtilization,
    WsConnections,
    WalEntries,
    EventBusBacklog,
    WorkerThreads,
    MemoryRss,
    FileDescriptors,
    DiskFree,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct CapacityMeasurement {
    pub(crate) dimension: ResourceDimension,
    pub(crate) current: u64,
    pub(crate) limit: u64,
    pub(crate) utilization_pct: f64,
    pub(crate) status: CapacityStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum CapacityStatus {
    Normal,
    Warning,
    Critical,
    Exhausted,
}

impl CapacityStatus {
    fn from_pct(pct: f64, warn: f64, crit: f64) -> Self {
        if pct >= 100.0 {
            Self::Exhausted
        } else if pct >= crit {
            Self::Critical
        } else if pct >= warn {
            Self::Warning
        } else {
            Self::Normal
        }
    }
}

// ── Capacity alert ───────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct CapacityAlert {
    pub(crate) dimension: ResourceDimension,
    pub(crate) status: CapacityStatus,
    pub(crate) utilization_pct: f64,
    pub(crate) message: String,
    pub(crate) generated_at: DateTime<Utc>,
}

// ── Capacity config ──────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CapacityConfig {
    pub(crate) warning_pct: f64,
    pub(crate) critical_pct: f64,
    pub(crate) scaling_target_pct: f64,
    pub(crate) min_disk_free_bytes: u64,
    pub(crate) max_memory_rss_bytes: u64,
}

impl Default for CapacityConfig {
    fn default() -> Self {
        Self {
            warning_pct: 70.0,
            critical_pct: 85.0,
            scaling_target_pct: 60.0,
            min_disk_free_bytes: 512 * 1024 * 1024,
            max_memory_rss_bytes: 4 * 1024 * 1024 * 1024,
        }
    }
}

// ── Capacity tracker ─────────────────────────────────────────

pub(crate) struct CapacityTracker {
    config: CapacityConfig,
}

impl CapacityTracker {
    pub(crate) fn new(config: CapacityConfig) -> Self {
        Self { config }
    }

    pub(crate) fn with_defaults() -> Self {
        Self::new(CapacityConfig::default())
    }

    pub(crate) fn snapshot(
        &self,
        engine: &PartitionedMatchingEngine,
        ws_connections: usize,
        ws_max: usize,
    ) -> CapacitySnapshot {
        let mut measurements = Vec::new();

        // 1. Queue utilization (worst partition).
        let depths = engine.queue_depths();
        let worst = depths.iter().max_by_key(|d| {
            if d.capacity > 0 {
                (d.inflight as f64 / d.capacity as f64 * 1000.0) as u64
            } else {
                0
            }
        });
        if let Some(w) = worst {
            let pct = if w.capacity > 0 {
                w.inflight as f64 / w.capacity as f64 * 100.0
            } else {
                0.0
            };
            measurements.push(CapacityMeasurement {
                dimension: ResourceDimension::QueueUtilization,
                current: w.inflight as u64,
                limit: w.capacity as u64,
                utilization_pct: pct,
                status: CapacityStatus::from_pct(
                    pct,
                    self.config.warning_pct,
                    self.config.critical_pct,
                ),
            });
        }

        // 2. WebSocket connections.
        let ws_pct = if ws_max > 0 {
            ws_connections as f64 / ws_max as f64 * 100.0
        } else {
            0.0
        };
        measurements.push(CapacityMeasurement {
            dimension: ResourceDimension::WsConnections,
            current: ws_connections as u64,
            limit: ws_max as u64,
            utilization_pct: ws_pct,
            status: CapacityStatus::from_pct(
                ws_pct,
                self.config.warning_pct,
                self.config.critical_pct,
            ),
        });

        // 3. Disk free (best-effort, platform-dependent).
        if let Some(disk) = self.probe_disk_free() {
            let used = disk.total.saturating_sub(disk.free);
            let pct = if disk.total > 0 {
                used as f64 / disk.total as f64 * 100.0
            } else {
                0.0
            };
            let status = if disk.free < self.config.min_disk_free_bytes {
                CapacityStatus::Critical
            } else {
                CapacityStatus::from_pct(pct, self.config.warning_pct, self.config.critical_pct)
            };
            measurements.push(CapacityMeasurement {
                dimension: ResourceDimension::DiskFree,
                current: disk.free,
                limit: disk.total,
                utilization_pct: pct,
                status,
            });
        }

        // 4. Memory RSS (best-effort).
        if let Some(rss) = self.probe_memory_rss() {
            let limit = self.config.max_memory_rss_bytes;
            let pct = rss as f64 / limit as f64 * 100.0;
            measurements.push(CapacityMeasurement {
                dimension: ResourceDimension::MemoryRss,
                current: rss,
                limit,
                utilization_pct: pct,
                status: CapacityStatus::from_pct(
                    pct,
                    self.config.warning_pct,
                    self.config.critical_pct,
                ),
            });
        }

        // 5. Worker thread count.
        let active_threads = self.probe_thread_count();
        let max_threads = num_cpus() * 2;
        let thread_pct = if max_threads > 0 {
            active_threads as f64 / max_threads as f64 * 100.0
        } else {
            0.0
        };
        measurements.push(CapacityMeasurement {
            dimension: ResourceDimension::WorkerThreads,
            current: active_threads as u64,
            limit: max_threads as u64,
            utilization_pct: thread_pct,
            status: CapacityStatus::from_pct(
                thread_pct,
                self.config.warning_pct,
                self.config.critical_pct,
            ),
        });

        // Generate alerts.
        let now = Utc::now();
        let alerts: Vec<CapacityAlert> = measurements
            .iter()
            .filter(|m| m.status != CapacityStatus::Normal)
            .map(|m| CapacityAlert {
                dimension: m.dimension,
                status: m.status,
                utilization_pct: m.utilization_pct,
                message: format!(
                    "{:?} at {:.1}% ({}/{})",
                    m.dimension, m.utilization_pct, m.current, m.limit
                ),
                generated_at: now,
            })
            .collect();

        let scaling = self.compute_scaling_recommendation(&measurements);

        CapacitySnapshot {
            measurements,
            alerts,
            scaling,
            collected_at: now,
        }
    }

    fn compute_scaling_recommendation(
        &self,
        measurements: &[CapacityMeasurement],
    ) -> ScalingRecommendation {
        let worst_pct = measurements
            .iter()
            .map(|m| m.utilization_pct)
            .fold(0.0_f64, f64::max);

        let target = self.config.scaling_target_pct;
        if worst_pct > self.config.critical_pct {
            ScalingRecommendation {
                action: ScalingAction::ScaleUp,
                reason: format!(
                    "worst dimension at {worst_pct:.1}% > critical {:.1}%",
                    self.config.critical_pct
                ),
                desired_replicas: ((worst_pct / target).ceil() as u32).max(2),
            }
        } else if worst_pct < target * 0.5 && worst_pct > 0.0 {
            ScalingRecommendation {
                action: ScalingAction::ScaleDown,
                reason: format!("all dimensions below {:.1}% (half of target)", target * 0.5),
                desired_replicas: 1,
            }
        } else {
            ScalingRecommendation {
                action: ScalingAction::NoChange,
                reason: "within target range".into(),
                desired_replicas: 1,
            }
        }
    }

    fn probe_disk_free(&self) -> Option<DiskInfo> {
        #[cfg(target_os = "linux")]
        {
            None // would use statvfs in production
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    fn probe_memory_rss(&self) -> Option<u64> {
        #[cfg(target_os = "linux")]
        {
            if let Ok(content) = std::fs::read_to_string("/proc/self/status") {
                for line in content.lines() {
                    if line.starts_with("VmRSS:") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            if let Ok(kb) = parts[1].parse::<u64>() {
                                return Some(kb * 1024);
                            }
                        }
                    }
                }
            }
            None
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    fn probe_thread_count(&self) -> usize {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

// ── Snapshot & scaling types ─────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct CapacitySnapshot {
    pub(crate) measurements: Vec<CapacityMeasurement>,
    pub(crate) alerts: Vec<CapacityAlert>,
    pub(crate) scaling: ScalingRecommendation,
    pub(crate) collected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ScalingRecommendation {
    pub(crate) action: ScalingAction,
    pub(crate) reason: String,
    pub(crate) desired_replicas: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ScalingAction {
    NoChange,
    ScaleUp,
    ScaleDown,
}

struct DiskInfo {
    total: u64,
    free: u64,
}

// ── Admin routes ─────────────────────────────────────────────

pub(crate) fn build_capacity_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    ws_hub: Arc<WsHub>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let tracker = Arc::new(CapacityTracker::with_defaults());

    let ip1 = ip_rate_limiter.clone();
    let adm1 = admin_rate_limiter.clone();
    let engine1 = partitioned_engine.clone();
    let ws1 = ws_hub.clone();
    let tracker1 = tracker.clone();
    let capacity_route = warp::path!("admin" / "capacity")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip1.clone();
                let adm_rl = adm1.clone();
                let engine = engine1.clone();
                let ws = ws1.clone();
                let tr = tracker1.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let ws_max = cfg().websocket.max_connections;
                    let ws_active = ws.connection_count();
                    let snap = tr.snapshot(&engine, ws_active, ws_max);

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "capacity": snap,
                    })))
                }
            },
        );

    let ip2 = ip_rate_limiter.clone();
    let adm2 = admin_rate_limiter.clone();
    let engine2 = partitioned_engine.clone();
    let ws2 = ws_hub.clone();
    let tracker2 = tracker.clone();
    let alerts_route = warp::path!("admin" / "capacity" / "alerts")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip2.clone();
                let adm_rl = adm2.clone();
                let engine = engine2.clone();
                let ws = ws2.clone();
                let tr = tracker2.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let ws_max = cfg().websocket.max_connections;
                    let ws_active = ws.connection_count();
                    let snap = tr.snapshot(&engine, ws_active, ws_max);

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "alerts": snap.alerts,
                        "alert_count": snap.alerts.len(),
                        "scaling": snap.scaling,
                    })))
                }
            },
        );

    capacity_route.or(alerts_route).unify().boxed()
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_status_from_pct() {
        assert_eq!(
            CapacityStatus::from_pct(0.0, 70.0, 85.0),
            CapacityStatus::Normal
        );
        assert_eq!(
            CapacityStatus::from_pct(50.0, 70.0, 85.0),
            CapacityStatus::Normal
        );
        assert_eq!(
            CapacityStatus::from_pct(70.0, 70.0, 85.0),
            CapacityStatus::Warning
        );
        assert_eq!(
            CapacityStatus::from_pct(85.0, 70.0, 85.0),
            CapacityStatus::Critical
        );
        assert_eq!(
            CapacityStatus::from_pct(100.0, 70.0, 85.0),
            CapacityStatus::Exhausted
        );
        assert_eq!(
            CapacityStatus::from_pct(150.0, 70.0, 85.0),
            CapacityStatus::Exhausted
        );
    }

    #[test]
    fn default_capacity_config() {
        let cfg = CapacityConfig::default();
        assert_eq!(cfg.warning_pct, 70.0);
        assert_eq!(cfg.critical_pct, 85.0);
        assert_eq!(cfg.scaling_target_pct, 60.0);
        assert_eq!(cfg.min_disk_free_bytes, 512 * 1024 * 1024);
        assert_eq!(cfg.max_memory_rss_bytes, 4 * 1024 * 1024 * 1024);
    }

    #[test]
    fn scaling_recommendation_scale_up() {
        let tr = CapacityTracker::with_defaults();
        let measurements = vec![CapacityMeasurement {
            dimension: ResourceDimension::QueueUtilization,
            current: 3600,
            limit: 4096,
            utilization_pct: 90.0,
            status: CapacityStatus::Critical,
        }];
        let rec = tr.compute_scaling_recommendation(&measurements);
        assert_eq!(rec.action, ScalingAction::ScaleUp);
        assert!(rec.desired_replicas >= 2);
    }

    #[test]
    fn scaling_recommendation_no_change() {
        let tr = CapacityTracker::with_defaults();
        let measurements = vec![CapacityMeasurement {
            dimension: ResourceDimension::QueueUtilization,
            current: 2000,
            limit: 4096,
            utilization_pct: 50.0,
            status: CapacityStatus::Normal,
        }];
        let rec = tr.compute_scaling_recommendation(&measurements);
        assert_eq!(rec.action, ScalingAction::NoChange);
    }

    #[test]
    fn scaling_recommendation_scale_down() {
        let tr = CapacityTracker::with_defaults();
        let measurements = vec![CapacityMeasurement {
            dimension: ResourceDimension::QueueUtilization,
            current: 100,
            limit: 4096,
            utilization_pct: 5.0,
            status: CapacityStatus::Normal,
        }];
        let rec = tr.compute_scaling_recommendation(&measurements);
        assert_eq!(rec.action, ScalingAction::ScaleDown);
    }

    #[test]
    fn num_cpus_nonzero() {
        assert!(num_cpus() > 0);
    }

    #[test]
    fn probe_thread_count_nonzero() {
        let tr = CapacityTracker::with_defaults();
        assert!(tr.probe_thread_count() > 0);
    }
}
