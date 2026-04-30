use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct BetaControlPlaneConfig {
    pub(crate) enabled: bool,
    pub(crate) require_whitelist: bool,
    pub(crate) updated_by: String,
    pub(crate) recorded_at: DateTime<Utc>,
}

impl Default for BetaControlPlaneConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            require_whitelist: false,
            updated_by: "system".to_string(),
            recorded_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct BetaUserControl {
    pub(crate) user_id: String,
    #[serde(default)]
    pub(crate) whitelisted: bool,
    #[serde(default)]
    pub(crate) max_cash_balance: Option<i64>,
    #[serde(default)]
    pub(crate) max_open_orders: Option<u32>,
    pub(crate) updated_by: String,
    pub(crate) recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct BetaMarketControl {
    pub(crate) market_id: String,
    #[serde(default)]
    pub(crate) max_order_notional: Option<i64>,
    #[serde(default)]
    pub(crate) max_leverage: Option<u32>,
    pub(crate) updated_by: String,
    pub(crate) recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "record_type", rename_all = "snake_case")]
pub(crate) enum BetaControlRecord {
    ControlPlane(BetaControlPlaneConfig),
    User(BetaUserControl),
    Market(BetaMarketControl),
}

pub(crate) struct BetaControlStore {
    control_plane: Mutex<BetaControlPlaneConfig>,
    users: DashMap<String, BetaUserControl>,
    markets: DashMap<String, BetaMarketControl>,
    store: Arc<dyn persistence::WalStore<BetaControlRecord>>,
    write_lock: Mutex<()>,
}

impl BetaControlStore {
    pub(crate) fn new(
        store: Arc<dyn persistence::WalStore<BetaControlRecord>>,
    ) -> anyhow::Result<Self> {
        let result = Self {
            control_plane: Mutex::new(BetaControlPlaneConfig::default()),
            users: DashMap::new(),
            markets: DashMap::new(),
            store,
            write_lock: Mutex::new(()),
        };
        for record in result.store.entries()? {
            match record {
                BetaControlRecord::ControlPlane(value) => {
                    *result.control_plane.lock() = value;
                }
                BetaControlRecord::User(value) => {
                    result.users.insert(value.user_id.clone(), value);
                }
                BetaControlRecord::Market(value) => {
                    result.markets.insert(value.market_id.clone(), value);
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<BetaControlRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Self::new(store)
    }

    pub(crate) fn control_plane(&self) -> BetaControlPlaneConfig {
        self.control_plane.lock().clone()
    }

    pub(crate) fn upsert_control_plane(
        &self,
        control_plane: BetaControlPlaneConfig,
    ) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        self.store
            .append(&BetaControlRecord::ControlPlane(control_plane.clone()))?;
        *self.control_plane.lock() = control_plane;
        Ok(())
    }

    pub(crate) fn user(&self, user_id: &str) -> Option<BetaUserControl> {
        self.users.get(user_id).map(|entry| entry.value().clone())
    }

    pub(crate) fn list_users(&self) -> Vec<BetaUserControl> {
        let mut items: Vec<_> = self
            .users
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        items.sort_by(|lhs, rhs| lhs.user_id.cmp(&rhs.user_id));
        items
    }

    pub(crate) fn upsert_user(&self, user: BetaUserControl) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        self.store.append(&BetaControlRecord::User(user.clone()))?;
        self.users.insert(user.user_id.clone(), user);
        Ok(())
    }

    pub(crate) fn market(&self, market_id: &str) -> Option<BetaMarketControl> {
        self.markets
            .get(market_id)
            .map(|entry| entry.value().clone())
    }

    pub(crate) fn list_markets(&self) -> Vec<BetaMarketControl> {
        let mut items: Vec<_> = self
            .markets
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        items.sort_by(|lhs, rhs| lhs.market_id.cmp(&rhs.market_id));
        items
    }

    pub(crate) fn upsert_market(&self, market: BetaMarketControl) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock();
        self.store
            .append(&BetaControlRecord::Market(market.clone()))?;
        self.markets.insert(market.market_id.clone(), market);
        Ok(())
    }

    pub(crate) fn allows_user(&self, user_id: &str) -> bool {
        let control_plane = self.control_plane();
        if !control_plane.enabled || !control_plane.require_whitelist {
            return true;
        }
        self.user(user_id).is_some_and(|entry| entry.whitelisted)
    }
}
