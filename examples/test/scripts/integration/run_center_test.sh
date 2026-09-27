#!/usr/bin/env bash
# =============================================================================
# Center Federation Integration Test — two-controller end-to-end
#
# Proves the converged Center<->Controller architecture with real binaries:
# 1 Center (standalone, SQLite) + 2 synced Controllers + 1 lifecycle Controller
# that first validates the center.enabled=false kill switch and is then
# restarted with an explicit deny-all rbac to validate the terminal watch
# denial. Federation runs mTLS with SPIFFE peer-identity binding.
#
# Covered surfaces (test IDs in run order):
#   Registration / stats
#     A1 controllers_online        — both controllers online via GET /api/v1/controllers;
#                                    kill-switch controller absent
#     A2 stats_counts              — key_count arrives via StatsReport (never a list)
#   Watch-cache reads (GlobalResources; no proxy involved)
#     B1 global_list               — initial full list is served from the watch cache
#     B2 multi_namespace_identity  — same name in two namespaces stays two rows
#     B3 global_detail             — detail matches the fixture document
#   Proxy reads & the EdgionConfigData write surface
#     C1 proxy_get                 — drill-in GET through the tunnel, version present
#     C2 proxy_create_watch_add    — POST creates a Selector; add event reaches the cache
#     C3 selector_active_switch    — PUT flips /spec/data/config/active (the only way
#                                    to switch an active profile)
#     C4 put_conflict_recover      — stale If-Match -> 409, terminal; fresh version succeeds
#     C5 delete_stale_precondition — DELETE with stale If-Match -> 409
#     C6 proxy_delete_watch_del    — DELETE with fresh If-Match; delete event reaches cache
#     C7 rbac_deny_secret          — proxy GET /api/v1/cluster/Secret -> 403
#     C8 rbac_allow_region_route   — proxy GET /api/v1/region-routes/effective -> 200
#     C9 rbac_deny_other_kind_write— proxy POST on a non-EdgionConfigData kind -> 403
#   Center write core (RegionRouteOverride fan-out) & outcomes
#     D1 failover_converged        — fan-out failover: modified=2, all outcomes converged,
#                                    both controllers observe failoverTo
#     D2 failover_idempotent_skip  — same failover again: controller versions unchanged
#     D3 failover_type_pinning     — wrong-type (IpList) target is refused
#     D4 failover_unknown_field    — deny_unknown_fields on the request body
#     D5 sync_source_to_target     — east's spec.data replaces west's row
#     D6 failover_clear            — empty failoverTo clears on both controllers
#   Reload & ConfigSyncServer lifecycle
#     E1 reload_converged          — Center reports a terminal outcome (never bare 200);
#                                    reload allowed under the default policy
#     E2 post_reload_watch        — server_id change re-established the watch
#   Disconnect / reconnect / eviction
#     F1 disconnect_offline        — killed controller goes offline on missed heartbeats
#     F2 reconnect_resync          — restart re-registers and re-lists (full resync)
#     F3 eviction                  — admin DELETE evicts the durable record; a later
#                                    restart re-registers cleanly
#   Large proxied response
#     H1 big_list_over_4mib        — a >4 MiB proxied list completes and the federation
#                                    stream survives it
#   Terminal watch denial (explicit deny-all rbac)
#     G1 watch_denied_terminal     — silo registers, watch is denied Forbidden, terminal
#                                    (no retry storm), reads and writes 403
#
# Deliberately NOT covered here, with the evidence that covers each instead:
#   - superseded/accepted/unknown/conflict write outcomes: need races, replicas, or
#     stalled watches that a single-host script cannot stage deterministically.
#     Evidence: center-app unit tests (config_data_ops, reload_ops, the
#     region_route_handlers status mapping) and web unit tests
#     (WriteOutcomeTag.test.tsx, reloadOutcomeText.test.ts) for distinct rendering.
#   - watch overflow: watch_cache::cache::tests::apply_events_overflow_rejects_batch_and_flags.
#   - unsupported watch kind: Center subscribes exactly EDGION_CONFIG_DATA
#     (federation/server.rs); the Controller-side denial path is unit-tested in
#     fed_client.
#   - version gap -> full resync: gap detection is unit-tested in center-runtime;
#     the e2e re-list path is exercised by E2/F2.
#   - DELETE without a precondition: the refusal lives in the dashboard client
#     (web/src/api/resources.ts requiredResourceVersionHeader) and its unit test;
#     the server contract deliberately mirrors Kubernetes (opt-in CAS).
#
# Port allocation (all on 127.0.0.1):
#   center:       gRPC 50952, HTTP 5910, probe 5919, metrics 5918
#   controller-1: gRPC 50953, admin 5911, probe 5931, metrics 5941  (east-cluster/ctrl-east)
#   controller-2: gRPC 50954, admin 5912, probe 5932, metrics 5942  (west-cluster/ctrl-west)
#   controller-3: gRPC 50955, admin 5913, probe 5933, metrics 5943  (silo-cluster/ctrl-silo)
#
# Usage:
#   ./run_center_test.sh             # Full run (build + test + cleanup)
#   ./run_center_test.sh --no-build  # Skip cargo build
# =============================================================================

set -euo pipefail
umask 077

# ── Paths ────────────────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
source "$SCRIPT_DIR/../utils/owned_runtime.sh"
# The controller binary comes from the sibling Edgion repo (Center was extracted
# out of that monorepo). Override with EDGION_DIR if it lives elsewhere.
EDGION_DIR="${EDGION_DIR:-$(cd "$REPO_ROOT/.." && pwd)/Edgion}"
CENTER_BIN="$REPO_ROOT/target/debug/edgion-center-standalone"
CTRL_BIN="$EDGION_DIR/target/debug/edgion-controller"
CONF_SRC="$REPO_ROOT/examples/test/conf/Center"

# Ports
CENTER_GRPC_PORT=50952
CENTER_HTTP_PORT=5910
CENTER_PROBE_PORT=5919
CENTER_METRICS_PORT=5918
CTRL1_GRPC_PORT=50953
CTRL1_ADMIN_PORT=5911
CTRL1_PROBE_PORT=5931
CTRL1_METRICS_PORT=5941
CTRL2_GRPC_PORT=50954
CTRL2_ADMIN_PORT=5912
CTRL2_PROBE_PORT=5932
CTRL2_METRICS_PORT=5942
CTRL3_GRPC_PORT=50955
CTRL3_ADMIN_PORT=5913
CTRL3_PROBE_PORT=5933
CTRL3_METRICS_PORT=5943

