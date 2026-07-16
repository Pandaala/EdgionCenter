#!/usr/bin/env bash
set -euo pipefail
mode="${1:-}"
if [[ "$mode" != standalone && "$mode" != kubernetes ]]; then echo 'usage: run.sh standalone|kubernetes' >&2; exit 2; fi
cd "$(dirname "$0")/../.."
export E2E_MODE="$mode"
export E2E_RUN_ID="${E2E_RUN_ID:-resource-ui-$(date -u +%Y%m%d-%H%M%SZ)-$$}"
export E2E_ARTIFACT_DIR="${E2E_ARTIFACT_DIR:-$PWD/test-results/$E2E_RUN_ID}"
export E2E_ALLOW_MUTATION=1
export E2E_INVENTORY_STRICT=1
: "${E2E_USERNAME:?}" "${E2E_PASSWORD:?}" "${E2E_CONTROLLER_A:?}" "${E2E_CONTROLLER_B:?}"
if [[ ! "$E2E_CONTROLLER_A" =~ ^[A-Za-z0-9._-]+$ || ! "$E2E_CONTROLLER_B" =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo 'E2E controller names must match [A-Za-z0-9._-]+' >&2; exit 2
fi
required_tools=(curl lsof npm npx openssl shasum cargo)
playwright_args=()
if [[ -n "${E2E_PLAYWRIGHT_GREP:-}" ]]; then playwright_args+=(--grep "$E2E_PLAYWRIGHT_GREP"); fi
if [[ "$mode" == kubernetes ]]; then required_tools+=(kubectl htpasswd); fi
for tool in "${required_tools[@]}"; do command -v "$tool" >/dev/null || { echo "required tool is unavailable: $tool" >&2; exit 2; }; done
mkdir -p "$E2E_ARTIFACT_DIR"
pids=()
pid_logs=()
stop_owned_processes() {
  local pid
  for pid in "${pids[@]:-}"; do if kill -0 "$pid" 2>/dev/null; then kill "$pid"; wait "$pid" || true; fi; done
}
trap stop_owned_processes EXIT INT TERM
check_owned_processes() {
  local index pid log
  for index in "${!pids[@]}"; do
    pid="${pids[$index]}"; log="${pid_logs[$index]}"
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "owned process exited before readiness (pid=$pid, log=$log)" >&2
      tail -n 120 "$log" >&2 2>/dev/null || true
      return 1
    fi
  done
}
wait_url() {
  local url="$1" deadline=$((SECONDS + 60))
  until curl --fail --silent --show-error "$url" >/dev/null; do
    check_owned_processes
    if (( SECONDS >= deadline )); then echo "readiness deadline exceeded: $url" >&2; return 1; fi
    sleep 1
  done
}
assert_can_i() {
  local expected="$1" actor="$2" verb="$3" resource="$4"; shift 4
  local actual status=0
  actual="$(kubectl --context "$E2E_KUBE_CONTEXT" auth can-i "$verb" "$resource" --as="$actor" "$@")" || status=$?
  if (( status != 0 )) && [[ ! ( "$status" == 1 && "$actual" == no ) ]]; then
    echo "RBAC check failed to execute: status=$status actor=$actor verb=$verb resource=$resource args=$*" >&2
    exit 1
  fi
  if [[ "$actual" != "$expected" ]]; then
    echo "RBAC assertion failed: expected=$expected actual=$actual actor=$actor verb=$verb resource=$resource args=$*" >&2
    exit 1
  fi
}
assert_port_free() { if lsof -nP -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1; then echo "refusing occupied port $1" >&2; exit 1; fi; }
npm run e2e:inventory
e2e/scripts/generate-tls.sh
if [[ "$mode" == standalone ]]; then
  for port in 12200 12201 12251 13100 13101 13151 13190 13200 13201 13251 13290 15173; do assert_port_free "$port"; done
  cargo build -p edgion-center-standalone --manifest-path ../Cargo.toml
  cargo build -p edgion-controller --manifest-path ../../Edgion-resource-ui/Cargo.toml
  npx tsx e2e/scripts/render-runtime.ts e2e/runtime/standalone.yaml "$E2E_ARTIFACT_DIR/standalone.yaml"
  npx tsx e2e/scripts/render-runtime.ts e2e/runtime/controllers/controller-a.yaml "$E2E_ARTIFACT_DIR/controller-a.yaml"
  npx tsx e2e/scripts/render-runtime.ts e2e/runtime/controllers/controller-b.yaml "$E2E_ARTIFACT_DIR/controller-b.yaml"
  for controller in controller-a controller-b; do
    mkdir -p "$E2E_ARTIFACT_DIR/$controller/config/crd"
    cp -R ../../Edgion-resource-ui/config/crd/. "$E2E_ARTIFACT_DIR/$controller/config/crd/"
  done
  e2e/scripts/seed.sh
  ../target/debug/edgion-center-standalone --config-file "$E2E_ARTIFACT_DIR/standalone.yaml" >"$E2E_ARTIFACT_DIR/center.log" 2>&1 & pids+=("$!"); pid_logs+=("$E2E_ARTIFACT_DIR/center.log")
  ../../Edgion-resource-ui/target/debug/edgion-controller --config-file "$E2E_ARTIFACT_DIR/controller-a.yaml" >"$E2E_ARTIFACT_DIR/controller-a.log" 2>&1 & pids+=("$!"); pid_logs+=("$E2E_ARTIFACT_DIR/controller-a.log")
  ../../Edgion-resource-ui/target/debug/edgion-controller --config-file "$E2E_ARTIFACT_DIR/controller-b.yaml" >"$E2E_ARTIFACT_DIR/controller-b.log" 2>&1 & pids+=("$!"); pid_logs+=("$E2E_ARTIFACT_DIR/controller-b.log")
  ./node_modules/.bin/vite --host 127.0.0.1 --port 15173 --strictPort >"$E2E_ARTIFACT_DIR/vite.log" 2>&1 & pids+=("$!"); pid_logs+=("$E2E_ARTIFACT_DIR/vite.log")
  wait_url http://127.0.0.1:12201/api/v1/auth/status
  wait_url http://127.0.0.1:13100/ready
  wait_url http://127.0.0.1:13200/ready
  wait_url http://127.0.0.1:15173/login
  npx tsx e2e/scripts/wait-controllers.ts
  if (( ${#playwright_args[@]} )); then npm run e2e:standalone -- "${playwright_args[@]}"; else npm run e2e:standalone; fi
else
  : "${KUBECONFIG:?}" "${E2E_OAUTH_CLIENT_SECRET:?}" "${E2E_KUBE_CONTEXT:?}"
  if [[ "$E2E_KUBE_CONTEXT" != orbstack ]]; then echo 'E2E_KUBE_CONTEXT must be exactly orbstack' >&2; exit 2; fi
  if [[ "$(kubectl config get-contexts orbstack -o name)" != orbstack ]]; then echo 'Kubernetes context is unavailable: orbstack' >&2; exit 2; fi
  npx tsx e2e/scripts/check-kubernetes-apis.ts
  assert_port_free 14180
  (cd .. && cicd/build-image.sh --mode kubernetes -t "edgion-center-kubernetes:$E2E_RUN_ID")
  ../../Edgion-resource-ui/cicd/build-image.sh --version "$E2E_RUN_ID"
  case "$(uname -m)" in
    arm64|aarch64) image_arch=arm64 ;;
    x86_64|amd64) image_arch=amd64 ;;
    *) echo "unsupported local image architecture: $(uname -m)" >&2; exit 2 ;;
  esac
  docker tag "docker.io/pandaala/edgion-controller:${E2E_RUN_ID}_${image_arch}" "pandaala/edgion-controller:$E2E_RUN_ID"
  npx tsx e2e/scripts/apply-runtime.ts
  prefix="eruie2e-$(printf %s "$E2E_RUN_ID" | shasum -a 256 | cut -c1-8)"; namespace="$prefix-system"
  center_actor="system:serviceaccount:$namespace:$prefix-center-service-account"
  assert_can_i yes "$center_actor" create subjectaccessreviews.authorization.k8s.io
  assert_can_i yes "$center_actor" create leases.coordination.k8s.io -n "$namespace"
  assert_can_i yes "$center_actor" get pods -n "$namespace"
  assert_can_i no "$center_actor" create leases.coordination.k8s.io -n default
  assert_can_i no "$center_actor" get pods -n default
  assert_can_i no "$center_actor" create edgioncontrollers.center.edgion.io -n default
  for controller in controller-a controller-b; do
    actor="system:serviceaccount:$namespace:$prefix-$controller-service-account"
    assert_can_i yes "$actor" list services -n "$prefix-a"
    assert_can_i yes "$actor" list secrets -n "$prefix-b"
    assert_can_i yes "$actor" list gatewayclasses.gateway.networking.k8s.io
    assert_can_i yes "$actor" list namespaces
    assert_can_i yes "$actor" create leases.coordination.k8s.io -n "$namespace"
    assert_can_i no "$actor" list leases.coordination.k8s.io -n "$namespace"
    assert_can_i no "$actor" create leases.coordination.k8s.io -n default
    assert_can_i yes "$actor" patch pods -n "$namespace"
    assert_can_i no "$actor" patch pods -n default
    assert_can_i no "$actor" list nodes
    assert_can_i no "$actor" list services -n default
    assert_can_i no "$actor" create secrets -n default
    assert_can_i no "$actor" list secrets --all-namespaces
  done
  actor_a="system:serviceaccount:$namespace:$prefix-controller-a-service-account"
  actor_b="system:serviceaccount:$namespace:$prefix-controller-b-service-account"
  npx tsx e2e/scripts/assert-sar.ts yes "$actor_a" get coordination.k8s.io leases "$namespace" "$prefix-controller-a"
  npx tsx e2e/scripts/assert-sar.ts no "$actor_a" get coordination.k8s.io leases "$namespace" "$prefix-controller-b"
  npx tsx e2e/scripts/assert-sar.ts yes "$actor_b" get coordination.k8s.io leases "$namespace" "$prefix-controller-b"
  npx tsx e2e/scripts/assert-sar.ts no "$actor_b" get coordination.k8s.io leases "$namespace" "$prefix-controller-a"
  issuer="https://$prefix-dex.$namespace.svc.cluster.local:5556/dex"
  dex_subject="$(node -e 'const id=Buffer.from(process.argv[1]); const value=Buffer.concat([Buffer.from([0x0a,id.length]),id,Buffer.from([0x12,0x05]),Buffer.from("local")]); process.stdout.write(value.toString("base64url"))' "$prefix-user")"
  oidc_actor="oidc:${#issuer}:$issuer:user:$dex_subject"
  assert_can_i yes "$oidc_actor" get "/edgion-center-authz/api/v1/proxy/e2e-a~$E2E_CONTROLLER_A/api/v1/access"
  assert_can_i yes "$oidc_actor" get "/edgion-center-authz/api/v1/proxy/e2e-b~$E2E_CONTROLLER_B/api/v1/access"
  assert_can_i no "$oidc_actor" get "/edgion-center-authz/api/v1/proxy/not-owned/api/v1/access"
  assert_can_i no "$oidc_actor" get "/edgion-center-authz/api/v1/center/admin/users"
  assert_can_i yes "$oidc_actor" list edgioncontrollers.center.edgion.io
  assert_can_i yes "$oidc_actor" get "/edgion-center-authz/permissions/proxy:access"
  assert_can_i no "$oidc_actor" get "/edgion-center-authz/permissions/users:manage"
  assert_can_i no "$oidc_actor" get "/edgion-center-authz/permissions/roles:manage"
  assert_can_i no "$oidc_actor" get "/edgion-center-authz/permissions/audit:read"
  e2e/scripts/seed.sh
  for deployment in "$prefix-center" "$prefix-controller-a" "$prefix-controller-b" "$prefix-dex" "$prefix-oauth2-proxy"; do kubectl --context "$E2E_KUBE_CONTEXT" -n "$namespace" rollout status "deployment/$deployment" --timeout=180s; done
  kubectl --context "$E2E_KUBE_CONTEXT" -n "$namespace" port-forward "service/$prefix-oauth2-proxy" 14180:80 >"$E2E_ARTIFACT_DIR/port-forward.log" 2>&1 & pids+=("$!"); pid_logs+=("$E2E_ARTIFACT_DIR/port-forward.log")
  wait_url http://127.0.0.1:14180/api/v1/auth/status
  npx tsx e2e/scripts/wait-controllers.ts
  if (( ${#playwright_args[@]} )); then npm run e2e:kubernetes -- "${playwright_args[@]}"; else npm run e2e:kubernetes; fi
fi
E2E_RETAIN_ENV=1 e2e/scripts/cleanup.sh
