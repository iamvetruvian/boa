#!/usr/bin/env bash
# Crash-to-regression pipeline step (P4.6).
#
# Usage: ./triage-crash.sh <target> <artifact> [-s sanitizer] [--timeout N]
#
#  1. Reproduces the crash through the fleet binary (proves fleet-visibility),
#     capturing exit code + a signature line (drill taxonomy).
#  2. Dedupes the signature against known-signatures.txt (duplicates exit
#     here, skipping the expensive minimization leg).
#  3. Minimizes the input via `cargo fuzz tmin` (same sanitizer).
#  4. On a NEW signature, writes pipeline/triage/<slug>/ with the minimized
#     input, rendered source, signature, and a regression-DB entry skeleton
#     (finder="fuzz", status=open) for human admission.
#
# Exit 0: processed (duplicate or new-with-skeleton). Exit 1: crash did not
# reproduce (stale artifact) or a pipeline step failed.
set -euo pipefail

cd "$(dirname "$0")"
FUZZ_DIR="$(cd .. && pwd)"
REPO_ROOT="$(cd ../../.. && pwd)"

export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-nightly-2026-10-01}"

TARGET="${1:?usage: triage-crash.sh <target> <artifact> [-s sanitizer] [--timeout N]}"
ARTIFACT="${2:?usage: triage-crash.sh <target> <artifact> [-s sanitizer] [--timeout N]}"
SANITIZER="none"
TIMEOUT=600
shift 2 || true
while [ $# -gt 0 ]; do
  case "$1" in
    -s) SANITIZER="$2"; shift 2 ;;
    --timeout) TIMEOUT="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

python3 - "$TARGET" "$ARTIFACT" "$SANITIZER" "$TIMEOUT" "$FUZZ_DIR" "$REPO_ROOT" <<'EOF'
import hashlib
import os
import shutil
import subprocess
import sys

target, artifact, sanitizer, timeout, fuzz_dir, repo_root = sys.argv[1:7]
timeout = int(timeout)
if not os.path.isabs(artifact):
    # Users pass fuzz-dir-relative paths (`artifacts/...`); the shell wrapper
    # already cd'd into pipeline/, so anchor relatives at the fuzz dir.
    artifact = os.path.join(fuzz_dir, artifact)
if not os.path.exists(artifact):
    # Fail fast: without this, cargo-fuzz's own "no such file" error
    # (exit != 0 + an ERROR line) triages as a bogus crash signature.
    print(f"triage ARTIFACT-MISSING: {artifact}")
    sys.exit(1)
pipeline = os.path.join(fuzz_dir, "pipeline")
known_path = os.path.join(pipeline, "known-signatures.txt")

BYTE_TARGETS = {"cli-exec", "source-bytes"}

def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)

# 1. Reproduce through the fleet binary.
print(f"triage: reproducing {artifact} on {target} (-s {sanitizer})")
try:
    repro = run(
        ["cargo", "fuzz", "run", "--dev", "-s", sanitizer, target, artifact],
        cwd=fuzz_dir, timeout=timeout,
    )
    exit_code = repro.returncode
    combined = (repro.stdout or "") + (repro.stderr or "")
except subprocess.TimeoutExpired as exc:
    exit_code = 124
    combined = ""
    for stream in (exc.stdout, exc.stderr):
        if isinstance(stream, str):
            combined += stream
if exit_code == 0:
    print("triage STALE: target exited cleanly, nothing to triage")
    sys.exit(1)

markers = ("panicked at", "has overflowed its stack", "memory allocation of",
           "AddressSanitizer", "SUMMARY:", "MemorySanitizer", "runtime error:",
           "assertion failed", "Fatal error", "Received signal", "deadly signal",
           "ERROR:", "fuzz target", "Segmentation fault")
sig_line = next((ln.strip() for ln in combined.splitlines()
                 if any(m in ln for m in markers)), "")
signature = f"exit={exit_code}" + (f" :: {sig_line[:160]}" if sig_line else "")
print(f"triage: signature: {signature[:200]}")
is_hang = exit_code == 124 and not sig_line

# 2. Dedupe against known signatures (substring rules, drill taxonomy).
# Runs BEFORE minimization: duplicates skip the expensive tmin leg.
known = []
if os.path.exists(known_path):
    with open(known_path) as fh:
        for line in fh:
            line = line.strip()
            if line and not line.startswith("#"):
                known.append(line)
possible_regression_of = ""
for rule in known:
    parts = [p.strip() for p in rule.split("::")]
    if len(parts) >= 2 and parts[1] and parts[1] in signature:
        status = parts[2] if len(parts) >= 3 else "open"
        if status == "open":
            print(f"triage DUPLICATE of {parts[0]} (rule: {parts[1][:100]})")
            print(f"triage: no skeleton written; append evidence to {parts[0]} if useful")
            sys.exit(0)
        # A match on a LANDED bug is a possible regression or a new bug in
        # the same class: fail safe, write the skeleton anyway.
        possible_regression_of = parts[0]
        print(f"triage: matches LANDED {parts[0]}; writing skeleton as possible regression")
        break