CENTER_HTTP="http://127.0.0.1:${CENTER_HTTP_PORT}"
CENTER_PROBE="http://127.0.0.1:${CENTER_PROBE_PORT}"

AUTH_USER="admin"
AUTH_PASS="test-center-pass"
JWT_SECRET="test-jwt-secret-center"
TRUST_DOMAIN="edgion.io"

CTRL1_ID="east-cluster/ctrl-east"
CTRL2_ID="west-cluster/ctrl-west"
CTRL3_ID="silo-cluster/ctrl-silo"
CTRL1_TID="east-cluster~ctrl-east"
CTRL2_TID="west-cluster~ctrl-west"
CTRL3_TID="silo-cluster~ctrl-silo"

# ── State ────────────────────────────────────────────────────────────────────
WORK_DIR=""
CENTER_PID=""
CTRL1_PID=""
CTRL2_PID=""
CTRL3_PID=""
TOKEN=""
PASS=0
FAIL=0

# ── Colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

# ── Helpers ──────────────────────────────────────────────────────────────────
log()  { echo "[$(date '+%H:%M:%S')] $*"; }
pass() { echo -e "[$(date '+%H:%M:%S')] ${GREEN}PASS${NC}: $1"; ((PASS++)) || true; }
fail() { echo -e "[$(date '+%H:%M:%S')] ${RED}FAIL${NC}: $1 — $2"; ((FAIL++)) || true; }


trap cleanup_owned_runtime EXIT

wait_for_http() {
  local url="$1" timeout="$2" elapsed=0
  log "Waiting for HTTP endpoint: $url (timeout ${timeout}s)"
  while ! curl -sf --max-time 2 "$url" >/dev/null 2>&1; do
    if [[ $elapsed -ge $timeout ]]; then
      echo -e "${RED}ERROR${NC}: Timed out waiting for $url" >&2
      exit 1
    fi
    sleep 1
    ((elapsed++)) || true
  done
  log "HTTP endpoint ready after ${elapsed}s"
}

# request METHOD URL [BODY] [EXTRA_HEADER] — prints "<code>\n<body>"
request() {
  local method="$1" url="$2" body="${3:-}" extra="${4:-}"
  local args=(-s --max-time 35 -X "$method" -H "Authorization: Bearer $TOKEN" -o /dev/stdout -w '\n__CODE__%{http_code}')
  [[ -n "$body" ]] && args+=(-H "Content-Type: application/json" --data-binary "$body")
  [[ -n "$extra" ]] && args+=(-H "$extra")
  local out
  out=$(curl "${args[@]}" "$url" 2>/dev/null || true)
  local code="${out##*__CODE__}"
  local resp="${out%$'\n'__CODE__*}"
  printf '%s\n%s' "$code" "$resp"
}
req_code() { head -n1 <<<"$1"; }
req_body() { tail -n +2 <<<"$1"; }

# jsonq JSON PY_EXPR — run a python expression over parsed JSON `d`; prints result
jsonq() {
  python3 -c "
import sys, json
try:
    d = json.loads(sys.argv[1])
except Exception:
    print('PARSE_ERROR'); sys.exit(0)
try:
    print($2)
except Exception as e:
    print('EXPR_ERROR:' + str(e))
" "$1"
}

do_login() {
  local resp
  resp=$(curl -sf --max-time 10 -H "Content-Type: application/json" \
    -d "{\"username\":\"${AUTH_USER}\",\"password\":\"${AUTH_PASS}\"}" \
    "${CENTER_HTTP}/api/v1/auth/login" 2>/dev/null || true)
  jsonq "$resp" "d['data']['token']"
}

# poll_until DESC TIMEOUT_SECS CMD... — retries CMD (a function) every second
# until it returns 0. Returns 1 on timeout.
poll_until() {
  local desc="$1" timeout="$2"; shift 2
  local elapsed=0
  while ! "$@"; do
    if [[ $elapsed -ge $timeout ]]; then
      log "poll_until timed out: $desc"
      return 1
    fi
    sleep 1
    ((elapsed++)) || true
  done
  return 0
}

online_count() {
  local r; r=$(request GET "$CENTER_HTTP/api/v1/controllers")
  jsonq "$(req_body "$r")" "sum(1 for c in d['data'] if c.get('online'))"
}

# ── Parse args ────────────────────────────────────────────────────────────────
BUILD=true
for arg in "$@"; do
  [[ "$arg" == "--no-build" ]] && BUILD=false
done

# ── Build ─────────────────────────────────────────────────────────────────────
if $BUILD; then
  log "Building binaries..."
  (cd "$REPO_ROOT" && cargo build -p edgion-center-standalone 2>&1 | tail -5)
  (cd "$EDGION_DIR" && cargo build --bin edgion-controller 2>&1 | tail -5)
  log "Build complete"
fi

for bin in "$CENTER_BIN" "$CTRL_BIN"; do
  if [[ ! -x "$bin" ]]; then
    echo -e "${RED}ERROR${NC}: Binary not found or not executable: $bin" >&2
    exit 1
  fi
done

# Refuse occupied ports rather than stopping another developer's services.
assert_runtime_ports_free "$CENTER_GRPC_PORT" "$CENTER_HTTP_PORT" "$CENTER_PROBE_PORT" "$CENTER_METRICS_PORT" \
  "$CTRL1_GRPC_PORT" "$CTRL1_ADMIN_PORT" "$CTRL1_PROBE_PORT" "$CTRL1_METRICS_PORT" \
  "$CTRL2_GRPC_PORT" "$CTRL2_ADMIN_PORT" "$CTRL2_PROBE_PORT" "$CTRL2_METRICS_PORT" \
  "$CTRL3_GRPC_PORT" "$CTRL3_ADMIN_PORT" "$CTRL3_PROBE_PORT" "$CTRL3_METRICS_PORT"

# ── Work dir ──────────────────────────────────────────────────────────────────
WORK_DIR=$(mktemp -d)
mkdir -p "$WORK_DIR/logs" "$WORK_DIR/certs" "$WORK_DIR/ctrl1/conf" "$WORK_DIR/ctrl2/conf" "$WORK_DIR/ctrl3/conf"
log "Work dir: $WORK_DIR"

