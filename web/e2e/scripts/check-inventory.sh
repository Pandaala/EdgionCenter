#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
exec npx tsx e2e/scripts/check-inventory.ts "$@"
