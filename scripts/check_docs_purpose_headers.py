#!/usr/bin/env python3
"""Fail if docs Markdown files lack an orienting purpose/header line."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


DOCS_ROOT = Path("docs")
HEADER_RE = re.compile(r"^#{1,6}\s+\S.*$")
PURPOSE_RE = re.compile(r"^purpose:\s*\S.*$", re.IGNORECASE)


def has_purpose_frontmatter(lines: list[str]) -> tuple[bool, int]:
    if not lines or lines[0].strip() != "---":
        return False, 0

    for index, line in enumerate(lines[1:], start=1):
        if line.strip() == "---":
            frontmatter = lines[1:index]
            return any(PURPOSE_RE.match(item.strip()) for item in frontmatter), index + 1

    return False, 0


def first_content_line(lines: list[str], start: int) -> tuple[int, str] | None:
    for index in range(start, len(lines)):
        if lines[index].strip():
            return index + 1, lines[index]
    return None


def check_file(path: Path) -> str | None:
    lines = path.read_text(encoding="utf-8").splitlines()
    return check_lines(str(path), lines)


def check_lines(path_label: str, lines: list[str]) -> str | None:
    has_purpose, content_start = has_purpose_frontmatter(lines)
    if has_purpose:
        return None

    first = first_content_line(lines, content_start)
    if first is None:
        return f"{path_label}: empty Markdown file"

    line_number, line = first
    if HEADER_RE.match(line.strip()):
        return None

    return (
        f"{path_label}:{line_number}: first content line must be a Markdown heading "
        "or frontmatter must include a one-line 'purpose:' field"
    )


def run_self_test() -> int:
    cases = [
        (
            "heading_pass.md",
            ["# Heading", "", "Body"],
            None,
        ),
        (
            "purpose_frontmatter_pass.md",
            ["---", "purpose: Explain the operator flow.", "---", "plain first line"],
            None,
        ),
        (
            "empty_fail.md",
            [],
            "empty Markdown file",
        ),
        (
            "plain_first_line_fail.md",
            ["plain first line"],
            "first content line must be a Markdown heading",
        ),
    ]
    failures = []
    for path_label, lines, expected_fragment in cases:
        result = check_lines(path_label, lines)
        if expected_fragment is None:
            if result is not None:
                failures.append(f"{path_label}: expected pass, got {result}")
            continue
        if result is None or expected_fragment not in result:
            failures.append(
                f"{path_label}: expected failure containing {expected_fragment!r}, got {result!r}"
            )

    if failures:
        print("Docs purpose/header self-test failed:", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1

    print(f"Docs purpose/header self-test passed: checked {len(cases)} case(s).")
    return 0


def run_repo_check() -> int:
    if not DOCS_ROOT.is_dir():
        print(f"{DOCS_ROOT} does not exist", file=sys.stderr)
        return 1

    markdown_files = sorted(path for path in DOCS_ROOT.rglob("*.md") if path.is_file())
    failures = [
        failure
        for path in markdown_files
        for failure in [check_file(path)]
        if failure is not None
    ]

    if failures:
        print("Docs purpose/header lint failed:", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1

    print(f"Docs purpose/header lint passed: checked {len(markdown_files)} file(s).")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Fail if docs Markdown files lack an orienting purpose/header line."
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run deterministic in-memory fixture cases instead of scanning docs/",
    )
    args = parser.parse_args(argv)

    if args.self_test:
        return run_self_test()
    return run_repo_check()


if __name__ == "__main__":
    raise SystemExit(main())
