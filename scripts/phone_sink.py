#!/usr/bin/env python3
"""A dev-time sink for reports posted by a phone on the LAN.

# Why this exists

A rendering fault that appears on one device and on no other is diagnosed by
asking that device questions. Until now the only channel was a person holding
the phone, loading a URL, screenshotting it and sending the picture back — one
scalar per round trip, through a lossy medium, at human latency. Worse, a
screenshot of a *probe* is a picture of a number that has been through a tone
map, which is how four "healthy white" readings turned out to mean nothing more
than "greater than about a tenth".

This is the other direction: the page POSTs a structured report and it lands on
disk here, where it can be read exactly. No rebuild is involved — the reporting
half lives in the app's `index.html` — so a new question costs an edit and a
reload rather than a wasm build.

# Deliberately not part of `axiom-serve`

The app server rebuilds on save and restarts; a diagnostic sink that restarted
with it would lose reports at exactly the moment the tree is being changed. It
is also a *write* endpoint, and axiom-serve is a static file server for a
checkout — keeping the two apart means nothing that serves the app can be made
to accept a POST by accident.

Cross-origin is handled by staying inside what a "simple request" allows: the
page posts `text/plain`, which needs no preflight, so this server answers CORS
without implementing it.

    uv run scripts/localhost_servers.py start phone-sink --port 8099 -- \\
        uv run scripts/phone_sink.py 8099

Reports land in `scripts/.phone-reports/` (git-ignored, like the other
dev-server state directories).
"""

from __future__ import annotations

import datetime
import pathlib
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

REPORTS = pathlib.Path(__file__).resolve().parent / ".phone-reports"

# A report is a text blob a phone chose to send. Cap it so a runaway page cannot
# fill the disk, and so a malformed body fails fast rather than streaming.
MAX_BODY_BYTES = 512 * 1024


class Sink(BaseHTTPRequestHandler):
    """One POST, one file. No routing beyond that on purpose."""

    # The default logs every request to stderr, which drowns the one line that
    # matters (the path a report landed at).
    def log_message(self, _format: str, *_args: object) -> None:
        return

    def _cors(self) -> None:
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Access-Control-Allow-Methods", "POST, OPTIONS")

    def do_OPTIONS(self) -> None:  # noqa: N802 — BaseHTTPRequestHandler's spelling
        self.send_response(204)
        self._cors()
        self.end_headers()

    def do_GET(self) -> None:  # noqa: N802
        """A liveness check, so the phone's owner can confirm reachability
        before wondering why nothing arrived."""
        body = b"axiom phone sink: ready. POST a report to /report\n"
        self.send_response(200)
        self._cors()
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length") or 0)
        if length <= 0 or length > MAX_BODY_BYTES:
            self.send_response(413)
            self._cors()
            self.end_headers()
            return
        body = self.rfile.read(length)
        REPORTS.mkdir(parents=True, exist_ok=True)
        stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S-%f")
        path = REPORTS / f"report-{stamp}.txt"
        path.write_bytes(body)
        # The one line worth printing: where to read what just arrived.
        print(f"[phone-sink] {len(body)} bytes -> {path}", flush=True)
        self.send_response(204)
        self._cors()
        self.end_headers()


def main() -> int:
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8099
    REPORTS.mkdir(parents=True, exist_ok=True)
    # 0.0.0.0: the whole point is that a different machine on the LAN reaches it.
    server = ThreadingHTTPServer(("0.0.0.0", port), Sink)
    print(f"[phone-sink] listening on 0.0.0.0:{port}, writing to {REPORTS}", flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
