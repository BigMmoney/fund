use super::*;

static ADMIN_ACTION_AUDIT_STORE: OnceLock<Arc<AdminActionAuditStore>> = OnceLock::new();

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct AdminActionAuditRecord {
    pub(crate) action: String,
    pub(crate) request_id: String,
    pub(crate) subject: String,
    pub(crate) role: PrincipalRole,
    pub(crate) session_id: Option<String>,
    pub(crate) recorded_at: DateTime<Utc>,
}

pub(crate) struct AdminActionAuditStore {
    store: Arc<dyn persistence::WalStore<AdminActionAuditRecord>>,
}

impl AdminActionAuditStore {
    pub(crate) fn new(store: Arc<dyn persistence::WalStore<AdminActionAuditRecord>>) -> Self {
        Self { store }
    }

    pub(crate) fn open_jsonl(path: impl Into<std::path::PathBuf>) -> anyhow::Result<Self> {
        let store: Arc<dyn persistence::WalStore<AdminActionAuditRecord>> =
            Arc::new(JsonlFileWal::new(path)?);
        Ok(Self::new(store))
    }

    pub(crate) fn append(&self, record: AdminActionAuditRecord) -> anyhow::Result<()> {
        self.store.append(&record)
    }

    pub(crate) fn list_recent(
        &self,
        limit: usize,
        action: Option<&str>,
        subject: Option<&str>,
        admin_only: bool,
    ) -> anyhow::Result<Vec<AdminActionAuditRecord>> {
        let mut items: Vec<_> = self
            .store
            .entries()?
            .into_iter()
            .filter(|record| {
                (!admin_only || record.role == PrincipalRole::Admin)
                    && action.is_none_or(|value| record.action == value)
                    && subject.is_none_or(|value| record.subject == value)
            })
            .collect();
        items.sort_by(|lhs, rhs| rhs.recorded_at.cmp(&lhs.recorded_at));
        items.truncate(limit);
        Ok(items)
    }
}

pub(crate) fn initialize_admin_action_audit_store(store: Arc<AdminActionAuditStore>) {
    let _ = ADMIN_ACTION_AUDIT_STORE.set(store);
}

pub(crate) fn append_admin_action_audit(
    action: &str,
    request_id: &str,
    principal: &AuthenticatedPrincipal,
) -> anyhow::Result<()> {
    let Some(store) = ADMIN_ACTION_AUDIT_STORE.get() else {
        return Ok(());
    };
    store.append(AdminActionAuditRecord {
        action: action.to_string(),
        request_id: request_id.to_string(),
        subject: principal.subject.clone(),
        role: principal.role,
        session_id: principal.session_id.clone(),
        recorded_at: Utc::now(),
    })
}
