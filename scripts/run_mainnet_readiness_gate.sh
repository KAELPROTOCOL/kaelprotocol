#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

LOG_DIR="${KAEL_MAINNET_READINESS_LOG_DIR:-/tmp/kael-mainnet-readiness-gate}"
SUMMARY_FILE="$LOG_DIR/summary.txt"
mkdir -p "$LOG_DIR"
: >"$SUMMARY_FILE"

STEPS=()
RESULTS=()

banner() {
  cat <<'BANNER'
==========================================
KAEL MAINNET READINESS GATE
==========================================
Scope: local/private audit readiness only.
No mainnet. No real funds. No production claim.
BANNER
}

record() {
  local name="$1"
  local result="$2"
  STEPS+=("$name")
  RESULTS+=("$result")
  printf "%-28s %s\n" "$name" "$result" | tee -a "$SUMMARY_FILE"
}

fail() {
  local message="$1"
  echo "FAIL $message" | tee -a "$SUMMARY_FILE" >&2
  print_summary "FAIL"
  exit 1
}

run_step() {
  local name="$1"
  shift
  local log_file="$LOG_DIR/${name// /_}.log"
  echo
  echo "==> $name"
  if "$@" >"$log_file" 2>&1; then
    record "$name" "PASS"
  else
    record "$name" "FAIL"
    echo "Log: $log_file" >&2
    tail -n 80 "$log_file" >&2 || true
    print_summary "FAIL"
    exit 1
  fi
}

print_summary() {
  local overall="$1"
  echo
  echo "=========================================="
  echo "KAEL MAINNET READINESS GATE"
  echo "=========================================="
  for i in "${!STEPS[@]}"; do
    printf "%-28s %s\n" "${STEPS[$i]}................" "${RESULTS[$i]}"
  done
  echo
  if [[ "$overall" == "PASS" ]]; then
    echo "Professional Audit Ready.. YES"
  else
    echo "Professional Audit Ready.. NO"
  fi
  echo "Production Ready.......... NO"
  echo "External Audit Pending.... YES"
  echo
  echo "Ready for professional audit: $([[ "$overall" == "PASS" ]] && echo YES || echo NO)"
  echo "Ready for production/mainnet: NO"
  echo "External audit pending: YES"
  echo "Logs: $LOG_DIR"
  echo "=========================================="
}

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing dependency: $1" >&2
    return 1
  fi
  "$1" --version >/dev/null 2>&1 || true
}

check_environment_and_git() {
  pwd
  whoami
  git branch --show-current
  git status --short
  git log --oneline -15
  local dirty path
  dirty="$(git status --short)"
  [[ -n "$dirty" ]] || return 0

  while IFS= read -r line; do
    path="${line:3}"
    case "$path" in
      scripts/run_mainnet_readiness_gate.sh|docs/AUDITOR_HANDOFF_INDEX.md|docs/MAINNET_READINESS_GATE.md)
        ;;
      *)
        echo "git worktree has unexpected dirty path: $line" >&2
        return 1
        ;;
    esac
  done <<<"$dirty"
}

check_dependencies() {
  need cargo
  need rustc
  need forge
  need anvil
  need cast
  need shellcheck
  need git
}

check_docs_exist() {
  local docs=(
    docs/AUDIT_PACKAGE.md
    docs/ARCHITECTURE.md
    docs/THREAT_MODEL.md
    docs/SECURITY_INVARIANTS.md
    docs/TRUST_ASSUMPTIONS.md
    docs/PRIVATE_TESTNET_RUNBOOK.md
    docs/MAINNET_RUNBOOK_DRAFT.md
    docs/INCIDENT_RESPONSE.md
    docs/TEST_MATRIX.md
    docs/KNOWN_LIMITATIONS.md
    docs/MAINNET_READINESS_GAP.md
    docs/INTERNAL_AUDIT_REPORT.md
    docs/FINDINGS_REGISTER.md
    docs/RISK_REGISTER.md
    docs/SESSION_WORK_REPORT.md
    docs/AUDITOR_HANDOFF_INDEX.md
    docs/MAINNET_READINESS_GATE.md
    docs/THIRTY_NODE_MARKET_TESTNET_SIMULATION.md
  )
  local doc
  for doc in "${docs[@]}"; do
    [[ -f "$doc" ]] || {
      echo "missing required document: $doc" >&2
      return 1
    }
  done
}

check_doc_sanity() {
  local docs=(
    README.md
    docs/AUDIT_PACKAGE.md
    docs/ARCHITECTURE.md
    docs/THREAT_MODEL.md
    docs/SECURITY_INVARIANTS.md
    docs/TRUST_ASSUMPTIONS.md
    docs/PRIVATE_TESTNET_RUNBOOK.md
    docs/MAINNET_RUNBOOK_DRAFT.md
    docs/INCIDENT_RESPONSE.md
    docs/TEST_MATRIX.md
    docs/KNOWN_LIMITATIONS.md
    docs/MAINNET_READINESS_GAP.md
    docs/INTERNAL_AUDIT_REPORT.md
    docs/FINDINGS_REGISTER.md
    docs/RISK_REGISTER.md
    docs/SESSION_WORK_REPORT.md
    docs/AUDITOR_HANDOFF_INDEX.md
    docs/MAINNET_READINESS_GATE.md
    docs/THIRTY_NODE_MARKET_TESTNET_SIMULATION.md
  )
  local file line lower
  for file in "${docs[@]}"; do
    while IFS= read -r line; do
      lower="$(printf "%s" "$line" | tr '[:upper:]' '[:lower:]')"
      case "$lower" in
        *"production ready"*|*"mainnet ready"*|*"ready for real funds"*|*"safe for real funds"*)
          case "$lower" in
            *"not ready"*|*"production ready: no"*|*"ready for production/mainnet: no"*|*"external audit pending"*|*"external audit"*|*"professional audit ready"*|*"local/private scope"*|*"not production"*|*"not mainnet"*)
              ;;
            *)
              echo "$file: dangerous unqualified readiness phrase: $line" >&2
              return 1
              ;;
          esac
          ;;
      esac
    done <"$file"
  done
}

