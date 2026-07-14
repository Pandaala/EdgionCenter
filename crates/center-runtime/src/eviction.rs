use std::sync::Arc;

use async_trait::async_trait;
use edgion_center_core::{ControllerId, CoreError, CoreResult, EvictionTarget, OwnershipFence};

use crate::{
    aggregator::ResourceAggregator,
    federation::registry::ControllerRegistry,
    internal_forwarding::{ForwardErrorKind, OwnerForwarding},
    watch_cache::CenterSyncClient,
};

#[async_trait]
pub trait ControllerEviction: Send + Sync {
    async fn evict_live(
        &self,
        id: &ControllerId,
        target: Option<&EvictionTarget>,
    ) -> CoreResult<()>;
}

/// Test/minimal-composition implementation. Production binaries compose a
/// local or owner-aware evictor explicitly.
pub struct NoopControllerEvictor;

#[async_trait]
impl ControllerEviction for NoopControllerEvictor {
    async fn evict_live(
        &self,
        _id: &ControllerId,
        _target: Option<&EvictionTarget>,
    ) -> CoreResult<()> {
        Ok(())
    }
}

/// Owns the process-local side of eviction. Removing the registry entry fences
/// and cancels the stream before clearing derived read models.
pub struct LocalControllerEvictor {
    registry: ControllerRegistry,
    aggregator: Arc<ResourceAggregator>,
    sync_client: Arc<CenterSyncClient>,
}

impl LocalControllerEvictor {
    pub fn new(
        registry: ControllerRegistry,
        aggregator: Arc<ResourceAggregator>,
        sync_client: Arc<CenterSyncClient>,
    ) -> Self {
        Self {
            registry,
            aggregator,
            sync_client,
        }
    }

    pub fn evict_unfenced(&self, controller_id: &str) {
        self.registry.remove(controller_id);
        self.aggregator.remove(controller_id);
        self.sync_client
            .plugin_metadata
            .remove_controller(controller_id);
    }

    pub fn evict_fenced(
        &self,
        controller_id: &str,
        holder: &str,
        fence: &OwnershipFence,
    ) -> CoreResult<()> {
        let Some(session) = self.registry.get_session(controller_id) else {
            return Err(CoreError::Conflict(
                "controller session moved before eviction".to_string(),
            ));
        };
        if !session.matches_ownership(holder, fence) {
            return Err(CoreError::Conflict(
                "controller ownership changed before eviction".to_string(),
            ));
        }
        self.evict_unfenced(controller_id);
        Ok(())
    }
}

pub struct OwnerAwareControllerEvictor {
    local: Arc<LocalControllerEvictor>,
    forwarding: Option<OwnerForwarding>,
}

impl OwnerAwareControllerEvictor {
    pub fn local(local: Arc<LocalControllerEvictor>) -> Self {
        Self {
            local,
            forwarding: None,
        }
    }

    pub fn with_owner_forwarding(
        local: Arc<LocalControllerEvictor>,
        forwarding: OwnerForwarding,
    ) -> Self {
        Self {
            local,
            forwarding: Some(forwarding),
        }
    }
}

