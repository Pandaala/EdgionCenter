#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${E2E_ALLOW_MUTATION:?set E2E_ALLOW_MUTATION=1 through run.sh}"
if [[ "${E2E_MODE:-standalone}" == "standalone" ]]; then exec npx tsx e2e/scripts/seed-standalone.ts --reset "$@"; fi
exec npx tsx e2e/scripts/reset.ts "$@"
