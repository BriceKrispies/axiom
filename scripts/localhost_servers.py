#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""
localhost_servers.py — a tiny daemon-style manager for background localhost servers.

It starts servers as FULLY DETACHED background processes that keep running after
the launching shell (and this script) exit — so an app you "reserve on localhost"
stays up across terminals and sessions until you stop it. Each command is a quick
one-shot invocation (like `scripts/playwright_controller.py`); the *servers* are the
long-lived daemons, tracked in a small JSON registry under
`scripts/.localhost-servers/` (git-ignored). Every server's stdout+stderr is
redirected to its own log file there.

Stdlib only — no pip deps — so `uv run` starts instantly.

Usage (via `uv run`):

  # Axiom apps — served through axiom-serve (correct import map + hot reload):
  uv run scripts/localhost_servers.py up                     # start the default set (home-run)
  uv run scripts/localhost_servers.py start-app home-run     # start one app (port 8080)
  uv run scripts/localhost_servers.py start-app heat-check --port 8081

  # Any command as a named server:
  uv run scripts/localhost_servers.py start docs --port 9000 --cwd site -- python -m http.server 9000

  # Manage them:
  uv run scripts/localhost_servers.py status                 # table of every server (default cmd)
  uv run scripts/localhost_servers.py logs home-run -n 40    # last N log lines
  uv run scripts/localhost_servers.py url home-run           # print its http://localhost:PORT
  uv run scripts/localhost_servers.py restart home-run
  uv run scripts/localhost_servers.py stop home-run
  uv run scripts/localhost_servers.py stop-all

Env: AXIOM_SERVERS_DIR overrides the state directory.
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import socket
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

IS_WIN = os.name == "nt"

# The default set `up` brings online: (app, port). Add your own here.
DEFAULT_APPS: list[tuple[str, int]] = [("home-run", 8080)]


# ── locations ────────────────────────────────────────────────────────────────────

def repo_root() -> Path:
    """The repo root — walk up from this script until a `.git` dir is found."""
    here = Path(__file__).resolve()
    for parent in [here.parent, *here.parents]:
        if (parent / ".git").exists():
            return parent
    return here.parent.parent


def state_dir() -> Path:
    override = os.environ.get("AXIOM_SERVERS_DIR")
    d = Path(override) if override else repo_root() / "scripts" / ".localhost-servers"
    d.mkdir(parents=True, exist_ok=True)
    (d / "logs").mkdir(exist_ok=True)
    return d


def registry_path() -> Path:
    return state_dir() / "registry.json"


def log_path(name: str) -> Path:
    return state_dir() / "logs" / f"{name}.log"


# ── registry ─────────────────────────────────────────────────────────────────────

def load_registry() -> dict:
    p = registry_path()
    if not p.exists():
        return {}
    try:
        return json.loads(p.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return {}


def save_registry(reg: dict) -> None:
    tmp = registry_path().with_suffix(".json.tmp")
    tmp.write_text(json.dumps(reg, indent=2), encoding="utf-8")
    tmp.replace(registry_path())


# ── process helpers ──────────────────────────────────────────────────────────────

def pid_alive(pid: int | None) -> bool:
    if not pid:
        return False
    if IS_WIN:
        out = subprocess.run(
            ["tasklist", "/FI", f"PID eq {pid}", "/NH"],
            capture_output=True, text=True,
        ).stdout
        return str(pid) in out
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def port_listening(port: int | None, host: str = "127.0.0.1") -> bool:
    if not port:
        return False
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.settimeout(0.35)
        return s.connect_ex((host, port)) == 0


def free_port(preferred: int) -> int:
    """The preferred port if free, else the next free port above it."""
    for port in range(preferred, preferred + 100):
        if not port_listening(port):
            return port
    return preferred


def kill_tree(pid: int) -> None:
    if not pid:
        return
    if IS_WIN:
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(pid)], capture_output=True)
        return
    # POSIX: the child was started in its own session/group, so kill the group.
    try:
        os.killpg(os.getpgid(pid), signal.SIGTERM)
    except (ProcessLookupError, PermissionError):
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass


