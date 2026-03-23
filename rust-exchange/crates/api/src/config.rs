use serde::Deserialize;
use std::env;
use std::path::Path;

/// Centralised configuration for the exchange.
///
/// Loading priority: environment variables → TOML file → hardcoded defaults.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ExchangeConfig {
    pub server: ServerConfig,
    pub wal: WalConfig,
    pub websocket: WebSocketConfig,
    pub risk: RiskConfig,
    pub cors: CorsConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind_host: String,
    pub bind_port: u16,
    pub log_level: String,
    pub max_body_size_bytes: u64,
    pub request_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WalConfig {
    pub data_dir: String,
    pub rotation_max_entries: u64,
    pub group_commit_size: u64,
    pub ledger: String,
    pub sequencer: String,
    pub matching_snapshot: String,
    pub trade_journal: String,
    pub trade_settlement: String,
    pub instruments_registry: String,
    pub funding_rates: String,
    pub risk_automation_audit: String,
    pub liquidation_queue: String,
    pub liquidation_auction: String,
    pub adl_governance: String,
    pub liquidation_policy: String,
    pub index_price: String,
    pub index_source_policy: String,
    pub position_cost_state: String,
    pub position_cost_events: String,
    pub governance_actions: String,
    pub withdrawals: String,
    pub fee_tiers: String,
    pub transfers: String,
    pub stop_orders: String,
    pub address_whitelist: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WebSocketConfig {
    pub orderbook_snapshot_interval_ms: u64,
    pub max_connections: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RiskConfig {
    pub automation_enabled: bool,
    pub liquidation_interval_secs: u64,
    pub funding_interval_secs: u64,
    pub liquidation_worker_interval_secs: u64,
    pub liquidation_auction_window_secs: i64,
    pub liquidator_user_id: String,
    pub maintenance_margin_bps: i64,
    pub liquidation_penalty_bps: i64,
    pub position_cost_resync_interval_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
}

// ── Defaults ─────────────────────────────────────────────────

#[allow(clippy::derivable_impls)]
impl Default for ExchangeConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            wal: WalConfig::default(),
            websocket: WebSocketConfig::default(),
            risk: RiskConfig::default(),
            cors: CorsConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_host: "127.0.0.1".into(),
            bind_port: 3030,
            log_level: "info".into(),
            max_body_size_bytes: 16_384,
            request_timeout_secs: 30,
        }
    }
}

impl Default for WalConfig {
    fn default() -> Self {
        Self {
            data_dir: "data".into(),
            rotation_max_entries: 100_000,
            group_commit_size: 64,
            ledger: "data/ledger.wal.jsonl".into(),
            sequencer: "data/sequencer.wal.jsonl".into(),
            matching_snapshot: "data/matching.snapshot.jsonl".into(),
            trade_journal: "data/trade_journal.wal.jsonl".into(),
            trade_settlement: "data/trade_settlement.wal.jsonl".into(),
            instruments_registry: "data/instruments.registry.jsonl".into(),
            funding_rates: "data/funding_rates.jsonl".into(),
            risk_automation_audit: "data/risk_automation.audit.jsonl".into(),
            liquidation_queue: "data/liquidation.queue.jsonl".into(),
            liquidation_auction: "data/liquidation.auction.jsonl".into(),
            adl_governance: "data/adl.governance.jsonl".into(),
            liquidation_policy: "data/liquidation.policy.jsonl".into(),
            index_price: "data/index.price.jsonl".into(),
            index_source_policy: "data/index.source.policy.jsonl".into(),
            position_cost_state: "data/position.cost.state.jsonl".into(),
            position_cost_events: "data/position.cost.events.jsonl".into(),
            governance_actions: "data/governance.actions.jsonl".into(),
            withdrawals: "data/withdrawals.wal.jsonl".into(),
            fee_tiers: "data/fee_tiers.jsonl".into(),
            transfers: "data/transfers.wal.jsonl".into(),
            stop_orders: "data/stop_orders.wal.jsonl".into(),
            address_whitelist: "data/address_whitelist.wal.jsonl".into(),
        }
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            automation_enabled: false,
            liquidation_interval_secs: 30,
            funding_interval_secs: 60,
            liquidation_worker_interval_secs: 5,
            liquidation_auction_window_secs: 15,
            liquidator_user_id: "system-liquidator".into(),
            maintenance_margin_bps: 1_000,
            liquidation_penalty_bps: 500,
            position_cost_resync_interval_ms: 60_000,
        }
    }
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec![
                "http://127.0.0.1:5173".into(),
                "http://localhost:5173".into(),
            ],
        }
    }
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            orderbook_snapshot_interval_ms: 200,
            max_connections: 1024,
        }
    }
}

