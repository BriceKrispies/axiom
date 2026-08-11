#!/usr/bin/env python3
"""Inventory the engine spine's violations of the Axiom State Law.

Runs the engine dylint rulebook, keeps only the ``engine_no_retained_state``
diagnostics, and writes a grouped Markdown inventory to
``docs/audits/retained-state-audit.md``.

    uv run scripts/retained_state_audit.py                    # run both arms, write the report
    uv run scripts/retained_state_audit.py --from native=n.json --from wasm32=w.json

This is repo tooling: stdlib only, outside the engine dependency graph — not a
layer, module, app, or Cargo package. It is an *inventory generator*, not a gate.
The gate is ``scripts/dylint-gate.sh``, and ``engine_no_retained_state`` has no
baseline entry there on purpose: the only acceptable count for a zero-tolerance
law is zero. This report exists so the migration that gets there has a work list.

Two arms are scanned, because a lint only ever sees code the compiler actually
compiles:

* ``native`` — ``cargo dylint --all -- --all-targets``, the repo's canonical
  invocation (``scripts/dylint-gate.sh``, ``.github/workflows/ci.yml``).
* ``wasm32`` — the same rulebook against ``--target wasm32-unknown-unknown``,
  the only way to reach the platform-facing modules' browser arms
  (``axiom-windowing``, ``axiom-gpu-backend``, ``axiom-debug-overlay``,
  ``axiom-canvas2d-backend``). Those arms are ``#[cfg(target_arch = "wasm32")]``
  and are invisible to a native check — the same scope hole the coverage gate
  has. Every finding records which arm(s) saw it.

Deduplication: ``--all-targets`` compiles each crate's lib, tests, examples and
benches, so one source line is linted several times. Findings are deduplicated on
(file, line, column, message) — the count in this report is *distinct source
sites*, which is not the number the dylint gate counts (that one counts
compilation units emitting the lint).
"""

from __future__ import annotations

import argparse
import collections
import json
import pathlib
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
LINT = "engine_no_retained_state"
REPORT = REPO / "docs" / "audits" / "retained-state-audit.md"

# `--lib` on the wasm pass because a workspace's test / example / bench targets
# do not all build for wasm32.
DYLINT_PASSES = [
    ("native", ["cargo", "dylint", "--all", "--", "--all-targets"]),
    (
        "wasm32",
        ["cargo", "dylint", "--all", "--", "--target", "wasm32-unknown-unknown", "--lib"],
    ),
]


class Finding:
    __slots__ = (
        "package",
        "file",
        "line",
        "column",
        "category",
        "construct",
        "reason",
        "arms",
    )

    def __init__(self, package, file, line, column, category, construct, reason):
        self.package = package
        self.file = file
        self.line = line
        self.column = column
        self.category = category
        self.construct = construct
        self.reason = reason
        self.arms: set[str] = set()

    @property
    def arm(self) -> str:
        """`native`, `wasm32`, or `both` — which compilation arm(s) saw this."""
        return "both" if len(self.arms) > 1 else next(iter(sorted(self.arms)), "?")

    def key(self):
        return (self.file, self.line, self.column, self.category, self.construct)

    def sort_key(self):
        return (self.package, self.file, self.line, self.column, self.category)


def owning_package(path: str) -> str:
    """The layer/module crate directory a spine source path belongs to."""
    parts = pathlib.PurePosixPath(path.replace("\\", "/")).parts
    for tier in ("crates", "modules"):
        if tier in parts:
            index = parts.index(tier)
            if index + 1 < len(parts):
                return f"{tier}/{parts[index + 1]}"
    return "(unclassified)"


def parse(stream, arm: str, findings: dict[tuple, Finding]) -> dict[tuple, Finding]:
    """Merge the state-law findings from one cargo JSON message stream."""
    for raw in stream:
        raw = raw.strip()
        if not raw.startswith("{"):
            continue
        try:
            record = json.loads(raw)
        except json.JSONDecodeError:
            continue
        message = record.get("message")
        if not isinstance(message, dict):
            continue
        if (message.get("code") or {}).get("code") != LINT:
            continue
        spans = [s for s in message.get("spans", []) if s.get("is_primary")]
        if not spans:
            continue
        span = spans[0]
        # "[category] construct" — the shape `report()` in the lint emits.
        category, _, construct = message.get("message", "").partition("] ")
        category = category.lstrip("[")
        # The help subdiagnostic is "<why>; <rewrite direction>"; the short
        # reason for the inventory is the "why" clause, taken from the lint
        # itself so this script never restates the law in its own words.
        reason = ""
        for child in message.get("children", []):
            if child.get("level") == "help":
                reason = child.get("message", "").split(";")[0].strip()
                break
        file = span["file_name"].replace("\\", "/")
        finding = Finding(
            package=owning_package(file),
            file=file,
            line=span["line_start"],
            column=span["column_start"],
            category=category,
            construct=construct,
            reason=reason,
        )
        findings.setdefault(finding.key(), finding).arms.add(arm)
    return findings


