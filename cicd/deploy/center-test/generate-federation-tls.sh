#!/usr/bin/env bash
set -euo pipefail

NAMESPACE="${NAMESPACE:-edgion-test}"
TRUST_DOMAIN="${TRUST_DOMAIN:-edgion.io}"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

openssl req -x509 -newkey rsa:2048 -nodes -days 7 \
  -keyout "${WORK_DIR}/ca.key" -out "${WORK_DIR}/ca.crt" \
  -subj "/CN=Edgion Center integration CA" >/dev/null 2>&1

issue_certificate() {
  local name="$1" usage="$2" san="$3"
  openssl req -newkey rsa:2048 -nodes \
    -keyout "${WORK_DIR}/${name}.key" -out "${WORK_DIR}/${name}.csr" \
    -subj "/CN=${name}" >/dev/null 2>&1
  {
    echo "subjectAltName=${san}"
    echo "extendedKeyUsage=${usage}"
  } >"${WORK_DIR}/${name}.ext"
  openssl x509 -req -days 7 \
    -in "${WORK_DIR}/${name}.csr" \
    -CA "${WORK_DIR}/ca.crt" -CAkey "${WORK_DIR}/ca.key" -CAcreateserial \
    -out "${WORK_DIR}/${name}.crt" -extfile "${WORK_DIR}/${name}.ext" \
    >/dev/null 2>&1
}

issue_certificate server serverAuth \
  "DNS:center,DNS:center.${NAMESPACE}.svc,DNS:center.${NAMESPACE}.svc.cluster.local"

controllers=(
  "controller-1:cluster-east:ctrl-01"
  "controller-2:cluster-west:ctrl-02"
  "controller-3:cluster-north:ctrl-03"
  "controller-4:cluster-south:ctrl-04"
  "controller-5:cluster-dr-east:ctrl-05"
  "controller-6:cluster-dr-west:ctrl-06"
)
for item in "${controllers[@]}"; do
  IFS=: read -r file cluster name <<<"${item}"
  issue_certificate "${file}" clientAuth \
    "URI:spiffe://${TRUST_DOMAIN}/controllers/${cluster}/${name}"
done

kubectl get namespace "${NAMESPACE}" >/dev/null 2>&1 || kubectl create namespace "${NAMESPACE}"
args=(kubectl -n "${NAMESPACE}" create secret generic center-test-federation-tls)
args+=(--from-file="ca.crt=${WORK_DIR}/ca.crt")
args+=(--from-file="server.crt=${WORK_DIR}/server.crt")
args+=(--from-file="server.key=${WORK_DIR}/server.key")
for item in "${controllers[@]}"; do
  IFS=: read -r file _ <<<"${item}"
  args+=(--from-file="${file}.crt=${WORK_DIR}/${file}.crt")
  args+=(--from-file="${file}.key=${WORK_DIR}/${file}.key")
done
"${args[@]}" --dry-run=client -o yaml | kubectl apply -f -

echo "Refreshed test-only federation certificates in ${NAMESPACE}/center-test-federation-tls"
