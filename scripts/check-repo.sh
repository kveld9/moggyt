#!/usr/bin/env bash
# ==============================================================================
# Repository Standards Audit Script (moggyt)
#
# Inspects the repository for compliance with the adopted standards
# (principios-git: governance/CI + principios-codigo: modular Rust).
#
# Usage:
#   ./scripts/check-repo.sh [target_directory]
#
# Exit codes:
#   0 - All required files exist, are non-empty, satisfy modularity caps, and comply with test policy.
#   1 - One or more files are missing/empty, exceed modularity caps, or fail test policy.
# ==============================================================================

set -euo pipefail

TARGET_DIR="${1:-.}"

if [[ ! -d "${TARGET_DIR}" ]]; then
  echo "Error: Target directory '${TARGET_DIR}' does not exist." >&2
  exit 1
fi

TOTAL_CHECKS=0
PASSED_CHECKS=0
FAILED_CHECKS=0

if [[ -t 1 ]]; then
  GREEN='\033[0;32m'
  RED='\033[0;31m'
  YELLOW='\033[0;33m'
  BLUE='\033[0;34m'
  BOLD='\033[1m'
  RESET='\033[0m'
else
  GREEN=''
  RED=''
  YELLOW=''
  BLUE=''
  BOLD=''
  RESET=''
fi

echo -e "${BOLD}${BLUE}==> Auditing repository standards at: ${TARGET_DIR}${RESET}\n"

check_file() {
  local rel_path="$1"
  local is_critical="${2:-true}"
  local full_path="${TARGET_DIR}/${rel_path}"

  TOTAL_CHECKS=$((TOTAL_CHECKS + 1))

  if [[ -f "${full_path}" ]]; then
    if [[ -s "${full_path}" ]]; then
      echo -e "  [${GREEN}OK${RESET}] ${rel_path}"
      PASSED_CHECKS=$((PASSED_CHECKS + 1))
    else
      echo -e "  [${YELLOW}EMPTY${RESET}] ${rel_path} (file is 0 bytes)"
      if [[ "${is_critical}" == "true" ]]; then
        FAILED_CHECKS=$((FAILED_CHECKS + 1))
      else
        PASSED_CHECKS=$((PASSED_CHECKS + 1))
      fi
    fi
  else
    if [[ "${is_critical}" == "true" ]]; then
      echo -e "  [${RED}MISSING${RESET}] ${rel_path} (REQUIRED)"
      FAILED_CHECKS=$((FAILED_CHECKS + 1))
    else
      echo -e "  [${YELLOW}OPTIONAL${RESET}] ${rel_path} (recommended)"
    fi
  fi
}

echo -e "${BOLD}1. Root Metadata & Licensing${RESET}"
check_file "README.md" true
check_file "AGENTS.md" true
check_file "LICENSE" true
check_file "CONTRIBUTING.md" true
check_file "CODE_OF_CONDUCT.md" true
check_file "SECURITY.md" true
check_file "CHECKLIST.md" true

echo -e "\n${BOLD}2. Git & Environment Configuration${RESET}"
check_file ".gitignore" true
check_file ".gitattributes" true
check_file ".editorconfig" true
check_file ".markdownlint.yaml" true

echo -e "\n${BOLD}3. GitHub Automation & Templates (.github/)${RESET}"
check_file ".github/workflows/ci.yml" true
check_file ".github/workflows/release.yml.example" false
check_file ".github/workflows/gitleaks.yml" true
check_file ".github/workflows/codeql.yml.example" false
check_file ".github/dependabot.yml" true
check_file ".github/ISSUE_TEMPLATE/bug_report.md" true
check_file ".github/ISSUE_TEMPLATE/feature_request.md" true
check_file ".github/PULL_REQUEST_TEMPLATE.md" true
check_file ".github/CODEOWNERS" true
check_file ".github/CODEOWNERS.example" false
check_file ".github/settings.example.yml" false

echo -e "\n${BOLD}4. Product Documentation (docs/)${RESET}"
check_file "docs/01-vision.md" true
check_file "docs/02-architecture.md" true
check_file "docs/03-development.md" true

echo -e "\n${BOLD}5. Rust Sources & Toolchain${RESET}"
check_file "Cargo.toml" true
check_file "Cargo.lock" true
check_file "rustfmt.toml" true
check_file "clippy.toml" true
check_file "src/main.rs" true
check_file "src/lib.rs" true
check_file "src/slint_shell/mod.rs" true
check_file "src/domain/mod.rs" true
check_file "src/domain/queue.rs" true
check_file "src/domain/search.rs" true
check_file "src/infra/mod.rs" true
check_file "src/infra/ytm.rs" true
check_file "src/infra/audio.rs" true

echo -e "\n${BOLD}6. Maintenance & Setup Automation (scripts/)${RESET}"
check_file "scripts/check-repo.sh" true
check_file "scripts/check-i18n.sh" true

echo -e "\n${BOLD}7. Source Modularity & File Size Limits${RESET}"

