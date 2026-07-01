"""The gallery demos the smoke suite drives — its single source of truth.

Repo tooling (alongside e2e/conftest.py + test_smoke.py), NOT part of the engine
dependency graph. Mirrors gallery/gallery.js's DEMOS, plus the per-demo signals a
browser test needs (which the manifest does not carry): how the demo proves it
loaded, its canvas, and whether that canvas paints on entry.

`kind` selects the ready-signal + render-proof strategy in test_smoke.py:
  * windowing3d  — boots the engine run-loop; logs `axiom: render backend = …`.
                   With ?backend=canvas2d it must read `… = Canvas2d`.
  * canvas2d_app — a pure 2D-canvas game (no WebGPU); logs `[<id>] ready`.
                   ?backend=canvas2d is a harmless no-op (still loaded to prove it).
  * growth       — multi-screen; only its entry screen is smoke-tested (status text).
                   Its canvas is hidden until "Generate", so it gets no canvas check.
  * harness      — the debug-overlay dev page; signals via #boot text.
"""

# id -> demo spec. `path` is relative to the gallery root (dist/). Shared-shell demos
# boot via demo.html?id=<id>; self-hosted demos own <dir>/index.html.
DEMOS = [
    {"id": "rotating-cube", "kind": "windowing3d", "path": "demo.html?id=rotating-cube", "canvas": "#axiom-cube-canvas"},
    {"id": "netplay", "skip": "multiplayer — needs a relay (needsRelay)"},
    {"id": "retro_fps", "kind": "windowing3d", "path": "demo.html?id=retro_fps", "canvas": "#axiom-retro-fps-canvas"},
    {"id": "stress-cubes", "kind": "windowing3d", "path": "demo.html?id=stress-cubes", "canvas": "#axiom-stress-canvas"},
    {"id": "growth", "kind": "growth", "path": "growth/index.html", "check_canvas": False},
    {"id": "zanzoban", "kind": "canvas2d_app", "path": "zanzoban/index.html",
     "canvas": "#axiom-puzzle-canvas", "ready_log": "[zanzoban] ready"},
    {"id": "quintet", "kind": "canvas2d_app", "path": "quintet/index.html",
     "canvas": "#axiom-quintet-canvas", "ready_log": "[quintet] ready"},
    {"id": "harness", "kind": "harness", "path": "harness/index.html", "canvas": "#axiom-harness-canvas"},
]


def with_backend(path: str, backend: str) -> str:
    """Append the engine's `backend=canvas2d` override (read by force_canvas2d())."""
    if backend != "canvas2d":
        return path
    sep = "&" if "?" in path else "?"
    return f"{path}{sep}backend=canvas2d"
