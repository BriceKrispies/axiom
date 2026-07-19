#!/usr/bin/env python3
"""Hot-reload dev server for the Godot Web export.

Watches the GDScript sources, re-exports the Web build on save with the standard
(non-mono) Godot, and live-reloads the browser over SSE -- the `axiom-serve` loop
for this Godot port. The .NET/mono Godot cannot export web, so point GODOT_WEB_BIN
(or the 3rd CLI arg) at a standard Godot 4.6 binary.

    GODOT_WEB_BIN=/path/to/godot python dev_web.py [port] [dir]

Open http://localhost:<port>/ and edit any script under scripts/ -- the build
re-exports and the page reloads on its own.
"""
import glob
import http.server
import os
import subprocess
import sys
import threading
import time

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8060
DIST = sys.argv[2] if len(sys.argv) > 2 else "dist"
HOST = os.environ.get("HOST", "0.0.0.0")  # 0.0.0.0 = reachable from phones on the LAN
GODOT = os.environ.get("GODOT_WEB_BIN") or (sys.argv[3] if len(sys.argv) > 3 else "godot")
ROOT = os.path.dirname(os.path.abspath(__file__))
WATCH = ["scripts/*.gd", "main.tscn", "project.godot", "export_presets.cfg"]
SNIPPET = "<script>/*livereload*/new EventSource('/events').onmessage=(e)=>{if(e.data==='reload')location.reload()};</script>"

_version = 0
_lock = threading.Lock()


def _snapshot() -> dict:
    files: list[str] = []
    for pat in WATCH:
        files += glob.glob(os.path.join(ROOT, pat))
    return {f: os.path.getmtime(f) for f in files if os.path.exists(f)}


def _inject_reload() -> None:
    idx = os.path.join(ROOT, DIST, "index.html")
    try:
        with open(idx, encoding="utf-8") as fh:
            html = fh.read()
    except OSError:
        return
    if "/*livereload*/" in html:
        return
    with open(idx, "w", encoding="utf-8") as fh:
        fh.write(html.replace("</body>", SNIPPET + "</body>"))


def _export() -> bool:
    print("[dev] exporting web build...", flush=True)
    r = subprocess.run(
        [GODOT, "--headless", "--path", ROOT, "--export-release", "Web", os.path.join(DIST, "index.html")],
        cwd=ROOT, capture_output=True, text=True)
    _inject_reload()
    ok = r.returncode == 0
    print("[dev] export " + ("ok" if ok else "FAILED\n" + (r.stderr or "")[-1200:]), flush=True)
    return ok


def _watch() -> None:
    global _version
    last = _snapshot()
    _export()
    with _lock:
        _version += 1
    while True:
        time.sleep(0.7)
        cur = _snapshot()
        if cur != last:
            time.sleep(0.4)  # debounce a burst of saves
            last = _snapshot()
            if _export():
                with _lock:
                    _version += 1


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".wasm": "application/wasm", ".js": "text/javascript", ".pck": "application/octet-stream",
    }

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=os.path.join(ROOT, DIST), **kwargs)

    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        self.send_header("Cross-Origin-Resource-Policy", "cross-origin")
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def do_GET(self):  # noqa: N802
        if self.path.split("?")[0] == "/events":
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            seen = _version
            try:
                self.wfile.write(b"retry: 1000\n\n")
                self.wfile.flush()
                while True:
                    time.sleep(0.5)
                    with _lock:
                        v = _version
                    if v != seen:
                        self.wfile.write(b"data: reload\n\n")
                        self.wfile.flush()
                        return
                    self.wfile.write(b": ping\n\n")
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                return
        return super().do_GET()

    def log_message(self, *args):  # quieter
        pass


def main() -> None:
    threading.Thread(target=_watch, daemon=True).start()
    http.server.ThreadingHTTPServer.allow_reuse_address = True
    with http.server.ThreadingHTTPServer((HOST, PORT), Handler) as httpd:
        print(f"[dev] hot-reload server on {HOST}:{PORT} (edit scripts/*.gd to reload; open from a LAN device at http://<this-machine-ip>:{PORT}/)", flush=True)
        httpd.serve_forever()


if __name__ == "__main__":
    main()
