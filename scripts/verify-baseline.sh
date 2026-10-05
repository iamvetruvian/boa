#!/usr/bin/env bash
# Verifies that the working tree matches the P0 baseline pins in
# `docs/baseline.md`. Fails (nonzero exit) on any drift.
#
# Usage:
#   scripts/verify-baseline.sh            # full check
#   scripts/verify-baseline.sh --test262-only
#
# Environment:
#   RUSTUP_TOOLCHAIN  When set to a nightly pin, the active-toolchain check
#                     expects the nightly pin instead of the stable pin (this is
#                     how the Miri job overrides `rust-toolchain.toml`).
set -euo pipefail

ROOT="${BASELINE_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$ROOT"

STABLE_PIN="1.94.0"
NIGHTLY_PIN="nightly-2026-10-01"
TEST262_PIN="d86b2294eb0a17eaa281ff12c73c473ec864c72f"
WPT_PIN="82a84e1842d583f1f197d64950453222b9f52c67"

failures=0
fail() { echo "FAIL: $1" >&2; failures=$((failures + 1)); }
pass() { echo "ok: $1"; }

check_toolchain_file() {
  if [ ! -f rust-toolchain.toml ]; then
    fail "rust-toolchain.toml is missing"
    return
  fi
  local channel
  channel="$(grep -E '^channel' rust-toolchain.toml | sed -E 's/.*"(.*)".*/\1/')"
  if [ "$channel" != "$STABLE_PIN" ]; then
    fail "rust-toolchain.toml channel is '$channel', expected '$STABLE_PIN'"
  else
    pass "rust-toolchain.toml pins stable $STABLE_PIN"
  fi
}

check_active_toolchain() {
  if ! command -v rustc >/dev/null 2>&1; then
    fail "rustc not found on PATH"
    return
  fi
  local version
  version="$(rustc --version | awk '{print $2}')"
  local expected="$STABLE_PIN"
  case "${RUSTUP_TOOLCHAIN:-}" in
    nightly*) expected="$NIGHTLY_PIN" ;;
  esac
  if [ "$expected" = "$NIGHTLY_PIN" ]; then
    # Nightly reports e.g. "1.101.0-nightly"; accept any nightly and let the
    # workflow pin the exact date (checked below from CI config).
    case "$version" in
      *nightly*) pass "active toolchain is nightly ($version)" ;;
      *) fail "RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN but rustc reports '$version'" ;;
    esac
  elif [ "$version" != "$expected" ]; then
    fail "active rustc is '$version', expected pinned stable '$expected'"
  else
    pass "active rustc matches pinned stable $expected"
  fi
}

check_test262_pin() {
  local config_commit
  config_commit="$(grep -E '^commit' test262_config.toml | sed -E 's/.*"(.*)".*/\1/')"
  if [ "$config_commit" != "$TEST262_PIN" ]; then
    fail "test262_config.toml pins '$config_commit', expected '$TEST262_PIN'"
  else
    pass "test262_config.toml pins $TEST262_PIN"
  fi
  if [ -d test262/.git ] || [ -f test262/.git ]; then
    local head
    head="$(git -C test262 rev-parse HEAD)"
    if [ "$head" != "$TEST262_PIN" ]; then
      fail "test262 checkout HEAD is '$head', expected '$TEST262_PIN'"
    else
      pass "test262 checkout HEAD matches pin"
    fi
  else
    echo "skip: no ./test262 checkout to verify (clone happens on first tester run)"
  fi
}

check_wpt_pin() {
  local rev
  rev="$(grep -E '^rev' test_wpt_config.toml | sed -E 's/.*"(.*)".*/\1/')"
  if [ "$rev" != "$WPT_PIN" ]; then
    fail "test_wpt_config.toml pins '$rev', expected '$WPT_PIN'"
  else
    pass "test_wpt_config.toml pins $WPT_PIN"
  fi
}

