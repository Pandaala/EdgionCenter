//! Permission-key catalog and the route → permission-key map.
//!
//! Every business route on the Admin API maps to exactly one permission key.
//! `route_permission` is the single source of truth consulted by the authz
//! middleware; `all_keys` is the universe materialized for an `all()`
//! permission set (under `authz.mode = allow_all` `/auth/me` reports the full
//! list, since every key is implicitly granted).
//!
//! Keys are grouped by dashboard page. GET endpoints map to a `:read` key and
//! mutating endpoints to a `:write` key. `users:manage` / `roles:manage` gate
//! the db_auth user/role admin endpoints (`/api/v1/center/admin/users` and
//! `/api/v1/center/admin/roles` plus `/permission-catalog`, added in DAC-07).

use http::Method;

// Controllers page (list / clusters / reload / admin delete).
pub const CONTROLLERS_READ: &str = "controllers:read";
pub const CONTROLLERS_WRITE: &str = "controllers:write";
// Region routes page (cluster + service region routes, failover, sync, consistency).
pub const REGION_ROUTES_READ: &str = "region-routes:read";
pub const REGION_ROUTES_WRITE: &str = "region-routes:write";
// Global connection IP restrictions page.
pub const IP_RESTRICTIONS_READ: &str = "ip-restrictions:read";
pub const IP_RESTRICTIONS_WRITE: &str = "ip-restrictions:write";
// Audit log page.
pub const AUDIT_READ: &str = "audit:read";
// Server / diagnostics reads (server-info, watch-status, metadata-store).
pub const SERVER_READ: &str = "server:read";
// HTTP proxy to controllers (any method).
pub const PROXY_ACCESS: &str = "proxy:access";
// User / role administration (db_auth; gates the /admin/users & /admin/roles endpoints).
pub const USERS_MANAGE: &str = "users:manage";
pub const ROLES_MANAGE: &str = "roles:manage";
// Cloudflare-specific DNS zone inventory.
/// High-trust inventory access across every configured Cloudflare account.
/// This permission is not scoped to one ProviderAccount.
pub const CLOUDFLARE_DNS_READ: &str = "cloudflare-dns:read";
/// High-trust mutation access across every configured Cloudflare account.
/// This permission is not scoped to one ProviderAccount.
pub const CLOUDFLARE_DNS_WRITE: &str = "cloudflare-dns:write";
/// Dedicated authority for writing an authenticated caller marker with an RRset mutation.
/// It does not imply ordinary Cloudflare DNS mutation access.
pub const CLOUDFLARE_DNS_REMOTE_WRITE: &str = "cloudflare-dns:remote-write";
/// Read Cloudflare Zone WAF rulesets across configured ProviderAccounts.
pub const CLOUDFLARE_WAF_READ: &str = "cloudflare-waf:read";
/// Create, update, enable, disable, or delete Center-owned Cloudflare WAF rules.
pub const CLOUDFLARE_WAF_WRITE: &str = "cloudflare-waf:write";
/// Reorder Center-owned Cloudflare WAF rules without granting other mutations.
pub const CLOUDFLARE_WAF_ORDER: &str = "cloudflare-waf:order";
/// Configure bounded managed-rule skips and exceptions.
pub const CLOUDFLARE_WAF_EXCEPTION: &str = "cloudflare-waf:exception";
/// Confirm an operation that weakens effective Cloudflare WAF protection.
pub const CLOUDFLARE_WAF_SECURITY_WEAKEN: &str = "cloudflare-waf:security-weaken";
/// High-trust Route 53 inventory access across every configured AWS ProviderAccount.
/// Kubernetes authorization may additionally narrow this to exact non-resource paths.
pub const ROUTE53_DNS_READ: &str = "route53-dns:read";
/// High-trust Route 53 RRset mutation access across every configured AWS ProviderAccount.
/// It does not imply inventory reads or hosted-zone lifecycle authority.
pub const ROUTE53_DNS_WRITE: &str = "route53-dns:write";
pub const PROVIDER_ACCOUNTS_READ: &str = "provider-accounts:read";
pub const PROVIDER_ACCOUNTS_WRITE: &str = "provider-accounts:write";
pub const PROVIDER_CREDENTIALS_USE: &str = "provider-credentials:use";
pub const PROVIDER_CAPABILITIES_READ: &str = "provider-capabilities:read";
pub const PROVIDER_CREDENTIALS_INSPECT: &str = "provider-credentials:inspect";

