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
#   0 - All required files exist and are non-empty.
#   1 - One or more required files are missing or empty.
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
check_file "docs/02-arquitectura.md" true
check_file "docs/03-desarrollo.md" true

echo -e "\n${BOLD}5. Rust Sources & Toolchain${RESET}"
check_file "Cargo.toml" true
check_file "Cargo.lock" true
check_file "rustfmt.toml" true
check_file "clippy.toml" true
check_file "src/main.rs" true
check_file "src/lib.rs" true
check_file "src/app.rs" true
check_file "src/domain/mod.rs" true
check_file "src/domain/queue.rs" true
check_file "src/domain/search.rs" true
check_file "src/infra/mod.rs" true
check_file "src/infra/ytm.rs" true
check_file "src/infra/audio.rs" true

echo -e "\n${BOLD}6. Maintenance & Setup Automation (scripts/)${RESET}"
check_file "scripts/check-repo.sh" true

echo -e "\n--------------------------------------------------"
echo -e "Audit Summary: ${TOTAL_CHECKS} checked | ${GREEN}${PASSED_CHECKS} passed${RESET} | ${RED}${FAILED_CHECKS} failed${RESET}"

if [[ ${FAILED_CHECKS} -gt 0 ]]; then
  echo -e "${RED}${BOLD}Result: FAILED - Required standard files are missing or empty.${RESET}\n"
  exit 1
else
  echo -e "${GREEN}${BOLD}Result: PASSED - Repository satisfies all structural standards.${RESET}\n"
  exit 0
fi
