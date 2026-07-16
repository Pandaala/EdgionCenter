//! Authorization seam for the Admin API.
//!
//! The middleware consumes the platform-neutral core [`Authorizer`] port and
//! materializes a [`PermissionSet`] for downstream handlers. Enforcement and
//! permission-resolution happen in a single middleware (see [`middleware`])
//! applied INSIDE `unified_auth`, so it covers both the shared auth routes
//! (notably `/auth/me`) and the business routes.
//!
//! Under `authz.mode = allow_all` the core allow-all policy grants every
//! permission. Under `authz.mode = rbac` the SQL adapter resolves permissions.
//!
//! `unified_auth` must never import this module: authentication stays free of
//! any authorization concept. The dependency direction is one-way — authz reads
//! the `UnifiedAuthClaims` that `unified_auth` injects.

pub mod catalog;
pub mod middleware;

use std::collections::HashSet;

/// The set of permission keys granted to a principal.
///
#[derive(Clone, Debug)]
pub struct PermissionSet {
    keys: HashSet<String>,
}

impl PermissionSet {
    /// Build a set from an explicit list of permission keys.
    pub fn from_keys(keys: impl IntoIterator<Item = String>) -> Self {
        Self {
            keys: keys.into_iter().collect(),
        }
    }

    /// Render the concrete list of granted keys, sorted for stable output.
    pub fn materialize(&self) -> Vec<String> {
        let mut v: Vec<String> = self.keys.iter().cloned().collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_keys_materialize_is_sorted() {
        let s = PermissionSet::from_keys(["b:write".to_string(), "a:read".to_string()]);
        assert_eq!(
            s.materialize(),
            vec!["a:read".to_string(), "b:write".to_string()]
        );
    }
}
