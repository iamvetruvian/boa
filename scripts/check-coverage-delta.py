#!/usr/bin/env python3
"""P7.1a: per-crate line-coverage non-decreasing gate.

Compares fresh tarpaulin cobertura XML against the committed baseline
(`docs/coverage-baseline.json`) for the choke-point crates only
(dependency files in the XML are ignored). Fails when any choke crate's
line rate decreases; prints old -> new per crate either way.

The battery runs one canonical tarpaulin command (all four crates, one
XML) so fresh and baseline runs share the same instrumentation universe;
never compare across different package sets.

Usage:
  scripts/check-coverage-delta.py <fresh-cobertura.xml> [baseline.json]
  scripts/check-coverage-delta.py --emit-baseline <cobertura.xml> > baseline.json
  scripts/check-coverage-delta.py --merge <out.xml> <in1.xml> [in2.xml ...]

The battery runs tarpaulin split (small crates, then engine: a single
4-crate run flakes collection timeouts) and merges the XMLs by union of
covered lines before comparing, so fresh and baseline always share the
same procedure.
"""
import json
import sys
import xml.etree.ElementTree as ET

CRATES = {
    "core/engine/": "boa_engine",
    "core/gc/": "boa_gc",
    "core/string/": "boa_string",
    "core/interner/": "boa_interner",
}

EPS = 1e-9


def crate_rates(xml_path):
    tree = ET.parse(xml_path)
    agg = {c: [0, 0] for c in CRATES.values()}  # covered, total
    for cls in tree.getroot().iter("class"):
        filename = cls.get("filename", "")
        crate = next(
            (v for prefix, v in CRATES.items() if filename.startswith(prefix)),
            None,
        )
        if crate is None:
            continue
        for line in cls.iter("line"):
            agg[crate][1] += 1
            if int(line.get("hits", 0)) > 0:
                agg[crate][0] += 1
    out = {}
    for crate, (covered, total) in agg.items():
        out[crate] = {
            "rate": covered / total if total else 0.0,
            "covered": covered,
            "total": total,
        }
    return out


def merge_xmls(out_path, in_paths):
    """Union-merge cobertura files: a line counts covered if hit in any input."""
    files = {}  # filename -> {lineno -> hits}
    for path in in_paths:
        tree = ET.parse(path)
        for cls in tree.getroot().iter("class"):
            lines = files.setdefault(cls.get("filename", ""), {})
            for line in cls.iter("line"):
                no = int(line.get("number", 0))
                lines[no] = max(lines.get(no, 0), int(line.get("hits", 0)))
    total = sum(len(v) for v in files.values())
    covered = sum(1 for v in files.values() for h in v.values() if h > 0)
    root = ET.Element(
        "coverage",
        {
            "line-rate": f"{covered / total if total else 0:.4f}",
            "version": "merged",
        },
    )
    packages = ET.SubElement(root, "packages")
    package = ET.SubElement(packages, "package", {"name": "merged"})
    classes = ET.SubElement(package, "classes")
    for filename in sorted(files):
        cls = ET.SubElement(classes, "class", {"filename": filename})
        lines_el = ET.SubElement(cls, "lines")
        for no in sorted(files[filename]):
            ET.SubElement(
                lines_el, "line", {"number": str(no), "hits": str(files[filename][no])}
            )
    ET.ElementTree(root).write(out_path, xml_declaration=True)
    print(f"merged {len(in_paths)} files -> {out_path} ({covered}/{total} lines)")


def main(argv):
    if len(argv) == 3 and argv[1] == "--emit-baseline":
        print(json.dumps(crate_rates(argv[2]), indent=2, sort_keys=True))
        return 0
    if len(argv) >= 4 and argv[1] == "--merge":
        merge_xmls(argv[2], argv[3:])
        return 0
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    baseline_path = argv[2] if len(argv) == 3 else "docs/coverage-baseline.json"
    with open(baseline_path) as f:
        baseline = json.load(f)
    fresh = crate_rates(argv[1])
    failed = False
    for crate in sorted(set(baseline) | set(fresh)):
        old = baseline.get(crate)
        new = fresh.get(crate)
        if old is None or new is None:
            print(f"error: crate set drifted: {crate} missing on one side")
            failed = True
            continue
        arrow = "->"
        status = "ok"
        if new["rate"] < old["rate"] - EPS:
            status = "REGRESSED"
            failed = True
        elif new["rate"] > old["rate"] + EPS:
            status = "improved (re-baseline to ratchet up)"
        print(
            f"{crate}: {old['rate']:.4f} ({old['covered']}/{old['total']}) "
            f"{arrow} {new['rate']:.4f} ({new['covered']}/{new['total']}) [{status}]"
        )
    if failed:
        print("coverage gate: REGRESSION", file=sys.stderr)
        return 1
    print("coverage gate: no per-crate regression")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
