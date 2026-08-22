#!/usr/bin/env python3
"""Claude Code hooks that route agent work through `ax` (tools/axiom-atlas).

Two modes, wired in `.claude/settings.json`:

  route-search   PreToolUse on Grep. Exits 2, which blocks the call and feeds
                 stderr back to the agent, redirecting it to `ax q`. Search is
                 where the ledger earns its keep: a grep that bypasses `ax` is a
                 question the repo never gets to hear.

  record-edit    PostToolUse on Edit/Write/MultiEdit. Logs the change through
                 `ax record` so the ledger stays a complete account of what
                 changed, and so the agent is reminded which of Axiom's laws
                 govern the file it just touched.

Repo tooling: stdlib only, outside the engine dependency graph.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def _ax_binary() -> Path | None:
    for name in ("ax.exe", "ax"):
        candidate = ROOT / "target" / "release" / name
        if candidate.exists():
            return candidate
    return None


def route_search() -> int:
    """Block Grep and point the agent at `ax q`."""
    sys.stderr.write(
        "This repo routes search through `ax` so that what agents look for is "
        "recorded (and so `ax miss` can show what the repo is missing).\n\n"
        "Use instead:\n"
        "  ax q <regex> [--path RE] [--lang rs|ts|...] [--limit N] [-i] [-F]\n"
        "  ax def <symbol>     where a symbol is defined\n"
        "  ax refs <symbol>    every mention of a symbol\n"
        "  ax file <regex>     find files by path\n\n"
        "Run it as `scripts/ax ...` (or `target/release/ax` directly). It is "
        "gitignore-aware and faster than raw ripgrep on this repo.\n"
    )
    return 2


def record_edit() -> int:
    """Log an edit made by a direct file tool, then get out of the way."""
    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        return 0

    tool_input = payload.get("tool_input") or {}
    path = tool_input.get("file_path") or tool_input.get("path")
    if not path:
        return 0

    binary = _ax_binary()
    if binary is None:
        return 0

    env = dict(os.environ)
    env.setdefault("AXIOM_ATLAS_AGENT", "claude-code")
    if payload.get("session_id"):
        env.setdefault("AXIOM_ATLAS_SESSION", str(payload["session_id"]))

    try:
        done = subprocess.run(
            [str(binary), "record", str(path), "--tool", str(payload.get("tool_name", "edit"))],
            capture_output=True,
            text=True,
            timeout=10,
            env=env,
            cwd=ROOT,
        )
    except (OSError, subprocess.SubprocessError):
        return 0

    # `ax record` prints the governing laws on stderr for spine files. Passing
    # that through gives the agent immediate, accurate feedback.
    if done.stderr:
        sys.stderr.write(done.stderr)
    return 0


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    if mode == "route-search":
        return route_search()
    if mode == "record-edit":
        return record_edit()
    sys.stderr.write(f"atlas_hooks: unknown mode {mode!r}\n")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
