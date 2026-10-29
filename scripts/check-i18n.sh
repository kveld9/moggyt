#!/usr/bin/env bash
# i18n & Static Chrome Audit Script (moggyt)
# Enforces zero non-ASCII in src (outside i18n.rs/tests), zero language branches,
# zero hardcoded API locales, and zero unallowed static text in Slint.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

ERRORS=0
echo "==> Auditing i18n and static chrome invariants..."

# 1. Non-ASCII outside src/i18n.rs (excluding test modules)
while IFS= read -r file; do
  HITS=$(sed -e '/#\[cfg(test)\]/,$d' -e '/mod tests {/,$d' "${file}" | grep -P -n '[^\x00-\x7F]' || true)
  if [[ -n "${HITS}" ]]; then
    sed "s|^|${file}:|" <<< "${HITS}"
    ERRORS=$((ERRORS + 1))
  fi
done < <(find src -type f -name "*.rs" ! -name "i18n.rs" | sort)

# 2. Language-branch equality checks in ui/shell.slint
BRANCH_HITS=$(grep -H -En '==\s*"[A-Z][a-zA-Z]+"' ui/shell.slint || true)
if [[ -n "${BRANCH_HITS}" ]]; then
  echo "${BRANCH_HITS}"
  ERRORS=$((ERRORS + 1))
fi

# 3. Hardcoded API locale literals in src/
LOCALE_HITS=$(grep -rnE '"hl":\s*"en"|"gl":\s*"US"' src/ || true)
if [[ -n "${LOCALE_HITS}" ]]; then
  echo "${LOCALE_HITS}"
  ERRORS=$((ERRORS + 1))
fi

# 4. Disallowed static text/placeholder literals in ui/shell.slint
DISALLOWED=$(grep -H -nE '^[[:space:]]*(text|placeholder-text): "' ui/shell.slint \
  | grep -vE 'text: "(Moggyt|0:00|-10s|\+10s|1\.00x|)"' \
  | grep -vE 'text: "\\\{' || true)
if [[ -n "${DISALLOWED}" ]]; then
  echo "${DISALLOWED}"
  ERRORS=$((ERRORS + 1))
fi

if [[ ${ERRORS} -gt 0 ]]; then
  echo "Result: FAILED - ${ERRORS} i18n invariant violation(s) found." >&2
  exit 1
fi
echo "Result: PASSED - All i18n invariants satisfied."
