use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};

use async_trait::async_trait;
use edgion_center_adapter_credential_files::{
    CredentialPurpose, MountedCredentialResolver, ResolveCredentialRequest,
};
use edgion_center_adapter_route53::{
    Route53Api, Route53ChangeBatch, Route53ChangeInfo, Route53CreateHostedZoneRequest,
    Route53CreateHostedZoneResult, Route53CursorKey, Route53DnsAdapter, Route53DnssecInfo,
    Route53HostedZone, Route53HostedZonePage, Route53MutationReceiptKey,
    Route53MutationReceiptVerifier, Route53RecordCursor, Route53RecordPage,
};
use edgion_center_app::api::route53_dns::{
    Route53ChangeReceiptDto, Route53DnsAdminError, Route53DnsWriteAdminService,
    Route53RecordBatchChangeDto, Route53RecordChangeBatchRequest, Route53RecordMutationGuardDto,
    Route53RecordSetDeleteRequest, Route53RecordSetDesiredDto, Route53RecordSetKey,
    Route53RecordSetPutRequest, SharedRoute53DnsWriteAdminService,
};
use edgion_center_core::{
    CloudProvider, CloudResourceId, CoreError, CoreResult, CredentialRef, CredentialSource,
    DnsChangeId, DnsGuardStrength, DnsMutationGuard, DnsProvider, DnsRecordChange, DnsRecordSetKey,
    DnsZoneId, DnsZoneRef, ManagementPolicy, NormalizedProviderError, ObservedDnsRecordSet,
    ProviderAccount, ProviderAccountScope, ProviderAccountStore, ProviderErrorCategory,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::dns_admin_service::{ApiFactory, ProductionApiFactory};

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 180;
const DEFAULT_GLOBAL_CONCURRENCY: usize = 4;
const MAX_GLOBAL_CONCURRENCY: usize = 16;
const DEFAULT_ACCOUNT_CONCURRENCY: usize = 1;
const MAX_ACCOUNT_CONCURRENCY: usize = 2;
const MAX_TRACKED_ACCOUNTS: usize = 1_024;

/// Independently default-off Route 53 RRset mutation surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Route53DnsWriteConfig {
    pub enabled: bool,
    pub cursor_key_ref: Option<String>,
    pub mutation_receipt_key_ref: Option<String>,
    pub operation_timeout_secs: u64,
    pub global_concurrency: usize,
    pub per_account_concurrency: usize,
}

impl Default for Route53DnsWriteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cursor_key_ref: None,
            mutation_receipt_key_ref: None,
            operation_timeout_secs: DEFAULT_TIMEOUT_SECS,
            global_concurrency: DEFAULT_GLOBAL_CONCURRENCY,
            per_account_concurrency: DEFAULT_ACCOUNT_CONCURRENCY,
        }
    }
}

pub fn compose_dns_write_admin(
    config: &Route53DnsWriteConfig,
    account_store: Option<Arc<dyn ProviderAccountStore>>,
    mounted_resolver: Option<Arc<MountedCredentialResolver>>,
) -> CoreResult<Option<SharedRoute53DnsWriteAdminService>> {
    let Some((cursor_key_ref, mutation_receipt_key_ref)) = validated_config(config)? else {
        return Ok(None);
    };
    let account_store = account_store.ok_or_else(|| {
        CoreError::Conflict("Route 53 DNS write requires a provider account store".into())
    })?;
    let mounted_resolver = mounted_resolver.ok_or_else(|| {
        CoreError::Conflict("Route 53 DNS write requires mounted credentials".into())
    })?;
    Ok(Some(Arc::new(Route53DnsWriteService {
        account_store,
        mounted_resolver,
        cursor_key_ref,
        mutation_receipt_key_ref,
        timeout: Duration::from_secs(config.operation_timeout_secs),
        global: Arc::new(Semaphore::new(config.global_concurrency)),
        per_account_concurrency: config.per_account_concurrency,
        accounts: Mutex::new(HashMap::new()),
        api_factory: Arc::new(ProductionApiFactory),
    })))
}

struct Route53DnsWriteService {
    account_store: Arc<dyn ProviderAccountStore>,
    mounted_resolver: Arc<MountedCredentialResolver>,
    cursor_key_ref: CredentialRef,
    mutation_receipt_key_ref: CredentialRef,
    timeout: Duration,
    global: Arc<Semaphore>,
    per_account_concurrency: usize,
    accounts: Mutex<HashMap<CloudResourceId, Weak<Semaphore>>>,
    api_factory: Arc<dyn ApiFactory>,
}

#[derive(Clone)]
struct RequestAuthority {
    account: ProviderAccount,
    cursor_revision: String,
    mutation_revision: String,
}

struct AuthorityFence {
    account_store: Arc<dyn ProviderAccountStore>,
    mounted_resolver: Arc<MountedCredentialResolver>,
    cursor_key_ref: CredentialRef,
    mutation_receipt_key_ref: CredentialRef,
    expected: RequestAuthority,
}

impl AuthorityFence {
    async fn is_current(&self) -> bool {
        let Ok(Some(account)) = self
            .account_store
            .get(&self.expected.account.metadata.id)
            .await
        else {
            return false;
        };
        if account != self.expected.account {
            return false;
        }
        let cursor = resolve_revision(
            &self.mounted_resolver,
            &account,
            &self.cursor_key_ref,
            CredentialPurpose::Route53DnsCursorHmac,
        )
        .await;
        let mutation = resolve_revision(
            &self.mounted_resolver,
            &account,
            &self.mutation_receipt_key_ref,
            CredentialPurpose::Route53DnsMutationReceiptHmac,
        )
        .await;
        matches!(
            (cursor, mutation),
            (Ok(cursor), Ok(mutation))
                if cursor == self.expected.cursor_revision
                    && mutation == self.expected.mutation_revision
        ) && matches!(
            self.account_store.get(&account.metadata.id).await,
            Ok(Some(final_account)) if final_account == account
        )
    }
}

struct DispatchTrackingApi {
    inner: Arc<dyn Route53Api>,
    authority: Arc<AuthorityFence>,
    dispatched: Arc<AtomicBool>,
}

#[async_trait]
impl Route53Api for DispatchTrackingApi {
    fn verified_account_id(&self) -> &str {
        self.inner.verified_account_id()
    }

