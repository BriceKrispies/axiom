"""The gallery demos the smoke suite drives — its single source of truth.

Repo tooling (alongside e2e/conftest.py + test_smoke.py), NOT part of the engine
dependency graph. A SUBSET of the gallery's registered apps (each app's own
apps/<app>/app.json, collected into dist/manifest.json at package time), plus the
per-demo signals a browser test needs and the manifest deliberately does not
carry: how the demo proves it loaded, its canvas, and whether that canvas paints
on entry. Kept as its own list because those signals are test concerns, not
gallery metadata — adding an app to the gallery does not oblige it to be smoke-
tested here.

`kind` selects the ready-signal + render-proof strategy in test_smoke.py:
  * windowing3d  — boots the engine run-loop; logs `axiom: render backend = …`.
                   With ?backend=canvas2d it must read `… = Canvas2d`.
  * canvas2d_app — a pure 2D-canvas game (no WebGPU); logs `[<id>] ready`.
                   ?backend=canvas2d is a harmless no-op (still loaded to prove it).
  * growth       — multi-screen; only its entry screen is smoke-tested (status text).
                   Its canvas is hidden until "Generate", so it gets no canvas check.
"""

# id -> demo spec. `path` is relative to the gallery root (dist/). Every demo is a
# standalone app packaged into dist/<dir>/ and self-hosts its own <dir>/index.html
# (there is no shared demo.html shell anymore).
DEMOS = [
    {"id": "rotating-cube", "kind": "windowing3d", "path": "rotating-cube/index.html", "canvas": "#axiom-cube-canvas"},
    {"id": "growth", "kind": "growth", "path": "growth/index.html", "check_canvas": False},
    {"id": "zanzoban", "kind": "canvas2d_app", "path": "zanzoban/index.html",
     "canvas": "#axiom-puzzle-canvas", "ready_log": "[zanzoban] ready"},
    {"id": "quintet", "kind": "canvas2d_app", "path": "quintet/index.html",
     "canvas": "#axiom-quintet-canvas", "ready_log": "[quintet] ready"},
]


def with_backend(path: str, backend: str) -> str:
    """Append the engine's `backend=canvas2d` override (read by force_canvas2d())."""
    if backend != "canvas2d":
        return path
    sep = "&" if "?" in path else "?"
    return f"{path}{sep}backend=canvas2d"