CERTS_DIR="$WORK_DIR/certs"

# ── Generate mTLS certificates ────────────────────────────────────────────────
log "Generating mTLS certificates (trust domain: ${TRUST_DOMAIN})..."

openssl req -x509 -newkey rsa:2048 -nodes \
  -keyout "$CERTS_DIR/ca.key" -out "$CERTS_DIR/ca.crt" -days 30 \
  -subj "/CN=Edgion Fed Test CA/O=EdgionTest" 2>/dev/null

openssl req -newkey rsa:2048 -nodes \
  -keyout "$CERTS_DIR/server.key" -out "$CERTS_DIR/server.csr" \
  -subj "/CN=edgion-center/O=EdgionTest" 2>/dev/null
cat > "$CERTS_DIR/server.ext" <<EOF
subjectAltName=IP:127.0.0.1,DNS:localhost
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$CERTS_DIR/server.csr" -CA "$CERTS_DIR/ca.crt" \
  -CAkey "$CERTS_DIR/ca.key" -CAcreateserial -out "$CERTS_DIR/server.crt" \
  -days 30 -extfile "$CERTS_DIR/server.ext" 2>/dev/null

# gen_client_cert NAME CLUSTER
gen_client_cert() {
  local name="$1" cluster="$2"
  openssl req -newkey rsa:2048 -nodes \
    -keyout "$CERTS_DIR/${name}.key" -out "$CERTS_DIR/${name}.csr" \
    -subj "/CN=${name}/O=EdgionTest" 2>/dev/null
  cat > "$CERTS_DIR/${name}.ext" <<EOF
subjectAltName=URI:spiffe://${TRUST_DOMAIN}/controllers/${cluster}/${name}
extendedKeyUsage=clientAuth
EOF
  openssl x509 -req -in "$CERTS_DIR/${name}.csr" -CA "$CERTS_DIR/ca.crt" \
    -CAkey "$CERTS_DIR/ca.key" -CAcreateserial -out "$CERTS_DIR/${name}.crt" \
    -days 30 -extfile "$CERTS_DIR/${name}.ext" 2>/dev/null
  log "  ${name} cert: spiffe://${TRUST_DOMAIN}/controllers/${cluster}/${name}"
}
gen_client_cert "ctrl-east" "east-cluster"
gen_client_cert "ctrl-west" "west-cluster"
gen_client_cert "ctrl-silo" "silo-cluster"
log "Certificate generation complete."

# ── Write center config ───────────────────────────────────────────────────────
# `sync` accepts exactly ping_interval_secs + command_timeout_secs now; the old
# list_interval/list_timeout keys were removed with the periodic list design and
# are a hard parse error under deny_unknown_fields.
cat > "$WORK_DIR/center.yaml" <<EOF
server:
  grpc_addr: "0.0.0.0:${CENTER_GRPC_PORT}"
  http_addr: "0.0.0.0:${CENTER_HTTP_PORT}"
  probe_addr: "0.0.0.0:${CENTER_PROBE_PORT}"
  metrics_addr: "0.0.0.0:${CENTER_METRICS_PORT}"

sync:
  ping_interval_secs: 5
  command_timeout_secs: 15

database:
  enabled: true
  backend: sqlite
  sqlite_path: "${WORK_DIR}/center.db"

grpc_security:
  active: fed
  certs:
    - name: fed
      cert: ${CERTS_DIR}/server.crt
      key: ${CERTS_DIR}/server.key
      ca: ${CERTS_DIR}/ca.crt

peer_identity:
  trust_domain: "${TRUST_DOMAIN}"

local_auth:
  enabled: true
  username: "${AUTH_USER}"
  password: "${AUTH_PASS}"
  jwt_secret: "${JWT_SECRET}"
  jwt_expiry_hours: 24
EOF

# ── Write controller configs ─────────────────────────────────────────────────
# write_controller_config IDX GRPC ADMIN PROBE METRICS CLUSTER NAME [ENABLED] [RBAC_YAML]
# RBAC_YAML is appended verbatim under `center:` (newline + 2-space indent).
write_controller_config() {
  local idx="$1" grpc_port="$2" admin_port="$3" probe_port="$4" metrics_port="$5"
  local cluster="$6" ctrl_name="$7"
  local center_enabled="${8:-true}"
  local rbac_yaml="${9:-}"
  local dir="$WORK_DIR/ctrl${idx}"
  cat > "${dir}/controller.yaml" <<EOF
work_dir: "${dir}"

server:
  grpc_listen: "0.0.0.0:${grpc_port}"
  admin_listen: "0.0.0.0:${admin_port}"
  probe_listen: "0.0.0.0:${probe_port}"
  metrics_listen: "0.0.0.0:${metrics_port}"

logging:
  log_dir: "${dir}/logs"
  log_prefix: "center-test-ctrl${idx}"
  log_level: "info"
  console: false

conf_center:
  type: "file_system"
  controller_name: "edgion.io/gateway-controller"
  conf_dir: "${dir}/conf"

conf_sync:
  no_sync_kinds: ["ReferenceGrant", "Secret"]

# The conf_sync (Controller->Gateway) channel refuses to start without an
# explicit transport choice. No gateway connects in this harness; plaintext is
# the deliberate, audited pick for the loopback-only test topology.
conf_sync_security:
  tls:
    skip_tls: true

center:
  address: "https://127.0.0.1:${CENTER_GRPC_PORT}"
  name: "${ctrl_name}"
  cluster: "${cluster}"
  env: ["testing"]
  enabled: ${center_enabled}
  security:
    active: fed
    certs:
      - name: fed
        cert: ${CERTS_DIR}/${ctrl_name}.crt
        key: ${CERTS_DIR}/${ctrl_name}.key
        ca: ${CERTS_DIR}/ca.crt${rbac_yaml}
EOF
  mkdir -p "${dir}/logs"
}