    async fn create_hosted_zone(
        &self,
        request: &Route53CreateHostedZoneRequest,
    ) -> Result<Route53CreateHostedZoneResult, NormalizedProviderError> {
        self.inner.create_hosted_zone(request).await
    }

    async fn get_hosted_zone(
        &self,
        zone_id: &str,
    ) -> Result<Option<Route53HostedZone>, NormalizedProviderError> {
        self.inner.get_hosted_zone(zone_id).await
    }

    async fn list_hosted_zones(
        &self,
        marker: Option<&str>,
        max_items: u16,
    ) -> Result<Route53HostedZonePage, NormalizedProviderError> {
        self.inner.list_hosted_zones(marker, max_items).await
    }

    async fn list_record_sets(
        &self,
        zone_id: &str,
        cursor: Option<&Route53RecordCursor>,
        max_items: u16,
    ) -> Result<Route53RecordPage, NormalizedProviderError> {
        self.inner
            .list_record_sets(zone_id, cursor, max_items)
            .await
    }

    async fn change_record_sets(
        &self,
        zone_id: &str,
        batch: &Route53ChangeBatch,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        if !self.authority.is_current().await {
            return Err(provider_error(
                ProviderErrorCategory::Validation,
                "route53_authority_changed_before_dispatch",
            ));
        }
        self.dispatched.store(true, Ordering::Release);
        self.inner.change_record_sets(zone_id, batch).await
    }

    async fn get_change(
        &self,
        change_id: &str,
    ) -> Result<Option<Route53ChangeInfo>, NormalizedProviderError> {
        self.inner.get_change(change_id).await
    }

    async fn delete_hosted_zone(
        &self,
        zone_id: &str,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        self.inner.delete_hosted_zone(zone_id).await
    }

    async fn get_dnssec(
        &self,
        zone_id: &str,
    ) -> Result<Route53DnssecInfo, NormalizedProviderError> {
        self.inner.get_dnssec(zone_id).await
    }

    async fn enable_hosted_zone_dnssec(
        &self,
        zone_id: &str,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        self.inner.enable_hosted_zone_dnssec(zone_id).await
    }

    async fn disable_hosted_zone_dnssec(
        &self,
        zone_id: &str,
    ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
        self.inner.disable_hosted_zone_dnssec(zone_id).await
    }
}

impl Route53DnsWriteService {
    async fn prepare(
        &self,
        account_id: &CloudResourceId,
        dispatched: Arc<AtomicBool>,
    ) -> Result<(Route53DnsAdapter, Arc<AuthorityFence>), Route53DnsAdminError> {
        let account = self
            .account_store
            .get(account_id)
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?
            .ok_or(Route53DnsAdminError::NotFound)?;
        validate_write_account(account_id, &account)?;
        let (cursor_key, cursor_revision) = self.resolve_cursor_key(&account).await?;
        let (mutation_key, mutation_revision) = self.resolve_mutation_key(&account).await?;
        let expected = RequestAuthority {
            account: account.clone(),
            cursor_revision,
            mutation_revision,
        };
        let authority = Arc::new(AuthorityFence {
            account_store: self.account_store.clone(),
            mounted_resolver: self.mounted_resolver.clone(),
            cursor_key_ref: self.cursor_key_ref.clone(),
            mutation_receipt_key_ref: self.mutation_receipt_key_ref.clone(),
            expected,
        });
        let api = self
            .api_factory
            .build()
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        let api = Arc::new(DispatchTrackingApi {
            inner: api,
            authority: authority.clone(),
            dispatched: dispatched.clone(),
        });
        let adapter = Route53DnsAdapter::new_with_record_write_key(
            account.metadata.id.clone(),
            &account.spec,
            api,
            cursor_key,
            mutation_key,
        )
        .map_err(|_| Route53DnsAdminError::Unavailable)?;
        Ok((adapter, authority))
    }

    async fn prepare_observe_after_receipt_preflight(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        receipt: &DnsChangeId,
    ) -> Result<(Route53DnsAdapter, Arc<AuthorityFence>), Route53DnsAdminError> {
        let account = self
            .account_store
            .get(account_id)
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?
            .ok_or(Route53DnsAdminError::NotFound)?;
        validate_write_account(account_id, &account)?;
        let (mutation_key, mutation_revision) = self.resolve_mutation_key(&account).await?;
        let verifier = Route53MutationReceiptVerifier::new(
            account.metadata.id.clone(),
            &account.spec,
            mutation_key,
        )
        .map_err(|_| Route53DnsAdminError::Unavailable)?;
        verifier
            .validate_scope(account_id, zone_id, receipt)
            .map_err(map_observe_error)?;

        let (cursor_key, cursor_revision) = self.resolve_cursor_key(&account).await?;
        let authority = Arc::new(AuthorityFence {
            account_store: self.account_store.clone(),
            mounted_resolver: self.mounted_resolver.clone(),
            cursor_key_ref: self.cursor_key_ref.clone(),
            mutation_receipt_key_ref: self.mutation_receipt_key_ref.clone(),
            expected: RequestAuthority {
                account: account.clone(),
                cursor_revision,
                mutation_revision,
            },
        });
        if !authority.is_current().await {
            return Err(Route53DnsAdminError::Unavailable);
        }
        let api = self
            .api_factory
            .build()
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        let api = Arc::new(DispatchTrackingApi {
            inner: api,
            authority: authority.clone(),
            dispatched: Arc::new(AtomicBool::new(false)),
        });
        let adapter = Route53DnsAdapter::new_with_record_write_verifier(
            account.metadata.id.clone(),
            &account.spec,
            api,
            cursor_key,
            verifier,
        )
        .map_err(|_| Route53DnsAdminError::Unavailable)?;
        Ok((adapter, authority))
    }

    async fn resolve_cursor_key(
        &self,
        account: &ProviderAccount,
    ) -> Result<(Route53CursorKey, String), Route53DnsAdminError> {
        let (bytes, revision) = self
            .resolve_key(
                account,
                &self.cursor_key_ref,
                CredentialPurpose::Route53DnsCursorHmac,
            )
            .await?;
        Ok((
            Route53CursorKey::new(bytes).map_err(|_| Route53DnsAdminError::Unavailable)?,
            revision,
        ))
    }

