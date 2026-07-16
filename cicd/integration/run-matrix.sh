#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets -- --nocapture
cargo test -p edgion-center-app --no-default-features

standalone_tree="$(cargo tree -p edgion-center-standalone)"
if rg -q '(^| )kube v|k8s-openapi' <<<"$standalone_tree"; then
  echo "standalone dependency graph contains Kubernetes crates" >&2
  exit 1
fi
kubernetes_tree="$(cargo tree -p edgion-center-kubernetes)"
if rg -q 'sqlx|edgion-center-adapter-sql' <<<"$kubernetes_tree"; then
  echo "Kubernetes dependency graph contains SQL crates" >&2
  exit 1
fi

if [[ "${EDGION_SKIP_KUBECTL:-0}" == "1" ]]; then
  echo "skipping manifest validation: EDGION_SKIP_KUBECTL=1"
else
  command -v kubectl >/dev/null 2>&1 || {
    echo "kubectl is required for manifest validation (or set EDGION_SKIP_KUBECTL=1)" >&2
    exit 1
  }
  kubectl kustomize cicd/deploy/center-kubernetes >/dev/null
fi

if [[ "${EDGION_TEST_KUBERNETES:-0}" == "1" ]]; then
  : "${EDGION_TEST_KUBERNETES_NAMESPACE:?set a disposable namespace with the CRD installed}"
  kubectl apply --dry-run=client -k cicd/deploy/center-kubernetes >/dev/null
  cargo test -p edgion-center-adapter-kubernetes --test real_cluster -- --nocapture
else
  echo "skipping real kube-apiserver matrix: EDGION_TEST_KUBERNETES=1 is not set"
fi

if [[ -n "${EDGION_TEST_MYSQL_URL:-}" ]]; then
  cargo test -p edgion-center-adapter-sql -- --nocapture
else
  echo "skipping external MySQL matrix: EDGION_TEST_MYSQL_URL is not set"
fi

if [[ "${EDGION_SKIP_WEB:-0}" != "1" ]]; then
  npm --prefix web ci
  npm --prefix web run lint
  npm --prefix web test
  npm --prefix web run build
fi

cicd/checks/check_english_only.sh
cicd/checks/check_no_legacy_pm.sh
git diff --check
