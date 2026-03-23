#![allow(dead_code)]
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Operations Automation — Drain, Backup, Node Identity
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Adds:
//  • Graceful drain mode — stop accepting new work while finishing in-flight
//  • On-demand checkpoint/backup trigger
//  • Node identity & cluster membership abstraction
//  • `/admin/ops/drain`      — enter/exit drain mode
//  • `/admin/ops/checkpoint` — force checkpoint all WALs
//  • `/admin/ops/node`       — node identity & status
//
// Distributed boundary: the NodeIdentity and ClusterTopology structs
// define the abstraction boundary for future multi-node deployment.
// Currently single-node; the types are designed so a network-aware
// implementation can replace the local stub without API changes.

use super::*;

// ── Drain mode ───────────────────────────────────────────────

/// Global drain flag — when set, all new order submissions are rejected
/// but in-flight work and admin ops continue.
pub(crate) static DRAIN_MODE: AtomicBool = AtomicBool::new(false);

/// Check if the system is in drain mode.
pub(crate) fn is_draining() -> bool {
    DRAIN_MODE.load(Ordering::Relaxed)
}

// ── Node identity (distributed boundary abstraction) ─────────

/// Unique identity of this exchange node.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct NodeIdentity {
    /// Unique node ID (generated at startup).
    pub(crate) node_id: String,
    /// Human-readable node name (from env or hostname).
    pub(crate) node_name: String,
    /// Role of this node in the cluster.
    pub(crate) role: NodeRole,
    /// Startup timestamp.
    pub(crate) started_at: DateTime<Utc>,
    /// Partitions owned by this node.
    pub(crate) owned_partitions: Vec<usize>,
}

/// Node role within a cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum NodeRole {
    /// Standalone: single-node deployment (current default).
    Standalone,
    /// Leader: primary node that processes writes.
    Leader,
    /// Follower: read-replica that receives state via replication.
    Follower,
    /// Candidate: node participating in leader election.
    Candidate,
}

impl NodeIdentity {
    pub(crate) fn standalone(partitions: usize) -> Self {
        Self {
            node_id: types::generate_id(),
            node_name: std::env::var("NODE_NAME").unwrap_or_else(|_| "exchange-0".to_string()),
            role: NodeRole::Standalone,
            started_at: Utc::now(),
            owned_partitions: (0..partitions).collect(),
        }
    }
}

// ── Cluster topology (distributed boundary) ──────────────────

/// Cluster membership view — currently a single-node stub.
/// Designed for future extension with gossip/Raft membership.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ClusterTopology {
    /// All known cluster members.
    pub(crate) members: Vec<ClusterMember>,
    /// Total number of partitions across all nodes.
    pub(crate) total_partitions: usize,
    /// Replication factor (1 = no replication).
    pub(crate) replication_factor: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ClusterMember {
    pub(crate) node_id: String,
    pub(crate) node_name: String,
    pub(crate) role: NodeRole,
    pub(crate) partitions: Vec<usize>,
    pub(crate) healthy: bool,
}

impl ClusterTopology {
    /// Create a single-node topology.
    pub(crate) fn standalone(identity: &NodeIdentity) -> Self {
        Self {
            members: vec![ClusterMember {
                node_id: identity.node_id.clone(),
                node_name: identity.node_name.clone(),
                role: identity.role,
                partitions: identity.owned_partitions.clone(),
                healthy: true,
            }],
            total_partitions: identity.owned_partitions.len(),
            replication_factor: 1,
        }
    }

    /// Check whether this node owns a given partition.
    pub(crate) fn owns_partition(&self, partition: usize, node_id: &str) -> bool {
        self.members
            .iter()
            .any(|m| m.node_id == node_id && m.partitions.contains(&partition))
    }
}

// ── State transfer hook (distributed boundary) ───────────────

/// Trait for state transfer between cluster nodes.
/// Implemented as no-op for single-node; future network impl
/// would stream snapshots over gRPC/TCP.
pub(crate) trait StateTransfer: Send + Sync {
    /// Export a partition snapshot for transfer to another node.
    fn export_partition(&self, partition_id: usize) -> Result<Vec<u8>, String>;
    /// Import a partition snapshot from another node.
    fn import_partition(&self, partition_id: usize, data: &[u8]) -> Result<(), String>;
}

/// No-op state transfer for single-node deployment.
pub(crate) struct LocalStateTransfer;

impl StateTransfer for LocalStateTransfer {
    fn export_partition(&self, partition_id: usize) -> Result<Vec<u8>, String> {
        Err(format!(
            "partition {partition_id}: state transfer not supported in standalone mode"
        ))
    }
    fn import_partition(&self, partition_id: usize, _data: &[u8]) -> Result<(), String> {
        Err(format!(
            "partition {partition_id}: state transfer not supported in standalone mode"
        ))
    }
}

// ── Admin routes ─────────────────────────────────────────────