    async fn resolve_mutation_key(
        &self,
        account: &ProviderAccount,
    ) -> Result<(Route53MutationReceiptKey, String), Route53DnsAdminError> {
        let (bytes, revision) = self
            .resolve_key(
                account,
                &self.mutation_receipt_key_ref,
                CredentialPurpose::Route53DnsMutationReceiptHmac,
            )
            .await?;
        Ok((
            Route53MutationReceiptKey::new(bytes).map_err(|_| Route53DnsAdminError::Unavailable)?,
            revision,
        ))
    }

    async fn resolve_key(
        &self,
        account: &ProviderAccount,
        credential_ref: &CredentialRef,
        purpose: CredentialPurpose,
    ) -> Result<([u8; 32], String), Route53DnsAdminError> {
        let resolved = self
            .mounted_resolver
            .resolve(ResolveCredentialRequest {
                provider_account_id: &account.metadata.id,
                provider: &CloudProvider::Aws,
                purpose,
                credential_ref,
            })
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        let revision = resolved.revision().as_str().to_owned();
        let bytes = resolved
            .with_bytes(|value| <[u8; 32]>::try_from(value).ok())
            .ok_or(Route53DnsAdminError::Unavailable)?;
        Ok((bytes, revision))
    }

    fn account_semaphore(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<Arc<Semaphore>, Route53DnsAdminError> {
        let mut accounts = self
            .accounts
            .lock()
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        if let Some(existing) = accounts.get(account_id).and_then(Weak::upgrade) {
            return Ok(existing);
        }
        accounts.retain(|_, value| value.strong_count() > 0);
        if accounts.len() >= MAX_TRACKED_ACCOUNTS {
            return Err(Route53DnsAdminError::Unavailable);
        }
        let semaphore = Arc::new(Semaphore::new(self.per_account_concurrency));
        accounts.insert(account_id.clone(), Arc::downgrade(&semaphore));
        Ok(semaphore)
    }

    async fn admission(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit), Route53DnsAdminError> {
        let account = self
            .account_semaphore(account_id)?
            .acquire_owned()
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        let global = self
            .global
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Route53DnsAdminError::Unavailable)?;
        Ok((account, global))
    }

    async fn mutate(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        request: MutationRequest<'_>,
    ) -> Result<Route53ChangeReceiptDto, Route53DnsAdminError> {
        let dispatched = Arc::new(AtomicBool::new(false));
        let result = tokio::time::timeout(self.timeout, async {
            let (_account_permit, _global_permit) = self.admission(account_id).await?;
            let (adapter, authority) = self.prepare(account_id, dispatched.clone()).await?;
            let zone = observe_exact_zone_direct(&adapter, account_id, zone_id).await?;
            let inventory = observe_all_records(&adapter, &zone.zone).await?;
            let changes = build_changes(&zone.zone, inventory, request)?;
            let receipt = adapter
                .apply_record_changes(&zone.zone, &changes, DnsGuardStrength::Atomic)
                .await
                .map_err(|error| map_mutation_error(error, dispatched.load(Ordering::Acquire)))?;
            if !authority.is_current().await {
                return Err(Route53DnsAdminError::UnknownOutcome);
            }
            Route53ChangeReceiptDto::from_core(receipt)
                .map_err(|_| Route53DnsAdminError::UnknownOutcome)
        })
        .await;
        match result {
            Ok(value) => value,
            Err(_) if dispatched.load(Ordering::Acquire) => {
                Err(Route53DnsAdminError::UnknownOutcome)
            }
            Err(_) => Err(Route53DnsAdminError::Unavailable),
        }
    }
}

enum MutationRequest<'a> {
    Put(&'a Route53RecordSetKey, &'a Route53RecordSetPutRequest),
    Delete(&'a Route53RecordSetKey, &'a Route53RecordSetDeleteRequest),
    Batch(&'a Route53RecordChangeBatchRequest),
}

#[async_trait]
impl Route53DnsWriteAdminService for Route53DnsWriteService {
    async fn put_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &Route53RecordSetKey,
        request: &Route53RecordSetPutRequest,
    ) -> Result<Route53ChangeReceiptDto, Route53DnsAdminError> {
        self.mutate(account_id, zone_id, MutationRequest::Put(key, request))
            .await
    }

    async fn delete_record_set(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        key: &Route53RecordSetKey,
        request: &Route53RecordSetDeleteRequest,
    ) -> Result<Route53ChangeReceiptDto, Route53DnsAdminError> {
        self.mutate(account_id, zone_id, MutationRequest::Delete(key, request))
            .await
    }

    async fn apply_change_batch(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        request: &Route53RecordChangeBatchRequest,
    ) -> Result<Route53ChangeReceiptDto, Route53DnsAdminError> {
        request
            .validate_shape()
            .map_err(|_| Route53DnsAdminError::InvalidRequest)?;
        self.mutate(account_id, zone_id, MutationRequest::Batch(request))
            .await
    }

    async fn get_change(
        &self,
        account_id: &CloudResourceId,
        zone_id: &DnsZoneId,
        receipt: &DnsChangeId,
    ) -> Result<Route53ChangeReceiptDto, Route53DnsAdminError> {
        receipt
            .validate()
            .map_err(|_| Route53DnsAdminError::InvalidRequest)?;
        let result = tokio::time::timeout(self.timeout, async {
            let (_account_permit, _global_permit) = self.admission(account_id).await?;
            let (adapter, authority) = self
                .prepare_observe_after_receipt_preflight(account_id, zone_id, receipt)
                .await?;
            let zone = observe_exact_zone_direct(&adapter, account_id, zone_id).await?;
            if !authority.is_current().await {
                return Err(Route53DnsAdminError::Unavailable);
            }
            let receipt = adapter
                .observe_change(&zone.zone, receipt)
                .await
                .map_err(map_observe_error)?;
            if !authority.is_current().await {
                return Err(Route53DnsAdminError::Unavailable);
            }
            Route53ChangeReceiptDto::from_core(receipt)
                .map_err(|_| Route53DnsAdminError::InvalidProviderObservation)
        })
        .await;
        result.unwrap_or(Err(Route53DnsAdminError::Unavailable))
    }
}

async fn observe_exact_zone_direct(
    adapter: &Route53DnsAdapter,
    account_id: &CloudResourceId,
    zone_id: &DnsZoneId,
) -> Result<edgion_center_core::ObservedDnsZone, Route53DnsAdminError> {
    adapter
        .observe_zone_by_id(account_id, zone_id)
        .await
        .map_err(crate::dns_admin_service::map_provider_error)?
        .ok_or(Route53DnsAdminError::NotFound)
}