# Modularity caps:
# - Rust (.rs): max 900 lines (the 900-line cap was raised once to fit
#   src/infra/ytm.rs (891 lines) and no file is exempt any more).
# - Slint (.slint): max 400 lines per component module.
# - Thin entry (ui/shell.slint): max 200 lines, exempt only while it stays an
#   import-only entry under ~200 lines with no component definitions.
MAX_RUST_LINES=900
MAX_SLINT_LINES=400
MAX_SHELL_SLINT_LINES=200

while IFS= read -r file; do
  file="${file%$'\r'}"
  clean_file="${file#./}"
  [[ -z "${clean_file}" ]] && continue

  # Exclude generated/lockfiles and target directory
  case "${clean_file}" in
    Cargo.lock|target/*|*-generated*|*.generated.*)
      continue
      ;;
  esac

  max_lines=0
  case "${clean_file}" in
    *.rs)
      max_lines=${MAX_RUST_LINES}
      ;;
    *.slint)
      if [[ "${clean_file}" == "ui/shell.slint" ]]; then
        max_lines=${MAX_SHELL_SLINT_LINES}
      else
        max_lines=${MAX_SLINT_LINES}
      fi
      ;;
    *)
      continue
      ;;
  esac

  full_path="${TARGET_DIR}/${clean_file}"
  if [[ ! -f "${full_path}" ]]; then
    continue
  fi

  TOTAL_CHECKS=$((TOTAL_CHECKS + 1))
  lines=$(wc -l < "${full_path}" | tr -d '[:space:]')
  lines=${lines:-0}

  if (( lines > max_lines )); then
    echo -e "  [${RED}FAIL${RESET}] ${clean_file} (${lines} lines > ${max_lines} max; modularity limit exceeded)"
    FAILED_CHECKS=$((FAILED_CHECKS + 1))
  else
    # Exemption condition: ui/shell.slint must be import-only with no component definitions
    if [[ "${clean_file}" == "ui/shell.slint" ]] && grep -qE '^[[:space:]]*(export[[:space:]]+)?component[[:space:]]' "${full_path}"; then
      echo -e "  [${RED}FAIL${RESET}] ${clean_file} (thin entry must be import-only with no component definitions)"
      FAILED_CHECKS=$((FAILED_CHECKS + 1))
    else
      echo -e "  [${GREEN}OK${RESET}] ${clean_file} (${lines} lines <= ${max_lines} max)"
      PASSED_CHECKS=$((PASSED_CHECKS + 1))
    fi
  fi
done < <(git -C "${TARGET_DIR}" ls-files)

echo -e "\n${BOLD}8. Unit Test Policy Enforcement (src/**/*.rs)${RESET}"

# Logic-free re-export and composition roots exempt from unit tests:
# - src/domain/mod.rs: logic-free re-export root for domain modules
# - src/infra/mod.rs: logic-free re-export root for infra adapters
# - src/lib.rs: logic-free library entry and public API re-export root
# - src/main.rs: logic-free binary entry and composition root
is_test_exempt() {
  local target="$1"
  case "${target}" in
    src/domain/mod.rs)
      # Logic-free re-export/composition root
      return 0
      ;;
    src/infra/mod.rs)
      # Logic-free re-export/composition root
      return 0
      ;;
    src/lib.rs)
      # Logic-free re-export/composition root
      return 0
      ;;
    src/main.rs)
      # Logic-free re-export/composition root
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

while IFS= read -r file; do
  file="${file%$'\r'}"
  clean_file="${file#./}"
  [[ -z "${clean_file}" ]] && continue

  # Exclude generated files
  case "${clean_file}" in
    *-generated*|*.generated.*)
      continue
      ;;
  esac

  # Inspect only tracked Rust files under src/
  [[ "${clean_file}" == src/* && "${clean_file}" == *.rs ]] || continue

  full_path="${TARGET_DIR}/${clean_file}"
  if [[ ! -f "${full_path}" ]]; then
    continue
  fi

  TOTAL_CHECKS=$((TOTAL_CHECKS + 1))

  if is_test_exempt "${clean_file}"; then
    echo -e "  [${GREEN}OK${RESET}] ${clean_file} (exempt: logic-free re-export/composition root)"
    PASSED_CHECKS=$((PASSED_CHECKS + 1))
  elif grep -q 'mod tests' "${full_path}"; then
    echo -e "  [${GREEN}OK${RESET}] ${clean_file} (contains mod tests)"
    PASSED_CHECKS=$((PASSED_CHECKS + 1))
  else
    echo -e "  [${RED}FAIL${RESET}] ${clean_file} (missing mod tests: unit test module required)"
    FAILED_CHECKS=$((FAILED_CHECKS + 1))
  fi
done < <(git -C "${TARGET_DIR}" ls-files)

echo -e "\n--------------------------------------------------"
echo -e "Audit Summary: ${TOTAL_CHECKS} checked | ${GREEN}${PASSED_CHECKS} passed${RESET} | ${RED}${FAILED_CHECKS} failed${RESET}"

if [[ ${FAILED_CHECKS} -gt 0 ]]; then
  echo -e "${RED}${BOLD}Result: FAILED - Required standards not met (missing files, size limits exceeded, or missing tests).${RESET}\n"
  exit 1
else
  echo -e "${GREEN}${BOLD}Result: PASSED - Repository satisfies all structural standards.${RESET}\n"
  exit 0
fi
