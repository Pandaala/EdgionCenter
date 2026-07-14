# EdgionCenter

EdgionCenter is Edgion's multi-cluster federation hub. It exposes one shared protocol and
Admin API through two intentionally separate binaries:

- `edgion-center-standalone`: SQLite/MySQL persistence, password or OIDC authentication,
  database RBAC, and queryable audit history.
- `edgion-center-kubernetes`: database-free CRD/Lease state, OIDC, Kubernetes
  SubjectAccessReview, stdout audit, and active-active replica forwarding.

Build locally with `cargo build -p edgion-center-standalone` or
`cargo build -p edgion-center-kubernetes`. Build container images with:

```sh
cicd/build-image.sh --mode standalone
cicd/build-image.sh --mode kubernetes
```

Container builds use this repository as their only source context; no sibling Edgion
checkout is required. Kubernetes manifests and secret setup live in
`cicd/deploy/center-kubernetes/`. Run `cicd/integration/run-matrix.sh` for the complete
hermetic validation suite. Both images read `/etc/edgion-center/config.yaml` by default;
mount the mode-specific configuration there or pass `--config-file` explicitly.

See `AGENTS.md` and `skills/SKILL.md` for architecture and development routing.