async fn observe_all_records(
    adapter: &Route53DnsAdapter,
    zone: &DnsZoneRef,
) -> Result<BTreeMap<DnsRecordSetKey, ObservedDnsRecordSet>, Route53DnsAdminError> {
    let mut records = BTreeMap::new();
    for record in adapter
        .observe_all_record_sets(zone)
        .await
        .map_err(crate::dns_admin_service::map_provider_error)?
    {
        if record.zone != *zone
            || records
                .insert(record.record_set.key.clone(), record)
                .is_some()
        {
            return Err(Route53DnsAdminError::InvalidProviderObservation);
        }
    }
    Ok(records)
}

fn build_changes(
    zone: &DnsZoneRef,
    mut inventory: BTreeMap<DnsRecordSetKey, ObservedDnsRecordSet>,
    request: MutationRequest<'_>,
) -> Result<Vec<DnsRecordChange>, Route53DnsAdminError> {
    let capacity = match request {
        MutationRequest::Batch(batch) => batch.changes.len(),
        _ => 1,
    };
    let mut changes = Vec::with_capacity(capacity);
    match request {
        MutationRequest::Put(key, desired) => {
            push_put_change(
                zone,
                &mut inventory,
                &mut changes,
                key,
                &desired.desired,
                &desired.guard,
            )?;
        }
        MutationRequest::Delete(key, request) => {
            push_delete_change(
                &mut inventory,
                &mut changes,
                key,
                &request.expected_revision,
            )?;
        }
        MutationRequest::Batch(batch) => {
            for change in &batch.changes {
                match change {
                    Route53RecordBatchChangeDto::Create { key, desired } => {
                        let key = key.key();
                        push_put_change(
                            zone,
                            &mut inventory,
                            &mut changes,
                            &key,
                            desired,
                            &Route53RecordMutationGuardDto::MustNotExist {},
                        )?;
                    }
                    Route53RecordBatchChangeDto::Replace {
                        key,
                        expected_revision,
                        desired,
                    } => {
                        let key = key.key();
                        push_put_change(
                            zone,
                            &mut inventory,
                            &mut changes,
                            &key,
                            desired,
                            &Route53RecordMutationGuardDto::MatchRevision {
                                revision: expected_revision.clone(),
                            },
                        )?;
                    }
                    Route53RecordBatchChangeDto::Delete {
                        key,
                        expected_revision,
                    } => {
                        let key = key.key();
                        push_delete_change(&mut inventory, &mut changes, &key, expected_revision)?;
                    }
                }
            }
        }
    }
    Ok(changes)
}

fn push_put_change(
    zone: &DnsZoneRef,
    inventory: &mut BTreeMap<DnsRecordSetKey, ObservedDnsRecordSet>,
    changes: &mut Vec<DnsRecordChange>,
    key: &Route53RecordSetKey,
    desired: &Route53RecordSetDesiredDto,
    guard: &Route53RecordMutationGuardDto,
) -> Result<(), Route53DnsAdminError> {
    let record_set = desired
        .record_set(key)
        .map_err(|_| Route53DnsAdminError::InvalidRequest)?;
    record_set
        .validate(zone)
        .map_err(|_| Route53DnsAdminError::InvalidRequest)?;
    let previous = inventory.remove(&key.core());
    match (guard, previous) {
        (Route53RecordMutationGuardDto::MustNotExist {}, None) => {
            changes.push(DnsRecordChange::Create {
                record_set,
                guard: DnsMutationGuard::MustNotExist,
            });
            Ok(())
        }
        (Route53RecordMutationGuardDto::MatchRevision { revision }, Some(previous))
            if &previous.revision == revision =>
        {
            changes.push(DnsRecordChange::Replace {
                previous,
                desired: record_set,
                guard: DnsMutationGuard::MatchObserved {
                    revision: revision.clone(),
                },
            });
            Ok(())
        }
        _ => Err(Route53DnsAdminError::Conflict),
    }
}

fn push_delete_change(
    inventory: &mut BTreeMap<DnsRecordSetKey, ObservedDnsRecordSet>,
    changes: &mut Vec<DnsRecordChange>,
    key: &Route53RecordSetKey,
    expected_revision: &edgion_center_core::DnsRecordRevision,
) -> Result<(), Route53DnsAdminError> {
    expected_revision
        .validate()
        .map_err(|_| Route53DnsAdminError::InvalidRequest)?;
    let previous = inventory
        .remove(&key.core())
        .ok_or(Route53DnsAdminError::Conflict)?;
    if &previous.revision != expected_revision {
        return Err(Route53DnsAdminError::Conflict);
    }
    changes.push(DnsRecordChange::Delete {
        previous,
        guard: DnsMutationGuard::MatchObserved {
            revision: expected_revision.clone(),
        },
    });
    Ok(())
}

async fn resolve_revision(
    resolver: &MountedCredentialResolver,
    account: &ProviderAccount,
    credential_ref: &CredentialRef,
    purpose: CredentialPurpose,
) -> CoreResult<String> {
    resolver
        .resolve(ResolveCredentialRequest {
            provider_account_id: &account.metadata.id,
            provider: &CloudProvider::Aws,
            purpose,
            credential_ref,
        })
        .await
        .map(|credential| credential.revision().as_str().to_owned())
        .map_err(|_| CoreError::Adapter("Route 53 signing authority is unavailable".into()))
}

fn validate_write_account(
    requested_account_id: &CloudResourceId,
    account: &ProviderAccount,
) -> Result<(), Route53DnsAdminError> {
    if &account.metadata.id != requested_account_id
        || account.metadata.management_policy != ManagementPolicy::Managed
        || account.spec.provider != CloudProvider::Aws
        || !matches!(
            account.spec.scope.as_ref(),
            Some(ProviderAccountScope::Aws { .. })
        )
        || account.spec.credential_source != CredentialSource::Ambient
    {
        return Err(Route53DnsAdminError::InvalidRequest);
    }
    Ok(())
}

fn map_mutation_error(error: NormalizedProviderError, dispatched: bool) -> Route53DnsAdminError {
    if error.code() == "route53_authority_changed_before_dispatch" {
        return Route53DnsAdminError::Unavailable;
    }
    match error.category() {
        ProviderErrorCategory::UnknownOutcome => Route53DnsAdminError::UnknownOutcome,
        ProviderErrorCategory::Conflict | ProviderErrorCategory::NotFound => {
            Route53DnsAdminError::Conflict
        }
        ProviderErrorCategory::Validation => Route53DnsAdminError::InvalidRequest,
        _ if dispatched => Route53DnsAdminError::UnknownOutcome,
        _ => Route53DnsAdminError::Unavailable,
    }
}