/// Every permission key known to the system, in a stable order.
pub fn all_keys() -> &'static [&'static str] {
    &[
        CONTROLLERS_READ,
        CONTROLLERS_WRITE,
        REGION_ROUTES_READ,
        REGION_ROUTES_WRITE,
        IP_RESTRICTIONS_READ,
        IP_RESTRICTIONS_WRITE,
        AUDIT_READ,
        SERVER_READ,
        PROXY_ACCESS,
        USERS_MANAGE,
        ROLES_MANAGE,
        CLOUDFLARE_DNS_READ,
        CLOUDFLARE_DNS_WRITE,
        CLOUDFLARE_DNS_REMOTE_WRITE,
        CLOUDFLARE_WAF_READ,
        CLOUDFLARE_WAF_WRITE,
        CLOUDFLARE_WAF_ORDER,
        CLOUDFLARE_WAF_EXCEPTION,
        CLOUDFLARE_WAF_SECURITY_WEAKEN,
        ROUTE53_DNS_READ,
        ROUTE53_DNS_WRITE,
        PROVIDER_ACCOUNTS_READ,
        PROVIDER_ACCOUNTS_WRITE,
        PROVIDER_CREDENTIALS_USE,
        PROVIDER_CAPABILITIES_READ,
        PROVIDER_CREDENTIALS_INSPECT,
    ]
}

/// Whether `key` is a permission key known to the system.
pub fn is_known_key(key: &str) -> bool {
    all_keys().contains(&key)
}

/// A named group of permission keys for the role/permission matrix UI.
///
/// The flattened union of every group's `keys` is exactly [`all_keys`] — the
/// `catalog_groups_cover_all_keys` test enforces this so a newly added key can
/// never be silently omitted from the matrix.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PermissionGroup {
    pub group: &'static str,
    pub keys: Vec<&'static str>,
}

/// The permission catalog grouped by dashboard page, for the matrix UI.
///
/// Collectively the groups cover EXACTLY [`all_keys`] (enforced by test).
pub fn catalog_groups() -> Vec<PermissionGroup> {
    vec![
        PermissionGroup {
            group: "Controllers",
            keys: vec![CONTROLLERS_READ, CONTROLLERS_WRITE],
        },
        PermissionGroup {
            group: "Region Routes",
            keys: vec![REGION_ROUTES_READ, REGION_ROUTES_WRITE],
        },
        PermissionGroup {
            group: "IP Restrictions",
            keys: vec![IP_RESTRICTIONS_READ, IP_RESTRICTIONS_WRITE],
        },
        PermissionGroup {
            group: "Audit",
            keys: vec![AUDIT_READ],
        },
        PermissionGroup {
            group: "Server",
            keys: vec![SERVER_READ],
        },
        PermissionGroup {
            group: "Proxy",
            keys: vec![PROXY_ACCESS],
        },
        PermissionGroup {
            group: "Access Control",
            keys: vec![USERS_MANAGE, ROLES_MANAGE],
        },
        PermissionGroup {
            group: "Cloudflare DNS",
            keys: vec![
                CLOUDFLARE_DNS_READ,
                CLOUDFLARE_DNS_WRITE,
                CLOUDFLARE_DNS_REMOTE_WRITE,
            ],
        },
        PermissionGroup {
            group: "Cloudflare WAF",
            keys: vec![
                CLOUDFLARE_WAF_READ,
                CLOUDFLARE_WAF_WRITE,
                CLOUDFLARE_WAF_ORDER,
                CLOUDFLARE_WAF_EXCEPTION,
                CLOUDFLARE_WAF_SECURITY_WEAKEN,
            ],
        },
        PermissionGroup {
            group: "Route 53 DNS",
            keys: vec![ROUTE53_DNS_READ, ROUTE53_DNS_WRITE],
        },
        PermissionGroup {
            group: "Provider Accounts",
            keys: vec![PROVIDER_ACCOUNTS_READ, PROVIDER_ACCOUNTS_WRITE],
        },
        PermissionGroup {
            group: "Provider Credentials",
            keys: vec![PROVIDER_CREDENTIALS_USE, PROVIDER_CREDENTIALS_INSPECT],
        },
        PermissionGroup {
            group: "Provider Capabilities",
            keys: vec![PROVIDER_CAPABILITIES_READ],
        },
    ]
}

/// Whether `path` is a business path subject to fail-closed enforcement.
///
/// True for any `/api/v1/` path EXCEPT the public auth routes (`/api/v1/auth/`),
/// which carry no authorization requirement. The authz middleware uses this so
/// an unmapped business route (one nobody added to [`route_permission`]) denies
/// by default for non-superusers, rather than leaking access once real RBAC
/// lands. Probe/metrics paths (`/health`, `/metrics`, ...) are not under
/// `/api/v1/` and so are not business paths.
pub fn is_business_path(path: &str) -> bool {
    path.starts_with("/api/v1/") && !path.starts_with("/api/v1/auth/")
}

/// Whether `path` is exactly `base` or sits under it at a segment boundary
/// (`base` followed by `/`). Avoids a bare `starts_with` also matching a
/// sibling like `base + "-v2"`.
fn under_segment(path: &str, base: &str) -> bool {
    path == base || path.starts_with(&format!("{base}/"))
}

