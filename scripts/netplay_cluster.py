"""Run a local SCALEOUT netplay cluster: one director + two game nodes.

The director (http://localhost:8100) serves the page and the matchmaker; it
redirects each browser to one of the two game nodes (8101 / 8102), which host the
authoritative rooms. Open http://localhost:8100 in two browser windows to see two
players matched into a room on whichever node the director picked.

Repo tooling, outside the engine dependency graph (same status as scripts/ and the
Makefile). Requires the .NET 10 SDK; build the worker first with
`cargo build -p axiom-netplay-ffi --release` and the wasm bundle with
`make netplay-build`. Ctrl+C stops the whole cluster.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SERVER_DIR = REPO_ROOT / "examples" / "axiom-netplay-dotnet"
WEB_ROOT = REPO_ROOT / "apps" / "axiom-netplay-browser" / "web"
DIRECTOR_PORT = 8100
NODE_PORTS = (8101, 8102)


def main() -> int:
    dotnet = shutil.which("dotnet")
    if dotnet is None:
        print("error: the .NET 10 SDK (`dotnet`) is required", file=sys.stderr)
        return 1

    subprocess.run(["dotnet", "build", "-c", "Debug", str(SERVER_DIR)], cwd=REPO_ROOT, check=True)
    dll = next((SERVER_DIR / "bin" / "Debug").glob("net*/axiom-netplay-dotnet.dll"))
    ffi = str(REPO_ROOT / "target" / "release" / "axiom_netplay_ffi.dll")

    def launch(port: int, extra: dict[str, str]) -> subprocess.Popen:
        env = {
            **os.environ,
            "ASPNETCORE_URLS": f"http://localhost:{port}",
            "AXIOM_WEB_ROOT": str(WEB_ROOT),
            "AXIOM_FFI_LIB": ffi,
            **extra,
        }
        return subprocess.Popen([dotnet, str(dll)], cwd=REPO_ROOT, env=env)

    procs: list[subprocess.Popen] = []
    try:
        procs.append(launch(DIRECTOR_PORT, {"AXIOM_ROLE": "director"}))
        for port in NODE_PORTS:
            procs.append(launch(port, {
                "AXIOM_ROLE": "node",
                "AXIOM_DIRECTOR_URL": f"http://localhost:{DIRECTOR_PORT}",
                "AXIOM_NODE_URL": f"ws://localhost:{port}/ws",
            }))
        print(f"\n  Scaleout cluster up: director http://localhost:{DIRECTOR_PORT}, nodes {list(NODE_PORTS)}.")
        print(f"  Open http://localhost:{DIRECTOR_PORT} in two browser windows. Ctrl+C to stop.\n")
        while all(p.poll() is None for p in procs):
            time.sleep(0.5)
        print("a cluster process exited; shutting down")
        return 1
    except KeyboardInterrupt:
        print("\nstopping cluster")
        return 0
    finally:
        for p in procs:
            p.terminate()
        for p in procs:
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()


if __name__ == "__main__":
    raise SystemExit(main())
