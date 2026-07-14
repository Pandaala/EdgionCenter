//! Compatibility adapters between the current monolith and `center-core`.
//!
//! These adapters keep behavior stable while the SQL and runtime modules move
//! into their final workspace crates. They intentionally contain translation
//! only; application behavior remains in the existing modules.

use std::sync::Arc;

use edgion_center_core::{
    Action, Authorizer, ControllerDirectory, ControllerId, ControllerPhase, ControllerRecord,
    ControllerRegistration, CoreError, CoreResult, Decision, EvictionOutcome, Principal, SessionId,
};

use crate::common::authz::{AuthzStore, Principal as LegacyPrincipal};
use crate::store::Store;

/// SQL-backed controller projection with persistent session fencing.
pub(crate) struct SqlControllerDirectory {
    store: Arc<Store>,
}

impl SqlControllerDirectory {
    pub(crate) fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    fn adapter_error(error: impl std::fmt::Display) -> CoreError {
        CoreError::Adapter(error.to_string())
    }
}

#[async_trait::async_trait]
impl ControllerDirectory for SqlControllerDirectory {
    async fn upsert_registration(&self, registration: ControllerRegistration) -> CoreResult<()> {
        self.store
            .project_controller_registration(&registration)
            .await
            .map_err(Self::adapter_error)?;
        Ok(())
    }

    async fn mark_offline(
        &self,
        id: &ControllerId,
        observed_session: &SessionId,
    ) -> CoreResult<()> {
        self.store
            .mark_session_offline(id.as_str(), observed_session.as_str())
            .await
            .map_err(Self::adapter_error)?;
        Ok(())
    }

    async fn list(&self) -> CoreResult<Vec<ControllerRecord>> {
        let rows = self
            .store
            .list_controllers()
            .await
            .map_err(Self::adapter_error)?;
        rows.into_iter()
            .map(|row| {
                let controller_id = ControllerId::new(row.controller_id)?;
                Ok(ControllerRecord {
                    current_session_id: row.session_id.map(SessionId::new).transpose()?,
                    controller_id,
                    cluster: row.cluster,
                    environments: row.env,
                    tags: row.tag,
                    connected_replica: row.connected_replica,
                    phase: if row.online {
                        ControllerPhase::Online
                    } else {
                        ControllerPhase::Offline
                    },
                    last_seen_unix_ms: row.last_seen_at.saturating_mul(1000),
                })
            })
            .collect()
    }

    async fn evict(&self, id: &ControllerId) -> CoreResult<EvictionOutcome> {
        let removed = self
            .store
            .evict_controller(id.as_str())
            .await
            .map_err(Self::adapter_error)?;
        Ok(if removed {
            EvictionOutcome::Evicted
        } else {
            EvictionOutcome::AlreadyAbsent
        })
    }
}

/// Adapts the current permission-set resolver to action-based authorization.
#[allow(dead_code)] // Wired when shared API authorization moves in KN-03.
pub(crate) struct RuntimeAuthorizer {
    inner: Arc<dyn AuthzStore>,
}

#[allow(dead_code)] // Constructor becomes live when KN-03 swaps API authorization.
impl RuntimeAuthorizer {
    fn new(inner: Arc<dyn AuthzStore>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl Authorizer for RuntimeAuthorizer {
    async fn authorize(&self, principal: &Principal, action: &Action) -> CoreResult<Decision> {
        let permissions = self
            .inner
            .permissions_for(&LegacyPrincipal {
                subject: principal.subject.clone(),
                provider: principal.provider.clone(),
            })
            .await;
        Ok(if permissions.contains(&action.permission) {
            Decision::allow()
        } else {
            Decision::deny(format!("missing permission {}", action.permission))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::authz::{allow_all::AllowAllAuthz, PermissionSet};

    fn registration(controller: &str, session: &str) -> ControllerRegistration {
        ControllerRegistration {
            controller_id: ControllerId::new(controller).unwrap(),
            session_id: SessionId::new(session).unwrap(),
            cluster: "cluster-a".to_string(),
            environments: vec!["prod".to_string()],
            tags: vec!["edge".to_string()],
            connected_replica: Some("center-0".to_string()),
            observed_at_unix_ms: 1,
        }
    }

    #[tokio::test]
    async fn sql_directory_contract_fences_stale_sessions_and_evicts_idempotently() {
        let directory =
            SqlControllerDirectory::new(Arc::new(Store::open_in_memory().await.unwrap()));
        directory
            .upsert_registration(registration("c1", "s1"))
            .await
            .unwrap();
        directory
            .upsert_registration(registration("c1", "s2"))
            .await
            .unwrap();

        let stale = SessionId::new("s1").unwrap();
        let current = SessionId::new("s2").unwrap();
        let id = ControllerId::new("c1").unwrap();
        directory.mark_offline(&id, &stale).await.unwrap();
        assert_eq!(
            directory.list().await.unwrap()[0].phase,
            ControllerPhase::Online
        );

        directory.mark_offline(&id, &current).await.unwrap();
        let record = directory.list().await.unwrap().pop().unwrap();
        assert_eq!(record.phase, ControllerPhase::Offline);
        assert_eq!(record.current_session_id, None);
        assert_eq!(
            directory.evict(&id).await.unwrap(),
            EvictionOutcome::Evicted
        );
        assert_eq!(
            directory.evict(&id).await.unwrap(),
            EvictionOutcome::AlreadyAbsent
        );
    }

    struct ReadOnlyAuthz;

    #[async_trait::async_trait]
    impl AuthzStore for ReadOnlyAuthz {
        async fn permissions_for(&self, _principal: &LegacyPrincipal) -> PermissionSet {
            PermissionSet::from_keys(["controllers:read".to_string()])
        }
    }

    fn principal() -> Principal {
        Principal {
            subject: "alice".to_string(),
            provider: "oidc".to_string(),
            groups: vec!["operators".to_string()],
        }
    }

    #[tokio::test]
    async fn authorizer_contract_preserves_allow_all_and_explicit_permissions() {
        let action = Action {
            permission: "controllers:write".to_string(),
            controller_id: None,
        };
        let allow_all = RuntimeAuthorizer::new(Arc::new(AllowAllAuthz));
        assert!(
            allow_all
                .authorize(&principal(), &action)
                .await
                .unwrap()
                .allowed
        );

        let read_only = RuntimeAuthorizer::new(Arc::new(ReadOnlyAuthz));
        let decision = read_only.authorize(&principal(), &action).await.unwrap();
        assert!(!decision.allowed);
        assert_eq!(
            decision.reason.as_deref(),
            Some("missing permission controllers:write")
        );
    }
}