write_controller_config 1 "$CTRL1_GRPC_PORT" "$CTRL1_ADMIN_PORT" "$CTRL1_PROBE_PORT" "$CTRL1_METRICS_PORT" "east-cluster" "ctrl-east"
write_controller_config 2 "$CTRL2_GRPC_PORT" "$CTRL2_ADMIN_PORT" "$CTRL2_PROBE_PORT" "$CTRL2_METRICS_PORT" "west-cluster" "ctrl-west"
# Phase 1 for ctrl-silo: kill switch (enabled=false). Phase G restarts it with
# enabled=true + explicit deny-all rbac to exercise the terminal watch denial.
write_controller_config 3 "$CTRL3_GRPC_PORT" "$CTRL3_ADMIN_PORT" "$CTRL3_PROBE_PORT" "$CTRL3_METRICS_PORT" "silo-cluster" "ctrl-silo" "false"

# ── Copy CRD schemas + fixture resources ─────────────────────────────────────
for idx in 1 2; do
  local_dir="$WORK_DIR/ctrl${idx}"
  mkdir -p "${local_dir}/config"
  cp -r "$EDGION_DIR/config/crd" "${local_dir}/config/"
  cp "$CONF_SRC/ctrl${idx}/"*.yaml "${local_dir}/conf/"
done
local_dir="$WORK_DIR/ctrl3"
mkdir -p "${local_dir}/config"
cp -r "$EDGION_DIR/config/crd" "${local_dir}/config/"
log "Copied CRD schemas and EdgionConfigData fixtures"

# ── Start processes ──────────────────────────────────────────────────────────
log "Starting edgion-center..."
"$CENTER_BIN" -c "$WORK_DIR/center.yaml" > "$WORK_DIR/logs/center.log" 2>&1 &
CENTER_PID=$!
sleep 1
if ! kill -0 "$CENTER_PID" 2>/dev/null; then
  echo -e "${RED}ERROR${NC}: edgion-center exited immediately. Last log lines:" >&2
  tail -20 "$WORK_DIR/logs/center.log" >&2
  exit 1
fi
wait_for_http "$CENTER_PROBE/health" 15

log "Logging in to center..."
# The probe listener comes up before the admin HTTP composition; retry until
# the login route is mounted.
login_elapsed=0
while true; do
  TOKEN=$(do_login)
  [[ -n "$TOKEN" && "$TOKEN" != PARSE_ERROR* && "$TOKEN" != EXPR_ERROR* ]] && break
  if [[ $login_elapsed -ge 15 ]]; then
    echo -e "${RED}ERROR${NC}: Failed to login to center (token: $TOKEN)" >&2
    tail -20 "$WORK_DIR/logs/center.log" >&2
    exit 1
  fi
  sleep 1
  ((login_elapsed++)) || true
done
log "Login successful"

start_controller() {
  local idx="$1" pid_var="CTRL$1_PID"
  "$CTRL_BIN" -c "$WORK_DIR/ctrl${idx}/controller.yaml" >> "$WORK_DIR/logs/ctrl${idx}.log" 2>&1 &
  printf -v "$pid_var" '%s' "$!"
  log "edgion-controller ${idx} PID: ${!pid_var}"
}
start_controller 1
start_controller 2
start_controller 3

sleep 1
for i in 1 2 3; do
  pid_var="CTRL${i}_PID"
  if ! kill -0 "${!pid_var}" 2>/dev/null; then
    echo -e "${RED}ERROR${NC}: edgion-controller $i exited immediately. Last log lines:" >&2
    tail -20 "$WORK_DIR/logs/ctrl${i}.log" >&2
    exit 1
  fi
done

# ── Wait for registration ────────────────────────────────────────────────────
two_online() { [[ "$(online_count)" == "2" ]]; }
if ! poll_until "two controllers online" 40 two_online; then
  echo -e "${RED}ERROR${NC}: controllers never came online" >&2
  echo "--- center.log ---" >&2
  tail -30 "$WORK_DIR/logs/center.log" >&2
  for i in 1 2; do
    echo "--- ctrl${i} stdout ---" >&2
    tail -10 "$WORK_DIR/logs/ctrl${i}.log" >&2
    echo "--- ctrl${i} file logs ---" >&2
    find "$WORK_DIR/ctrl${i}/logs" -type f -name '*.log' -exec tail -25 {} + >&2 2>/dev/null || true
  done
  log "Work dir kept for inspection: $WORK_DIR"
  exit 1
fi
log "Both controllers registered and online"

# ══ A — registration & stats ═════════════════════════════════════════════════

# ─── A1 controllers_online ───────────────────────────────────────────────────
r=$(request GET "$CENTER_HTTP/api/v1/controllers")
ids=$(jsonq "$(req_body "$r")" "sorted(c['controller_id'] for c in d['data'] if c.get('online'))")
if [[ "$ids" == "['east-cluster/ctrl-east', 'west-cluster/ctrl-west']" ]]; then
  pass "A1 controllers_online"
else
  fail "A1 controllers_online" "online set: $ids (kill-switch ctrl-silo must be absent)"
fi

# ─── A2 stats_counts (StatsReport, coalesced to 5 s) ─────────────────────────
counts_arrived() {
  local r; r=$(request GET "$CENTER_HTTP/api/v1/controllers")
  local n; n=$(jsonq "$(req_body "$r")" "sum(1 for c in d['data'] if c.get('online') and (c.get('key_count') or 0) >= 2)")
  [[ "$n" == "2" ]]
}
if poll_until "stats counts" 20 counts_arrived; then
  pass "A2 stats_counts"
else
  r=$(request GET "$CENTER_HTTP/api/v1/controllers")
  fail "A2 stats_counts" "key_count never reached fixtures count on both: $(req_body "$r")"
fi

# ══ B — watch-cache reads (GlobalResources) ══════════════════════════════════

GR="$CENTER_HTTP/api/v1/center/global-resources/resources/edgion-config-data"

# ─── B1 global_list ──────────────────────────────────────────────────────────
gr_has_fixture() {
  local r; r=$(request GET "$GR?limit=100")
  local n; n=$(jsonq "$(req_body "$r")" "sum(1 for g in d.get('groups', []) if g['key']['name'] == 'region-route-override')")
  [[ "$n" =~ ^[0-9]+$ ]] && [[ "$n" -ge 1 ]]
}
if poll_until "global list serves fixtures" 20 gr_has_fixture; then
  pass "B1 global_list"
else
  r=$(request GET "$GR?limit=100")
  fail "B1 global_list" "region-route-override absent from global list: $(req_body "$r" | head -c 600)"
fi

