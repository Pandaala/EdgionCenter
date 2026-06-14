//! LITE-tier authorization store: every authenticated caller is a full admin.

use super::{AuthzStore, PermissionSet, Principal};

/// Grants every permission to every principal (login = admin).
///
/// Installed when `config.access.mode = lite` (the default). Because it returns
/// [`PermissionSet::all`], the authz middleware never denies a mapped route and
/// `/auth/me` reports the entire key catalog.
pub struct AllowAllAuthz;

#[async_trait::async_trait]
impl AuthzStore for AllowAllAuthz {
    async fn permissions_for(&self, _p: &Principal) -> PermissionSet {
        PermissionSet::all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::authz::catalog;

    #[tokio::test]
    async fn allow_all_contains_everything() {
        let store = AllowAllAuthz;
        let p = Principal {
            subject: "anyone".to_string(),
            provider: "local".to_string(),
        };
        let perms = store.permissions_for(&p).await;
        for key in catalog::all_keys() {
            assert!(perms.contains(key), "AllowAll must contain {key}");
        }
        // And an arbitrary unknown key as well (the `all` short-circuit).
        assert!(perms.contains("future:key"));
    }
}
