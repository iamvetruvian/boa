#!/usr/bin/env python3
"""Generates the P0 machine-readable unsafe inventory.

Scans Rust sources for `unsafe` items and blocks and writes a deterministic,
sorted JSON inventory. Every site defaults to `safety_status: "unjustified"`;
P6 flips each entry to `justified` with an invariant argument, covering tests,
and an owner (or refactors the site away).

Usage:
  scripts/generate-unsafe-inventory.py [path ...] > docs/unsafe-inventory.json
Default path: core

Schema v2 (P6): adds the `trace_bypass` kind for `Trace`-bypass attributes
(`#[unsafe_ignore_trace]`, `#[boa_gc(unsafe_empty_trace)]`,
`#[boa_gc(unsafe_no_drop)]`), which must each prove the skipped field holds
no live `Gc`. Audit content is applied afterwards by
scripts/apply-unsafe-audit.py (the generator only produces the shape).

Schema v3 (P6.1): `trace_bypass` sites carry the applied-to context: when
the attribute stands alone on its line, `code` appends ` // => ` plus the
next non-attribute code line (the skipped field/variant), so audit rules
can match on the target type.
"""
import json
import re
import sys
from pathlib import Path

PATTERNS = [
    ("unsafe_fn", re.compile(r"\bunsafe\s+fn\b")),
    ("unsafe_impl", re.compile(r"\bunsafe\s+impl\b")),
    ("unsafe_trait", re.compile(r"\bunsafe\s+trait\b")),
    ("unsafe_extern", re.compile(r"\bunsafe\s+extern\b")),
    ("unsafe_block", re.compile(r"\bunsafe\s*\{")),
    (
        "trace_bypass",
        re.compile(r"unsafe_ignore_trace|unsafe_empty_trace|unsafe_no_drop"),
    ),
]

FULL_LINE_COMMENT = re.compile(r"^\s*//")


def scan_file(path: Path, root: Path):
    sites = []
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return sites
    lines = text.splitlines()
    for index, line in enumerate(lines):
        lineno = index + 1
        if FULL_LINE_COMMENT.match(line):
            continue
        # Skip matches that only occur inside a trailing comment: cut the
        # line at the first `//` unless it looks like part of a string
        # literal (a `"` appears before the `//` on the same line).
        code = line
        comment_at = line.find("//")
        if comment_at != -1 and '"' not in line[:comment_at]:
            code = line[:comment_at]
        for kind, pattern in PATTERNS:
            if pattern.search(code):
                snippet = code.strip()
                if kind == "trace_bypass" and snippet.startswith("#") and snippet.endswith("]"):
                    # Standalone attribute: attach the applied-to line so
                    # rules can match the skipped field/variant type.
                    target = ""
                    for ahead in lines[index + 1 : index + 6]:
                        probe = ahead.strip()
                        if not probe or probe.startswith("//") or probe.startswith("#"):
                            continue
                        target = probe
                        break
                    if target:
                        snippet = f"{snippet} // => {target}"
                sites.append(
                    {
                        "file": str(path.relative_to(root)),
                        "line": lineno,
                        "kind": kind,
                        "code": snippet[:200],
                        "safety_status": "unjustified",
                        "invariant": None,
                        "covering_test": None,
                        "owner": None,
                    }
                )
    return sites


def main(argv):
    root = Path(__file__).resolve().parent.parent
    targets = argv[1:] or ["core"]
    sites = []
    for target in targets:
        base = root / target
        if base.is_file():
            files = [base]
        else:
            files = sorted(base.rglob("*.rs"))
        for path in files:
            # Skip vendored or generated trees if any appear under the target.
            if "target/" in str(path):
                continue
            sites.extend(scan_file(path, root))
    sites.sort(key=lambda s: (s["file"], s["line"], s["kind"]))
    inventory = {
        "schema_version": 3,
        "generated_by": "scripts/generate-unsafe-inventory.py",
        "pins_ref": "docs/baseline.md",
        "scope": targets,
        "sites": sites,
    }
    json.dump(inventory, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
