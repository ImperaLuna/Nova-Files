#!/usr/bin/env python3
"""Deletes every comment the comment lint reports under PATHS, except the
lines listed in KEEP (one `path:line` per line). A comment alone on its line
takes the line with it.

Usage: python3 tools/comments/remove.py [--keep KEEP] PATH...
Then run `cargo fmt --all` and tools/comments/same-code.py.
"""
import argparse
import collections
import json
import subprocess

CONFIG = "tools/ast-grep/comments/sgconfig.yml"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--keep")
    parser.add_argument("paths", nargs="+")
    args = parser.parse_args()
    keep = set()
    if args.keep:
        with open(args.keep) as file:
            keep = {line.strip() for line in file if line.strip()}
    scan = subprocess.run(
        ["ast-grep", "scan", "--config", CONFIG, "--json=stream", *args.paths],
        capture_output=True,
        check=False,
    )
    ranges = collections.defaultdict(list)
    for line in scan.stdout.splitlines():
        match = json.loads(line)
        start_line = match["range"]["start"]["line"] + 1
        if f"{match['file']}:{start_line}" in keep:
            continue
        offsets = match["range"]["byteOffset"]
        ranges[match["file"]].append((offsets["start"], offsets["end"]))
    removed = 0
    for path, spans in ranges.items():
        with open(path, "rb") as file:
            source = bytearray(file.read())
        for start, end in sorted(spans, reverse=True):
            while end > start and source[end - 1 : end] == b"\n":
                end -= 1
            line_start = source.rfind(b"\n", 0, start) + 1
            line_end = source.find(b"\n", end)
            line_end = len(source) if line_end < 0 else line_end
            if not source[line_start:start].strip() and not source[end:line_end].strip():
                del source[line_start : min(line_end + 1, len(source))]
            else:
                while start > line_start and source[start - 1 : start] in (b" ", b"\t"):
                    start -= 1
                del source[start:end]
            removed += 1
        with open(path, "wb") as file:
            file.write(source)
    print(f"removed {removed} comments in {len(ranges)} files")


if __name__ == "__main__":
    main()
