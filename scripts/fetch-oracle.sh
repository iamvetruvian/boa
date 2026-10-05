#!/usr/bin/env bash
# Fetches the pinned P3 differential oracle (SpiderMonkey jsshell) and
# verifies it against the pin in `docs/baseline.md`.
#
# Usage:
#   scripts/fetch-oracle.sh            # fetch into ./target/oracle (idempotent)
#   ORACLE_DIR=/path/to/dir scripts/fetch-oracle.sh
#
# The pin below is the single source of fetch truth; `verify-baseline.sh`
# sources this file so the enforcement check cannot drift from it, and
# `docs/baseline.md` records the same values for review.
#
# Layout after fetch: $ORACLE_DIR/js (+ SpiderMonkey support libs).
# Pass the binary to the differential runner via `--oracle $ORACLE_DIR/js`
# or `BOA_DIFF_ORACLE=$ORACLE_DIR/js`.
set -euo pipefail

# P3.1 v1 oracle pin (Firefox 128.14.0esr release jsshell, linux-x86_64).
ORACLE_NAME="jsshell"
ORACLE_VERSION="JavaScript-C128.14.0"
ORACLE_URL="https://archive.mozilla.org/pub/firefox/releases/128.14.0esr/jsshell/jsshell-linux-x86_64.zip"
ORACLE_ZIP_SHA256="dfe276d0966fd5597c11a1a283245e18a10a90f4becd93e61fe19f13fe0d269e"
ORACLE_BINARY_SHA256="9cdfbda6940d10e213b7c045e78005ec4aecbb0390ba400bb257de649dc6da4b"
ORACLE_SOURCE_REV="df0b4a4887880a1b267ca2b4902afee84e80f6e9"
ORACLE_SOURCE_TAG="FIREFOX_128_14_0esr_RELEASE"

fetch_oracle() {
  local root oracle_dir zip_path
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  oracle_dir="${ORACLE_DIR:-$root/target/oracle}"
  mkdir -p "$oracle_dir"

  if [ -x "$oracle_dir/js" ]; then
    local have
    have="$(sha256sum "$oracle_dir/js" | awk '{print $1}')"
    if [ "$have" = "$ORACLE_BINARY_SHA256" ]; then
      echo "oracle already fetched: $oracle_dir/js ($ORACLE_VERSION)"
      return 0
    fi
    echo "stale oracle binary (hash $have), re-fetching" >&2
  fi

  zip_path="$oracle_dir/jsshell.zip"
  echo "downloading $ORACLE_URL ..."
  curl -sSL --retry 3 -o "$zip_path" "$ORACLE_URL"

  local got
  got="$(sha256sum "$zip_path" | awk '{print $1}')"
  if [ "$got" != "$ORACLE_ZIP_SHA256" ]; then
    echo "FAIL: oracle zip hash is '$got', expected '$ORACLE_ZIP_SHA256'" >&2
    rm -f "$zip_path"
    return 1
  fi

  unzip -o -q "$zip_path" -d "$oracle_dir"
  rm -f "$zip_path"

  got="$(sha256sum "$oracle_dir/js" | awk '{print $1}')"
  if [ "$got" != "$ORACLE_BINARY_SHA256" ]; then
    echo "FAIL: extracted oracle binary hash is '$got', expected '$ORACLE_BINARY_SHA256'" >&2
    return 1
  fi

  local version
  version="$("$oracle_dir/js" --version)"
  if [ "$version" != "$ORACLE_VERSION" ]; then
    echo "FAIL: oracle reports version '$version', expected '$ORACLE_VERSION'" >&2
    return 1
  fi

  echo "oracle ready: $oracle_dir/js ($version)"
}

# Allow `verify-baseline.sh` to source the pin vars without fetching.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  fetch_oracle
fi