check_oracle_pin() {
  # shellcheck source=fetch-oracle.sh
  . "$ROOT/scripts/fetch-oracle.sh"
  if ! grep -q "$ORACLE_VERSION" docs/baseline.md || ! grep -q "$ORACLE_BINARY_SHA256" docs/baseline.md; then
    fail "docs/baseline.md oracle block does not match scripts/fetch-oracle.sh pin ($ORACLE_VERSION)"
    return
  fi
  pass "docs/baseline.md matches the oracle pin ($ORACLE_VERSION)"
  local oracle_bin="${ORACLE_DIR:-$ROOT/target/oracle}/js"
  if [ ! -x "$oracle_bin" ]; then
    echo "skip: oracle not fetched ($oracle_bin missing; run scripts/fetch-oracle.sh) — differential jobs stay disabled"
    return
  fi
  local have_version have_hash
  have_version="$("$oracle_bin" --version)"
  have_hash="$(sha256sum "$oracle_bin" | awk '{print $1}')"
  if [ "$have_version" != "$ORACLE_VERSION" ]; then
    fail "oracle binary reports '$have_version', expected '$ORACLE_VERSION'"
  else
    pass "oracle binary reports pinned version $ORACLE_VERSION"
  fi
  if [ "$have_hash" != "$ORACLE_BINARY_SHA256" ]; then
    fail "oracle binary hash is '$have_hash', expected '$ORACLE_BINARY_SHA256'"
  else
    pass "oracle binary hash matches pin"
  fi
}

check_lockfile() {
  if [ ! -f Cargo.lock ]; then
    fail "Cargo.lock is missing"
    return
  fi
  local version
  version="$(awk '/^version = /{print $3; exit}' Cargo.lock)"
  if [ "$version" != "4" ]; then
    fail "Cargo.lock version is '$version', expected '4'"
  else
    pass "Cargo.lock is version 4"
  fi
  # The lockfile must be self-consistent: routine runs resolve with --locked.
  if cargo metadata --locked --format-version 1 >/dev/null 2>&1; then
    pass "Cargo.lock is consistent (cargo metadata --locked)"
  else
    fail "Cargo.lock is inconsistent (cargo metadata --locked failed)"
  fi
  if [ -d .git ] && ! git diff --quiet -- Cargo.lock; then
    fail "Cargo.lock has uncommitted changes (routine runs must not dirty it)"
  else
    pass "Cargo.lock has no uncommitted changes"
  fi
}

check_ci_pins() {
  local stable_hits nightly_hits
  stable_hits="$(grep -h "toolchain: $STABLE_PIN" .github/workflows/rust.yml .github/workflows/test262.yml .github/workflows/test262_full.yml .github/workflows/nightly_build.yml | wc -l)"
  if [ "$stable_hits" -lt 2 ]; then
    fail "CI workflows do not reference pinned stable $STABLE_PIN (hits: $stable_hits)"
  else
    pass "CI workflows reference pinned stable ($stable_hits hits)"
  fi
  nightly_hits="$(grep -h "toolchain: $NIGHTLY_PIN" .github/workflows/rust.yml | wc -l)"
  if [ "$nightly_hits" -lt 1 ]; then
    fail "CI Miri job does not reference pinned $NIGHTLY_PIN"
  else
    pass "CI Miri job references pinned $NIGHTLY_PIN"
  fi
  if grep -q "RUSTUP_TOOLCHAIN" .github/workflows/rust.yml; then
    pass "CI overrides rust-toolchain.toml via RUSTUP_TOOLCHAIN where needed"
  else
    fail "CI has no RUSTUP_TOOLCHAIN override (Miri/MSRV jobs would run on stable)"
  fi
}

case "${1:-}" in
  --test262-only)
    check_test262_pin
    ;;
  *)
    check_toolchain_file
    check_active_toolchain
    check_test262_pin
    check_wpt_pin
    check_oracle_pin
    check_lockfile
    check_ci_pins
    ;;
esac

if [ "$failures" -ne 0 ]; then
  echo "$failures baseline check(s) failed" >&2
  exit 1
fi
echo "baseline verification passed"