/// Map a concrete request `(method, path)` to the permission key it requires.
///
/// Returns `None` for non-business routes — the shared auth endpoints
/// (`/api/v1/auth/*`) and probe/metrics paths — which carry no authorization
/// requirement. A `None` result means the authz middleware lets the request
/// through unconditionally.
///
/// `path` is the request URI path (no query string), e.g.
/// `/api/v1/center/global-connection-ip-restrictions/default/foo`.
pub fn route_permission(method: &Method, path: &str) -> Option<&'static str> {
    let is_read = method == Method::GET || method == Method::HEAD;

    // HTTP proxy — any method forwards to a controller.
    if path.starts_with("/api/v1/proxy/") {
        return Some(PROXY_ACCESS);
    }

    if is_cloudflare_remote_control_path(path) {
        return (method == Method::PUT).then_some(CLOUDFLARE_DNS_REMOTE_WRITE);
    }

    if under_segment(path, "/api/v1/center/cloudflare/dns") {
        return match *method {
            Method::GET | Method::HEAD => Some(CLOUDFLARE_DNS_READ),
            Method::POST | Method::PUT | Method::PATCH | Method::DELETE => {
                Some(CLOUDFLARE_DNS_WRITE)
            }
            _ => None,
        };
    }

    if under_segment(path, "/api/v1/center/cloudflare/waf") {
        return cloudflare_waf_route_permission(method, path);
    }

    if let Some(route) = route53_dns_route(path) {
        return match (route, method) {
            (Route53DnsRoute::Read, &Method::GET) => Some(ROUTE53_DNS_READ),
            (Route53DnsRoute::RecordDetail, &Method::GET) => Some(ROUTE53_DNS_READ),
            (Route53DnsRoute::RecordDetail, &Method::PUT | &Method::DELETE) => {
                Some(ROUTE53_DNS_WRITE)
            }
            (Route53DnsRoute::ChangeBatch, &Method::POST) => Some(ROUTE53_DNS_WRITE),
            _ => None,
        };
    }

    if let Some(suffix) = path.strip_prefix("/api/v1/center/cloud/provider-capabilities/accounts/")
    {
        let mut segments = suffix.split('/');
        if segments
            .next()
            .is_some_and(|account_id| !account_id.is_empty())
            && segments.next().is_none()
        {
            return is_read.then_some(PROVIDER_CAPABILITIES_READ);
        }
    }

    if let Some(suffix) =
        path.strip_prefix("/api/v1/center/cloud/provider-credential-inspections/accounts/")
    {
        if suffix
            .strip_suffix("/refresh")
            .is_some_and(|account_id| !account_id.is_empty() && !account_id.contains('/'))
        {
            return (method == Method::POST).then_some(PROVIDER_CREDENTIALS_INSPECT);
        }
    }

    if under_segment(path, "/api/v1/center/cloud/provider-accounts") {
        return match *method {
            Method::GET | Method::HEAD => Some(PROVIDER_ACCOUNTS_READ),
            Method::POST | Method::PUT => Some(PROVIDER_ACCOUNTS_WRITE),
            _ => None,
        };
    }

    // Audit log read (distinct path; matched before the admin-controllers prefix).
    if path == "/api/v1/center/admin/audit-logs" {
        return Some(AUDIT_READ);
    }

    // Watch-cache / metadata-store diagnostics.
    if path == "/api/v1/center/admin/watch-status" || path == "/api/v1/center/admin/metadata-store"
    {
        return Some(SERVER_READ);
    }

    // DB-backed admin controllers (list GET / delete DELETE).
    if path == "/api/v1/center/admin/controllers"
        || path.starts_with("/api/v1/center/admin/controllers/")
    {
        return Some(if is_read {
            CONTROLLERS_READ
        } else {
            CONTROLLERS_WRITE
        });
    }

    // User administration (db_auth): all /users routes require users:manage.
    if under_segment(path, "/api/v1/center/admin/users") {
        return Some(USERS_MANAGE);
    }

    // Role administration + permission catalog (db_auth): all /roles routes
    // and the permission-catalog read require roles:manage.
    if under_segment(path, "/api/v1/center/admin/roles")
        || path == "/api/v1/center/admin/permission-catalog"
    {
        return Some(ROLES_MANAGE);
    }

    // Region routes: list/consistency are GET reads, failover/sync are writes.
    // Keep the two legacy prefixes mapped while their redirect routes remain.
    if under_segment(path, "/api/v1/center/region-routes")
        || under_segment(path, "/api/v1/center/cluster-region-routes")
        || under_segment(path, "/api/v1/center/service-region-routes")
    {
        return Some(if is_read {
            REGION_ROUTES_READ
        } else {
            REGION_ROUTES_WRITE
        });
    }

    // Global connection IP restrictions: all reads are GET, every mutation
    // (POST/PUT/DELETE/PATCH) is a write.
    if under_segment(path, "/api/v1/center/global-connection-ip-restrictions") {
        return Some(if is_read {
            IP_RESTRICTIONS_READ
        } else {
            IP_RESTRICTIONS_WRITE
        });
    }

    // Server info.
    if path == "/api/v1/server-info" {
        return Some(SERVER_READ);
    }

    // Controller list + cluster list.
    if path == "/api/v1/controllers" || path == "/api/v1/clusters" {
        return Some(CONTROLLERS_READ);
    }
    // Controller sub-resources (e.g. /{id}/reload) — mutating.
    if path.starts_with("/api/v1/controllers/") {
        return Some(CONTROLLERS_WRITE);
    }

    None
}

