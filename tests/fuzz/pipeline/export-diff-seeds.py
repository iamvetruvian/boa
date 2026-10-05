#!/usr/bin/env python3
"""Export P3 mismatch minimizations as fuzz seeds (P4.6).

Reads differential-runs/*/triage-queue.json, strips the Test262 harness
prelude from each minimized program (the minimizer deletes whole lines and
preserves order, and corpus.rs concatenates harness-first/test-after, so
dropping the leading run of harness-known lines isolates the reduced test
body), dedupes, and writes seeds/diff-<shard>-<slug>.js.

Default exports boa-bug verdicts only (directed engine-bug seeds); --verdicts
extends to other kinds (deterministic order, --max-total cap).

Usage:
  ./export-diff-seeds.py [--verdicts boa-bug,spec-ambiguity] [--max-total N] [--dry-run]
"""
import hashlib
import json
import os
import re
import sys

FUZZ_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO_ROOT = os.path.dirname(os.path.dirname(FUZZ_DIR))
RUNS_DIR = os.path.join(REPO_ROOT, "differential-runs")
HARNESS_DIR = os.path.join(REPO_ROOT, "test262", "harness")
SEEDS_DIR = os.path.join(FUZZ_DIR, "seeds")
MIRROR_LINE = "var TypedArray = Object.getPrototypeOf(Uint8Array);"


def harness_lines():
    """Union of verbatim lines across all Test262 harness files."""
    lines = {MIRROR_LINE}
    for name in sorted(os.listdir(HARNESS_DIR)):
        if not name.endswith(".js"):
            continue
        with open(os.path.join(HARNESS_DIR, name)) as fh:
            lines.update(line.rstrip("\n") for line in fh)
    return lines


def strip_harness(program, harness):
    """Drop the leading run of harness-known lines; keep the rest."""
    kept = program.split("\n")
    idx = 0
    while idx < len(kept) and kept[idx] in harness:
        idx += 1
    return "\n".join(kept[idx:]).strip() + "\n"


def slug(entry_id):
    stem = entry_id.rsplit("/", 1)[-1]
    stem = re.sub(r"\.js$", "", stem)
    stem = re.sub(r"[^a-zA-Z0-9]+", "-", stem).strip("-").lower()[:60]
    digest = hashlib.sha256(entry_id.encode()).hexdigest()[:8]
    return f"{stem}-{digest}" if stem else digest


def main(argv):
    verdicts = {"boa-bug"}
    max_total = None
    dry_run = False
    i = 0
    while i < len(argv):
        if argv[i] == "--verdicts":
            verdicts = set(argv[i + 1].split(","))
            i += 2
        elif argv[i] == "--max-total":
            max_total = int(argv[i + 1])
            i += 2
        elif argv[i] == "--dry-run":
            dry_run = True
            i += 1
        else:
            print(f"unknown arg: {argv[i]}", file=sys.stderr)
            return 2

    harness = harness_lines()
    exported, skipped_empty, skipped_dup, skipped_verdict = 0, 0, 0, 0
    seen_hashes = set()
    names = []
    for shard in sorted(os.listdir(RUNS_DIR)):
        queue = os.path.join(RUNS_DIR, shard, "triage-queue.json")
        if not os.path.isfile(queue):
            continue
        with open(queue) as fh:
            entries = json.load(fh)["entries"]
        for entry_id in sorted(entries):
            entry = entries[entry_id]
            verdict = entry.get("verdict")
            kind = verdict.get("kind") if isinstance(verdict, dict) else verdict
            if kind not in verdicts:
                skipped_verdict += 1
                continue
            body = strip_harness(entry.get("minimized_program") or "", harness)
            if not body.strip():
                skipped_empty += 1
                continue
            digest = hashlib.sha256(body.encode()).hexdigest()
            if digest in seen_hashes:
                skipped_dup += 1
                continue
            seen_hashes.add(digest)
            if max_total is not None and exported >= max_total:
                continue
            name = f"diff-{shard}-{slug(entry_id)}.js"
            names.append((shard, entry_id, name, len(body.splitlines())))
            if not dry_run:
                with open(os.path.join(SEEDS_DIR, name), "w") as fh:
                    fh.write(f"// Differential minimization: {shard}/{entry_id} ({kind}).\n")
                    fh.write(body)
            exported += 1

    for shard, entry_id, name, lines in names:
        print(f"{name}  ({lines} lines)  <- {shard}/{entry_id}")
    print(
        f"exported={exported} skipped_empty={skipped_empty} "
        f"skipped_dup={skipped_dup} skipped_verdict={skipped_verdict} dry_run={dry_run}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