check_security_strings() {
  if rg -n 'dbg!\(' contracts/src swapkit/src orderbook/src maestro/src scripts; then
    echo "dbg! macro found in critical runtime areas" >&2
    return 1
  fi

  if rg -n 'println!\s*\([^)]*(private key|secret)|eprintln!\s*\([^)]*(private key|secret)' swapkit/src orderbook/src maestro/src scripts; then
    echo "possible sensitive runtime print found" >&2
    return 1
  fi

  local panic_hits
  panic_hits="$(
    while IFS= read -r -d '' file; do
      awk '
        /#\[cfg\(test\)\]/ { in_test = 1 }
        /panic![[:space:]]*\(/ && !in_test { print FILENAME ":" FNR ":" $0 }
      ' "$file"
    done < <(find contracts/src swapkit/src/exec swapkit/src/bin scripts -type f -print0)
  )"
  if [[ -n "$panic_hits" ]]; then
    printf "%s\n" "$panic_hits"
    echo "panic! found in critical runtime or broadcast paths" >&2
    return 1
  fi

  if rg -n '[T]ODO|[F]IXME' contracts/src swapkit/src/exec swapkit/src/bin scripts; then
    if ! rg -n '[T]ODO|[F]IXME' docs/MAINNET_READINESS_GAP.md >/dev/null 2>&1; then
      echo "unresolved task marker found in critical files without documented gap entry" >&2
      return 1
    fi
  fi

  if rg -n 'rm\s+-rf\s+(/|\$HOME|~|\*)' scripts; then
    echo "dangerous broad rm -rf pattern found in scripts" >&2
    return 1
  fi

  if rg -n '[k]illall' scripts; then
    echo "dangerous broad process-kill pattern found in scripts" >&2
    return 1
  fi
  if rg -n '[p]kill' scripts | grep -v 'anvil' >/dev/null; then
    echo "dangerous broad process-kill pattern found in scripts" >&2
    return 1
  fi

  if rg -n --glob '!run_mainnet_readiness_gate.sh' '[f]orce(-| )?push|git push .*--[f]orce|git push .*-f' scripts; then
    echo "prohibited forced git publish reference found in operational scripts" >&2
    return 1
  fi

  if find . -maxdepth 2 -type f \( -name '.env*mainnet*' -o -name '.env*private*' \) ! -name '*.example' | grep -q .; then
    echo "non-example mainnet/private env file found" >&2
    return 1
  fi

  local allowlist
  allowlist="$(awk '/ALLOWED_TEST_CHAINS/{flag=1} flag{print} flag && /\];/{exit}' swapkit/src/exec/signer.rs)"
  if printf "%s\n" "$allowlist" | rg -n '^[[:space:]]*(1|10|56|137|8453|42161|43114),'; then
    echo "mainnet chain id present in signer allowlist" >&2
    return 1
  fi
}

check_audit_handoff() {
  grep -F './scripts/run_private_testnet_full.sh' docs/AUDITOR_HANDOFF_INDEX.md
  grep -F 'mainnet' docs/AUDITOR_HANDOFF_INDEX.md
  grep -F 'real funds' docs/AUDITOR_HANDOFF_INDEX.md
  [[ -x ./scripts/run_private_testnet_full.sh ]]
}

banner

run_step "Git" check_environment_and_git
run_step "Dependencies" check_dependencies
run_step "Formatting" cargo fmt --all -- --check
run_step "Lint" cargo clippy --workspace --all-targets -- -D warnings
run_step "Rust tests" cargo test --workspace
run_step "Foundry tests" bash -c 'cd contracts && forge test'
run_step "Shellcheck" bash -c 'shellcheck scripts/*.sh'
run_step "Development flow" ./scripts/run_dev_swap_test.sh
run_step "Closed private testnet" ./scripts/run_closed_testnet_local.sh
run_step "Full private testnet" ./scripts/run_private_testnet_full.sh
run_step "30-node market sim quick" bash -c 'KAEL_SIM_NODES=5 KAEL_SIM_WALLETS_PER_NODE=1 KAEL_SIM_DAYS=1 KAEL_SIM_ORDERS_PER_DAY=10 KAEL_SIM_CONCURRENCY=2 ./scripts/run_30node_market_testnet_simulation.sh'
run_step "Docs" check_docs_exist
run_step "Documentation sanity" check_doc_sanity
run_step "Security scan" check_security_strings
run_step "Audit handoff" check_audit_handoff
run_step "Git final" check_environment_and_git

print_summary "PASS"