#[async_trait]
impl ControllerEviction for OwnerAwareControllerEvictor {
    async fn evict_live(
        &self,
        id: &ControllerId,
        target: Option<&EvictionTarget>,
    ) -> CoreResult<()> {
        let Some(forwarding) = &self.forwarding else {
            self.local.evict_unfenced(id.as_str());
            return Ok(());
        };
        let Some(expected_fence) = target.and_then(|target| target.ownership_fence.as_ref()) else {
            // Kubernetes records without a live ownership fence have no
            // authoritative stream to cancel.
            return Ok(());
        };
        let mut previous = None;
        for attempt in 0..2 {
            let Some(route) = forwarding.locator.locate(id).await? else {
                // The target Lease has already disappeared. A stale local task
                // is fenced by Lease maintenance and must not justify an
                // unfenced removal of a possible newer session.
                return Ok(());
            };
            if &route.ownership_fence != expected_fence {
                // A session newer than the durable eviction linearization point
                // is explicitly outside this DELETE operation.
                return Ok(());
            }
            if previous
                .as_ref()
                .is_some_and(|old: &edgion_center_core::ControllerOwnerRoute| old == &route)
            {
                return Err(CoreError::Conflict(
                    "controller ownership did not advance during eviction".to_string(),
                ));
            }
            let result = if route.holder == forwarding.local_holder {
                self.local
                    .evict_fenced(id.as_str(), &route.holder, &route.ownership_fence)
                    .map_err(|error| crate::internal_forwarding::ForwardError {
                        kind: ForwardErrorKind::StaleOwnership,
                        message: error.to_string(),
                    })
            } else {
                forwarding
                    .transport
                    .forward_evict(&route, id.as_str())
                    .await
            };
            match result {
                Ok(()) => return Ok(()),
                Err(error) if attempt == 0 && error.kind == ForwardErrorKind::StaleOwnership => {
                    previous = Some(route);
                }
                Err(error) => {
                    return Err(match error.kind {
                        ForwardErrorKind::StaleOwnership => CoreError::Conflict(error.message),
                        _ => CoreError::Adapter(error.message),
                    });
                }
            }
        }
        unreachable!("bounded eviction forwarding loop returns")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        federation::proto::{command_request::Command, CommandResponse, HttpProxyResponse},
        internal_forwarding::{ForwardError, ForwardHttpOperation, InternalForwardTransport},
        metadata_store::CenterMetaDataStore,
        watch_cache::{CenterSyncClient, CenterWatchCacheRegistry},
    };
    use edgion_center_core::{ControllerOwnerLocator, ControllerOwnerRoute, EvictionTarget};
    use parking_lot::Mutex;
    use std::{
        collections::VecDeque,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    struct Routes(Mutex<VecDeque<ControllerOwnerRoute>>);

    #[async_trait]
    impl ControllerOwnerLocator for Routes {
        async fn locate(&self, _: &ControllerId) -> CoreResult<Option<ControllerOwnerRoute>> {
            Ok(self.0.lock().pop_front())
        }
    }

    struct Transport(AtomicUsize);

    #[async_trait]
    impl InternalForwardTransport for Transport {
        async fn forward_command(
            &self,
            _: &ControllerOwnerRoute,
            _: &str,
            _: Command,
            _: Duration,
        ) -> Result<CommandResponse, ForwardError> {
            unreachable!()
        }

        async fn forward_http(
            &self,
            _: &ControllerOwnerRoute,
            _: &str,
            _: ForwardHttpOperation,
            _: Duration,
        ) -> Result<HttpProxyResponse, ForwardError> {
            unreachable!()
        }

        async fn forward_evict(
            &self,
            _: &ControllerOwnerRoute,
            _: &str,
        ) -> Result<(), ForwardError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn route(epoch: u64) -> ControllerOwnerRoute {
        ControllerOwnerRoute {
            holder: format!("center-{epoch}/uid-{epoch}"),
            endpoint: format!("https://center-{epoch}:12252"),
            ownership_fence: OwnershipFence {
                token: format!("token-{epoch}"),
                epoch,
            },
        }
    }

    fn local_evictor() -> Arc<LocalControllerEvictor> {
        let metadata = Arc::new(CenterMetaDataStore::new());
        Arc::new(LocalControllerEvictor::new(
            ControllerRegistry::new(),
            Arc::new(ResourceAggregator::new()),
            Arc::new(CenterSyncClient {
                plugin_metadata: CenterWatchCacheRegistry::new(metadata),
            }),
        ))
    }

    #[tokio::test]
    async fn durable_eviction_never_cancels_newer_reconnected_fence() {
        let transport = Arc::new(Transport(AtomicUsize::new(0)));
        let locator = Arc::new(Routes(Mutex::new(VecDeque::from([route(2)]))));
        let evictor = OwnerAwareControllerEvictor::with_owner_forwarding(
            local_evictor(),
            OwnerForwarding {
                locator,
                transport: transport.clone(),
                local_holder: "center-request/uid".to_string(),
            },
        );
        let target = EvictionTarget {
            session_id: None,
            connected_replica: Some("center-1/uid-1".to_string()),
            ownership_fence: Some(route(1).ownership_fence),
        };
        evictor
            .evict_live(&ControllerId::new("c1").unwrap(), Some(&target))
            .await
            .unwrap();
        assert_eq!(transport.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn exact_durable_fence_is_forwarded_once() {
        let transport = Arc::new(Transport(AtomicUsize::new(0)));
        let locator = Arc::new(Routes(Mutex::new(VecDeque::from([route(1)]))));
        let evictor = OwnerAwareControllerEvictor::with_owner_forwarding(
            local_evictor(),
            OwnerForwarding {
                locator,
                transport: transport.clone(),
                local_holder: "center-request/uid".to_string(),
            },
        );
        let target = EvictionTarget {
            session_id: None,
            connected_replica: Some("center-1/uid-1".to_string()),
            ownership_fence: Some(route(1).ownership_fence),
        };
        evictor
            .evict_live(&ControllerId::new("c1").unwrap(), Some(&target))
            .await
            .unwrap();
        assert_eq!(transport.0.load(Ordering::SeqCst), 1);
    }
}
