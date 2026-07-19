#!/usr/bin/env python3
"""Tiny static server for the Godot Web export.

Godot 4's threaded web build needs cross-origin isolation (SharedArrayBuffer), so
this sends COOP/COEP headers a plain `python -m http.server` does not. Usage:

    python serve_web.py [port] [dir]     # defaults: 8060 dist
"""
import http.server
import socketserver
import sys

import os

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8060
DIRECTORY = sys.argv[2] if len(sys.argv) > 2 else "dist"
HOST = os.environ.get("HOST", "0.0.0.0")  # 0.0.0.0 = reachable from phones on the LAN


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".wasm": "application/wasm",
        ".js": "text/javascript",
        ".pck": "application/octet-stream",
    }

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=DIRECTORY, **kwargs)

    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        self.send_header("Cross-Origin-Resource-Policy", "cross-origin")
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


socketserver.TCPServer.allow_reuse_address = True
with socketserver.TCPServer((HOST, PORT), Handler) as httpd:
    print(f"serving {DIRECTORY} on {HOST}:{PORT} (COOP/COEP enabled; LAN devices use http://<this-machine-ip>:{PORT}/)")
    httpd.serve_forever()