def spawn_detached(command: list[str], cwd: Path, log: Path) -> int:
    """Start `command` fully detached from this process; return the child PID.
    stdout+stderr → `log`; stdin → devnull. Survives this script's exit."""
    log.parent.mkdir(parents=True, exist_ok=True)
    header = f"\n===== started {datetime.now(timezone.utc).isoformat()} : {' '.join(command)} =====\n"
    fh = open(log, "a", buffering=1, encoding="utf-8", errors="replace")
    fh.write(header)
    fh.flush()
    kwargs: dict = dict(
        cwd=str(cwd),
        stdin=subprocess.DEVNULL,
        stdout=fh,
        stderr=subprocess.STDOUT,
        close_fds=True,
    )
    if IS_WIN:
        # DETACHED_PROCESS: no console tied to the parent; NEW_PROCESS_GROUP so a
        # Ctrl-C in this shell never reaches it.
        kwargs["creationflags"] = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        kwargs["start_new_session"] = True  # setsid: its own process group/session
    proc = subprocess.Popen(command, **kwargs)
    return proc.pid


# ── commands ─────────────────────────────────────────────────────────────────────

def do_start(name: str, command: list[str], cwd: Path, port: int | None, *, replace: bool = False) -> int:
    reg = load_registry()
    existing = reg.get(name)
    if existing and pid_alive(existing.get("pid")):
        if not replace:
            print(f"- '{name}' already running (pid {existing['pid']}, port {existing.get('port')}). "
                  f"Use `restart {name}` to relaunch.")
            return 0
        kill_tree(existing["pid"])
        time.sleep(0.8)

    log = log_path(name)
    pid = spawn_detached(command, cwd, log)
    reg[name] = {
        "command": command,
        "cwd": str(cwd),
        "port": port,
        "pid": pid,
        "log": str(log),
        "started_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
    }
    save_registry(reg)

    # Give it a moment; report whether it stayed up (and, if a port is known, bound it).
    deadline = time.time() + (12.0 if port else 2.0)
    bound = False
    while time.time() < deadline:
        if not pid_alive(pid):
            print(f"[x] '{name}' exited immediately. Recent log:")
            _print_log(name, 20)
            return 1
        if port and port_listening(port):
            bound = True
            break
        time.sleep(0.4)
    where = f"http://localhost:{port}/" if port else "(no port declared)"
    state = "listening" if bound else ("starting..." if port else "running")
    print(f"[ok] '{name}' {state}  pid {pid}  {where}")
    print(f"  log: {log}")
    return 0


def do_stop(name: str) -> int:
    reg = load_registry()
    entry = reg.get(name)
    if not entry:
        print(f"- no server named '{name}'")
        return 1
    if pid_alive(entry.get("pid")):
        kill_tree(entry["pid"])
        print(f"[ok] stopped '{name}' (pid {entry['pid']})")
    else:
        print(f"- '{name}' was not running")
    reg.pop(name, None)
    save_registry(reg)
    return 0


def do_stop_all() -> int:
    reg = load_registry()
    if not reg:
        print("- nothing to stop")
        return 0
    for name, entry in list(reg.items()):
        if pid_alive(entry.get("pid")):
            kill_tree(entry["pid"])
            print(f"[ok] stopped '{name}' (pid {entry['pid']})")
    save_registry({})
    return 0


def do_restart(name: str) -> int:
    reg = load_registry()
    entry = reg.get(name)
    if not entry:
        print(f"- no server named '{name}'")
        return 1
    return do_start(name, entry["command"], Path(entry["cwd"]), entry.get("port"), replace=True)