// ── Loading ──────────────────────────────────────────────────

impl ExchangeConfig {
    /// Load configuration from TOML file (if exists) then overlay environment
    /// variables.  The file path itself can be set via `EXCHANGE_CONFIG_PATH`.
    pub fn load() -> Self {
        let explicit_path = env::var("EXCHANGE_CONFIG_PATH").ok();
        let config_path = explicit_path
            .clone()
            .unwrap_or_else(|| "config/exchange.toml".to_string());

        let mut cfg = if Path::new(&config_path).exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(contents) => match toml::from_str::<ExchangeConfig>(&contents) {
                    Ok(c) => {
                        tracing::info!(path = %config_path, "loaded configuration from file");
                        c
                    }
                    Err(e) => {
                        if explicit_path.is_some() {
                            panic!("EXCHANGE_CONFIG_PATH={config_path} exists but failed to parse: {e}");
                        }
                        tracing::warn!(error = %e, path = %config_path, "failed to parse config file, using defaults");
                        ExchangeConfig::default()
                    }
                },
                Err(e) => {
                    if explicit_path.is_some() {
                        panic!("EXCHANGE_CONFIG_PATH={config_path} exists but failed to read: {e}");
                    }
                    tracing::warn!(error = %e, path = %config_path, "failed to read config file, using defaults");
                    ExchangeConfig::default()
                }
            }
        } else if explicit_path.is_some() {
            panic!("EXCHANGE_CONFIG_PATH={config_path} does not exist");
        } else {
            tracing::info!("no config file found, using defaults + env vars");
            ExchangeConfig::default()
        };

        // Environment variable overrides (backward-compatible with existing env vars).
        Self::apply_env_overrides(&mut cfg);
        cfg
    }

    /// Validate configuration values. Returns a list of problems (empty = valid).
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.server.bind_host.is_empty() {
            problems.push("server.bind_host must not be empty".into());
        }
        if self.server.bind_port == 0 {
            problems.push("server.bind_port must be > 0".into());
        }
        if self.server.max_body_size_bytes == 0 {
            problems.push("server.max_body_size_bytes must be > 0".into());
        }
        if self.wal.data_dir.is_empty() {
            problems.push("wal.data_dir must not be empty".into());
        }
        if self.wal.rotation_max_entries > 0 && self.wal.rotation_max_entries < 100 {
            problems.push("wal.rotation_max_entries must be >= 100 (or 0 to disable)".into());
        }
        if self.risk.maintenance_margin_bps < 0 || self.risk.maintenance_margin_bps > 10_000 {
            problems.push("risk.maintenance_margin_bps must be in [0, 10000]".into());
        }
        if self.risk.liquidation_penalty_bps < 0 || self.risk.liquidation_penalty_bps > 10_000 {
            problems.push("risk.liquidation_penalty_bps must be in [0, 10000]".into());
        }
        if self.risk.automation_enabled && self.risk.liquidation_interval_secs == 0 {
            problems.push(
                "risk.liquidation_interval_secs must be > 0 when automation is enabled".into(),
            );
        }
        if self.risk.automation_enabled && self.risk.funding_interval_secs == 0 {
            problems
                .push("risk.funding_interval_secs must be > 0 when automation is enabled".into());
        }
        if self.websocket.max_connections == 0 {
            problems.push("websocket.max_connections must be > 0".into());
        }
        if self.websocket.orderbook_snapshot_interval_ms == 0 {
            problems.push("websocket.orderbook_snapshot_interval_ms must be > 0".into());
        }
        // Validate WAL paths do not escape the data directory.
        let wal_paths = [
            ("wal.ledger", &self.wal.ledger),
            ("wal.sequencer", &self.wal.sequencer),
            ("wal.matching_snapshot", &self.wal.matching_snapshot),
            ("wal.trade_journal", &self.wal.trade_journal),
            ("wal.trade_settlement", &self.wal.trade_settlement),
            ("wal.instruments_registry", &self.wal.instruments_registry),
            ("wal.funding_rates", &self.wal.funding_rates),
            ("wal.risk_automation_audit", &self.wal.risk_automation_audit),
            ("wal.liquidation_queue", &self.wal.liquidation_queue),
            ("wal.liquidation_auction", &self.wal.liquidation_auction),
            ("wal.adl_governance", &self.wal.adl_governance),
            ("wal.liquidation_policy", &self.wal.liquidation_policy),
            ("wal.index_price", &self.wal.index_price),
            ("wal.index_source_policy", &self.wal.index_source_policy),
            ("wal.position_cost_state", &self.wal.position_cost_state),
            ("wal.position_cost_events", &self.wal.position_cost_events),
            ("wal.governance_actions", &self.wal.governance_actions),
            ("wal.withdrawals", &self.wal.withdrawals),
            ("wal.fee_tiers", &self.wal.fee_tiers),
            ("wal.transfers", &self.wal.transfers),
            ("wal.address_whitelist", &self.wal.address_whitelist),
        ];
        for (name, path) in &wal_paths {
            if path.contains("..") {
                problems.push(format!(
                    "{name} must not contain '..' (path traversal): {path}"
                ));
            }
            if path.trim().is_empty() {
                problems.push(format!("{name} must not be empty"));
            }
        }
        problems
    }

    fn apply_env_overrides(cfg: &mut ExchangeConfig) {
        if let Ok(v) = env::var("API_BIND_HOST") {
            cfg.server.bind_host = v;
        }
        if let Ok(v) = env::var("API_BIND_PORT") {
            if let Ok(p) = v.parse::<u16>() {
                cfg.server.bind_port = p;
            }
        }
        if let Ok(v) = env::var("RUST_LOG") {
            cfg.server.log_level = v;
        }
        // WAL paths
        if let Ok(v) = env::var("WAL_ROTATION_MAX_ENTRIES") {
            if let Ok(n) = v.parse() {
                cfg.wal.rotation_max_entries = n;
            }
        }
        if let Ok(v) = env::var("WAL_GROUP_COMMIT_SIZE") {
            if let Ok(n) = v.parse() {
                cfg.wal.group_commit_size = n;
            }
        }
        macro_rules! env_path {
            ($env:expr, $field:expr) => {
                if let Ok(v) = env::var($env) {
                    $field = v;
                }
            };
        }
        env_path!("LEDGER_WAL_PATH", cfg.wal.ledger);
        env_path!("SEQUENCER_WAL_PATH", cfg.wal.sequencer);
        env_path!("MATCHING_SNAPSHOT_WAL_PATH", cfg.wal.matching_snapshot);
        env_path!("TRADE_JOURNAL_WAL_PATH", cfg.wal.trade_journal);
        env_path!("TRADE_SETTLEMENT_WAL_PATH", cfg.wal.trade_settlement);
        env_path!(
            "INSTRUMENTS_REGISTRY_WAL_PATH",
            cfg.wal.instruments_registry
        );
        env_path!("FUNDING_RATES_WAL_PATH", cfg.wal.funding_rates);
        env_path!(
            "RISK_AUTOMATION_AUDIT_WAL_PATH",
            cfg.wal.risk_automation_audit
        );
        env_path!("LIQUIDATION_QUEUE_WAL_PATH", cfg.wal.liquidation_queue);
        env_path!("LIQUIDATION_AUCTION_WAL_PATH", cfg.wal.liquidation_auction);
        env_path!("ADL_GOVERNANCE_WAL_PATH", cfg.wal.adl_governance);
        env_path!("LIQUIDATION_POLICY_WAL_PATH", cfg.wal.liquidation_policy);
        env_path!("INDEX_PRICE_WAL_PATH", cfg.wal.index_price);
        env_path!("INDEX_SOURCE_POLICY_WAL_PATH", cfg.wal.index_source_policy);
        env_path!("POSITION_COST_STATE_WAL_PATH", cfg.wal.position_cost_state);
        env_path!("POSITION_COST_EVENT_WAL_PATH", cfg.wal.position_cost_events);
        env_path!("GOVERNANCE_ACTION_WAL_PATH", cfg.wal.governance_actions);
        env_path!("WITHDRAWALS_WAL_PATH", cfg.wal.withdrawals);
        env_path!("FEE_TIERS_WAL_PATH", cfg.wal.fee_tiers);
        env_path!("TRANSFERS_WAL_PATH", cfg.wal.transfers);
        env_path!("ADDRESS_WHITELIST_WAL_PATH", cfg.wal.address_whitelist);
        // Risk
        if let Ok(v) = env::var("RISK_AUTOMATION_ENABLED") {
            cfg.risk.automation_enabled = matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }
        macro_rules! env_u64 {
            ($env:expr, $field:expr) => {
                if let Ok(v) = env::var($env) {
                    if let Ok(n) = v.parse::<u64>() {
                        $field = n;
                    }
                }
            };
        }
        macro_rules! env_i64 {
            ($env:expr, $field:expr) => {
                if let Ok(v) = env::var($env) {
                    if let Ok(n) = v.parse::<i64>() {
                        $field = n;
                    }
                }
            };
        }
        env_u64!(
            "RISK_LIQUIDATION_INTERVAL_SECS",
            cfg.risk.liquidation_interval_secs
        );
        env_u64!("RISK_FUNDING_INTERVAL_SECS", cfg.risk.funding_interval_secs);
        env_u64!(
            "RISK_LIQUIDATION_WORKER_INTERVAL_SECS",
            cfg.risk.liquidation_worker_interval_secs
        );
        env_i64!(
            "RISK_LIQUIDATION_AUCTION_WINDOW_SECS",
            cfg.risk.liquidation_auction_window_secs
        );
        env_path!(
            "RISK_AUTOMATION_LIQUIDATOR_USER_ID",
            cfg.risk.liquidator_user_id
        );
        env_i64!(
            "RISK_AUTOMATION_MAINTENANCE_MARGIN_BPS",
            cfg.risk.maintenance_margin_bps
        );
        env_i64!(
            "RISK_AUTOMATION_LIQUIDATION_PENALTY_BPS",
            cfg.risk.liquidation_penalty_bps
        );
        env_u64!(
            "POSITION_COST_RESYNC_INTERVAL_MS",
            cfg.risk.position_cost_resync_interval_ms
        );
        // WebSocket
        env_u64!(
            "WS_ORDERBOOK_SNAPSHOT_INTERVAL_MS",
            cfg.websocket.orderbook_snapshot_interval_ms
        );
        if let Ok(v) = env::var("WS_MAX_CONNECTIONS") {
            if let Ok(n) = v.parse::<usize>() {
                cfg.websocket.max_connections = n;
            }
        }
        // Server extras
        env_u64!("API_MAX_BODY_SIZE_BYTES", cfg.server.max_body_size_bytes);
        env_u64!("API_REQUEST_TIMEOUT_SECS", cfg.server.request_timeout_secs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let cfg = ExchangeConfig::default();
        assert_eq!(cfg.server.bind_port, 3030);
        assert_eq!(cfg.wal.rotation_max_entries, 100_000);
        assert!(!cfg.risk.automation_enabled);
        assert_eq!(cfg.cors.allowed_origins.len(), 2);
        assert!(
            cfg.validate().is_empty(),
            "default config should pass validation"
        );
    }

    #[test]
    fn toml_round_trip() {
        let toml_str = r#"
[server]
bind_host = "0.0.0.0"
bind_port = 8080

[risk]
automation_enabled = true
maintenance_margin_bps = 500
"#;
        let cfg: ExchangeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.server.bind_host, "0.0.0.0");
        assert_eq!(cfg.server.bind_port, 8080);
        assert!(cfg.risk.automation_enabled);
        assert_eq!(cfg.risk.maintenance_margin_bps, 500);
        // Fields not in TOML get defaults
        assert_eq!(cfg.wal.ledger, "data/ledger.wal.jsonl");
    }

    #[test]
    fn validation_catches_bad_port() {
        let mut cfg = ExchangeConfig::default();
        cfg.server.bind_port = 0;
        let problems = cfg.validate();
        assert!(!problems.is_empty());
        assert!(problems.iter().any(|p| p.contains("bind_port")));
    }

    #[test]
    fn validation_catches_empty_host() {
        let mut cfg = ExchangeConfig::default();
        cfg.server.bind_host = String::new();
        let problems = cfg.validate();
        assert!(problems.iter().any(|p| p.contains("bind_host")));
    }

    #[test]
    fn validation_catches_automation_with_zero_interval() {
        let mut cfg = ExchangeConfig::default();
        cfg.risk.automation_enabled = true;
        cfg.risk.liquidation_interval_secs = 0;
        let problems = cfg.validate();
        assert!(problems
            .iter()
            .any(|p| p.contains("liquidation_interval_secs")));
    }

    #[test]
    fn websocket_config_defaults() {
        let cfg = ExchangeConfig::default();
        assert_eq!(cfg.websocket.orderbook_snapshot_interval_ms, 200);
        assert_eq!(cfg.websocket.max_connections, 1024);
    }

    #[test]
    fn validation_catches_margin_bps_out_of_range() {
        let mut cfg = ExchangeConfig::default();
        cfg.risk.maintenance_margin_bps = 11_000;
        assert!(cfg
            .validate()
            .iter()
            .any(|p| p.contains("maintenance_margin_bps")));

        cfg.risk.maintenance_margin_bps = -1;
        assert!(cfg
            .validate()
            .iter()
            .any(|p| p.contains("maintenance_margin_bps")));
    }

    #[test]
    fn validation_catches_penalty_bps_out_of_range() {
        let mut cfg = ExchangeConfig::default();
        cfg.risk.liquidation_penalty_bps = 15_000;
        assert!(cfg
            .validate()
            .iter()
            .any(|p| p.contains("liquidation_penalty_bps")));
    }

    #[test]
    fn validation_catches_low_rotation_max_entries() {
        let mut cfg = ExchangeConfig::default();
        cfg.wal.rotation_max_entries = 50;
        assert!(cfg
            .validate()
            .iter()
            .any(|p| p.contains("rotation_max_entries")));
    }

    #[test]
    fn validation_allows_zero_rotation_to_disable() {
        let mut cfg = ExchangeConfig::default();
        cfg.wal.rotation_max_entries = 0;
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn validation_catches_zero_ws_snapshot_interval() {
        let mut cfg = ExchangeConfig::default();
        cfg.websocket.orderbook_snapshot_interval_ms = 0;
        assert!(cfg
            .validate()
            .iter()
            .any(|p| p.contains("orderbook_snapshot_interval_ms")));
    }

    #[test]
    fn validation_catches_zero_max_body_size() {
        let mut cfg = ExchangeConfig::default();
        cfg.server.max_body_size_bytes = 0;
        assert!(cfg
            .validate()
            .iter()
            .any(|p| p.contains("max_body_size_bytes")));
    }

    #[test]
    fn validation_catches_wal_path_traversal() {
        let mut cfg = ExchangeConfig::default();
        cfg.wal.ledger = "../../../etc/shadow".to_string();
        let problems = cfg.validate();
        assert!(problems.iter().any(|p| p.contains("path traversal")));
    }

    #[test]
    fn validation_catches_empty_wal_path() {
        let mut cfg = ExchangeConfig::default();
        cfg.wal.sequencer = "".to_string();
        let problems = cfg.validate();
        assert!(problems.iter().any(|p| p.contains("must not be empty")));
    }
}