if is_hang and not possible_regression_of:
    # Bare hangs carry no marker line; two hangs never auto-dedupe (drill
    # precedent), so every hang gets a skeleton and a human dedupes.
    print("triage: bare hang (no marker line); skeleton flagged for manual dedupe")

# 3. Minimize (same sanitizer, or the crash may not reproduce).
# Hangs skip tmin: each tmin probe would spin to the timeout, so minimizing
# a hang is inherently manual (bisect the input by hand, then re-run triage).
minimized = artifact
minimized_note = "tmin-skipped-hang" if is_hang else ""
if is_hang:
    print("triage: hang input; tmin skipped (minimize manually, then re-run)")
else:
    print("triage: minimizing via tmin (this can take minutes)")
    try:
        tmin = run(
            ["cargo", "fuzz", "tmin", "-D", f"--sanitizer={sanitizer}", target, artifact],
            cwd=fuzz_dir, timeout=timeout,
        )
        tmin_out = (tmin.stdout or "") + (tmin.stderr or "")
        tmin_ok = tmin.returncode == 0
    except subprocess.TimeoutExpired:
        tmin_out, tmin_ok = "", False
    minimized = None
    for line in tmin_out.splitlines():
        if "Test unit written to " in line:
            minimized = line.split("Test unit written to ", 1)[1].strip()
    if not tmin_ok or not minimized or not os.path.exists(minimized):
        print("triage TMIN-FAILED: keeping the original artifact")
        minimized = artifact
        minimized_note = "tmin failed; original artifact preserved"
    else:
        minimized_note = "tmin-ok"
        orig_size = os.path.getsize(artifact)
        new_size = os.path.getsize(minimized)
        print(f"triage: minimized {orig_size} -> {new_size} bytes ({minimized_note})")

# 4. New signature: render + skeleton.
slug = f"{target}-{hashlib.sha256(signature.encode()).hexdigest()[:12]}"
outdir = os.path.join(pipeline, "triage", slug)
os.makedirs(outdir, exist_ok=True)
shutil.copy(minimized, os.path.join(outdir, "minimized.bin"))
with open(os.path.join(outdir, "signature.txt"), "w") as fh:
    fh.write(f"{signature}\n{minimized_note}\nartifact={minimized}\n")

render = run(
    ["cargo", "run", "-q", "--example", "render", "--", target, minimized],
    cwd=fuzz_dir, timeout=120,
)
rendered = (render.stdout or "").strip()
is_bytes = target in BYTE_TARGETS
repro_name = "repro.js" if is_bytes else "rendered.js"
with open(os.path.join(outdir, repro_name), "w") as fh:
    if is_bytes:
        with open(minimized, "rb") as src:
            fh.write(src.read().decode("utf-8", "replace"))
    else:
        fh.write("// Rendered fuzzer input (exact reproducer is minimized.bin).\n")
        fh.write("// Human: confirm this JS demonstrates the crash under plain eval;\n")
        fh.write("// if the crash needs the exact Arbitrary encoding, note that instead.\n")
        fh.write(rendered + "\n")

entry_id = slug
flags = []
if possible_regression_of:
    flags.append(f"possible-regression-of = {possible_regression_of}")
if is_hang:
    flags.append("hang: manual dedupe required (no marker line)")
flags_note = ("\n# FLAGS: " + "; ".join(flags)) if flags else ""
skeleton = f"""# Skeleton for regressions.toml (finder=fuzz). Human: fill expect/layer/unit_test,
# move repro + seed into place, run the DB test, then flip status per outcome.{flags_note}
[[entries]]
id = "{entry_id}"
title = "fuzz: {target}: {signature[:100]}"
status = "open"
reproducer = "tests/regression/cases/{entry_id}/repro.js"
expect = "throw <Name>"  # TODO: post-fix behavior
layer = "engine"  # TODO
unit_test = "TODO"
no_test262_reason = "fuzz-found; no Test262 coverage"
fuzz_seed = "tests/fuzz/seeds/{entry_id}.js"  # stage minimized.bin here
symptom = "{signature[:200]}"
root_cause = "TODO (triage)"
finder = "fuzz"
"""
with open(os.path.join(outdir, "skeleton.toml"), "w") as fh:
    fh.write(skeleton)
print(f"triage NEW: wrote {outdir}/ (minimized.bin, {repro_name}, signature.txt, skeleton.toml)")
print("triage: next: admit to cases/ + regressions.toml, stage the seed, extend known-signatures.txt")
EOF
