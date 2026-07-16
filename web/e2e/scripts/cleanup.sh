#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${E2E_ALLOW_MUTATION:?set E2E_ALLOW_MUTATION=1 explicitly}"
if [[ "${E2E_MODE:-standalone}" == "standalone" ]]; then exec npx tsx e2e/scripts/cleanup-standalone.ts "$@"; fi
exec npx tsx e2e/scripts/cleanup.ts "$@"