# ─── B2 multi_namespace_identity ─────────────────────────────────────────────
iplist_pairs() {
  local r; r=$(request GET "$GR?limit=100")
  local pairs; pairs=$(jsonq "$(req_body "$r")" "sorted({(g['key']['namespace'], g['key']['name']) for g in d.get('groups', []) if g['key']['name'] == 'app-iplist'})")
  [[ "$pairs" == "[('default', 'app-iplist'), ('edge-apps', 'app-iplist')]" ]]
}
if poll_until "app-iplist rows in both namespaces" 20 iplist_pairs; then
  pass "B2 multi_namespace_identity"
else
  r=$(request GET "$GR?limit=100")
  fail "B2 multi_namespace_identity" "expected the same name in two namespaces as two rows: $(req_body "$r" | head -c 400)"
fi

# ─── B3 global_detail ────────────────────────────────────────────────────────
r=$(request GET "$GR/default/region-route-override?cluster=east-cluster")
endpoint=$(jsonq "$(req_body "$r")" "d['object']['spec']['data']['config']['regions'][1]['backendEndpoint']")
if [[ "$(req_code "$r")" == "200" && "$endpoint" == "127.0.0.1:30002" ]]; then
  pass "B3 global_detail"
else
  fail "B3 global_detail" "code=$(req_code "$r") west endpoint=$endpoint (want 127.0.0.1:30002)"
fi

# ══ C — proxy reads & the EdgionConfigData write surface ═════════════════════

ECD1="$CENTER_HTTP/api/v1/proxy/${CTRL1_TID}/api/v1/namespaced/edgionconfigdata"

# ─── C1 proxy_get ────────────────────────────────────────────────────────────
r=$(request GET "$ECD1/default/region-route-override")
rv=$(jsonq "$(req_body "$r")" "d['metadata']['resourceVersion']")
if [[ "$(req_code "$r")" == "200" && -n "$rv" && "$rv" != *ERROR* ]]; then
  pass "C1 proxy_get"
else
  fail "C1 proxy_get" "code=$(req_code "$r") resourceVersion=$rv"
fi

# ─── C2 proxy_create_watch_add ───────────────────────────────────────────────
create_body='{"apiVersion":"edgion.io/v1","kind":"EdgionConfigData","metadata":{"name":"canary-selector","namespace":"default"},"spec":{"data":{"type":"Selector","config":{"active":"stable"}}}}'
r=$(request POST "$ECD1/default" "$create_body")
code=$(req_code "$r")
selector_in_cache() {
  local rr; rr=$(request GET "$GR/default/canary-selector?cluster=east-cluster")
  [[ "$(req_code "$rr")" == "200" ]]
}
if [[ "$code" == "200" || "$code" == "201" ]] && poll_until "selector add event" 15 selector_in_cache; then
  pass "C2 proxy_create_watch_add"
else
  fail "C2 proxy_create_watch_add" "create code=$code or add event never reached the watch cache"
fi

# ─── C3 selector_active_switch ───────────────────────────────────────────────
r=$(request GET "$ECD1/default/canary-selector")
sel_rv=$(jsonq "$(req_body "$r")" "d['metadata']['resourceVersion']")
update_body="{\"apiVersion\":\"edgion.io/v1\",\"kind\":\"EdgionConfigData\",\"metadata\":{\"name\":\"canary-selector\",\"namespace\":\"default\",\"resourceVersion\":\"${sel_rv}\"},\"spec\":{\"data\":{\"type\":\"Selector\",\"config\":{\"active\":\"canary\"}}}}"
r=$(request PUT "$ECD1/default/canary-selector" "$update_body" "If-Match: \"${sel_rv}\"")
active_switched() {
  local rr; rr=$(request GET "$GR/default/canary-selector?cluster=east-cluster")
  local a; a=$(jsonq "$(req_body "$rr")" "d['object']['spec']['data']['config']['active']")
  [[ "$a" == "canary" ]]
}
if [[ "$(req_code "$r")" == "200" ]] && poll_until "selector active switch observed" 15 active_switched; then
  pass "C3 selector_active_switch"
else
  fail "C3 selector_active_switch" "PUT code=$(req_code "$r") or active never became canary in the cache"
fi

# ─── C4 put_conflict_recover ─────────────────────────────────────────────────
stale_body="{\"apiVersion\":\"edgion.io/v1\",\"kind\":\"EdgionConfigData\",\"metadata\":{\"name\":\"canary-selector\",\"namespace\":\"default\"},\"spec\":{\"data\":{\"type\":\"Selector\",\"config\":{\"active\":\"stale-write\"}}}}"
r=$(request PUT "$ECD1/default/canary-selector" "$stale_body" "If-Match: \"${sel_rv}\"")
conflict_code=$(req_code "$r")
r=$(request GET "$ECD1/default/canary-selector")
fresh_rv=$(jsonq "$(req_body "$r")" "d['metadata']['resourceVersion']")
recover_body="{\"apiVersion\":\"edgion.io/v1\",\"kind\":\"EdgionConfigData\",\"metadata\":{\"name\":\"canary-selector\",\"namespace\":\"default\",\"resourceVersion\":\"${fresh_rv}\"},\"spec\":{\"data\":{\"type\":\"Selector\",\"config\":{\"active\":\"stable\"}}}}"
r=$(request PUT "$ECD1/default/canary-selector" "$recover_body" "If-Match: \"${fresh_rv}\"")
if [[ "$conflict_code" == "409" && "$(req_code "$r")" == "200" ]]; then
  pass "C4 put_conflict_recover"
else
  fail "C4 put_conflict_recover" "stale PUT=$conflict_code (want 409), refreshed PUT=$(req_code "$r") (want 200)"
fi

# ─── C5 delete_stale_precondition ────────────────────────────────────────────
r=$(request DELETE "$ECD1/default/canary-selector" "" "If-Match: \"${sel_rv}\"")
if [[ "$(req_code "$r")" == "409" ]]; then
  pass "C5 delete_stale_precondition"
else
  fail "C5 delete_stale_precondition" "expected 409 for stale If-Match delete, got $(req_code "$r")"
fi

