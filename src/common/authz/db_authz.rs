//! RBAC authorization store: resolves permissions from the database.
//!
//! [`DbAuthz`] is the RBAC [`AuthzStore`], installed when `authz.mode = rbac`.
//! For each principal it resolves the effective permission keys by joining
//! `users -> user_roles -> role_permissions` (via
//! [`Store::permission_keys_for_user`]) into a concrete [`PermissionSet`]
//! (never an `all()` superuser set — RBAC admins get their power from a
//! role that holds every catalog key, see the startup bootstrap).
//!
//! ## Caching
//!
//! Resolution hits the database, so a short-lived in-process cache keyed by the
//! principal's subject (username) avoids a query on every request. The cache is
//! a hand-rolled `RwLock<HashMap<..>>` with a 30s TTL — no external cache
//! dependency. The TTL is also the staleness bound: a role/permission change is
//! visible at most 30s later. Active cache invalidation on user/role mutation is
//! a later (DAC-07) concern; until then the TTL is the only bound. The per-subject
//! `HashMap` grows with the set of authenticated usernames and is bounded only by
//! that set (entries are overwritten on TTL refresh but never actively evicted) —
//! acceptable for DB-user populations; flagged for future bounding if ever needed.
//!
//! ## Fail-closed
//!
//! On a database error the resolver returns an EMPTY permission set (not a
//! cached or partial one) and logs a warning. Combined with the authz
//! middleware's deny-by-default, a transient DB outage denies access rather
//! than leaking it.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use super::{AuthzStore, PermissionSet, Principal};
use crate::store::Store;

/// Time-to-live for a cached permission set. Also the maximum staleness bound
/// for a role/permission change to take effect (see module docs).
const CACHE_TTL: Duration = Duration::from_secs(30);

/// Database-backed authorization store (RBAC mode).
pub struct DbAuthz {
    store: Arc<Store>,
    /// subject (username) -> (cached_at, permissions). Guarded by a plain
    /// `RwLock`; the guard is always dropped before any `.await`.
    cache: RwLock<HashMap<String, (Instant, PermissionSet)>>,
}

impl DbAuthz {
    /// Build a `DbAuthz` over the given store with an empty cache.
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Return a still-fresh cached set for `subject`, if any. Drops the read
    /// guard before returning so callers never hold it across an `.await`.
    fn cached(&self, subject: &str, now: Instant) -> Option<PermissionSet> {
        let guard = self.cache.read().ok()?;
        let (cached_at, set) = guard.get(subject)?;
        if now.duration_since(*cached_at) < CACHE_TTL {
            Some(set.clone())
        } else {
            None
        }
    }
}

#[async_trait::async_trait]
impl AuthzStore for DbAuthz {
    async fn permissions_for(&self, p: &Principal) -> PermissionSet {
        let now = Instant::now();

        // Fast path: a fresh cached entry. The read guard is scoped to `cached`
        // and dropped before we ever await below.
        if let Some(set) = self.cached(&p.subject, now) {
            return set;
        }

        // Miss (or expired): resolve from the database.
        match self.store.permission_keys_for_user(&p.subject).await {
            Ok(keys) => {
                let set = PermissionSet::from_keys(keys);
                if let Ok(mut guard) = self.cache.write() {
                    guard.insert(p.subject.clone(), (now, set.clone()));
                }
                set
            }
            Err(e) => {
                // Fail-closed: deny on DB error rather than serve stale/partial
                // permissions. Do NOT cache this empty set so the next request
                // re-attempts the query.
                tracing::warn!(
                    component = "authz",
                    subject = %p.subject,
                    error = %e,
                    "DbAuthz: permission resolution failed; denying (empty permission set)"
                );
                PermissionSet::from_keys(Vec::<String>::new())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(subject: &str) -> Principal {
        Principal {
            subject: subject.to_string(),
            provider: "local".to_string(),
        }
    }

    /// A user's role permissions resolve through `permissions_for`.
    #[tokio::test]
    async fn resolves_user_keys() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let uid = store.create_user("alice", "hash", "Alice").await.unwrap();
        let rid = store.create_role("ops", "Operators").await.unwrap();
        store
            .set_role_permissions(rid, &["controllers:read".into(), "controllers:write".into()])
            .await
            .unwrap();
        store.set_user_roles(uid, &[rid]).await.unwrap();

        let authz = DbAuthz::new(store);
        let perms = authz.permissions_for(&principal("alice")).await;

        assert!(perms.contains("controllers:read"));
        assert!(perms.contains("controllers:write"));
        // Not a superuser set: a key the user was never granted is absent.
        assert!(!perms.contains("audit:read"));
        assert!(!perms.is_all(), "DbAuthz must never produce an all() set");
        // A user with no roles resolves to an empty set.
        let none = authz.permissions_for(&principal("nobody")).await;
        assert!(none.materialize().is_empty());
    }

    /// A second call within the TTL serves the cached set even when the
    /// underlying DB has changed — proving the cache short-circuits the query.
    #[tokio::test]
    async fn caches() {
        let store = Arc::new(Store::open_in_memory().await.unwrap());
        let uid = store.create_user("bob", "hash", "Bob").await.unwrap();
        let rid = store.create_role("viewer", "Read-only").await.unwrap();
        store
            .set_role_permissions(rid, &["controllers:read".into()])
            .await
            .unwrap();
        store.set_user_roles(uid, &[rid]).await.unwrap();

        let authz = DbAuthz::new(store.clone());

        // First call populates the cache: read-only.
        let first = authz.permissions_for(&principal("bob")).await;
        assert!(first.contains("controllers:read"));
        assert!(!first.contains("controllers:write"));

        // Mutate the DB underneath: grant an additional key.
        store
            .set_role_permissions(rid, &["controllers:read".into(), "controllers:write".into()])
            .await
            .unwrap();

        // Second call within the 30s TTL must return the STALE cached set
        // (no DB re-query), so the newly added key is still absent.
        let second = authz.permissions_for(&principal("bob")).await;
        assert!(second.contains("controllers:read"));
        assert!(
            !second.contains("controllers:write"),
            "within TTL the cached set must be served, hiding the DB mutation"
        );
    }
}
