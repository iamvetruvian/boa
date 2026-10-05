#!/usr/bin/env bash
# Seeded-crash drill runner (P4 validation gate).
#
# Usage: ./run.sh [--inventory inventory.toml]
#
# For every entry in the inventory: reproduce the crash, minimize the input
# when the harness supports it (fuzz-target entries via `cargo fuzz tmin`),
# record the observed signature, and verify all signatures are distinct
# (dedupe check). Exit 0 when every entry crashed with a distinct signature.
#
# An entry that EXITS CLEANLY is reported RETIRED (the bug may be fixed):
# the drill fails until a human graduates it to the regression DB (or
# re-files it) and updates the inventory. This fail-closed rule keeps the
# drill honest: silent fixes must surface, not rot.
set -euo pipefail

cd "$(dirname "$0")"
REPO_ROOT="$(cd ../../.. && pwd)"
INVENTORY="${1:-inventory.toml}"

export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-nightly-2026-10-01}"

python3 - "$INVENTORY" "$REPO_ROOT" <<'EOF'
import os, re, subprocess, sys, tempfile

try:
    import tomllib
except ImportError:
    import tomli as tomllib

inventory_path, repo_root = sys.argv[1], sys.argv[2]

with open(inventory_path, "rb") as fh:
    entries = tomllib.load(fh).get("entry", [])

print(f"drill: {len(entries)} entries (gate target: 5)")
results = []
signatures = {}

def normalize(sig):
    # Pids (`thread 'main' (1234) ...`) differ every run and would make any
    # two signatures trivially "distinct". Strip them so dedupe compares
    # message + location only.
    return re.sub(r" \(\d+\)", "", sig)

for entry in entries:
    entry_id = entry["id"]
    harness = entry["harness"]
    input_path = os.path.join(repo_root, entry["input"])
    timeout = int(entry.get("timeout", 120))
    want = entry.get("signature_match", "")

    if not os.path.exists(input_path):
        print(f"[{entry_id}] MISSING INPUT: {input_path}")
        results.append((entry_id, "missing", ""))
        continue

    if harness == "boa-eval":
        boa = os.path.join(repo_root, "target/debug/boa")
        if not os.path.exists(boa):
            print(f"[{entry_id}] SKIP: target/debug/boa not built")
            results.append((entry_id, "skipped", "no boa binary"))
            continue
        try:
            proc = subprocess.run(
                [boa, input_path], capture_output=True, text=True, timeout=timeout
            )
            exit_code = proc.returncode
            output = (proc.stdout or "") + (proc.stderr or "")
        except subprocess.TimeoutExpired as exc:
            exit_code = 124
            output = ""
            for stream in (exc.stdout, exc.stderr):
                if isinstance(stream, str):
                    output += stream
        if exit_code == 0:
            print(f"[{entry_id}] RETIRED: exited cleanly (bug may be fixed!)")
            results.append((entry_id, "retired", "exit 0"))
            continue
        sig = next(
            (line.strip() for line in output.splitlines() if want and want in line),
            f"exit={exit_code}",
        )
        print(f"[{entry_id}] CRASH: {sig[:120]}")
        results.append((entry_id, "crash", normalize(sig)))
    elif harness == "fuzz-target":
        target = entry["target"]
        fuzz_dir = os.path.join(repo_root, "tests/fuzz")
        try:
            proc = subprocess.run(
                ["cargo", "fuzz", "run", "--dev", "-s", "none", target, input_path],
                cwd=fuzz_dir, capture_output=True, text=True, timeout=timeout,
            )
        except subprocess.TimeoutExpired as exc:
            # Hang-class crash (e.g. dev-profile OOM spin): the timeout IS
            # the crash. tmin is skipped below (every probe would spin).
            print(f"[{entry_id}] CRASH: exit=124 (hang; tmin-skipped-hang)")
            results.append((entry_id, "crash", "exit=124"))
            continue
        if proc.returncode == 0:
            print(f"[{entry_id}] RETIRED: target exited cleanly (bug may be fixed!)")
            results.append((entry_id, "retired", "exit 0"))
            continue
        combined = (proc.stdout or "") + (proc.stderr or "")
        with open(f"/tmp/drill-{entry_id}.out", "w") as fh:
            fh.write(f"exit={proc.returncode}\n{combined}")
        print(f"[{entry_id}] output saved to /tmp/drill-{entry_id}.out")
        sig = next(
            (line.strip() for line in combined.splitlines() if want and want in line),
            f"exit={proc.returncode}",
        )
        # Minimize: proves the tmin leg of the pipeline on this input.
        with tempfile.TemporaryDirectory() as tmp:
            tmin = subprocess.run(
                ["cargo", "fuzz", "tmin", "-D", "--sanitizer=none", target, input_path],
                cwd=fuzz_dir, capture_output=True, text=True, timeout=timeout,
            )
            minimized = "tmin-failed" if tmin.returncode != 0 else "tmin-ok"
        print(f"[{entry_id}] CRASH: {sig[:120]} ({minimized})")
        results.append((entry_id, "crash", normalize(sig)))
    else:
        print(f"[{entry_id}] UNKNOWN HARNESS: {harness}")
        results.append((entry_id, "error", harness))

crashed = [r for r in results if r[1] == "crash"]
sigs = [r[2] for r in crashed]
dupes = sorted({s for s in sigs if sigs.count(s) > 1})
print(f"drill: {len(crashed)}/{len(results)} crashed, {len(set(sigs))} distinct signatures")
if dupes:
    print(f"drill DEDUPE FAIL: {len(dupes)} shared signature(s):")
    for dup in dupes:
        print(f"  - {dup[:120]}")
problems = [r for r in results if r[1] != "crash"]
if problems:
    print("drill INCOMPLETE:")
    for entry_id, status, detail in problems:
        print(f"  - {entry_id}: {status} ({detail[:100]})")
sys.exit(0 if not problems and not dupes else 1)
EOF
