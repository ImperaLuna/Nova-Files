#!/usr/bin/env python3
"""Proves a change removed only comments: every Rust file changed since BASE
has the same tokens, comments left out, as it had at BASE.

Usage: python3 tools/comments/same-code.py [BASE]   (default origin/main)
Needs: pip install tree-sitter tree-sitter-rust
"""
import subprocess
import sys

import tree_sitter
import tree_sitter_rust

COMMENTS = {"line_comment", "block_comment"}
CLOSERS = {")", "]", "}"}
PARSER = tree_sitter.Parser(tree_sitter.Language(tree_sitter_rust.language()))


def tokens(source):
    found, comments = [], 0
    stack = [PARSER.parse(source).root_node]
    while stack:
        node = stack.pop()
        if node.type in COMMENTS:
            comments += 1
            continue
        if node.child_count == 0:
            found.append((node.type, node.text))
        stack.extend(reversed(node.children))
    return without_trailing_commas(found), comments


def without_trailing_commas(found):
    """rustfmt adds or drops a comma before a closing bracket when a list
    changes lines; that comma means nothing."""
    return [
        token
        for i, token in enumerate(found)
        if not (token[0] == "," and i + 1 < len(found) and found[i + 1][0] in CLOSERS)
    ]


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True).stdout


def main():
    base = sys.argv[1] if len(sys.argv) > 1 else "origin/main"
    status = git("diff", "--name-status", "--no-renames", base, "--", "*.rs").decode()
    files = total = before_comments = after_comments = 0
    failed = []
    for line in status.splitlines():
        kind, path = line.split("\t", 1)
        if kind != "M":
            failed.append(f"{path}: {kind} (only edits to existing files are allowed)")
            continue
        before, comments_before = tokens(git("show", f"{base}:{path}"))
        with open(path, "rb") as file:
            after, comments_after = tokens(file.read())
        files += 1
        total += len(after)
        before_comments += comments_before
        after_comments += comments_after
        if before != after:
            first = next(
                (i for i, pair in enumerate(zip(before, after)) if pair[0] != pair[1]),
                min(len(before), len(after)),
            )
            failed.append(f"{path}: tokens differ at token {first}")
    print(f"files: {files}, tokens: {total}, comments: {before_comments} -> {after_comments}")
    for failure in failed:
        print(f"DIFFERENT {failure}")
    print("same code" if not failed else "CODE CHANGED")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
