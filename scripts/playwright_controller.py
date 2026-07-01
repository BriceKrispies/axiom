# /// script
# requires-python = ">=3.10"
# dependencies = ["playwright>=1.48"]
# ///
"""Axiom Playwright controller — a persistent, self-healing browser you drive
with short commands.

The first command spins up a background daemon that holds ONE real browser open.
Every later command reconnects to that same daemon instead of paying browser
startup again. If the browser (or the daemon) has died, the next command detects
it and brings it back up before running. Every command is appended to a log
file.

Run it with uv (it installs its own deps and, on first launch, the Chromium
binary):

    uv run scripts/playwright_controller.py status
    uv run scripts/playwright_controller.py goto http://localhost:8080/
    uv run scripts/playwright_controller.py wait 1500
    uv run scripts/playwright_controller.py screenshot cubes
    uv run scripts/playwright_controller.py eval "document.title"
    uv run scripts/playwright_controller.py console
    uv run scripts/playwright_controller.py stop      # shut the browser down

Env knobs: AXIOM_PW_PORT (default 8787), AXIOM_PW_HEADLESS=0 for a visible
window.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

HOST = "127.0.0.1"
PORT = int(os.environ.get("AXIOM_PW_PORT", "8787"))
HEADLESS = os.environ.get("AXIOM_PW_HEADLESS", "1") != "0"


def _viewport():
    """Optional fixed viewport from AXIOM_PW_VIEWPORT="WxH" (e.g. mobile 390x844)."""
    raw = os.environ.get("AXIOM_PW_VIEWPORT", "").lower().split("x")
    if len(raw) == 2 and raw[0].isdigit() and raw[1].isdigit():
        return {"width": int(raw[0]), "height": int(raw[1])}
    return None


VIEWPORT = _viewport()

STATE_DIR = Path(__file__).resolve().parent / ".playwright-controller"
LOG_FILE = STATE_DIR / "commands.log"
DAEMON_LOG = STATE_DIR / "daemon.log"
SCREENSHOT_DIR = STATE_DIR / "screenshots"

# Chromium flags that make WebGPU/WebGL usable (the rotating-cube slice needs a
# real GPU adapter). Harmless on pages that do not use the GPU.
BROWSER_ARGS = [
    "--enable-unsafe-webgpu",
    "--enable-features=Vulkan",
    "--use-gl=angle",
    # Expose real local IPs in WebRTC ICE (default Chromium hides them behind
    # mDNS .local names, which a same-host native peer can't resolve on loopback).
    "--disable-features=WebRtcHideLocalIpsWithMdns",
]


def _now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def log(line: str) -> None:
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    with LOG_FILE.open("a", encoding="utf-8") as fh:
        fh.write(f"{_now()} {line}\n")


# Daemon: owns the Playwright browser and answers commands over local HTTP.
# Single-threaded on purpose — Playwright's sync API must be used from the one
# thread that created it, and serializing commands matches one browser/one page.
class Daemon:
    def __init__(self) -> None:
        self._pw = None
        self.browser = None
        self.context = None
        self.page = None
        self.console: list[dict] = []
        self.should_stop = False

    def _start_playwright(self):
        from playwright.sync_api import sync_playwright

        if self._pw is None:
            self._pw = sync_playwright().start()
        return self._pw

    def _launch_browser(self):
        from playwright.sync_api import Error

        pw = self._start_playwright()
        try:
            return pw.chromium.launch(headless=HEADLESS, args=BROWSER_ARGS)
        except Error as exc:
            # First run: the Chromium binary is not installed yet.
            if "install" in str(exc).lower() or "executable" in str(exc).lower():
                log("daemon: installing chromium")
                subprocess.run(
                    [sys.executable, "-m", "playwright", "install", "chromium"],
                    check=True,
                )
                return pw.chromium.launch(headless=HEADLESS, args=BROWSER_ARGS)
            raise

    def ensure_browser(self) -> None:
        """Bring the browser/page up if it is missing or has crashed."""
        if self.browser is not None and self.browser.is_connected():
            return
        log("daemon: (re)launching browser")
        try:
            if self.browser is not None:
                self.browser.close()
        except Exception:
            pass
        self.browser = self._launch_browser()
        self.context = self.browser.new_context(
            **({"viewport": VIEWPORT} if VIEWPORT else {})
        )
        self.page = self.context.new_page()
        self.console = []
        self.page.on(
            "console",
            lambda m: self.console.append({"type": m.type, "text": m.text}),
        )
        self.page.on(
            "pageerror",
            lambda e: self.console.append({"type": "pageerror", "text": str(e)}),
        )

    def handle(self, action: str, args: list[str]) -> dict:
        if action == "stop":
            self.should_stop = True
            return {"ok": True, "stopped": True}
        if action == "ping":
            return {"ok": True, "pid": os.getpid()}

        self.ensure_browser()

        if action == "status":
            return {
                "ok": True,
                "connected": bool(self.browser and self.browser.is_connected()),
                "url": self.page.url,
                "title": self.page.title(),
                "headless": HEADLESS,
                "pid": os.getpid(),
            }
        if action == "goto":
            if not args:
                return {"ok": False, "error": "goto needs a URL"}
            self.console = []
            resp = self.page.goto(args[0], wait_until="load", timeout=30000)
            return {
                "ok": True,
                "url": self.page.url,
                "title": self.page.title(),
                "http_status": resp.status if resp else None,
            }
        if action == "reload":
            self.console = []
            self.page.reload(wait_until="load", timeout=30000)
            return {"ok": True, "url": self.page.url, "title": self.page.title()}
        if action == "wait":
            ms = int(args[0]) if args else 1000
            self.page.wait_for_timeout(ms)
            return {"ok": True, "waited_ms": ms}
        if action == "screenshot":
            SCREENSHOT_DIR.mkdir(parents=True, exist_ok=True)
            name = args[0] if args else "shot"
            stamp = datetime.now().strftime("%H%M%S")
            path = SCREENSHOT_DIR / f"{name}-{stamp}.png"
            self.page.screenshot(path=str(path), full_page=False)
            return {"ok": True, "path": str(path)}
        if action == "eval":
            if not args:
                return {"ok": False, "error": "eval needs a JS expression"}
            result = self.page.evaluate(" ".join(args))
            return {"ok": True, "result": result}
        if action == "console":
            return {"ok": True, "messages": self.console}

        return {"ok": False, "error": f"unknown action: {action}"}

    def serve(self) -> None:
        STATE_DIR.mkdir(parents=True, exist_ok=True)
        daemon = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a):  # silence default stderr noise
                pass

            def do_POST(self):
                length = int(self.headers.get("Content-Length", "0"))
                payload = json.loads(self.rfile.read(length) or b"{}")
                action = payload.get("action", "")
                args = payload.get("args", [])
                log(f"recv {action} {' '.join(map(str, args))}".rstrip())
                try:
                    body = daemon.handle(action, args)
                except Exception as exc:  # never let one bad command kill us
                    body = {"ok": False, "error": f"{type(exc).__name__}: {exc}"}
                    log(f"error {action}: {exc}")
                data = json.dumps(body).encode("utf-8")
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        server = HTTPServer((HOST, PORT), Handler)
        log(f"daemon: up on {HOST}:{PORT} (pid {os.getpid()}, headless={HEADLESS})")
        while not self.should_stop:
            server.handle_request()
        log("daemon: stopping")
        try:
            if self.browser is not None:
                self.browser.close()
            if self._pw is not None:
                self._pw.stop()
        except Exception:
            pass
        server.server_close()


# Client: ensures the daemon is up (starting it if not), then sends one command.
def _post(action: str, args: list[str], timeout: float = 60.0) -> dict | None:
    req = urllib.request.Request(
        f"http://{HOST}:{PORT}/",
        data=json.dumps({"action": action, "args": args}).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read())
    except (urllib.error.URLError, ConnectionError, OSError):
        return None


def _spawn_daemon() -> None:
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    out = DAEMON_LOG.open("a", encoding="utf-8")
    kwargs: dict = {"stdout": out, "stderr": out, "stdin": subprocess.DEVNULL}
    if os.name == "nt":
        kwargs["creationflags"] = (
            subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP
        )
    else:
        kwargs["start_new_session"] = True
    subprocess.Popen([sys.executable, str(Path(__file__).resolve()), "--daemon"], **kwargs)


def ensure_daemon() -> None:
    if _post("ping", [], timeout=2.0) is not None:
        return
    log("client: daemon not responding — starting it")
    _spawn_daemon()
    # Wait for the daemon to come up (first run also installs Chromium).
    deadline = time.time() + 180
    while time.time() < deadline:
        time.sleep(1.0)
        if _post("ping", [], timeout=2.0) is not None:
            return
    raise SystemExit("error: daemon did not come up in time (see daemon.log)")


def main(argv: list[str]) -> int:
    if argv and argv[0] == "--daemon":
        Daemon().serve()
        return 0

    if not argv:
        print(__doc__)
        return 2

    action, args = argv[0], argv[1:]
    log(f"send {action} {' '.join(args)}".rstrip())

    # `stop` should not resurrect a dead daemon just to kill it.
    if action != "stop":
        ensure_daemon()

    resp = _post(action, args)
    if resp is None:
        print(json.dumps({"ok": False, "error": "daemon unreachable"}))
        return 1
    print(json.dumps(resp, indent=2))
    return 0 if resp.get("ok") else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
