#!/usr/bin/env bash
set -euo pipefail
: "${E2E_ARTIFACT_DIR:?}" "${E2E_RUN_ID:?}" "${E2E_CONTROLLER_A:?}" "${E2E_CONTROLLER_B:?}"
tls_dir="$E2E_ARTIFACT_DIR/tls"
mkdir -p "$tls_dir"
chmod 700 "$tls_dir"
prefix="eruie2e-$(printf %s "$E2E_RUN_ID" | shasum -a 256 | cut -c1-8)"
make_ca() {
  local name="$1" cn="$2"
  openssl genrsa -out "$tls_dir/$name.key" 2048 >/dev/null 2>&1
  openssl req -x509 -new -key "$tls_dir/$name.key" -sha256 -days 2 -subj "/CN=$cn" -out "$tls_dir/$name.crt"
}
make_ca ca 'Edgion E2E Federation CA'
make_ca internal-ca 'Edgion E2E Internal CA'
make_ca oidc-ca 'Edgion E2E OIDC CA'
issue() {
  local ca_name="$1" name="$2" uri="$3" san="$4"
  openssl genrsa -out "$tls_dir/$name.key" 2048 >/dev/null 2>&1
  openssl req -new -key "$tls_dir/$name.key" -subj "/CN=$name" -out "$tls_dir/$name.csr"
  openssl x509 -req -in "$tls_dir/$name.csr" -CA "$tls_dir/$ca_name.crt" -CAkey "$tls_dir/$ca_name.key" -CAcreateserial -days 2 -sha256 -extfile <(printf 'subjectAltName=URI:%s,%s\nextendedKeyUsage=serverAuth,clientAuth\n' "$uri" "$san") -out "$tls_dir/$name.crt" >/dev/null 2>&1
  rm -f "$tls_dir/$name.csr"
}
issue ca server 'spiffe://edgion.io/center' "IP:127.0.0.1,DNS:center,DNS:$prefix-center,DNS:$prefix-center.$prefix-system.svc,DNS:$prefix-center.$prefix-system.svc.cluster.local"
issue ca controller-a "spiffe://edgion.io/controllers/e2e-a/$E2E_CONTROLLER_A" 'DNS:controller-a'
issue ca controller-b "spiffe://edgion.io/controllers/e2e-b/$E2E_CONTROLLER_B" 'DNS:controller-b'
issue internal-ca internal "spiffe://edgion.io/ns/$prefix-system/sa/$prefix-center-service-account" "DNS:$prefix-center-internal.$prefix-system.svc,DNS:$prefix-center-internal.$prefix-system.svc.cluster.local"
issue oidc-ca dex 'spiffe://edgion.io/e2e/dex' "DNS:$prefix-dex,DNS:$prefix-dex.$prefix-system.svc,DNS:$prefix-dex.$prefix-system.svc.cluster.local"
chmod 600 "$tls_dir"/*.key