fn cloudflare_waf_route_permission(method: &Method, path: &str) -> Option<&'static str> {
    let suffix = path.strip_prefix("/api/v1/center/cloudflare/waf/accounts/")?;
    let segments = suffix.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| segment.is_empty()) || segments.get(1) != Some(&"zones") {
        return None;
    }
    let is_read = method == Method::GET || method == Method::HEAD;
    match segments.as_slice() {
        [_, "zones", _, "rulesets"] => is_read.then_some(CLOUDFLARE_WAF_READ),
        [_, "zones", _, "managed-rules"]
        | [_, "zones", _, "custom-rules"]
        | [_, "zones", _, "rate-limits"] => {
            if is_read {
                Some(CLOUDFLARE_WAF_READ)
            } else {
                (method == Method::POST).then_some(CLOUDFLARE_WAF_WRITE)
            }
        }
        [_, "zones", _, "managed-rules", "exceptions"] => {
            (method == Method::PUT).then_some(CLOUDFLARE_WAF_EXCEPTION)
        }
        [_, "zones", _, "managed-rules", _, "order"]
        | [_, "zones", _, "custom-rules", _, "order"]
        | [_, "zones", _, "rate-limits", _, "order"] => {
            (method == Method::PUT).then_some(CLOUDFLARE_WAF_ORDER)
        }
        [_, "zones", _, "managed-rules", _, "security-weaken"]
        | [_, "zones", _, "custom-rules", _, "security-weaken"]
        | [_, "zones", _, "rate-limits", _, "security-weaken"] => {
            matches!(*method, Method::PUT | Method::DELETE)
                .then_some(CLOUDFLARE_WAF_SECURITY_WEAKEN)
        }
        [_, "zones", _, "managed-rules", _]
        | [_, "zones", _, "custom-rules", _]
        | [_, "zones", _, "rate-limits", _] => {
            (method == Method::PUT).then_some(CLOUDFLARE_WAF_WRITE)
        }
        _ => None,
    }
}

fn is_cloudflare_remote_control_path(path: &str) -> bool {
    let Some(suffix) = path.strip_prefix("/api/v1/center/cloudflare/dns/accounts/") else {
        return false;
    };
    let segments = suffix.split('/').collect::<Vec<_>>();
    segments.len() == 6
        && segments.iter().all(|segment| !segment.is_empty())
        && segments[1] == "zones"
        && segments[3] == "record-sets"
        && segments[5] == "remote-control"
}

#[derive(Clone, Copy)]
enum Route53DnsRoute {
    Read,
    RecordDetail,
    ChangeBatch,
}

