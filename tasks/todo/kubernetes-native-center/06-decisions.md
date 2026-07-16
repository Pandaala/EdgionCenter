# Decisions

| Date | ID | Decision | Reason |
|---|---|---|---|
| 2026-07-14 | KN-D01 | Use owner-aware internal forwarding for active-active command and proxy requests | Any replica can serve global reads while connection-bound operations reach the replica holding the stream |
| 2026-07-14 | KN-D02 | Keep controller eviction as an explicit operation rather than overloading CR deletion | Registration is observed state; deleting a projection has ambiguous desired-state semantics and reconnect can recreate it |
| 2026-07-14 | KN-D03 | Do not reproduce Kubernetes Role/RoleBinding management in the Center dashboard initially | Kubernetes/IdP remains the canonical identity and role-management plane; Center only exposes capability metadata |
| 2026-07-14 | KN-D04 | Release separate minimal Kubernetes and standalone images | Keeps SQL and Kube dependency trees out of the opposite artifact |
| 2026-07-14 | KN-D05 | Keep `edgion-center` as a temporary alias of standalone behavior during migration | Preserves current scripts and deployment behavior until explicit replacements pass parity checks |