def do_status() -> int:
    reg = load_registry()
    if not reg:
        print("no servers registered. Start one with `start-app <app>` or `up`.")
        return 0
    rows = []
    for name, e in reg.items():
        alive = pid_alive(e.get("pid"))
        port = e.get("port")
        listening = port_listening(port) if port else False
        status = "listening" if (alive and listening) else ("running" if alive else "DEAD")
        url = f"http://localhost:{port}/" if port else "-"
        rows.append((name, status, str(e.get("pid") or "-"), str(port or "-"), url, e.get("started_at", "-")))
    widths = [max(len(str(r[i])) for r in [("NAME", "STATUS", "PID", "PORT", "URL", "STARTED"), *rows]) for i in range(6)]
    def fmt(r): return "  ".join(str(c).ljust(widths[i]) for i, c in enumerate(r))
    print(fmt(("NAME", "STATUS", "PID", "PORT", "URL", "STARTED")))
    print(fmt(tuple("-" * w for w in widths)))
    for r in rows:
        print(fmt(r))
    return 0


def do_url(name: str) -> int:
    entry = load_registry().get(name)
    if not entry or not entry.get("port"):
        print(f"- no URL for '{name}'")
        return 1
    print(f"http://localhost:{entry['port']}/")
    return 0


def _print_log(name: str, n: int) -> None:
    p = log_path(name)
    if not p.exists():
        print(f"  (no log at {p})")
        return
    lines = p.read_text(encoding="utf-8", errors="replace").splitlines()
    for line in lines[-n:]:
        print("  " + line)


def do_logs(name: str, n: int) -> int:
    if name not in load_registry():
        print(f"- no server named '{name}'")
        return 1
    _print_log(name, n)
    return 0


def do_up() -> int:
    for app, port in DEFAULT_APPS:
        do_start(*app_command(app, port))
    return 0


# ── axiom-serve convenience ──────────────────────────────────────────────────────

def app_command(app: str, port: int, *, name: str | None = None) -> tuple[str, list[str], Path, int]:
    """The (name, command, cwd, port) tuple that serves an Axiom app under
    axiom-serve — the tool that injects the `@axiom/web-engine` import map and
    hot-reloads on save. Port auto-bumps if the preferred one is taken."""
    port = free_port(port)
    command = ["cargo", "run", "-q", "-p", "axiom-serve", "--", app, "--no-open", "--port", str(port)]
    return (name or app, command, repo_root(), port)


# ── CLI ──────────────────────────────────────────────────────────────────────────

def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Manage background localhost servers.")
    sub = parser.add_subparsers(dest="cmd")

    sub.add_parser("status", help="show every registered server (default)")
    sub.add_parser("up", help="start the default app set")
    sub.add_parser("stop-all", help="stop every server")

    p_app = sub.add_parser("start-app", help="serve an Axiom app via axiom-serve")
    p_app.add_argument("app")
    p_app.add_argument("--port", type=int, default=8080)
    p_app.add_argument("--name", default=None)

    p_start = sub.add_parser("start", help="start any command as a named server")
    p_start.add_argument("name")
    p_start.add_argument("--port", type=int, default=None)
    p_start.add_argument("--cwd", default=None)
    p_start.add_argument("rest", nargs=argparse.REMAINDER,
                         help="-- then the command to run, e.g. -- python -m http.server 9000")

    for cname in ("stop", "restart", "url"):
        sp = sub.add_parser(cname)
        sp.add_argument("name")

    p_logs = sub.add_parser("logs", help="print a server's recent log")
    p_logs.add_argument("name")
    p_logs.add_argument("-n", type=int, default=30)

    args = parser.parse_args(argv)
    cmd = args.cmd or "status"

    if cmd == "status":
        return do_status()
    if cmd == "up":
        return do_up()
    if cmd == "stop-all":
        return do_stop_all()
    if cmd == "start-app":
        return do_start(*app_command(args.app, args.port, name=args.name))
    if cmd == "start":
        rest = args.rest
        if rest and rest[0] == "--":
            rest = rest[1:]
        if not rest:
            print("error: provide the command after `--`, e.g. start docs --port 9000 -- python -m http.server 9000")
            return 2
        cwd = Path(args.cwd).resolve() if args.cwd else Path.cwd()
        return do_start(args.name, rest, cwd, args.port)
    if cmd == "stop":
        return do_stop(args.name)
    if cmd == "restart":
        return do_restart(args.name)
    if cmd == "url":
        return do_url(args.name)
    if cmd == "logs":
        return do_logs(args.name, args.n)
    return do_status()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
