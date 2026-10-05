#!/usr/bin/env python3
"""Per-target coverage ratchet (P4.6).

For each fuzz target: cmin the corpus, collect coverage over it, and compare
covered regions against pipeline/coverage-baseline.toml. Fails on any
decrease (strict >=: cmin+coverage is bit-deterministic on a fixed corpus,
verified empirically).

The baseline measures the COMMITTED seed bank (CI syncs seeds/ only), so the
ratchet rewards merge-back: every admitted seed that covers new regions lets
a human ratchet the baseline up with --update-baseline. Code removal that
legitimately drops totals is handled the same way, with the reason recorded
in the commit message — never silently.

Usage:
  ./coverage-gate.py [--update-baseline] [--targets a,b] [--timeout N]
"""
import json
import os
import shutil
import subprocess
import sys

try:
    import tomllib
except ImportError:
    import tomli as tomllib

FUZZ_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASELINE = os.path.join(FUZZ_DIR, "pipeline", "coverage-baseline.toml")
TARGETS = ["parser-idempotency", "vm-implied", "bytecompiler-implied",
           "source-bytes", "module-specifier", "url-parse", "fetch-response",
           "cli-exec"]
STEP_TIMEOUT = 1800

os.environ.setdefault("RUSTUP_TOOLCHAIN", "nightly-2026-10-01")


def run(cmd, timeout):
    return subprocess.run(cmd, capture_output=True, text=True, cwd=FUZZ_DIR,
                          timeout=timeout)


def llvm_cov():
    proc = run(["rustc", "--print", "sysroot"], 60)
    sysroot = proc.stdout.strip()
    cov = os.path.join(sysroot, "lib", "rustlib", "x86_64-unknown-linux-gnu",
                       "bin", "llvm-cov")
    if not os.path.exists(cov):
        print("coverage-gate: llvm-cov not found; install llvm-tools-preview")
        sys.exit(1)
    return cov


def measure(target, cov_bin, timeout):
    """cmin + coverage + export totals for one target."""
    corpus_dir = os.path.join(FUZZ_DIR, "corpus", target)
    before = sorted(os.listdir(corpus_dir)) if os.path.isdir(corpus_dir) else []
    cmin = run(["cargo", "fuzz", "cmin", "--dev", "-s", "none", target], timeout)
    if cmin.returncode != 0:
        return None, f"cmin failed: {(cmin.stderr or '')[-300:]}"
    after = sorted(os.listdir(corpus_dir)) if os.path.isdir(corpus_dir) else []
    if before and not after:
        # cmin replaces the corpus in place; a crash mid-merge can leave it
        # empty. Restore seeds so the next step (and human) sees sense, then
        # fail loudly: an empty minimized corpus is signal, not noise.
        run(["./sync-seeds.sh", target], 120)
        return None, "cmin emptied a nonempty corpus (crash during merge?)"
    # `cargo fuzz coverage` appends profraw batches without clearing: without
    # this, every measurement unions all previous runs' coverage and the
    # ratchet only ever climbs (observed: +13k phantom regions on parser).
    shutil.rmtree(os.path.join(FUZZ_DIR, "coverage", target), ignore_errors=True)
    cov = run(["cargo", "fuzz", "coverage", "--dev", "-s", "none", target], timeout)
    if cov.returncode != 0:
        return None, f"coverage failed: {(cov.stderr or '')[-300:]}"
    binary = os.path.join(
        FUZZ_DIR, "target", "x86_64-unknown-linux-gnu", "coverage",
        "x86_64-unknown-linux-gnu", "debug", target)
    profdata = os.path.join(FUZZ_DIR, "coverage", target, "coverage.profdata")
    export = run([cov_bin, "export", binary, f"-instr-profile={profdata}",
                  "--summary-only"], 300)
    try:
        summary = json.loads(export.stdout or "")
        totals_all = summary["data"][0]["totals"]
        totals = {key: totals_all[key]["covered"]
                  for key in ("regions", "functions", "lines")}
    except (ValueError, KeyError, IndexError, TypeError) as err:
        return None, f"could not parse llvm-cov JSON: {err}"
    return totals, ""


def load_baseline():
    if not os.path.exists(BASELINE):
        return {}
    with open(BASELINE, "rb") as fh:
        return tomllib.load(fh)


HEADER = """\
# Per-target coverage baselines for the P4.6 ratchet (covered regions over
# the cmin-minimized committed seed bank; functions/lines recorded for context).
# Regenerate with: ./pipeline/coverage-gate.py --update-baseline
# Policy: the gate fails on any REPRODUCIBLE decrease (a first below-baseline
# reading re-measures once; coverage wobbles a few regions run to run).
# Ratchet UP when new seeds cover
# more (celebrate). Ratchet DOWN only when code removal legitimately drops
# totals, with the reason in the commit message. Never edit by hand otherwise.
"""


def main(argv):
    update, targets, timeout = False, list(TARGETS), STEP_TIMEOUT
    i = 0
    while i < len(argv):
        if argv[i] == "--update-baseline":
            update = True
            i += 1
        elif argv[i] == "--targets":
            targets = argv[i + 1].split(",")
            unknown = [t for t in targets if t not in TARGETS]
            if unknown:
                print(f"unknown targets: {unknown}")
                return 2
            i += 2
        elif argv[i] == "--timeout":
            timeout = int(argv[i + 1])
            i += 2
        else:
            print(f"unknown arg: {argv[i]}")
            return 2

    cov_bin = llvm_cov()
    baseline = load_baseline()
    measured = {}
    failed = False
    for target in targets:
        totals, err = measure(target, cov_bin, timeout)
        if totals is None:
            print(f"[{target}] ERROR: {err}")
            failed = True
            continue
        measured[target] = totals
        base = baseline.get(target, {})
        if not base:
            print(f"[{target}] SKIP: no baseline "
                  f"(regions={totals['regions']}; run --update-baseline to set it)")
            continue
        delta = totals["regions"] - base.get("regions", 0)
        if delta < 0:
            # Coverage wobbles run to run (observed: -15 regions on
            # bytecompiler-implied, then = baseline twice). A breach must
            # reproduce before it fails the gate: measure once more.
            print(f"[{target}] tentative BREACH: regions {totals['regions']} < "
                  f"baseline {base['regions']} (delta {delta}); re-measuring...")
            totals2, err2 = measure(target, cov_bin, timeout)
            if totals2 is None:
                print(f"[{target}] ERROR on re-measure: {err2}")
                failed = True
                continue
            measured[target] = totals = totals2
            delta = totals["regions"] - base.get("regions", 0)
            if delta < 0:
                print(f"[{target}] BREACH (reproduced): regions {totals['regions']} < "
                      f"baseline {base['regions']} (delta {delta})")
                failed = True
                continue
            print(f"[{target}] noise, not breach: re-measure {totals['regions']} "
                  f"(delta {delta:+}); continuing")
        elif delta > 0:
            print(f"[{target}] OK: regions {totals['regions']} "
                  f"(+{delta} above baseline; ratchet with --update-baseline)")
        else:
            print(f"[{target}] OK: regions {totals['regions']} (= baseline)")

    if update and measured:
        merged = dict(baseline)
        merged.update(measured)
        with open(BASELINE, "w") as fh:
            fh.write(HEADER)
            for target in TARGETS:
                if target in merged:
                    m = merged[target]
                    fh.write(f"\n[{target}]\nregions = {m['regions']}\n"
                             f"functions = {m['functions']}\nlines = {m['lines']}\n")
        print(f"baseline written to {BASELINE}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