fn map_observe_error(error: NormalizedProviderError) -> Route53DnsAdminError {
    match error.category() {
        ProviderErrorCategory::NotFound => Route53DnsAdminError::NotFound,
        ProviderErrorCategory::Validation => Route53DnsAdminError::InvalidRequest,
        _ => Route53DnsAdminError::Unavailable,
    }
}

fn provider_error(category: ProviderErrorCategory, code: &'static str) -> NormalizedProviderError {
    NormalizedProviderError::new(category, code, code, None, None).expect("static provider error")
}

fn validated_config(
    config: &Route53DnsWriteConfig,
) -> CoreResult<Option<(CredentialRef, CredentialRef)>> {
    if !config.enabled {
        return Ok(None);
    }
    if !(1..=MAX_TIMEOUT_SECS).contains(&config.operation_timeout_secs)
        || !(1..=MAX_GLOBAL_CONCURRENCY).contains(&config.global_concurrency)
        || !(1..=MAX_ACCOUNT_CONCURRENCY).contains(&config.per_account_concurrency)
        || config.per_account_concurrency > config.global_concurrency
    {
        return Err(CoreError::Conflict(
            "Route 53 DNS write limits are invalid".into(),
        ));
    }
    let cursor = CredentialRef::new(
        config
            .cursor_key_ref
            .clone()
            .ok_or_else(|| CoreError::Conflict("Route 53 DNS cursor key is required".into()))?,
    )?;
    let mutation =
        CredentialRef::new(config.mutation_receipt_key_ref.clone().ok_or_else(|| {
            CoreError::Conflict("Route 53 DNS mutation receipt key is required".into())
        })?)?;
    if cursor == mutation {
        return Err(CoreError::Conflict(
            "Route 53 DNS signing key references must be distinct".into(),
        ));
    }
    Ok(Some((cursor, mutation)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, sync::atomic::AtomicUsize};

    use edgion_center_adapter_credential_files::{
        MountedCredentialBinding, MountedCredentialConfig,
    };
    use edgion_center_adapter_route53::Route53RecordSet;
    use edgion_center_app::api::route53_dns::{
        Route53BatchRecordKeyDto, Route53RecordTtlDto, Route53RecordType, Route53RecordValueDto,
    };
    use edgion_center_core::{
        provider_account_from_desired, AbsoluteDnsName, CloudProvider, DeletionPolicy,
        DnsOwnerName, ProviderAccountCreateResult, ProviderAccountDesired, ProviderAccountPage,
        ProviderAccountPageRequest, ProviderAccountReplaceResult, ProviderAccountSpec,
        ZoneVisibility,
    };

    const CENTER_ACCOUNT: &str = "aws-main";
    const AWS_ACCOUNT: &str = "123456789012";
    const ZONE_ID: &str = "Z1234567890EXAMPLE";

    #[test]
    fn write_config_is_default_off_strict_bounded_and_domain_separated() {
        assert!(validated_config(&Route53DnsWriteConfig::default())
            .unwrap()
            .is_none());
        assert!(serde_yaml::from_str::<Route53DnsWriteConfig>("unknown: true\n").is_err());
        let valid: Route53DnsWriteConfig = serde_yaml::from_str(
            "enabled: true\ncursor_key_ref: aws/cursor\nmutation_receipt_key_ref: aws/mutation\noperation_timeout_secs: 60\nglobal_concurrency: 4\nper_account_concurrency: 1\n",
        )
        .unwrap();
        assert!(validated_config(&valid).unwrap().is_some());
        assert!(validated_config(&Route53DnsWriteConfig {
            mutation_receipt_key_ref: Some("aws/cursor".into()),
            ..valid.clone()
        })
        .is_err());
        assert!(validated_config(&Route53DnsWriteConfig {
            operation_timeout_secs: 0,
            ..valid
        })
        .is_err());
    }

    struct Store(Mutex<Option<ProviderAccount>>);

    #[async_trait]
    impl ProviderAccountStore for Store {
        async fn create(
            &self,
            _: &CloudResourceId,
            _: &ProviderAccountDesired,
        ) -> CoreResult<ProviderAccountCreateResult> {
            unreachable!()
        }

        async fn get(&self, _: &CloudResourceId) -> CoreResult<Option<ProviderAccount>> {
            Ok(self.0.lock().unwrap().clone())
        }

        async fn list(&self, _: &ProviderAccountPageRequest) -> CoreResult<ProviderAccountPage> {
            unreachable!()
        }

        async fn replace_if_generation(
            &self,
            _: &CloudResourceId,
            _: u64,
            _: &ProviderAccountDesired,
        ) -> CoreResult<ProviderAccountReplaceResult> {
            unreachable!()
        }
    }

    fn account(generation: u64, policy: ManagementPolicy) -> ProviderAccount {
        provider_account_from_desired(
            CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
            generation,
            &ProviderAccountDesired {
                display_name: "AWS main".into(),
                owner: None,
                labels: BTreeMap::new(),
                management_policy: policy,
                deletion_policy: DeletionPolicy::Retain,
                spec: ProviderAccountSpec {
                    provider: CloudProvider::Aws,
                    scope: Some(ProviderAccountScope::Aws {
                        account_id: AWS_ACCOUNT.into(),
                    }),
                    credential_source: CredentialSource::Ambient,
                },
            },
        )
        .unwrap()
    }

    async fn mounted() -> (tempfile::TempDir, Arc<MountedCredentialResolver>) {
        let directory = tempfile::tempdir().unwrap();
        tokio::fs::write(directory.path().join("revision.key"), [7_u8; 32])
            .await
            .unwrap();
        tokio::fs::write(directory.path().join("cursor.key"), [8_u8; 32])
            .await
            .unwrap();
        tokio::fs::write(directory.path().join("mutation.key"), [9_u8; 32])
            .await
            .unwrap();
        let resolver = MountedCredentialResolver::from_config(&MountedCredentialConfig {
            enabled: true,
            root_directory: Some(directory.path().to_string_lossy().into_owned()),
            revision_key_file: Some("revision.key".into()),
            bindings: vec![
                MountedCredentialBinding {
                    credential_ref: "aws/route53-cursor".into(),
                    provider_account_id: CENTER_ACCOUNT.into(),
                    provider: CloudProvider::Aws,
                    purpose: CredentialPurpose::Route53DnsCursorHmac,
                    file: "cursor.key".into(),
                },
                MountedCredentialBinding {
                    credential_ref: "aws/route53-mutation".into(),
                    provider_account_id: CENTER_ACCOUNT.into(),
                    provider: CloudProvider::Aws,
                    purpose: CredentialPurpose::Route53DnsMutationReceiptHmac,
                    file: "mutation.key".into(),
                },
            ],
        })
        .await
        .unwrap()
        .unwrap();
        (directory, Arc::new(resolver))
    }

    struct FakeApi {
        records: Vec<Route53RecordSet>,
        record_calls: AtomicUsize,
        change_calls: AtomicUsize,
        get_change_calls: AtomicUsize,
        build_calls: Arc<AtomicUsize>,
        list_delay: Duration,
        change_delay: Duration,
        rotate_before_dispatch_at_record_call: Option<usize>,
        rotate_after_dispatch: bool,
        rotate_key_before_dispatch: Option<PathBuf>,
        rotate_key_after_dispatch: Option<PathBuf>,
        store: Arc<Store>,
        last_change: Mutex<Option<Route53ChangeInfo>>,
        last_batch: Mutex<Option<Route53ChangeBatch>>,
    }

    impl FakeApi {
        fn new(store: Arc<Store>, records: Vec<Route53RecordSet>) -> Self {
            Self {
                records,
                record_calls: AtomicUsize::new(0),
                change_calls: AtomicUsize::new(0),
                get_change_calls: AtomicUsize::new(0),
                build_calls: Arc::new(AtomicUsize::new(0)),
                list_delay: Duration::ZERO,
                change_delay: Duration::ZERO,
                rotate_before_dispatch_at_record_call: None,
                rotate_after_dispatch: false,
                rotate_key_before_dispatch: None,
                rotate_key_after_dispatch: None,
                store,
                last_change: Mutex::new(None),
                last_batch: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl Route53Api for FakeApi {
        fn verified_account_id(&self) -> &str {
            AWS_ACCOUNT
        }

        async fn create_hosted_zone(
            &self,
            _: &Route53CreateHostedZoneRequest,
        ) -> Result<Route53CreateHostedZoneResult, NormalizedProviderError> {
            unreachable!()
        }

        async fn get_hosted_zone(
            &self,
            zone_id: &str,
        ) -> Result<Option<Route53HostedZone>, NormalizedProviderError> {
            tokio::time::sleep(self.list_delay).await;
            Ok((zone_id == ZONE_ID).then(|| Route53HostedZone {
                id: format!("/hostedzone/{ZONE_ID}"),
                name: "example.com.".into(),
                private_zone: false,
                caller_reference: "caller".into(),
                resource_record_set_count: self.records.len() as u64,
                name_servers: vec![],
                has_linked_service: false,
                has_unsupported_features: false,
            }))
        }

        async fn list_hosted_zones(
            &self,
            _: Option<&str>,
            _: u16,
        ) -> Result<Route53HostedZonePage, NormalizedProviderError> {
            tokio::time::sleep(self.list_delay).await;
            Ok(Route53HostedZonePage {
                items: vec![Route53HostedZone {
                    id: format!("/hostedzone/{ZONE_ID}"),
                    name: "example.com.".into(),
                    private_zone: false,
                    caller_reference: "caller".into(),
                    resource_record_set_count: self.records.len() as u64,
                    name_servers: vec![],
                    has_linked_service: false,
                    has_unsupported_features: false,
                }],
                is_truncated: false,
                next_marker: None,
            })
        }

        async fn list_record_sets(
            &self,
            _: &str,
            _: Option<&Route53RecordCursor>,
            _: u16,
        ) -> Result<Route53RecordPage, NormalizedProviderError> {
            tokio::time::sleep(self.list_delay).await;
            let call = self.record_calls.fetch_add(1, Ordering::SeqCst) + 1;
            if self.rotate_before_dispatch_at_record_call == Some(call) {
                *self.store.0.lock().unwrap() = Some(account(2, ManagementPolicy::Managed));
            }
            if call == 2 {
                if let Some(path) = &self.rotate_key_before_dispatch {
                    std::fs::write(path, [10_u8; 32]).unwrap();
                }
            }
            Ok(Route53RecordPage {
                items: self.records.clone(),
                is_truncated: false,
                next: None,
            })
        }

        async fn change_record_sets(
            &self,
            _: &str,
            batch: &Route53ChangeBatch,
        ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
            self.change_calls.fetch_add(1, Ordering::SeqCst);
            *self.last_batch.lock().unwrap() = Some(batch.clone());
            if self.rotate_after_dispatch {
                *self.store.0.lock().unwrap() = Some(account(2, ManagementPolicy::Managed));
            }
            if let Some(path) = &self.rotate_key_after_dispatch {
                std::fs::write(path, [11_u8; 32]).unwrap();
            }
            tokio::time::sleep(self.change_delay).await;
            let change = Route53ChangeInfo {
                id: "/change/C123".into(),
                status: "PENDING".into(),
                submitted_at_unix_seconds: 1,
                comment: Some(batch.comment.clone()),
            };
            *self.last_change.lock().unwrap() = Some(change.clone());
            Ok(change)
        }

        async fn get_change(
            &self,
            _: &str,
        ) -> Result<Option<Route53ChangeInfo>, NormalizedProviderError> {
            self.get_change_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.last_change.lock().unwrap().clone().map(|mut change| {
                change.status = "INSYNC".into();
                change
            }))
        }

        async fn delete_hosted_zone(
            &self,
            _: &str,
        ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
            unreachable!()
        }

        async fn get_dnssec(&self, _: &str) -> Result<Route53DnssecInfo, NormalizedProviderError> {
            unreachable!()
        }

        async fn enable_hosted_zone_dnssec(
            &self,
            _: &str,
        ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
            unreachable!()
        }

        async fn disable_hosted_zone_dnssec(
            &self,
            _: &str,
        ) -> Result<Route53ChangeInfo, NormalizedProviderError> {
            unreachable!()
        }
    }

    struct Factory {
        api: Arc<dyn Route53Api>,
        build_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ApiFactory for Factory {
        async fn build(&self) -> Result<Arc<dyn Route53Api>, NormalizedProviderError> {
            self.build_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.api.clone())
        }
    }

    fn service(
        store: Arc<Store>,
        resolver: Arc<MountedCredentialResolver>,
        api: Arc<FakeApi>,
        timeout: Duration,
    ) -> Route53DnsWriteService {
        Route53DnsWriteService {
            account_store: store,
            mounted_resolver: resolver,
            cursor_key_ref: CredentialRef::new("aws/route53-cursor").unwrap(),
            mutation_receipt_key_ref: CredentialRef::new("aws/route53-mutation").unwrap(),
            timeout,
            global: Arc::new(Semaphore::new(4)),
            per_account_concurrency: 1,
            accounts: Mutex::new(HashMap::new()),
            api_factory: Arc::new(Factory {
                api: api.clone(),
                build_calls: api.build_calls.clone(),
            }),
        }
    }

    fn existing_record() -> Route53RecordSet {
        Route53RecordSet {
            name: "old.example.com.".into(),
            record_type: "A".into(),
            ttl: Some(60),
            resource_records: vec!["192.0.2.10".into()],
            alias_target: None,
            set_identifier: None,
            weight: None,
            failover: None,
            region: None,
            geolocation: None,
            multivalue_answer: None,
            health_check_id: None,
            traffic_policy_instance_id: None,
            has_cidr_routing_config: false,
            has_geoproximity_location: false,
        }
    }

    fn create_request() -> Route53RecordSetPutRequest {
        Route53RecordSetPutRequest {
            guard: Route53RecordMutationGuardDto::MustNotExist {},
            desired: Route53RecordSetDesiredDto {
                ttl: Route53RecordTtlDto::Seconds(60),
                values: vec![Route53RecordValueDto::A {
                    address: "192.0.2.20".parse().unwrap(),
                }],
                alias_target: None,
                routing_policy: None,
                health_check_id: None,
            },
        }
    }

    fn key(owner: &str) -> Route53RecordSetKey {
        Route53RecordSetKey {
            owner: DnsOwnerName::new(owner).unwrap(),
            record_type: Route53RecordType::A,
            set_identifier: None,
        }
    }

    async fn current_revision(
        api: Arc<dyn Route53Api>,
        account: &ProviderAccount,
        record_key: &Route53RecordSetKey,
    ) -> edgion_center_core::DnsRecordRevision {
        let adapter = Route53DnsAdapter::new(
            account.metadata.id.clone(),
            &account.spec,
            api,
            Route53CursorKey::new([8_u8; 32]).unwrap(),
        )
        .unwrap();
        adapter
            .get_record_set(
                &DnsZoneRef {
                    provider_account_id: account.metadata.id.clone(),
                    provider: CloudProvider::Aws,
                    zone_id: DnsZoneId::new(ZONE_ID).unwrap(),
                    apex: AbsoluteDnsName::new("example.com").unwrap(),
                    visibility: ZoneVisibility::Public,
                },
                &record_key.core(),
            )
            .await
            .unwrap()
            .unwrap()
            .revision
    }

    #[tokio::test]
    async fn create_dispatches_once_and_receipt_observe_is_provider_only() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(
            1,
            ManagementPolicy::Managed,
        )))));
        let api = Arc::new(FakeApi::new(store.clone(), vec![]));
        let service = service(store, resolver, api.clone(), Duration::from_secs(2));
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let zone_id = DnsZoneId::new(ZONE_ID).unwrap();
        let receipt = service
            .put_record_set(
                &account_id,
                &zone_id,
                &key("new.example.com"),
                &create_request(),
            )
            .await
            .unwrap();
        assert_eq!(api.change_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            receipt.provider_application,
            edgion_center_app::api::route53_dns::Route53ProviderApplicationDto::Pending
        );
        let observed = service
            .get_change(&account_id, &zone_id, &receipt.receipt)
            .await
            .unwrap();
        assert_eq!(
            observed.provider_application,
            edgion_center_app::api::route53_dns::Route53ProviderApplicationDto::InSync
        );
        assert_eq!(api.get_change_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn stale_guard_and_observe_only_account_never_dispatch() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(
            1,
            ManagementPolicy::Managed,
        )))));
        let api = Arc::new(FakeApi::new(store.clone(), vec![existing_record()]));
        let service = service(
            store.clone(),
            resolver.clone(),
            api.clone(),
            Duration::from_secs(2),
        );
        let mut request = create_request();
        request.guard = Route53RecordMutationGuardDto::MatchRevision {
            revision: edgion_center_core::DnsRecordRevision::new("stale").unwrap(),
        };
        assert_eq!(
            service
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &key("old.example.com"),
                    &request,
                )
                .await,
            Err(Route53DnsAdminError::Conflict)
        );
        assert_eq!(api.change_calls.load(Ordering::SeqCst), 0);

        *store.0.lock().unwrap() = Some(account(1, ManagementPolicy::ObserveOnly));
        assert_eq!(
            service
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &key("new.example.com"),
                    &create_request(),
                )
                .await,
            Err(Route53DnsAdminError::InvalidRequest)
        );
        assert_eq!(api.change_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn authority_fences_and_timeout_distinguish_dispatch_boundary_without_retry() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(
            1,
            ManagementPolicy::Managed,
        )))));
        let mut before = FakeApi::new(store.clone(), vec![]);
        before.rotate_before_dispatch_at_record_call = Some(2);
        let before = Arc::new(before);
        let service_before = service(
            store.clone(),
            resolver.clone(),
            before.clone(),
            Duration::from_secs(2),
        );
        assert_eq!(
            service_before
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &key("new.example.com"),
                    &create_request(),
                )
                .await,
            Err(Route53DnsAdminError::Unavailable)
        );
        assert_eq!(before.change_calls.load(Ordering::SeqCst), 0);

        *store.0.lock().unwrap() = Some(account(1, ManagementPolicy::Managed));
        let mut after = FakeApi::new(store.clone(), vec![]);
        after.rotate_after_dispatch = true;
        let after = Arc::new(after);
        let service_after = service(
            store.clone(),
            resolver.clone(),
            after.clone(),
            Duration::from_secs(2),
        );
        assert_eq!(
            service_after
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &key("new.example.com"),
                    &create_request(),
                )
                .await,
            Err(Route53DnsAdminError::UnknownOutcome)
        );
        assert_eq!(after.change_calls.load(Ordering::SeqCst), 1);

        *store.0.lock().unwrap() = Some(account(1, ManagementPolicy::Managed));
        let mut slow_read = FakeApi::new(store.clone(), vec![]);
        slow_read.list_delay = Duration::from_millis(100);
        let slow_read = Arc::new(slow_read);
        let timed_before = service(
            store.clone(),
            resolver.clone(),
            slow_read.clone(),
            Duration::from_millis(20),
        );
        assert_eq!(
            timed_before
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &key("new.example.com"),
                    &create_request(),
                )
                .await,
            Err(Route53DnsAdminError::Unavailable)
        );
        assert_eq!(slow_read.change_calls.load(Ordering::SeqCst), 0);

        let mut hanging = FakeApi::new(store.clone(), vec![]);
        hanging.change_delay = Duration::from_millis(100);
        let hanging = Arc::new(hanging);
        let timed = service(store, resolver, hanging.clone(), Duration::from_millis(20));
        assert_eq!(
            timed
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &key("new.example.com"),
                    &create_request(),
                )
                .await,
            Err(Route53DnsAdminError::UnknownOutcome)
        );
        assert_eq!(hanging.change_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn mutation_receipt_key_rotation_is_fenced_before_and_after_dispatch() {
        let (directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(
            1,
            ManagementPolicy::Managed,
        )))));
        let mut before = FakeApi::new(store.clone(), vec![]);
        before.rotate_key_before_dispatch = Some(directory.path().join("mutation.key"));
        let before = Arc::new(before);
        let service_before = service(
            store.clone(),
            resolver,
            before.clone(),
            Duration::from_secs(2),
        );
        assert_eq!(
            service_before
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &key("new.example.com"),
                    &create_request(),
                )
                .await,
            Err(Route53DnsAdminError::Unavailable)
        );
        assert_eq!(before.change_calls.load(Ordering::SeqCst), 0);

        let (directory, resolver) = mounted().await;
        let mut after = FakeApi::new(store.clone(), vec![]);
        after.rotate_key_after_dispatch = Some(directory.path().join("mutation.key"));
        let after = Arc::new(after);
        let service_after = service(store, resolver, after.clone(), Duration::from_secs(2));
        assert_eq!(
            service_after
                .put_record_set(
                    &CloudResourceId::new(CENTER_ACCOUNT).unwrap(),
                    &DnsZoneId::new(ZONE_ID).unwrap(),
                    &key("new.example.com"),
                    &create_request(),
                )
                .await,
            Err(Route53DnsAdminError::UnknownOutcome)
        );
        assert_eq!(after.change_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn replace_delete_and_explicit_batch_each_use_one_atomic_provider_batch() {
        let (_directory, resolver) = mounted().await;
        let managed = account(1, ManagementPolicy::Managed);
        let store = Arc::new(Store(Mutex::new(Some(managed.clone()))));
        let api = Arc::new(FakeApi::new(store.clone(), vec![existing_record()]));
        let old_key = key("old.example.com");
        let revision = current_revision(api.clone(), &managed, &old_key).await;
        let service = service(store, resolver, api.clone(), Duration::from_secs(2));
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let zone_id = DnsZoneId::new(ZONE_ID).unwrap();

        let mut replace = create_request();
        replace.guard = Route53RecordMutationGuardDto::MatchRevision {
            revision: revision.clone(),
        };
        service
            .put_record_set(&account_id, &zone_id, &old_key, &replace)
            .await
            .unwrap();
        assert_eq!(api.change_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            api.last_batch
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .changes
                .len(),
            2
        );

        service
            .delete_record_set(
                &account_id,
                &zone_id,
                &old_key,
                &Route53RecordSetDeleteRequest {
                    expected_revision: revision.clone(),
                },
            )
            .await
            .unwrap();
        assert_eq!(api.change_calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            api.last_batch
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .changes
                .len(),
            1
        );

        let desired = create_request().desired;
        let batch = Route53RecordChangeBatchRequest {
            changes: vec![
                Route53RecordBatchChangeDto::Create {
                    key: Route53BatchRecordKeyDto {
                        owner: DnsOwnerName::new("new.example.com").unwrap(),
                        record_type: Route53RecordType::A,
                        set_identifier: None,
                    },
                    desired: desired.clone(),
                },
                Route53RecordBatchChangeDto::Replace {
                    key: Route53BatchRecordKeyDto {
                        owner: DnsOwnerName::new("old.example.com").unwrap(),
                        record_type: Route53RecordType::A,
                        set_identifier: None,
                    },
                    expected_revision: revision,
                    desired,
                },
            ],
        };
        service
            .apply_change_batch(&account_id, &zone_id, &batch)
            .await
            .unwrap();
        assert_eq!(api.change_calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            api.last_batch
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .changes
                .len(),
            3
        );
    }

    #[tokio::test]
    async fn wrong_zone_and_tampered_receipts_never_reach_get_change() {
        let (_directory, resolver) = mounted().await;
        let store = Arc::new(Store(Mutex::new(Some(account(
            1,
            ManagementPolicy::Managed,
        )))));
        let api = Arc::new(FakeApi::new(store.clone(), vec![]));
        let service = service(store, resolver, api.clone(), Duration::from_secs(2));
        let account_id = CloudResourceId::new(CENTER_ACCOUNT).unwrap();
        let zone_id = DnsZoneId::new(ZONE_ID).unwrap();
        let receipt = service
            .put_record_set(
                &account_id,
                &zone_id,
                &key("new.example.com"),
                &create_request(),
            )
            .await
            .unwrap()
            .receipt;

        assert_eq!(
            service
                .get_change(&account_id, &DnsZoneId::new("ZOTHER123").unwrap(), &receipt,)
                .await,
            Err(Route53DnsAdminError::NotFound)
        );
        let mut tampered = receipt.as_str().as_bytes().to_vec();
        let middle = tampered.len() / 2;
        tampered[middle] = if tampered[middle] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(tampered).unwrap();
        let tampered = DnsChangeId::new(tampered).unwrap();
        assert_eq!(
            service.get_change(&account_id, &zone_id, &tampered).await,
            Err(Route53DnsAdminError::NotFound)
        );
        assert_eq!(api.get_change_calls.load(Ordering::SeqCst), 0);
        assert_eq!(api.build_calls.load(Ordering::SeqCst), 1);
    }
}