pub(crate) fn build_ops_routes(
    partitioned_engine: Arc<PartitionedMatchingEngine>,
    ledger: Arc<LedgerService>,
    node_identity: Arc<NodeIdentity>,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    // POST /admin/ops/drain — toggle drain mode
    let ip1 = ip_rate_limiter.clone();
    let adm1 = admin_rate_limiter.clone();
    let engine1 = partitioned_engine.clone();
    let drain_route = warp::path!("admin" / "ops" / "drain")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and(body_limit())
        .and(verified_json_body())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  remote: Option<SocketAddr>,
                  req: DrainRequest| {
                let ip_rl = ip1.clone();
                let adm_rl = adm1.clone();
                let engine = engine1.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 10)?;

                    let enable = req.enable;
                    let previous = DRAIN_MODE.swap(enable, Ordering::SeqCst);

                    if enable {
                        tracing::warn!(
                            admin = %principal.subject,
                            "DRAIN MODE ENABLED — new order submissions will be rejected"
                        );
                    } else {
                        tracing::info!(
                            admin = %principal.subject,
                            "DRAIN MODE DISABLED — resuming normal operations"
                        );
                    }

                    let depths = engine.queue_depths();
                    let total_inflight: usize = depths.iter().map(|d| d.inflight).sum();

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "drain_mode": enable,
                        "previous": previous,
                        "inflight_orders": total_inflight,
                        "message": if enable {
                            "drain mode enabled — wait for inflight_orders to reach 0 before shutdown"
                        } else {
                            "drain mode disabled — normal operations resumed"
                        },
                    })))
                }
            },
        );

    // POST /admin/ops/checkpoint — force WAL checkpoint
    let ip2 = ip_rate_limiter.clone();
    let adm2 = admin_rate_limiter.clone();
    let engine2 = partitioned_engine.clone();
    let ledger2 = ledger.clone();
    let checkpoint_route = warp::path!("admin" / "ops" / "checkpoint")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip2.clone();
                let adm_rl = adm2.clone();
                let engine = engine2.clone();
                let ledger = ledger2.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 10)?;

                    let ckpt_start = Instant::now();

                    // Flush matching engine snapshots.
                    let snapshot_result = match engine.flush_all_snapshots().await {
                        Ok(()) => "ok",
                        Err(e) => {
                            tracing::error!(error = %e, "checkpoint: snapshot flush failed");
                            "error"
                        }
                    };

                    // Verify ledger invariant.
                    let invariant_ok = ledger.verify_global_invariant().is_ok();

                    let elapsed_ms = ckpt_start.elapsed().as_millis() as u64;
                    tracing::info!(
                        admin = %principal.subject,
                        elapsed_ms,
                        snapshot_result,
                        invariant_ok,
                        "checkpoint completed"
                    );

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "status": snapshot_result,
                        "invariant_ok": invariant_ok,
                        "elapsed_ms": elapsed_ms,
                        "timestamp": Utc::now(),
                    })))
                }
            },
        );

    // GET /admin/ops/node — node identity & cluster topology
    let ip3 = ip_rate_limiter.clone();
    let adm3 = admin_rate_limiter.clone();
    let node1 = node_identity.clone();
    let node_route = warp::path!("admin" / "ops" / "node")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let ip_rl = ip3.clone();
                let adm_rl = adm3.clone();
                let node = node1.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 30)?;

                    let topology = ClusterTopology::standalone(&node);

                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "node": node.as_ref(),
                        "cluster": topology,
                        "drain_mode": is_draining(),
                    })))
                }
            },
        );

    drain_route
        .or(checkpoint_route)
        .unify()
        .or(node_route)
        .unify()
        .boxed()
}

// ── DTOs ─────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub(crate) struct DrainRequest {
    pub(crate) enable: bool,
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_mode_toggle() {
        DRAIN_MODE.store(false, Ordering::Relaxed);
        assert!(!is_draining());
        DRAIN_MODE.store(true, Ordering::Relaxed);
        assert!(is_draining());
        DRAIN_MODE.store(false, Ordering::Relaxed);
    }

    #[test]
    fn node_identity_standalone() {
        let node = NodeIdentity::standalone(8);
        assert_eq!(node.role, NodeRole::Standalone);
        assert_eq!(node.owned_partitions.len(), 8);
    }

    #[test]
    fn cluster_topology_standalone() {
        let node = NodeIdentity::standalone(4);
        let topo = ClusterTopology::standalone(&node);
        assert_eq!(topo.members.len(), 1);
        assert_eq!(topo.total_partitions, 4);
        assert_eq!(topo.replication_factor, 1);
        assert!(topo.owns_partition(0, &node.node_id));
        assert!(!topo.owns_partition(10, &node.node_id));
    }

    #[test]
    fn cluster_topology_does_not_own_unknown_node() {
        let node = NodeIdentity::standalone(4);
        let topo = ClusterTopology::standalone(&node);
        assert!(!topo.owns_partition(0, "unknown-node"));
    }

    #[test]
    fn local_state_transfer_rejects() {
        let xfer = LocalStateTransfer;
        assert!(xfer.export_partition(0).is_err());
        assert!(xfer.import_partition(0, &[]).is_err());
    }
}