fn route53_dns_route(path: &str) -> Option<Route53DnsRoute> {
    let suffix = path.strip_prefix("/api/v1/center/aws/route53/accounts/")?;
    let segments = suffix.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    match segments.as_slice() {
        [_, "hosted-zones"]
        | [_, "hosted-zones", _]
        | [_, "hosted-zones", _, "record-sets"]
        | [_, "hosted-zones", _, "changes", _] => Some(Route53DnsRoute::Read),
        [_, "hosted-zones", _, "record-sets", _] => Some(Route53DnsRoute::RecordDetail),
        [_, "hosted-zones", _, "change-batches"] => Some(Route53DnsRoute::ChangeBatch),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Completeness guard: every current business route must map to a key.
    /// Paths use concrete instances of any `{param}` segments (what the
    /// middleware actually sees at runtime).
    #[test]
    fn every_business_route_has_a_key() {
        let routes: &[(Method, &str)] = &[
            (Method::GET, "/api/v1/server-info"),
            (Method::GET, "/api/v1/controllers"),
            (Method::GET, "/api/v1/clusters"),
            (Method::POST, "/api/v1/controllers/c1/reload"),
            (Method::GET, "/api/v1/center/cluster-region-routes"),
            (Method::GET, "/api/v1/center/service-region-routes"),
            (
                Method::POST,
                "/api/v1/center/cluster-region-routes/failover",
            ),
            (
                Method::POST,
                "/api/v1/center/service-region-routes/failover",
            ),
            (Method::POST, "/api/v1/center/cluster-region-routes/sync"),
            (Method::POST, "/api/v1/center/service-region-routes/sync"),
            (
                Method::GET,
                "/api/v1/center/cluster-region-routes/consistency",
            ),
            (
                Method::GET,
                "/api/v1/center/service-region-routes/consistency",
            ),
            (
                Method::GET,
                "/api/v1/center/global-connection-ip-restrictions",
            ),
            (
                Method::POST,
                "/api/v1/center/global-connection-ip-restrictions",
            ),
            (
                Method::GET,
                "/api/v1/center/global-connection-ip-restrictions/default/foo",
            ),
            (
                Method::PUT,
                "/api/v1/center/global-connection-ip-restrictions/default/foo",
            ),
            (
                Method::DELETE,
                "/api/v1/center/global-connection-ip-restrictions/default/foo",
            ),
            (
                Method::PATCH,
                "/api/v1/center/global-connection-ip-restrictions/default/foo/enable",
            ),
            (
                Method::PATCH,
                "/api/v1/center/global-connection-ip-restrictions/default/foo/active-profile",
            ),
            (
                Method::POST,
                "/api/v1/center/global-connection-ip-restrictions/default/foo/sync",
            ),
            (
                Method::GET,
                "/api/v1/center/global-connection-ip-restrictions/consistency",
            ),
            (Method::GET, "/api/v1/center/admin/controllers"),
            (Method::DELETE, "/api/v1/center/admin/controllers/c1"),
            (Method::GET, "/api/v1/center/admin/users"),
            (Method::POST, "/api/v1/center/admin/users"),
            (Method::PATCH, "/api/v1/center/admin/users/1"),
            (Method::DELETE, "/api/v1/center/admin/users/1"),
            (Method::GET, "/api/v1/center/admin/roles"),
            (Method::POST, "/api/v1/center/admin/roles"),
            (Method::PUT, "/api/v1/center/admin/roles/1/permissions"),
            (Method::DELETE, "/api/v1/center/admin/roles/1"),
            (Method::GET, "/api/v1/center/admin/permission-catalog"),
            (Method::GET, "/api/v1/center/admin/audit-logs"),
            (Method::GET, "/api/v1/center/admin/watch-status"),
            (Method::GET, "/api/v1/center/admin/metadata-store"),
            (
                Method::GET,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones",
            ),
            (
                Method::POST,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones",
            ),
            (
                Method::GET,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1",
            ),
            (
                Method::DELETE,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1",
            ),
            (
                Method::GET,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1/record-sets",
            ),
            (
                Method::GET,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1/record-sets/A",
            ),
            (
                Method::PUT,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1/record-sets/A",
            ),
            (
                Method::DELETE,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1/record-sets/A",
            ),
            (
                Method::PUT,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1/record-sets/A/remote-control",
            ),
            (
                Method::GET,
                "/api/v1/center/cloudflare/waf/accounts/cf-main/zones/0123456789abcdef0123456789abcdef/rulesets",
            ),
            (
                Method::PUT,
                "/api/v1/center/cloudflare/waf/accounts/cf-main/zones/0123456789abcdef0123456789abcdef/managed-rules/rule-1",
            ),
            (
                Method::PUT,
                "/api/v1/center/cloudflare/waf/accounts/cf-main/zones/0123456789abcdef0123456789abcdef/managed-rules/exceptions",
            ),
            (
                Method::POST,
                "/api/v1/center/cloudflare/waf/accounts/cf-main/zones/0123456789abcdef0123456789abcdef/custom-rules",
            ),
            (
                Method::PUT,
                "/api/v1/center/cloudflare/waf/accounts/cf-main/zones/0123456789abcdef0123456789abcdef/custom-rules/rule-1/order",
            ),
            (
                Method::DELETE,
                "/api/v1/center/cloudflare/waf/accounts/cf-main/zones/0123456789abcdef0123456789abcdef/custom-rules/rule-1/security-weaken",
            ),
            (
                Method::POST,
                "/api/v1/center/cloudflare/waf/accounts/cf-main/zones/0123456789abcdef0123456789abcdef/rate-limits",
            ),
            (
                Method::GET,
                "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones",
            ),
            (
                Method::GET,
                "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123",
            ),
            (
                Method::GET,
                "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/record-sets",
            ),
            (
                Method::GET,
                "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/record-sets/A",
            ),
            (
                Method::PUT,
                "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/record-sets/A",
            ),
            (
                Method::DELETE,
                "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/record-sets/A",
            ),
            (
                Method::POST,
                "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/change-batches",
            ),
            (
                Method::GET,
                "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/changes/C123",
            ),
            (Method::GET, "/api/v1/center/cloud/provider-accounts"),
            (Method::POST, "/api/v1/center/cloud/provider-accounts"),
            (
                Method::GET,
                "/api/v1/center/cloud/provider-accounts/aws-main",
            ),
            (
                Method::PUT,
                "/api/v1/center/cloud/provider-accounts/aws-main",
            ),
            (
                Method::GET,
                "/api/v1/center/cloud/provider-capabilities/accounts/aws-main",
            ),
            (
                Method::POST,
                "/api/v1/center/cloud/provider-credential-inspections/accounts/aws-main/refresh",
            ),
            (Method::GET, "/api/v1/proxy/c1/some/sub/path"),
            (Method::POST, "/api/v1/proxy/c1/some/sub/path"),
        ];
        for (m, p) in routes {
            assert!(
                route_permission(m, p).is_some(),
                "route {} {} must map to a permission key",
                m,
                p
            );
        }
    }

    /// All /users routes gate on users:manage; all /roles routes and the
    /// permission-catalog read gate on roles:manage (regardless of method).
    #[test]
    fn user_role_admin_keys() {
        assert_eq!(
            route_permission(&Method::GET, "/api/v1/center/admin/users"),
            Some(USERS_MANAGE)
        );
        assert_eq!(
            route_permission(&Method::POST, "/api/v1/center/admin/users"),
            Some(USERS_MANAGE)
        );
        assert_eq!(
            route_permission(&Method::PATCH, "/api/v1/center/admin/users/1"),
            Some(USERS_MANAGE)
        );
        assert_eq!(
            route_permission(&Method::DELETE, "/api/v1/center/admin/users/1"),
            Some(USERS_MANAGE)
        );
        assert_eq!(
            route_permission(&Method::GET, "/api/v1/center/admin/roles"),
            Some(ROLES_MANAGE)
        );
        assert_eq!(
            route_permission(&Method::POST, "/api/v1/center/admin/roles"),
            Some(ROLES_MANAGE)
        );
        assert_eq!(
            route_permission(&Method::PUT, "/api/v1/center/admin/roles/1/permissions"),
            Some(ROLES_MANAGE)
        );
        assert_eq!(
            route_permission(&Method::DELETE, "/api/v1/center/admin/roles/1"),
            Some(ROLES_MANAGE)
        );
        assert_eq!(
            route_permission(&Method::GET, "/api/v1/center/admin/permission-catalog"),
            Some(ROLES_MANAGE)
        );
    }

    /// The grouped catalog must cover EXACTLY `all_keys()` — no key omitted, no
    /// stray key, no duplicate across groups.
    #[test]
    fn catalog_groups_cover_all_keys() {
        use std::collections::BTreeSet;
        let grouped: Vec<&str> = catalog_groups().into_iter().flat_map(|g| g.keys).collect();
        // No duplicates across the flattened groups.
        let grouped_set: BTreeSet<&str> = grouped.iter().copied().collect();
        assert_eq!(
            grouped_set.len(),
            grouped.len(),
            "a key appears in more than one group"
        );
        // The set of grouped keys equals the set of all_keys().
        let all_set: BTreeSet<&str> = all_keys().iter().copied().collect();
        assert_eq!(
            grouped_set, all_set,
            "catalog_groups() must cover exactly all_keys()"
        );
    }

    /// GET endpoints resolve to `:read`, mutations to `:write`.
    #[test]
    fn read_vs_write_keys() {
        for path in [
            "/api/v1/center/cloud/provider-accounts",
            "/api/v1/center/cloud/provider-accounts/aws-main",
        ] {
            assert_eq!(
                route_permission(&Method::HEAD, path),
                Some(PROVIDER_ACCOUNTS_READ)
            );
        }
        assert_eq!(
            route_permission(
                &Method::DELETE,
                "/api/v1/center/cloud/provider-accounts/aws-main"
            ),
            None
        );
        let capability_path = "/api/v1/center/cloud/provider-capabilities/accounts/aws-main";
        assert_eq!(
            route_permission(&Method::GET, capability_path),
            Some(PROVIDER_CAPABILITIES_READ)
        );
        assert_eq!(
            route_permission(&Method::HEAD, capability_path),
            Some(PROVIDER_CAPABILITIES_READ)
        );
        assert_eq!(route_permission(&Method::POST, capability_path), None);
        let inspection_path =
            "/api/v1/center/cloud/provider-credential-inspections/accounts/aws-main/refresh";
        assert_eq!(
            route_permission(&Method::POST, inspection_path),
            Some(PROVIDER_CREDENTIALS_INSPECT)
        );
        assert_eq!(route_permission(&Method::GET, inspection_path), None);
        assert_eq!(
            route_permission(
                &Method::POST,
                "/api/v1/center/cloud/provider-credential-inspections/accounts/aws-main/nested/refresh"
            ),
            None
        );
        assert_ne!(
            route_permission(
                &Method::GET,
                "/api/v1/center/cloud/provider-capabilities/accounts/aws-main/nested"
            ),
            Some(PROVIDER_CAPABILITIES_READ)
        );
        assert_ne!(
            route_permission(
                &Method::GET,
                "/api/v1/center/cloud/provider-accounts/aws-main/capabilities"
            ),
            Some(PROVIDER_CAPABILITIES_READ)
        );
        assert_eq!(
            route_permission(&Method::GET, "/api/v1/center/region-routes"),
            Some(REGION_ROUTES_READ)
        );
        assert_eq!(
            route_permission(&Method::POST, "/api/v1/center/region-routes/failover"),
            Some(REGION_ROUTES_WRITE)
        );
        assert_eq!(
            route_permission(&Method::GET, "/api/v1/center/cluster-region-routes"),
            Some(REGION_ROUTES_READ)
        );
        assert_eq!(
            route_permission(
                &Method::POST,
                "/api/v1/center/cluster-region-routes/failover"
            ),
            Some(REGION_ROUTES_WRITE)
        );
        assert_eq!(
            route_permission(
                &Method::GET,
                "/api/v1/center/global-connection-ip-restrictions"
            ),
            Some(IP_RESTRICTIONS_READ)
        );
        assert_eq!(
            route_permission(
                &Method::PATCH,
                "/api/v1/center/global-connection-ip-restrictions/default/foo/enable"
            ),
            Some(IP_RESTRICTIONS_WRITE)
        );
        assert_eq!(
            route_permission(&Method::DELETE, "/api/v1/center/admin/controllers/c1"),
            Some(CONTROLLERS_WRITE)
        );
        assert_eq!(
            route_permission(&Method::GET, "/api/v1/center/admin/audit-logs"),
            Some(AUDIT_READ)
        );
        assert_eq!(
            route_permission(
                &Method::GET,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones"
            ),
            Some(CLOUDFLARE_DNS_READ)
        );
        assert_eq!(
            route_permission(
                &Method::POST,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones"
            ),
            Some(CLOUDFLARE_DNS_WRITE)
        );
        for path in [
            "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones",
            "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123",
            "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/record-sets",
            "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/record-sets/A",
        ] {
            assert_eq!(route_permission(&Method::GET, path), Some(ROUTE53_DNS_READ));
            assert_eq!(route_permission(&Method::HEAD, path), None);
        }
        let record_path =
            "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/record-sets/A";
        assert_eq!(
            route_permission(&Method::PUT, record_path),
            Some(ROUTE53_DNS_WRITE)
        );
        assert_eq!(
            route_permission(&Method::DELETE, record_path),
            Some(ROUTE53_DNS_WRITE)
        );
        assert_eq!(route_permission(&Method::HEAD, record_path), None);
        assert_eq!(route_permission(&Method::POST, record_path), None);
        assert_eq!(route_permission(&Method::PATCH, record_path), None);
        let batch_path =
            "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/change-batches";
        assert_eq!(
            route_permission(&Method::POST, batch_path),
            Some(ROUTE53_DNS_WRITE)
        );
        assert_eq!(route_permission(&Method::GET, batch_path), None);
        assert_eq!(route_permission(&Method::PUT, batch_path), None);
        assert_eq!(route_permission(&Method::DELETE, batch_path), None);
        let change_path =
            "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/changes/C123";
        assert_eq!(
            route_permission(&Method::GET, change_path),
            Some(ROUTE53_DNS_READ)
        );
        assert_eq!(route_permission(&Method::HEAD, change_path), None);
        assert_eq!(route_permission(&Method::POST, change_path), None);
        for path in [
            "/api/v1/center/aws/route53/accounts",
            "/api/v1/center/aws/route53/accounts/aws-main",
            "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/record-sets/A/extra",
            "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/change-batches/extra",
            "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/changes/C123/extra",
            "/api/v1/center/aws/route53/accounts//hosted-zones",
        ] {
            assert_eq!(route_permission(&Method::GET, path), None);
            assert_eq!(route_permission(&Method::POST, path), None);
        }
        let zone_path = "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123";
        assert_eq!(route_permission(&Method::POST, zone_path), None);
        assert_eq!(route_permission(&Method::PUT, zone_path), None);
        assert_eq!(route_permission(&Method::DELETE, zone_path), None);
        assert_eq!(
            route_permission(
                &Method::PUT,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1/record-sets/A/remote-control"
            ),
            Some(CLOUDFLARE_DNS_REMOTE_WRITE)
        );
        assert_eq!(
            route_permission(
                &Method::POST,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1/record-sets/A/remote-control"
            ),
            None
        );
        assert_eq!(
            route_permission(
                &Method::PUT,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1/record-sets/A/remote-control/extra"
            ),
            Some(CLOUDFLARE_DNS_WRITE)
        );
        assert_eq!(
            route_permission(
                &Method::DELETE,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1"
            ),
            Some(CLOUDFLARE_DNS_WRITE)
        );
        assert_eq!(
            route_permission(
                &Method::GET,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1/record-sets/A"
            ),
            Some(CLOUDFLARE_DNS_READ)
        );
        assert_eq!(
            route_permission(
                &Method::DELETE,
                "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1/record-sets/A"
            ),
            Some(CLOUDFLARE_DNS_WRITE)
        );
        for method in [Method::PUT, Method::PATCH] {
            assert_eq!(
                route_permission(
                    &method,
                    "/api/v1/center/cloudflare/dns/accounts/account-1/zones/zone-1"
                ),
                Some(CLOUDFLARE_DNS_WRITE)
            );
        }
    }

    #[test]
    fn cloudflare_waf_routes_use_the_narrow_permission_suffixes() {
        let base = "/api/v1/center/cloudflare/waf/accounts/cf-main/zones/zone-1";
        assert_eq!(
            route_permission(&Method::GET, &format!("{base}/rulesets")),
            Some(CLOUDFLARE_WAF_READ)
        );
        assert_eq!(
            route_permission(&Method::POST, &format!("{base}/custom-rules")),
            Some(CLOUDFLARE_WAF_WRITE)
        );
        assert_eq!(
            route_permission(&Method::PUT, &format!("{base}/custom-rules/rule-1/order")),
            Some(CLOUDFLARE_WAF_ORDER)
        );
        assert_eq!(
            route_permission(&Method::PUT, &format!("{base}/managed-rules/rule-1/order")),
            Some(CLOUDFLARE_WAF_ORDER)
        );
        assert_eq!(
            route_permission(&Method::PUT, &format!("{base}/managed-rules/exceptions")),
            Some(CLOUDFLARE_WAF_EXCEPTION)
        );
        assert_eq!(
            route_permission(
                &Method::DELETE,
                &format!("{base}/rate-limits/rule-1/security-weaken")
            ),
            Some(CLOUDFLARE_WAF_SECURITY_WEAKEN)
        );
        assert_eq!(
            route_permission(&Method::DELETE, &format!("{base}/rate-limits/rule-1")),
            None,
            "destructive WAF changes require the dedicated security-weaken route"
        );
    }

    /// `is_business_path` covers /api/v1/ except the public auth routes.
    #[test]
    fn business_path_classification() {
        assert!(is_business_path("/api/v1/controllers"));
        assert!(is_business_path("/api/v1/center/something-new"));
        // Public auth routes are NOT business paths.
        assert!(!is_business_path("/api/v1/auth/login"));
        assert!(!is_business_path("/api/v1/auth/status"));
        // Probe/metrics and other non-/api/v1 paths are not business paths.
        assert!(!is_business_path("/health"));
        assert!(!is_business_path("/metrics"));
        assert!(!is_business_path("/auth/me"));
    }

    /// Segment-boundary prefixes must not match a sibling like `base + "-v2"`.
    #[test]
    fn segment_safe_prefixes() {
        // Sibling paths sharing a textual prefix must NOT resolve to the base key.
        assert_eq!(
            route_permission(&Method::GET, "/api/v1/center/cluster-region-routes-v2"),
            None
        );
        assert_eq!(
            route_permission(&Method::GET, "/api/v1/center/service-region-routes-v2"),
            None
        );
        assert_eq!(
            route_permission(
                &Method::GET,
                "/api/v1/center/global-connection-ip-restrictions-v2"
            ),
            None
        );
        assert_eq!(
            route_permission(&Method::GET, "/api/v1/center/cloudflare/dns-v2"),
            None
        );
        // Exact base and segment-boundary children still resolve.
        assert_eq!(
            route_permission(&Method::GET, "/api/v1/center/cluster-region-routes"),
            Some(REGION_ROUTES_READ)
        );
        assert_eq!(
            route_permission(
                &Method::GET,
                "/api/v1/center/cluster-region-routes/consistency"
            ),
            Some(REGION_ROUTES_READ)
        );
    }

    /// Auth, probe, and metrics paths are NOT business routes → `None`.
    #[test]
    fn non_business_routes_return_none() {
        for p in [
            "/api/v1/auth/login",
            "/api/v1/auth/logout",
            "/api/v1/auth/me",
            "/api/v1/auth/status",
            "/health",
            "/ready",
            "/metrics",
            "/unmapped/path",
        ] {
            assert_eq!(
                route_permission(&Method::GET, p),
                None,
                "{p} must be unmapped"
            );
            assert_eq!(
                route_permission(&Method::POST, p),
                None,
                "{p} must be unmapped"
            );
        }
    }
}
