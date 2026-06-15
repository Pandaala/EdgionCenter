//! Authorization seam for the Admin API.
//!
//! This module introduces a pluggable `AuthzStore` that resolves an
//! authenticated [`Principal`] into a [`PermissionSet`]. Enforcement and
//! permission-resolution happen in a single middleware (see [`middleware`])
//! applied INSIDE `unified_auth`, so it covers both the shared auth routes
//! (notably `/auth/me`) and the business routes.
//!
//! Under `authz.mode = allow_all` the installed store is
//! [`allow_all::AllowAllAuthz`], which grants every permission to every
//! authenticated caller (login = admin). Under `authz.mode = rbac` the
//! database-backed `db_authz::DbAuthz` store resolves permissions per subject.
//!
//! `unified_auth` must never import this module: authentication stays free of
//! any authorization concept. The dependency direction is one-way — authz reads
//! the `UnifiedAuthClaims` that `unified_auth` injects.

pub mod allow_all;
pub mod catalog;
pub mod db_authz;
pub mod middleware;

use std::collections::HashSet;

/// An authenticated caller, derived from `UnifiedAuthClaims`.
pub struct Principal {
    /// The token subject (`sub`), or `"<unknown>"` when the token carried none.
    pub subject: String,
    /// The validating provider, lower-cased (`"oidc"` or `"local"`).
    pub provider: String,
}

/// The set of permission keys granted to a principal.
///
/// `all = true` is a short-circuit that contains every catalog key without
/// enumerating them; `allow_all` mode uses it so new keys are granted implicitly.
#[derive(Clone, Debug)]
pub struct PermissionSet {
    keys: HashSet<String>,
    all: bool,
}

impl PermissionSet {
    /// A set that contains every permission key (full admin).
    pub fn all() -> Self {
        Self {
            keys: HashSet::new(),
            all: true,
        }
    }

    /// Build a set from an explicit list of permission keys.
    pub fn from_keys(keys: impl IntoIterator<Item = String>) -> Self {
        Self {
            keys: keys.into_iter().collect(),
            all: false,
        }
    }

    /// Whether this set grants `key`. Always true for an `all()` set.
    pub fn contains(&self, key: &str) -> bool {
        self.all || self.keys.contains(key)
    }

    /// Whether this is an `all()` (full-admin / superuser) set. Used by the
    /// middleware to let superusers reach unmapped business routes that would
    /// otherwise deny by default.
    pub fn is_all(&self) -> bool {
        self.all
    }

    /// Render the concrete list of granted keys. For an `all()` set this is the
    /// full catalog; otherwise the explicit keys, sorted for stable output.
    pub fn materialize(&self) -> Vec<String> {
        if self.all {
            catalog::all_keys().iter().map(|k| (*k).to_string()).collect()
        } else {
            let mut v: Vec<String> = self.keys.iter().cloned().collect();
            v.sort();
            v
        }
    }
}

/// Resolves an authenticated [`Principal`] into the permissions it holds.
///
/// Implementations are installed once at startup (selected by
/// `config.authz.mode`) and shared as `Arc<dyn AuthzStore>`.
#[async_trait::async_trait]
pub trait AuthzStore: Send + Sync {
    /// Return the permission set granted to `p`.
    async fn permissions_for(&self, p: &Principal) -> PermissionSet;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_set_contains_any_key() {
        let s = PermissionSet::all();
        assert!(s.contains("controllers:read"));
        assert!(s.contains("anything:at-all"));
    }

    #[test]
    fn all_set_materializes_full_catalog() {
        let s = PermissionSet::all();
        let mut materialized = s.materialize();
        materialized.sort();
        let mut catalog: Vec<String> = catalog::all_keys().iter().map(|k| k.to_string()).collect();
        catalog.sort();
        assert_eq!(materialized, catalog);
    }

    #[test]
    fn from_keys_contains_only_listed() {
        let s = PermissionSet::from_keys(["controllers:read".to_string()]);
        assert!(s.contains("controllers:read"));
        assert!(!s.contains("controllers:write"));
    }

    #[test]
    fn from_keys_materialize_is_sorted() {
        let s = PermissionSet::from_keys(["b:write".to_string(), "a:read".to_string()]);
        assert_eq!(s.materialize(), vec!["a:read".to_string(), "b:write".to_string()]);
    }
}