def escape(text: str) -> str:
    return text.replace("|", "\\|")


def render(findings: list[Finding]) -> str:
    by_category = collections.Counter(f.category for f in findings)
    by_package = collections.Counter(f.package for f in findings)
    by_arm = collections.Counter(f.arm for f in findings)
    packages = collections.defaultdict(list)
    for finding in findings:
        packages[finding.package].append(finding)

    out: list[str] = []
    add = out.append
    add("# Axiom State Law — current-repository audit")
    add("")
    add(
        "Generated by `uv run scripts/retained_state_audit.py`, which runs the "
        f"engine dylint rulebook twice and keeps the `{LINT}` findings:"
    )
    add("")
    for arm, cmd in DYLINT_PASSES:
        add(f"* **{arm}** — `{' '.join(cmd)}`")
    add("")
    add(
        "The second pass exists because a lint only sees what the compiler "
        "compiles: the platform-facing modules' browser arms are "
        '`#[cfg(target_arch = "wasm32")]` and are entirely invisible to the '
        "canonical native invocation. The **Arm** column records which pass saw "
        "each finding."
    )
    add("")
    add(
        "**This is an inventory for a later migration task, not an allowed "
        "baseline.** `engine_no_retained_state` has deliberately no entry in "
        "`tools/lints/dylint-baseline.txt`: the only acceptable count for a "
        "zero-tolerance architectural law is zero, so the dylint gate fails on "
        "this lint until the engine is migrated. Nothing below is exempted, "
        "grandfathered, or `#[allow]`-ed."
    )
    add("")
    add(
        "Counts are **distinct source sites**, deduplicated on "
        "(file, line, column, message): `--all-targets` compiles each crate's "
        "lib, tests, examples and benches, so one line is linted several times. "
        "This is not the number `scripts/dylint-gate.sh` counts (that one counts "
        "compilation units emitting the lint)."
    )
    add("")
    add(f"**Total: {len(findings)} findings across {len(packages)} spine packages.**")
    add("")

    add("## Totals by category")
    add("")
    add("| Category | Findings |")
    add("| --- | ---: |")
    for category, count in sorted(by_category.items(), key=lambda kv: (-kv[1], kv[0])):
        add(f"| `{category}` | {count} |")
    add(f"| **total** | **{len(findings)}** |")
    add("")

    add("## Totals by compilation arm")
    add("")
    add("| Arm | Findings |")
    add("| --- | ---: |")
    for arm, count in sorted(by_arm.items(), key=lambda kv: (-kv[1], kv[0])):
        add(f"| `{arm}` | {count} |")
    add(f"| **total** | **{len(findings)}** |")
    add("")

    add("## Totals by package")
    add("")
    add("| Package | Findings |")
    add("| --- | ---: |")
    for package, count in sorted(by_package.items(), key=lambda kv: (-kv[1], kv[0])):
        add(f"| `{package}` | {count} |")
    add(f"| **total** | **{len(findings)}** |")
    add("")

    add("## Findings")
    add("")
    for package in sorted(packages):
        add(f"### `{package}` — {len(packages[package])} findings")
        add("")
        add("| File | Line | Arm | Category | Offending construct | Why it retains state |")
        add("| --- | ---: | --- | --- | --- | --- |")
        for finding in packages[package]:
            add(
                f"| `{escape(finding.file)}` | {finding.line} | {finding.arm} "
                f"| `{finding.category}` | {escape(finding.construct)} "
                f"| {escape(finding.reason)} |"
            )
        add("")
    return "\n".join(out) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--from",
        dest="sources",
        action="append",
        default=[],
        metavar="ARM=FILE",
        help="read a saved cargo JSON stream for one arm instead of running dylint",
    )
    args = parser.parse_args()

    collected: dict[tuple, Finding] = {}
    if args.sources:
        for spec in args.sources:
            arm, _, path = spec.partition("=")
            with open(path, encoding="utf-8", errors="replace") as handle:
                parse(handle, arm, collected)
    else:
        for arm, cmd in DYLINT_PASSES:
            print(f"running [{arm}]: {' '.join(cmd)} --message-format=json", file=sys.stderr)
            proc = subprocess.run(
                cmd + ["--message-format=json"],
                cwd=REPO,
                capture_output=True,
                text=True,
                errors="replace",
            )
            if proc.returncode != 0:
                sys.stderr.write(proc.stderr)
                print(
                    f"dylint driver error on the {arm} arm — the rulebook could not run.",
                    file=sys.stderr,
                )
                return 2
            parse(proc.stdout.splitlines(), arm, collected)

    findings = sorted(collected.values(), key=Finding.sort_key)
    REPORT.parent.mkdir(parents=True, exist_ok=True)
    REPORT.write_text(render(findings), encoding="utf-8")
    print(f"{len(findings)} findings -> {REPORT.relative_to(REPO)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