# ─── C6 proxy_delete_watch_del ───────────────────────────────────────────────
r=$(request GET "$ECD1/default/canary-selector")
del_rv=$(jsonq "$(req_body "$r")" "d['metadata']['resourceVersion']")
r=$(request DELETE "$ECD1/default/canary-selector" "" "If-Match: \"${del_rv}\"")
del_code=$(req_code "$r")
selector_gone() {
  local rr; rr=$(request GET "$GR/default/canary-selector?cluster=east-cluster")
  [[ "$(req_code "$rr")" == "404" ]]
}
if [[ "$del_code" == "200" || "$del_code" == "204" ]] && poll_until "selector delete event" 15 selector_gone; then
  pass "C6 proxy_delete_watch_del"
else
  fail "C6 proxy_delete_watch_del" "delete code=$del_code or delete event never reached the watch cache"
fi

# ─── C7 rbac_deny_secret ─────────────────────────────────────────────────────
r=$(request GET "$CENTER_HTTP/api/v1/proxy/${CTRL1_TID}/api/v1/cluster/Secret")
if [[ "$(req_code "$r")" == "403" ]]; then
  pass "C7 rbac_deny_secret"
else
  fail "C7 rbac_deny_secret" "expected 403, got $(req_code "$r")"
fi

# ─── C8 rbac_allow_region_route ──────────────────────────────────────────────
r=$(request GET "$CENTER_HTTP/api/v1/proxy/${CTRL1_TID}/api/v1/region-routes/effective")
if [[ "$(req_code "$r")" == "200" ]]; then
  pass "C8 rbac_allow_region_route"
else
  fail "C8 rbac_allow_region_route" "expected 200 (list RegionRoute is in the default policy), got $(req_code "$r")"
fi

# ─── C9 rbac_deny_other_kind_write ───────────────────────────────────────────
hr_body='{"apiVersion":"gateway.networking.k8s.io/v1","kind":"HTTPRoute","metadata":{"name":"denied","namespace":"default"},"spec":{}}'
r=$(request POST "$CENTER_HTTP/api/v1/proxy/${CTRL1_TID}/api/v1/namespaced/httproute/default" "$hr_body")
if [[ "$(req_code "$r")" == "403" ]]; then
  pass "C9 rbac_deny_other_kind_write"
else
  fail "C9 rbac_deny_other_kind_write" "expected 403 (writes are EdgionConfigData-only), got $(req_code "$r")"
fi

# ══ D — Center write core (failover / sync) & outcomes ═══════════════════════

FAILOVER="$CENTER_HTTP/api/v1/center/region-route-overrides/failover"
SYNC="$CENTER_HTTP/api/v1/center/region-route-overrides/sync"

ctrl_failover_to() { # CTRL_TID -> failoverTo of region east
  local r; r=$(request GET "$CENTER_HTTP/api/v1/proxy/$1/api/v1/namespaced/edgionconfigdata/default/region-route-override")
  jsonq "$(req_body "$r")" "next((x.get('failoverTo', '') for x in d['spec']['data']['config']['regions'] if x['name'] == 'east'), 'MISSING')"
}
ctrl_rv() { # CTRL_TID -> resourceVersion of the override doc
  local r; r=$(request GET "$CENTER_HTTP/api/v1/proxy/$1/api/v1/namespaced/edgionconfigdata/default/region-route-override")
  jsonq "$(req_body "$r")" "d['metadata']['resourceVersion']"
}

# ─── D1 failover_converged ───────────────────────────────────────────────────
r=$(request POST "$FAILOVER" '{"namespace":"default","name":"region-route-override","regionName":"east","failoverTo":"west"}')
code=$(req_code "$r")
summary=$(jsonq "$(req_body "$r")" "(d['data']['modified'], d['data']['failed'], sorted({o['state'] for o in d['data']['outcomes']}))")
both_failed_over() { [[ "$(ctrl_failover_to "$CTRL1_TID")" == "west" && "$(ctrl_failover_to "$CTRL2_TID")" == "west" ]]; }
if [[ "$code" == "200" && "$summary" == "(2, 0, ['converged'])" ]] && poll_until "failover on both controllers" 10 both_failed_over; then
  pass "D1 failover_converged"
else
  fail "D1 failover_converged" "code=$code summary=$summary ctrl1=$(ctrl_failover_to "$CTRL1_TID") ctrl2=$(ctrl_failover_to "$CTRL2_TID")"
fi

# ─── D2 failover_idempotent_skip ─────────────────────────────────────────────
rv1_before=$(ctrl_rv "$CTRL1_TID"); rv2_before=$(ctrl_rv "$CTRL2_TID")
r=$(request POST "$FAILOVER" '{"namespace":"default","name":"region-route-override","regionName":"east","failoverTo":"west"}')
code=$(req_code "$r")
rv1_after=$(ctrl_rv "$CTRL1_TID"); rv2_after=$(ctrl_rv "$CTRL2_TID")
if [[ "$code" == "200" && "$rv1_before" == "$rv1_after" && "$rv2_before" == "$rv2_after" ]]; then
  pass "D2 failover_idempotent_skip"
else
  fail "D2 failover_idempotent_skip" "code=$code rv1 $rv1_before->$rv1_after rv2 $rv2_before->$rv2_after (no-op must not write)"
fi

# ─── D3 failover_type_pinning ────────────────────────────────────────────────
r=$(request POST "$FAILOVER" '{"namespace":"edge-apps","name":"app-iplist","regionName":"east","failoverTo":"west"}')
code=$(req_code "$r")
# The wrong-type document exists in the watch cache, so the refusal happens in
# the write core's per-controller predicate — every outcome is `failed` with the
# type-pinning reason and the aggregate status is 502 (nothing landed), not a
# pre-flight 4xx.
d3_summary=$(jsonq "$(req_body "$r")" "(d['data']['modified'], d['data']['failed'], all('not a RegionRouteOverride' in (o.get('reason') or '') for o in d['data']['outcomes']))")
if [[ "$code" == "502" && "$d3_summary" == "(0, 2, True)" ]]; then
  pass "D3 failover_type_pinning (refused with 502, all outcomes failed on type)"
else
  fail "D3 failover_type_pinning" "want 502 with (modified=0, failed=2, type reason), got $code $d3_summary: $(req_body "$r" | head -c 300)"
fi

