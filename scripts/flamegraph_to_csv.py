#!/usr/bin/env python3
import argparse
import csv
import json
import re
from collections import Counter
from pathlib import Path
import xml.etree.ElementTree as ET


TITLE_RE = re.compile(r"^(.*) \((\d+) samples?, ([0-9.]+)%\)$")


def parse_svg(svg_path: Path):
    root = ET.fromstring(svg_path.read_text())
    nodes = []
    for g in root.iter():
        if not g.tag.endswith("g"):
            continue
        title_el = None
        rect_el = None
        for child in list(g):
            if child.tag.endswith("title"):
                title_el = child
            elif child.tag.endswith("rect"):
                rect_el = child
        if title_el is None or rect_el is None:
            continue
        title = title_el.text or ""
        match = TITLE_RE.match(title)
        if not match:
            continue
        function = match.group(1)
        samples = int(match.group(2))
        percent = float(match.group(3))
        nodes.append(
            {
                "function": function,
                "samples": samples,
                "percent": percent,
                "x": rect_el.attrib.get("x", ""),
                "y": rect_el.attrib.get("y", ""),
                "width": rect_el.attrib.get("width", ""),
                "height": rect_el.attrib.get("height", ""),
            }
        )
    return nodes


def write_nodes_csv(nodes, out_path: Path):
    with out_path.open("w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(["function", "samples", "percent", "x", "y", "width", "height"])
        for node in nodes:
            writer.writerow(
                [
                    node["function"],
                    node["samples"],
                    f'{node["percent"]:.2f}',
                    node["x"],
                    node["y"],
                    node["width"],
                    node["height"],
                ]
            )


def write_top_json(nodes, out_path: Path):
    counts = Counter()
    for node in nodes:
        counts[node["function"]] += node["samples"]
    total = counts.get("all", sum(counts.values()))
    items = []
    for function, samples in counts.most_common():
        percent = (samples / total * 100.0) if total else 0.0
        items.append({"function": function, "samples": samples, "percent": percent})
    out_path.write_text(json.dumps(items, indent=2))


def main():
    parser = argparse.ArgumentParser(
        description="Convert a flamegraph SVG into CSV (nodes) and JSON (top functions)."
    )
    parser.add_argument("svg", type=Path, help="Path to flamegraph SVG")
    parser.add_argument(
        "--out-prefix",
        type=Path,
        default=None,
        help="Output prefix (default: SVG path without extension)",
    )
    args = parser.parse_args()

    svg_path = args.svg
    if not svg_path.exists():
        raise SystemExit(f"SVG not found: {svg_path}")

    prefix = args.out_prefix or svg_path.with_suffix("")
    nodes_csv = prefix.with_suffix(".nodes.csv")
    top_json = prefix.with_suffix(".top.json")

    nodes = parse_svg(svg_path)
    write_nodes_csv(nodes, nodes_csv)
    write_top_json(nodes, top_json)

    print(f"Wrote {nodes_csv} and {top_json} (nodes={len(nodes)})")


if __name__ == "__main__":
    main()
