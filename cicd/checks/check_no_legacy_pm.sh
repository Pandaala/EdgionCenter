#!/usr/bin/env bash
# Regression gate: forbid legacy PluginMetaData / pluginmetadata / plugin-metadata
# references in src/ and web/src/.
#
# Rationale: the resource was renamed PluginMetaData -> EdgionConfigData.
# Any re-introduction of the old names in Rust src/ or web TypeScript is a regression.
#
# rg exits 0 when matches are found (= FAIL path), 1 when no matches (= PASS path).
# Using rg as the condition of an `if` is safe under set -e: bash exempts
# conditional expressions from the errexit trap.
#
# Exit 0 on clean; exit 1 with hit list otherwise.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}/../.."

PATTERN='PluginMetaData|pluginmetadata|plugin-metadata'
if rg -n --glob 'src/**' --glob 'web/src/**' -e "$PATTERN" ; then
  echo "ERROR: legacy PluginMetaData references found (use EdgionConfigData)." >&2
  exit 1
fi
echo "OK: no legacy PluginMetaData references."