# ─── D4 failover_unknown_field ───────────────────────────────────────────────
r=$(request POST "$FAILOVER" '{"namespace":"default","name":"region-route-override","regionName":"east","failoverTo":"west","myRegion":"evil"}')
code=$(req_code "$r")
if [[ "$code" == "400" || "$code" == "422" ]]; then
  pass "D4 failover_unknown_field"
else
  fail "D4 failover_unknown_field" "expected 400/422 for unknown body field, got $code"
fi

# ─── D5 sync_source_to_target ────────────────────────────────────────────────
r=$(request POST "$SYNC" "{\"namespace\":\"default\",\"name\":\"region-route-override\",\"sourceControllerId\":\"${CTRL1_ID}\",\"targetControllerIds\":[\"${CTRL2_ID}\"]}")
code=$(req_code "$r")
west_endpoint_synced() {
  local rr; rr=$(request GET "$CENTER_HTTP/api/v1/proxy/${CTRL2_TID}/api/v1/namespaced/edgionconfigdata/default/region-route-override")
  local ep; ep=$(jsonq "$(req_body "$rr")" "next((x['backendEndpoint'] for x in d['spec']['data']['config']['regions'] if x['name'] == 'west'), 'MISSING')")
  [[ "$ep" == "127.0.0.1:30002" ]]
}
if [[ "$code" == "200" || "$code" == "207" ]] && poll_until "west row replaced by east content" 10 west_endpoint_synced; then
  pass "D5 sync_source_to_target"
else
  fail "D5 sync_source_to_target" "code=$code or ctrl-west's west endpoint never became the source's 30002"
fi

# ─── D6 failover_clear ───────────────────────────────────────────────────────
r=$(request POST "$FAILOVER" '{"namespace":"default","name":"region-route-override","regionName":"east","failoverTo":""}')
code=$(req_code "$r")
both_cleared() { [[ "$(ctrl_failover_to "$CTRL1_TID")" == "" && "$(ctrl_failover_to "$CTRL2_TID")" == "" ]]; }
if [[ "$code" == "200" ]] && poll_until "failover cleared on both controllers" 10 both_cleared; then
  pass "D6 failover_clear"
else
  fail "D6 failover_clear" "code=$code ctrl1='$(ctrl_failover_to "$CTRL1_TID")' ctrl2='$(ctrl_failover_to "$CTRL2_TID")'"
fi

# ══ E — reload & ConfigSyncServer lifecycle ══════════════════════════════════

# ─── E1 reload_converged ─────────────────────────────────────────────────────
# Center holds the request up to 20 s and answers with a terminal outcome; the
# Controller's own 200 ("queued") is never surfaced as success.
r=$(request POST "$CENTER_HTTP/api/v1/controllers/${CTRL1_TID}/reload")
code=$(req_code "$r")
state=$(jsonq "$(req_body "$r")" "d['data']['state']")
server_id=$(jsonq "$(req_body "$r")" "d['data'].get('serverId', '')")
if [[ "$code" == "200" && "$state" == "converged" && -n "$server_id" ]]; then
  pass "E1 reload_converged"
else
  fail "E1 reload_converged" "code=$code state=$state serverId=$server_id (want 200/converged/new id)"
fi

# ─── E2 post_reload_watch ────────────────────────────────────────────────────
# The new server_id cascades into watch re-establishment and a full re-list;
# the cache must keep serving the fixture documents afterwards.
if poll_until "watch re-established after reload" 30 gr_has_fixture && two_online; then
  pass "E2 post_reload_watch"
else
  fail "E2 post_reload_watch" "global list no longer serves fixtures after the reload"
fi

# ══ F — disconnect / reconnect / eviction ════════════════════════════════════

# ─── F1 disconnect_offline ───────────────────────────────────────────────────
kill -9 "$CTRL2_PID" 2>/dev/null || true
west_offline() {
  local r; r=$(request GET "$CENTER_HTTP/api/v1/controllers")
  local o; o=$(jsonq "$(req_body "$r")" "next((c.get('online') for c in d['data'] if c['controller_id'] == '${CTRL2_ID}'), 'MISSING')")
  [[ "$o" == "False" ]]
}
if poll_until "ctrl-west offline after kill" 40 west_offline; then
  pass "F1 disconnect_offline"
else
  fail "F1 disconnect_offline" "ctrl-west never went offline after SIGKILL"
fi

# ─── F2 reconnect_resync ─────────────────────────────────────────────────────
start_controller 2
west_back() {
  local r; r=$(request GET "$CENTER_HTTP/api/v1/controllers")
  local o; o=$(jsonq "$(req_body "$r")" "next((c.get('online') for c in d['data'] if c['controller_id'] == '${CTRL2_ID}'), 'MISSING')")
  [[ "$o" == "True" ]]
}
west_doc_served() {
  local rr; rr=$(request GET "$GR/default/region-route-override?cluster=west-cluster")
  [[ "$(req_code "$rr")" == "200" ]]
}
if poll_until "ctrl-west re-registered" 30 west_back && poll_until "west docs re-listed" 30 west_doc_served; then
  pass "F2 reconnect_resync"
else
  fail "F2 reconnect_resync" "ctrl-west did not re-register or its documents were not re-listed"
fi

# ─── F3 eviction ─────────────────────────────────────────────────────────────
kill -9 "$CTRL2_PID" 2>/dev/null || true
poll_until "ctrl-west offline before eviction" 40 west_offline || true
r=$(request DELETE "$CENTER_HTTP/api/v1/center/admin/controllers/${CTRL2_TID}")
evict_code=$(req_code "$r")
west_gone() {
  local rr; rr=$(request GET "$CENTER_HTTP/api/v1/controllers")
  local n; n=$(jsonq "$(req_body "$rr")" "sum(1 for c in d['data'] if c['controller_id'] == '${CTRL2_ID}')")
  [[ "$n" == "0" ]]
}
if [[ "$evict_code" == "200" || "$evict_code" == "204" ]] && poll_until "ctrl-west evicted" 30 west_gone; then
  start_controller 2
  if poll_until "ctrl-west re-registered after eviction" 30 west_back; then
    pass "F3 eviction"
  else
    fail "F3 eviction" "evicted controller could not re-register"
  fi
else
  fail "F3 eviction" "evict code=$evict_code or ctrl-west still listed after eviction"
  start_controller 2
  poll_until "ctrl-west back for later phases" 30 west_back || true
fi

# ══ H — large proxied response ═══════════════════════════════════════════════

