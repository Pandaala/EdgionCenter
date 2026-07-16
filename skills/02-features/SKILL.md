---
name: center-features
description: Binary selection, ports, configuration, authentication, and deployment.
---

# Features and operations

Choose the composition by ownership model, not size:

| Mode | Use when | State/authz/audit |
|---|---|---|
| Standalone | Center owns application data independently of Kubernetes | SQLite/MySQL, optional DB RBAC, SQL audit queries |
| Kubernetes | Kubernetes API is the control-plane source of truth | CRDs/Leases, SAR, structured stdout audit |

There is no legacy `edgion-center` compatibility binary. Run
`edgion-center-standalone -c config/edgion-center.yaml` or deploy
`edgion-center-kubernetes` with `cicd/deploy/center-kubernetes`.

| Port | Purpose |
|---|---|
| `12251` | Controller-to-Center FederationSync, strict mTLS |
| `12201` | Admin HTTP API/dashboard |
| `12200` | `/health` and `/ready` |
| `12290` | Prometheus metrics |
| `12252` | Kubernetes replica forwarding, dedicated mTLS only |

Standalone configuration is documented inline in `config/edgion-center.yaml`; detailed
access-control behavior is in [access-control.md](access-control.md). Kubernetes config and
secret requirements are in `cicd/deploy/center-kubernetes/README.md`. Build mode-specific
images with `cicd/build-image.sh --mode standalone|kubernetes`.
