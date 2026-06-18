#!/usr/bin/env bash
# Fail if any non-English letter characters appear in the repository, except
# under web/.
#
# - web/ is the React dashboard, which ships bilingual (zh-CN / en) i18n
#   strings, so Chinese there is expected and allowed.
# - Everything else (src/, proto/, config/, examples/, skills/, cicd/, root
#   files, etc.) must be English only: identifiers, comments, log/error string
#   literals, and any other text that lands on disk in a source file.
#
# Blocked Unicode ranges (letter-bearing scripts of non-English languages):
#   U+0400-U+04FF  Cyrillic
#   U+0590-U+05FF  Hebrew
#   U+0600-U+06FF  Arabic
#   U+0900-U+097F  Devanagari
#   U+0E00-U+0E7F  Thai
#   U+1100-U+11FF  Hangul Jamo
#   U+3040-U+309F  Hiragana
#   U+30A0-U+30FF  Katakana
#   U+3400-U+4DBF  CJK Extension A
#   U+4E00-U+9FFF  CJK Unified Ideographs
#   U+AC00-U+D7AF  Hangul Syllables
#   U+F900-U+FAFF  CJK Compatibility Ideographs
#
# Punctuation / symbols (em-dash, smart quotes, arrows, bullets, accented
# Latin like é/ñ/ü, Greek letters like π) are NOT blocked — only letters from
# non-English writing systems are.
#
# rg respects .gitignore, so build artifacts (target/, web/node_modules/,
# web/dist/) are skipped automatically.
#
# rg exits 0 when matches are found (= FAIL path), 1 when none (= PASS path).
# Using rg as the condition of an `if` is safe under set -e: bash exempts
# conditional expressions from the errexit trap.
#
# Exit 0 on a clean tree; exit 1 with the offending matches on stderr otherwise.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}/../.."

PATTERN='[\x{0400}-\x{04FF}\x{0590}-\x{05FF}\x{0600}-\x{06FF}\x{0900}-\x{097F}\x{0E00}-\x{0E7F}\x{1100}-\x{11FF}\x{3040}-\x{309F}\x{30A0}-\x{30FF}\x{3400}-\x{4DBF}\x{4E00}-\x{9FFF}\x{AC00}-\x{D7AF}\x{F900}-\x{FAFF}]'

if matches=$(rg -nP "${PATTERN}" --glob '!web/**' . 2>/dev/null); then
    echo "ERROR: non-English letter characters found outside web/" >&2
    echo "       (only the web/ dashboard may carry bilingual zh-CN strings)" >&2
    echo "${matches}" >&2
    exit 1
fi

echo "OK: no non-English letter scripts found outside web/"