# ─── H1 big_list_over_4mib ───────────────────────────────────────────────────
# Six ~0.8 MiB Misc documents make the namespace list >4 MiB — above tonic's
# default gRPC message limit, which is exactly the regression this guards. Each
# create stays under both 1 MiB request-body caps (Center route + Controller).
bulk_ok=true
for i in 1 2 3 4 5 6; do
  # The ~0.8 MiB body goes through a file: a single argv argument this large
  # would exceed the platform ARG_MAX. curl reads @file for --data-binary.
  python3 -c "
import json, sys
doc = {'apiVersion': 'edgion.io/v1', 'kind': 'EdgionConfigData',
       'metadata': {'name': 'bulk-' + sys.argv[1], 'namespace': 'bulk'},
       'spec': {'data': {'type': 'Misc', 'config': {'blob': 'A' * 800000}}}}
open(sys.argv[2], 'w').write(json.dumps(doc))
" "$i" "$WORK_DIR/bulk_body.json"
  r=$(request POST "$ECD1/bulk" "@$WORK_DIR/bulk_body.json")
  code=$(req_code "$r")
  if [[ "$code" != "200" && "$code" != "201" ]]; then
    bulk_ok=false
    log "bulk-${i} create failed: $code $(req_body "$r" | head -c 200)"
    break
  fi
done
if $bulk_ok; then
  r=$(request GET "$ECD1/bulk")
  code=$(req_code "$r")
  size=$(req_body "$r" | wc -c | tr -d ' ')
  r2=$(request GET "$ECD1/default/region-route-override")
  if [[ "$code" == "200" && "$size" -gt 4194304 && "$(req_code "$r2")" == "200" ]] && two_online; then
    pass "H1 big_list_over_4mib (list size: ${size} bytes)"
  else
    fail "H1 big_list_over_4mib" "list code=$code size=$size followup=$(req_code "$r2") (stream must survive)"
  fi
else
  fail "H1 big_list_over_4mib" "could not create the bulk documents"
fi
# Cleanup the bulk namespace so later assertions see the fixture-only view.
for i in 1 2 3 4 5 6; do
  r=$(request GET "$ECD1/bulk/bulk-${i}")
  rv=$(jsonq "$(req_body "$r")" "d['metadata']['resourceVersion']")
  [[ -n "$rv" && "$rv" != *ERROR* ]] && request DELETE "$ECD1/bulk/bulk-${i}" "" "If-Match: \"${rv}\"" >/dev/null
done

# ══ G — terminal watch denial (explicit deny-all rbac) ═══════════════════════

# ─── G1 watch_denied_terminal ────────────────────────────────────────────────
# Restart ctrl-silo with the federation enabled and an explicit empty rbac:
# an explicit allow list fully replaces the built-in default, so [] = deny-all.
# Registration succeeds (identity is mTLS, not rbac), the reverse watch is
# denied with terminal Forbidden, and reads/writes through the proxy are 403.
kill -9 "$CTRL3_PID" 2>/dev/null || true
sleep 1
CTRL3_DENY_ALL_RBAC='
  rbac:
    allow: []'
write_controller_config 3 "$CTRL3_GRPC_PORT" "$CTRL3_ADMIN_PORT" "$CTRL3_PROBE_PORT" "$CTRL3_METRICS_PORT" "silo-cluster" "ctrl-silo" "true" "$CTRL3_DENY_ALL_RBAC"
start_controller 3
silo_online() {
  local r; r=$(request GET "$CENTER_HTTP/api/v1/controllers")
  local o; o=$(jsonq "$(req_body "$r")" "next((c.get('online') for c in d['data'] if c['controller_id'] == '${CTRL3_ID}'), 'MISSING')")
  [[ "$o" == "True" ]]
}
if ! poll_until "ctrl-silo registered" 30 silo_online; then
  fail "G1 watch_denied_terminal" "ctrl-silo (deny-all rbac) never registered"
else
  denial_seen() { grep -q "federation watch terminated by Controller RBAC" "$WORK_DIR/logs/center.log"; }
  poll_until "terminal watch denial logged" 20 denial_seen || true
  denials_before=$(grep -c "federation watch terminated by Controller RBAC" "$WORK_DIR/logs/center.log" || true)
  sleep 10
  denials_after=$(grep -c "federation watch terminated by Controller RBAC" "$WORK_DIR/logs/center.log" || true)
  r_read=$(request GET "$CENTER_HTTP/api/v1/proxy/${CTRL3_TID}/api/v1/namespaced/edgionconfigdata")
  r_write=$(request POST "$CENTER_HTTP/api/v1/proxy/${CTRL3_TID}/api/v1/namespaced/edgionconfigdata/default" '{"apiVersion":"edgion.io/v1","kind":"EdgionConfigData","metadata":{"name":"denied","namespace":"default"},"spec":{"data":{"type":"Selector","config":{"active":"x"}}}}')
  if [[ "$denials_before" -ge 1 && "$denials_after" == "$denials_before" \
        && "$(req_code "$r_read")" == "403" && "$(req_code "$r_write")" == "403" ]]; then
    pass "G1 watch_denied_terminal (denials: $denials_after, stable)"
  else
    fail "G1 watch_denied_terminal" "denials $denials_before->$denials_after (must be >=1 and stable), read=$(req_code "$r_read") write=$(req_code "$r_write") (want 403/403)"
  fi
fi

# ── Final report ──────────────────────────────────────────────────────────────
echo ""
log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
if [[ $FAIL -eq 0 ]]; then
  echo -e "[$(date '+%H:%M:%S')] ${GREEN}Results: $PASS passed, $FAIL failed — ALL TESTS PASSED${NC}"
else
  echo -e "[$(date '+%H:%M:%S')] ${RED}Results: $PASS passed, $FAIL failed — SOME TESTS FAILED${NC}"
  log "Logs:"
  log "  Center:       $WORK_DIR/logs/center.log"
  log "  Controller 1: $WORK_DIR/logs/ctrl1.log"
  log "  Controller 2: $WORK_DIR/logs/ctrl2.log"
  log "  Controller 3: $WORK_DIR/logs/ctrl3.log"
  log "Work dir kept for inspection: $WORK_DIR"
fi
log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

[[ $FAIL -eq 0 ]] && exit 0 || exit 1
