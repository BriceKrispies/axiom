# /// script
# requires-python = ">=3.11"
# dependencies = ["playwright>=1.48", "pillow>=10", "numpy>=1.26"]
# ///
"""**The shmup visual-parity instrument.**

One named shot, captured twice — once from the original browser FPS
(`apps/shmup`, Three.js r180) and once from the Rust/Axiom port
(`apps/axiom-shmup`) — under matched framing, matched resolution and, as far as
the port's hooks allow, a matched clock. It emits both PNGs, a diff image, and a
`report.json` carrying a **convergence number**.

    uv run scripts/parity_shot.py hero
    uv run scripts/parity_shot.py hero --out shots/parity --w 1280 --h 720 --settle 90

Why this exists
---------------
Every parity claim in this port is otherwise unfalsifiable. "The street looks
too grey" is a sentence, not a measurement, and two agents can hold opposite
opinions of the same screenshot indefinitely. Worse, the two most common ways
for a port to be wrong — *a different exposure* and *a different town* — look
identical to the eye and are trivially separable by numbers.

What it is NOT
--------------
It is not a pass/fail gate, and byte-equality is not the goal. The two images
come out of two different renderers (Three's deferred-ish forward stack with
TAA, GTAO, SSR and an auto-exposure meter, against Axiom's forward WebGPU/WebGL2
pass with a fitted exposure constant). They will never match to the byte, and a
harness that pretended otherwise would report "fail" forever and teach everyone
to ignore it. The output is a *distance*, tracked over time, decomposed far
enough that a change in lighting can be told apart from a change in geometry.

The provenance rule
-------------------
**A score whose pinning is unknown is worse than no score.** The port's console
hooks (`cam`, `freeze`, `dt`, `stats`) have to be wired into its browser frame
loop to take effect, and that wiring lives in a file this harness does not own.
So every run interrogates the port for which pins are actually *in force* — not
merely requested — and prints them beside the number:

    camera:      PINNED
    clock:       UNPINNED  (the port's frame loop never called frame_dt)

An UNPINNED axis does not stop the run. It annotates it.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import socket
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
from PIL import Image
from playwright.sync_api import sync_playwright

REPO = Path(__file__).resolve().parent.parent
SERVERS = REPO / "scripts" / "localhost_servers.py"
SHOTS_JS = REPO / "apps" / "shmup" / "src" / "dev" / "shots.js"
PROBE_JS = REPO / "apps" / "shmup" / "tools" / "probe.mjs"
LOOK_RS = REPO / "apps" / "axiom-shmup" / "src" / "scene" / "wiring" / "look.rs"


def port_hour() -> float | None:
    """The port's hour-of-day, read out of `look.rs`'s `HOUR` constant.

    The port has no `hour` command and no settable sky: its time of day is a
    source constant. So this axis cannot be *pinned* by the harness at all — the
    most it can do is read the constant and tell you whether it happens to equal
    the hour the shot asks for. `sunset` (19.2) and `night` (1.5) will never
    match a 16.5 constant, and a parity score for those two is measuring the
    time of day, not the port.

    Read from source rather than from the running binary because nothing in the
    binary publishes it. That is a real limitation and is labelled as one: the
    served wasm bundle could in principle be older than the source.
    """
    try:
        m = re.search(r"pub const HOUR:\s*f64\s*=\s*([\d.]+)", LOOK_RS.read_text(encoding="utf-8"))
    except OSError:
        return None
    return float(m.group(1)) if m else None

# The two apps, and the ports they are conventionally served on.
ORIGINAL_APP, ORIGINAL_PORT = "shmup", 8087
PORT_APP, PORT_PORT = "axiom-shmup", 8088

# Only these five shots are camera-and-lighting shots. The other six in
# `shots.js` (`weapon`, `ads`, `muzzle`, `combat`, `impacts`, `hud`) drive
# viewmodel / FX / HUD debug hooks the port has no equivalent for, so pointing
# this at them would compare the port's *absence* of a feature against the
# original's presence of it and call the difference a parity gap.
#
# Of these five, only the three at hour 16.5 (`hero`, `interior`, `detail`) can
# currently produce a number worth quoting: `sunset` (19.2) and `night` (1.5)
# need a time of day the port cannot be told about — its `look::HOUR` is a
# source constant with no console command behind it. The harness runs them and
# reports `time_of_day: DIVERGENT` rather than refusing, because the images are
# still worth looking at; the *number* is not.
PARITY_SHOTS = ("hero", "interior", "detail", "sunset", "night")

# Chromium flags. `--use-angle=gl` matters most: `metal` is macOS-only and its
# silent fallback on Windows is SwiftShader, a software rasterizer whose pixels
# are simply different from a GPU's. A reference captured on SwiftShader
# compares nothing. (This is why `apps/shmup/tools/capture.mjs`, which hardcodes
# `metal`, must not be used here.)
CHROME_ARGS = [
    "--use-angle=gl",
    "--ignore-gpu-blocklist",
    "--force-color-profile=srgb",
    "--force-device-scale-factor=1",
    "--hide-scrollbars",
    "--mute-audio",
    "--disable-frame-rate-limit",
]

# A channel delta at or below this is called unchanged. Two GPU passes over the
# same geometry disagree in the last bit or two of an 8-bit channel for reasons
# that are not parity defects (dither, rounding in the tone map, the compositor).
DEFAULT_TOL = 2


# --------------------------------------------------------------------------
# servers
# --------------------------------------------------------------------------
def port_open(port: int) -> bool:
    with socket.socket() as s:
        s.settimeout(0.4)
        return s.connect_ex(("127.0.0.1", port)) == 0


def manager(*args: str, timeout: int = 300) -> str:
    try:
        out = subprocess.run(
            ["uv", "run", str(SERVERS), *args],
            cwd=REPO,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return f"<localhost_servers.py {' '.join(args)} timed out after {timeout}s>"
    except FileNotFoundError as exc:
        raise SystemExit(
            "`uv` is not on PATH, and this harness drives the server manager through it. "
            "Install uv, or start both servers yourself and re-run:\n"
            f"  uv run {SERVERS} start-app {ORIGINAL_APP} --port {ORIGINAL_PORT}\n"
            f"  uv run {SERVERS} start-app {PORT_APP} --port {PORT_PORT}"
        ) from exc
    return (out.stdout or "") + (out.stderr or "")


def resolve_server(app: str, preferred: int) -> tuple[str, bool]:
    """`(url, started_it)` for one app, starting it if it is not up.

    Goes through `localhost_servers.py` rather than launching a server here: it
    owns the detached daemon, the named registry and the per-server log, and a
    second way of starting the same app is how you end up measuring a stale
    bundle on a port you did not expect.
    """
    url = manager("url", app).strip().splitlines()
    hit = next((ln.strip() for ln in url if ln.strip().startswith("http")), None)
    if hit and port_open(int(hit.rstrip("/").rsplit(":", 1)[1])):
        return hit.rstrip("/"), False

    print(f"starting `{app}` (this builds it; a cold wasm release build is not quick)…",
          file=sys.stderr)
    started = manager("start-app", app, "--port", str(preferred), timeout=2400)
    for _ in range(240):
        url = manager("url", app).strip().splitlines()
        hit = next((ln.strip() for ln in url if ln.strip().startswith("http")), None)
        if hit and port_open(int(hit.rstrip("/").rsplit(":", 1)[1])):
            return hit.rstrip("/"), True
        time.sleep(1.0)
    raise SystemExit(
        f"could not start `{app}`.\n"
        f"  manager said: {started.strip()[-600:]}\n"
        f"  full log:     uv run {SERVERS} logs {app} -n 60"
    )


# --------------------------------------------------------------------------
# the region table, read out of probe.mjs
# --------------------------------------------------------------------------
def shot_regions(shot: str) -> tuple[dict[str, list[float]], str]:
    """`SHOT_REGIONS[shot]` from `apps/shmup/tools/probe.mjs`.

    Parsed out of the original's own tool rather than copied here, for the same
    reason the shot definition is read from the running page: two copies of a
    calibration table drift, and the copy that drifts is always the one the
    measurement used.
    """
    if not PROBE_JS.exists():
        return {}, f"probe.mjs not found at {PROBE_JS}"
    src = PROBE_JS.read_text(encoding="utf-8")

    def entries(block: str) -> dict[str, list[float]]:
        return {
            m.group(1): [float(v) for v in m.group(2).split(",")]
            for m in re.finditer(r"(\w+)\s*:\s*\[([\d.,\s-]+)\]", block)
        }

    common_m = re.search(r"const COMMON\s*=\s*\{(.*?)\n\};", src, re.S)
    common = entries(common_m.group(1)) if common_m else {}

    table_m = re.search(r"const SHOT_REGIONS\s*=\s*\{(.*?)\n\};", src, re.S)
    if not table_m:
        return {}, "could not locate SHOT_REGIONS in probe.mjs"
    body = table_m.group(1)
    block = re.search(rf"\n  {re.escape(shot)}:\s*\{{(.*?)\n  \}},", body, re.S)
    if not block:
        return {}, f"probe.mjs defines no regions for shot `{shot}`"
    regions = dict(common) if "...COMMON" in block.group(1) else {}
    regions.update(entries(block.group(1)))
    return regions, ""


# --------------------------------------------------------------------------
# camera maths
# --------------------------------------------------------------------------
def yaw_pitch(pos: list[float], look: list[float]) -> tuple[float, float]:
    """Euler yaw/pitch (radians, `YXZ`) for a camera at `pos` looking at `look`.

    The original poses its shots with `THREE.Camera.lookAt`, and the port's
    `write_camera` composes `Ry(yaw) * Rx(pitch) * Rz(roll)` — Three's `'YXZ'`,
    which `engine.js:30` sets explicitly. Both look down local `-Z`, so

        forward = (-sin(yaw)cos(pitch), sin(pitch), -cos(yaw)cos(pitch))

    which inverts to the two lines below. (`player/system.rs:1505` builds the
    port's own forward vector as `[-sin(yaw), 0, -cos(yaw)]` — the same
    convention, independently, which is the check that this is not guesswork.)
    """
    dx, dy, dz = (look[i] - pos[i] for i in range(3))
    n = math.sqrt(dx * dx + dy * dy + dz * dz) or 1.0
    dx, dy, dz = dx / n, dy / n, dz / n
    return math.atan2(-dx, -dz), math.asin(max(-1.0, min(1.0, dy)))


# --------------------------------------------------------------------------
# the two legs
# --------------------------------------------------------------------------
WORLD_LINE = re.compile(
    r"built in ([\d.]+)\s*ms.*?([\d.]+)k static tris,\s*([\d.]+)k instanced tris in "
    r"(\d+) instances,\s*(\d+) draw calls,\s*([\d.]+)k collision tris"
)


def parse_world_census(logs: list[str]) -> dict | None:
    """The original's own `[world] built …` console line, as numbers.

    **This is the cross-app counterpart to the port's `stats`.** The original
    prints, at boot:

        [world] built in 1733ms - 586k static tris, 115k instanced tris in
        308 instances, 62 draw calls, 37.8k collision tris

    which is exactly the census `__ax_console("stats")` answers with on the port
    side. Putting the two in one report is what lets the *town* be compared
    numerically — the check that catches a level-seed divergence before anybody
    argues about a screenshot. Observed on this repo on 2026-08-28.
    """
    for line in logs:
        m = WORLD_LINE.search(line)
        if m:
            return {
                "build_ms": float(m.group(1)),
                "static_tris": float(m.group(2)) * 1000,
                "instanced_tris": float(m.group(3)) * 1000,
                "instances": int(m.group(4)),
                "draw_calls": int(m.group(5)),
                "collision_tris": float(m.group(6)) * 1000,
                "source": "[world] built console line",
            }
    return None


def capture_original(
    browser, url: str, shot: str, w: int, h: int, settle: int, out: Path,
    extra_query: str = "", ready_timeout: int = 180_000,
):
    """The original, through its own capture harness.

    Mirrors `apps/shmup/tools/baseline.mjs` exactly: a fresh page,
    `?capture=1&lockstep=1&shot=NAME`, the temporal-history drop, `__PUMP__(n)`
    for a fixed frame budget, then `__PRESENT__(2)` so the compositor has
    certainly picked the last rendered frame up before the shutter. Every one of
    those steps exists because leaving it out made two identical runs differ.
    """
    page = browser.new_page(viewport={"width": w, "height": h}, device_scale_factor=1)
    logs: list[str] = []
    page.on("console", lambda m: logs.append(f"[{m.type}] {m.text}"))
    page.on("pageerror", lambda e: logs.append(f"[pageerror] {e}"))
    info = {"logs": logs}
    try:
        page.goto(
            f"{url}/?capture=1&lockstep=1&shot={shot}{extra_query}",
            wait_until="domcontentloaded",
            timeout=90_000,
        )
        try:
            page.wait_for_function("window.__READY__ === true", timeout=ready_timeout)
        except Exception as exc:  # noqa: BLE001
            # **Fail loudly, and say what is actually stuck.** `__READY__` is
            # raised by `apps/shmup/src/main.js` only after `await
            # startPrewarm()` resolves and `__PUMP__(3)` has run. Observed on
            # this box on 2026-08-28: boot logs `[boot] prewarm.scene` and never
            # logs `[boot] prewarm`, so the stall is inside `prewarm()` — its
            # shader pre-compilation — not in the pump. `__PUMP__(1)` returned
            # normally when called by hand at that point, which is what pins the
            # blame on the prewarm await.
            #
            # `--query prewarm=0` is the original harness's own documented lever
            # for exactly this (`baseline.mjs --query=prewarm=0`) and skips only
            # program pre-compilation, which changes no pixels once the settle
            # frames have run.
            stalled = "prewarm" if any("prewarm.scene" in ln for ln in logs) else "boot"
            raise RuntimeError(
                f"the original never raised __READY__ within {ready_timeout / 1000:.0f}s. "
                f"Last boot stage reached: {stalled}. "
                + (
                    "The shader pre-warm did not resolve. Re-run with "
                    "`--query prewarm=0` — that is the original's own documented "
                    "lever for this and skips only program pre-compilation. "
                    if stalled == "prewarm"
                    else "The page did not finish booting at all — check the server log: "
                    f"uv run {SERVERS} logs {ORIGINAL_APP}. "
                )
                + "Page console tail: "
                + " | ".join(logs[-6:])
            ) from exc

        # The shot table, read off the RUNNING page. `shots.js` is the parity
        # vocabulary and stays its single definition; a copy in this file would
        # be a second one, and the second one is always the stale one.
        definition = page.evaluate(f'window.__SHOTS__["{shot}"] ?? null')
        if definition is None:
            raise RuntimeError(f"the original defines no shot `{shot}`")
        info["shot"] = definition

        applied = page.evaluate(
            "([s, n]) => window.__APPLY_SHOT__(s, { grabFrame: n })", [shot, settle]
        )
        info["applied"] = applied
        page.evaluate(
            "() => { const r = window.__ENGINE__?.ctx?.peek?.('render');"
            "        r?.resetTemporal?.() ?? r?.resetHistory?.() ?? r?.invalidateHistory?.(); }"
        )
        page.evaluate("(n) => window.__PUMP__(n)", settle)
        page.evaluate("() => window.__PRESENT__(2)")
        page.screenshot(path=str(out), type="png")
        info["render_info"] = page.evaluate("window.__RENDER_INFO__ ?? null")
        info["ok"] = True
    except Exception as exc:  # noqa: BLE001 — a leg that fails must still report
        info["ok"] = False
        info["error"] = f"{type(exc).__name__}: {exc}"
    finally:
        page.close()
    info["errors"] = [ln for ln in logs if "pageerror" in ln or "[error]" in ln]
    info["world_census"] = parse_world_census(logs)
    return info


def capture_port(
    browser, url: str, shot: dict, w: int, h: int, settle: int, out: Path,
    ready_timeout: float = 90.0,
):
    """The port, through `window.__ax_console`.

    There is no `__APPLY_SHOT__` and no lockstep pump on this side; the rAF loop
    belongs to `axiom_windowing::run_web_multi_skinned` and the app cannot take
    it over. So the pins are installed as console commands and then **verified**
    by reading `stats` back: `applied=yes` means a frame really went through the
    scripted camera, `dt_used=` means a frame really advanced by the pinned
    delta, and the `lock` line reports the live `Input::frozen` the game saw.

    Anything that comes back unverified is reported as UNPINNED. It is not
    silently assumed to have worked.
    """
    page = browser.new_page(viewport={"width": w, "height": h}, device_scale_factor=1)
    logs: list[str] = []
    page.on("console", lambda m: logs.append(f"[{m.type}] {m.text}"))
    page.on("pageerror", lambda e: logs.append(f"[pageerror] {e}"))
    info: dict = {"logs": logs, "console": {}, "notes": []}

    def cmd(text: str) -> str:
        try:
            return page.evaluate("(c) => window.__ax_console(c)", text) or ""
        except Exception as exc:  # noqa: BLE001
            return f"<{type(exc).__name__}: {exc}>"

    def note(text: str) -> None:
        if text not in info["notes"]:
            info["notes"].append(text)

    def require(text: str, verb: str, expect: str) -> str:
        """Run a console command and **check it was understood**.

        Two things go wrong here and both are silent. An older wasm bundle
        answers *every* unknown command with the help text — and `axiom-serve`
        keeps serving the last good bundle when a build fails, so a stale bundle
        is the normal failure, not an exotic one. And a malformed argument gets
        a `cam: expected …` complaint that also is not an acknowledgement.

        Either way, without this check the harness would issue `cam …`, get
        prose back, photograph the game's *own* camera and report a parity
        score — the exact "score of unknown provenance" this whole file exists
        to prevent. So a reply that does not carry `expect` is recorded as a
        note, and the pin audit downstream will call the axis UNPINNED.
        """
        reply = cmd(text)
        if expect in reply:
            return reply
        if reply.startswith("commands:"):
            note(
                f"the served bundle does not know `{verb}` — it answered with the help "
                f"text, so the wasm build is stale or the console hook was never added. "
                f"Check: uv run {SERVERS} logs {PORT_APP} -n 60"
            )
        else:
            note(f"`{verb}` was not acknowledged. Sent {text!r}, got {reply!r}")
        return reply

    try:
        # `?backend=webgl2` puts the port on the same rasterizer family as the
        # original (Three is WebGL2). Comparing a WebGPU frame against a WebGL2
        # one adds a whole second axis of difference to a measurement whose
        # entire purpose is to have exactly one.
        page.goto(f"{url}/?backend=webgl2", wait_until="domcontentloaded", timeout=90_000)
        page.wait_for_function("typeof window.__ax_console === 'function'", timeout=120_000)

        # `__ax_console` is installed BEFORE the GPU binds, so its existence is
        # not a ready signal. Three things might be: `__READY__` (the hook this
        # harness asks for), a non-zero frame count in `stats`, or — failing
        # both — the wall clock, which is a guess and is reported as one.
        ready = "none"
        deadline = time.time() + ready_timeout
        while time.time() < deadline:
            if page.evaluate("window.__READY__ === true"):
                ready = "__READY__"
                break
            reply = cmd("stats")
            if re.search(r"frame tick=", reply):
                ready = "stats-frame-count"
                break
            if reply.startswith("commands:"):
                # The bundle has no `stats` at all, so no amount of waiting will
                # produce a frame count. Say so at once rather than burning the
                # whole timeout on a signal that cannot arrive.
                note(
                    "the served bundle has no `stats` command, so there is no readiness "
                    "signal and no frame counter on this side. The wasm build is stale "
                    "or the console hooks were never wired into scene/boot.rs. "
                    f"Check: uv run {SERVERS} logs {PORT_APP} -n 60"
                )
                break
            time.sleep(0.25)
        if ready == "none":
            note(
                "the port signalled readiness by neither __READY__ nor a frame count; "
                f"waited {ready_timeout:.0f}s and then took the shot on the wall clock, "
                "so nothing guarantees a frame had been presented"
            )
            time.sleep(6.0)
        info["ready_signal"] = ready

        info["console"]["freeze"] = require("freeze on", "freeze", "freeze on")
        info["console"]["dt"] = require(f"dt {1.0 / 60.0:.10f}", "dt", "per frame")
        yaw, pitch = yaw_pitch(shot["pos"], shot["look"])
        fov = shot.get("fov")
        cam = "cam " + " ".join(
            f"{v:.6f}" for v in [*shot["pos"], yaw, pitch] + ([fov] if fov else [])
        )
        info["console"]["cam"] = require(cam, "cam", "cam eye=")
        info["camera_request"] = {"pos": shot["pos"], "yaw": yaw, "pitch": pitch, "fov": fov}

        # Settle. With a frame counter the wait is in frames, which is what the
        # original's `__PUMP__(settle)` means; without one it degrades to the
        # wall-clock equivalent and says so.
        before = frame_count(cmd("stats"))
        if before is None:
            note(
                "no frame counter — settled on the wall clock, so the frame index at "
                "the shutter is not a constant across runs and anything phase-locked "
                "to it (particle cursors, spring phase) lands differently each time"
            )
            time.sleep(settle / 60.0 + 1.0)
        else:
            deadline = time.time() + 60
            while time.time() < deadline:
                now = frame_count(cmd("stats"))
                if now is not None and now - before >= settle:
                    break
                time.sleep(0.05)
            else:
                note(
                    f"the port did not advance {settle} frames within 60s "
                    f"(from {before}); the shot was taken anyway"
                )

        info["console"]["stats"] = require("stats", "stats", "level placements=")
        info["console"]["lock"] = cmd("lock")
        page.screenshot(path=str(out), type="png")
        info["ok"] = True
    except Exception as exc:  # noqa: BLE001
        info["ok"] = False
        info["error"] = f"{type(exc).__name__}: {exc}"
    finally:
        page.close()
    info["errors"] = [ln for ln in logs if "pageerror" in ln or "[error]" in ln]
    return info


def frame_count(stats: str) -> int | None:
    m = re.search(r"observed=(\d+)", stats)
    return int(m.group(1)) if m else None


# --------------------------------------------------------------------------
# what the run could and could not pin
# --------------------------------------------------------------------------
def audit_pins(port_info: dict, orig_info: dict, shot: dict, w: int, h: int) -> dict:
    """Per-axis provenance for the number this run is about to print.

    Each entry is `PINNED` / `UNPINNED` / `MATCHED` / `DIVERGENT` plus the
    evidence it was decided on, so the verdict can be argued with.
    """
    stats = port_info.get("console", {}).get("stats", "") or ""
    lock = port_info.get("console", {}).get("lock", "") or ""

    camera = (
        ("PINNED", "stats reports camera=override applied=yes")
        if "camera=override applied=yes" in stats
        else ("UNPINNED", f"stats camera field: {stats_field(stats, 'camera')!r} — "
                          "the frame loop never called DevConsole::resolve_camera")
    )
    dt_used = stats_field(stats, "dt_used")
    clock = (
        ("PINNED", f"stats reports dt_used={dt_used}")
        if dt_used not in (None, "UNOBSERVED")
        else ("UNPINNED", "stats reports dt_used=UNOBSERVED — the frame loop never "
                          "called DevConsole::frame_dt, so the port advances on the wall "
                          "clock and its frame index at the shutter is not a constant")
    )
    frozen = (
        ("PINNED", "the live Input reports frozen=true")
        if "frozen=true" in lock
        else ("UNPINNED", f"lock line: {lock!r}")
    )
    fingerprint = stats_field(stats, "fingerprint")
    placements = stats_field(stats, "placements")
    seed = (
        ("RECORDED", f"port level fingerprint={fingerprint} over {placements} placements. "
                     "The fingerprint is a run-to-run baseline for the PORT (it moves the "
                     "moment a prop is added, renamed or relocated). The cross-app check "
                     "is the `town` block, which puts the port's census beside the "
                     "original's own `[world] built …` boot line")
        if fingerprint
        else ("UNPINNED", "the port reported no level census — `stats` is unwired, or the "
                          "served bundle predates it")
    )
    orig_size = orig_info.get("render_info") or {}
    return {
        "camera": {"state": camera[0], "evidence": camera[1]},
        "clock": {"state": clock[0], "evidence": clock[1]},
        "input": {"state": frozen[0], "evidence": frozen[1]},
        "level_seed": {"state": seed[0], "evidence": seed[1]},
        "resolution": {
            "state": "MATCHED",
            "evidence": f"both legs rendered into a {w}x{h} viewport at device scale 1; "
                        "the port's backbuffer is hardcoded to 1280x720, so neither "
                        "image is resampled only at that size",
        },
        "time_of_day": time_of_day_axis(shot),
        "renderer": {
            "state": "FAMILY-MATCHED",
            "evidence": "original: Three.js r180 forward+TAA+GTAO+SSR+auto-exposure on "
                        "ANGLE/gl. port: Axiom forward pass on ?backend=webgl2 with a "
                        "fitted exposure constant. Same rasterizer family, different "
                        "renderer — byte-equality is ruled out by construction",
        },
        "original_frame_index": {
            "state": "PINNED",
            "evidence": f"lockstep __PUMP__ + __PRESENT__(2); __RENDER_INFO__ frame="
                        f"{orig_size.get('frame')}",
        },
    }


def time_of_day_axis(shot: dict) -> dict:
    """Time of day — the axis the port cannot be told about at all.

    Never reported as PINNED, because nothing pins it: the harness sets the
    original's hour through `__APPLY_SHOT__` and can only *read* the port's,
    from `look.rs`'s `HOUR` constant. When they agree it is a coincidence the
    harness has checked; when they do not, the whole score is measuring the
    time of day and must not be quoted as a parity number.
    """
    wanted, have = shot.get("time"), port_hour()
    if wanted is None:
        return {"state": "UNPINNED", "evidence": "the shot names no hour"}
    if have is None:
        return {
            "state": "UNPINNED",
            "evidence": f"the shot sets hour={wanted}; the port's HOUR could not be read "
                        f"from {LOOK_RS.name}",
        }
    same = abs(have - wanted) < 1e-6
    return {
        "state": "MATCHED-BY-COINCIDENCE" if same else "DIVERGENT",
        "evidence": f"the original's shot sets hour={wanted}; the port's "
                    f"`look::HOUR` is {have} (read from source — the port has no `hour` "
                    f"command and no settable sky)."
                    + ("" if same else " THESE DIFFER: this run is comparing two different "
                                       "times of day and its delta is not a parity number."),
    }


def town_comparison(orig: dict, port: dict) -> dict:
    """**The two towns, side by side, in numbers.**

    The original prints its census at boot (`[world] built …`); the port answers
    `stats` with the same shape. This is the comparison that does not need a
    pixel — and the one that separates the two failure modes a screenshot cannot
    tell apart: a port that draws the *right* town badly, and a port that draws
    a *different* town well. If `static_tris` or `instances` differ by a factor,
    stop looking at the grade and go and look at the level seed.

    Ratios only, no verdict. What counts as "close enough" between a Three
    scene-graph census and an Axiom draw-list census is a judgement neither this
    function nor a threshold constant should be making.
    """
    left = orig.get("world_census")
    stats = (port.get("console", {}) or {}).get("stats", "") or ""
    right = {
        "static_tris": to_num(stats_field(stats, "tris")),          # meshes line
        "drawn_tris": to_num(stats_field(stats, "tris", nth=2)),    # frame line
        "instances": to_num(stats_field(stats, "instances")),
        "draw_calls": to_num(stats_field(stats, "draws")),
        "placements": to_num(stats_field(stats, "placements")),
        "fingerprint": stats_field(stats, "fingerprint"),
    }
    if not left:
        return {
            "comparable": False,
            "reason": "the original's `[world] built …` line was not in the page console "
                      "(it is printed once at boot; a reused page will not have it)",
            "port": right,
        }
    if right["static_tris"] is None:
        return {
            "comparable": False,
            "reason": "the port reported no census — `stats` is unwired or the bundle is stale",
            "original": left,
        }
    ratio = lambda a, b: round(b / a, 4) if a and b else None  # noqa: E731
    return {
        "comparable": True,
        "original": left,
        "port": right,
        "ratios_port_over_original": {
            "static_tris": ratio(left["static_tris"], right["static_tris"]),
            "instances": ratio(left["instances"], right["instances"]),
            "draw_calls": ratio(left["draw_calls"], right["draw_calls"]),
        },
        "note": "the original counts Three scene-graph geometry; the port counts its "
                "uploaded mesh set and its submitted draw list. The two are the same "
                "world measured by two different instruments, so a ratio near 1.0 is "
                "the signal and an exact match is not expected.",
    }


def to_num(text: str | None) -> float | None:
    try:
        return float(text)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return None


def stats_field(stats: str, key: str, nth: int = 1) -> str | None:
    hits = re.findall(rf"\b{re.escape(key)}=(\S+)", stats or "")
    return hits[nth - 1] if len(hits) >= nth else None


# --------------------------------------------------------------------------
# scoring
# --------------------------------------------------------------------------
LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float64)


def region_stats(a: np.ndarray, b: np.ndarray, regions: dict) -> dict:
    """Per-named-region means for both sides.

    The point of the decomposition: a global mean delta cannot tell a lighting
    error from a geometry error. `sunlit` vs `shade` is the key/fill ratio,
    `skyHi` is the sky model alone with no geometry in it at all, and `street` /
    `fg` are the surfaces the grade acts on most visibly. A run where `skyHi`
    matches and `shade` does not is a fill-light problem; one where every region
    is off by the same multiplier is an exposure problem; one where the regions
    disagree in different directions is geometry.
    """
    h, w, _ = a.shape
    out = {}
    for name, (u0, v0, u1, v1) in regions.items():
        x0, x1 = int(u0 * w), max(int(u1 * w), int(u0 * w) + 1)
        y0, y1 = int(v0 * h), max(int(v1 * h), int(v0 * h) + 1)
        ra, rb = a[y0:y1, x0:x1], b[y0:y1, x0:x1]
        ma, mb = ra.reshape(-1, 3).mean(0), rb.reshape(-1, 3).mean(0)
        la, lb = float(ma @ LUMA), float(mb @ LUMA)
        out[name] = {
            "uv": [u0, v0, u1, v1],
            "original_rgb": [round(float(v), 2) for v in ma],
            "port_rgb": [round(float(v), 2) for v in mb],
            "original_luma": round(la, 3),
            "port_luma": round(lb, 3),
            "luma_ratio": round(lb / la, 4) if la > 1e-6 else None,
            "mean_delta": round(float(np.abs(ra - rb).max(axis=2).mean()), 3),
        }
    return out


def score(orig_png: Path, port_png: Path, diff_png: Path, regions: dict, tol: int) -> dict:
    a_img, b_img = Image.open(orig_png).convert("RGB"), Image.open(port_png).convert("RGB")
    if a_img.size != b_img.size:
        return {
            "comparable": False,
            "reason": f"size mismatch: original {a_img.size}, port {b_img.size}. "
                      "Resampling one to the other would invent pixels and the score "
                      "would measure the resampler.",
        }
    a = np.asarray(a_img, dtype=np.float64)
    b = np.asarray(b_img, dtype=np.float64)
    delta = np.abs(a - b).max(axis=2)
    changed = delta > tol
    la = a @ LUMA
    lb = b @ LUMA

    # The diff image: the original dimmed to a quarter, magenta wherever the two
    # disagree by more than `tol`. Dimmed rather than blanked so the magenta can
    # be read against the thing it is complaining about.
    canvas = (a * 0.25).astype(np.uint8)
    canvas[changed] = np.array([255, 0, 255], dtype=np.uint8)
    Image.fromarray(canvas).save(diff_png)

    mean_original = float(la.mean())
    mean_port = float(lb.mean())
    return {
        "comparable": True,
        "size": list(a_img.size),
        "tolerance": tol,
        "meanDelta": round(float(delta.mean()), 4),
        "changedPct": round(float(changed.mean() * 100.0), 3),
        "maxDelta": int(delta.max()),
        "p95Delta": round(float(np.percentile(delta, 95)), 3),
        "meanLuma": {
            "original": round(mean_original, 4),
            "port": round(mean_port, 4),
            # The statistic the port's METERING_FIT constant was fitted against
            # (`scene/boot.rs`: 119 against the original's 95.7). A ratio far
            # from 1.0 is an exposure finding, not a geometry one, and it is the
            # first number to read because a mis-exposed frame makes every other
            # delta in this report large for one reason.
            "ratio": round(mean_port / mean_original, 4) if mean_original > 1e-6 else None,
        },
        "regions": region_stats(a, b, regions),
    }


# --------------------------------------------------------------------------
def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("shot", help=f"a shot from apps/shmup/src/dev/shots.js; parity set: {', '.join(PARITY_SHOTS)}")
    ap.add_argument("--out", default="shots/parity")
    ap.add_argument("--w", type=int, default=1280)
    ap.add_argument("--h", type=int, default=720)
    ap.add_argument("--settle", type=int, default=90)
    ap.add_argument("--tol", type=int, default=DEFAULT_TOL)
    ap.add_argument("--headed", action="store_true")
    ap.add_argument(
        "--query",
        default="",
        help="extra query string appended to the ORIGINAL's URL, e.g. `--query prewarm=0` "
             "(the same lever as baseline.mjs --query=). Use it if the original stalls "
             "before __READY__ during shader pre-warm.",
    )
    ap.add_argument(
        "--ready-timeout",
        type=int,
        default=180,
        help="seconds to wait for each app to signal it has rendered a frame (default 180)",
    )
    args = ap.parse_args()

    if args.shot not in PARITY_SHOTS:
        print(
            f"note: `{args.shot}` is outside the parity set {PARITY_SHOTS}. The other "
            "shots drive viewmodel/FX/HUD debug hooks the port has no equivalent for, "
            "so the difference they measure is a missing feature, not a parity gap.",
            file=sys.stderr,
        )

    out = (REPO / args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    orig_png, port_png = out / f"{args.shot}.original.png", out / f"{args.shot}.port.png"
    diff_png = out / f"{args.shot}.diff.png"
    report_json = out / f"{args.shot}.report.json"
    # Delete last run's images FIRST. A leg that fails leaves its PNG unwritten,
    # and scoring the previous run's file against this run's would produce a
    # number nobody could trace to a capture — the exact failure this harness is
    # built to make impossible.
    for stale in (orig_png, port_png, diff_png, report_json):
        stale.unlink(missing_ok=True)

    original_url, _ = resolve_server(ORIGINAL_APP, ORIGINAL_PORT)
    port_url, _ = resolve_server(PORT_APP, PORT_PORT)
    print(f"original {original_url}   port {port_url}", file=sys.stderr)

    regions, region_note = shot_regions(args.shot)

    with sync_playwright() as pw:
        try:
            browser = pw.chromium.launch(headless=not args.headed, args=CHROME_ARGS)
        except Exception as exc:  # noqa: BLE001
            raise SystemExit(
                f"could not launch Chromium: {exc}\n"
                "If this is the first run on this machine, fetch the browser once:\n"
                "  uv run --with playwright python -m playwright install chromium"
            ) from exc
        try:
            orig = capture_original(
                browser, original_url, args.shot, args.w, args.h, args.settle, orig_png,
                extra_query=f"&{args.query}" if args.query else "",
                ready_timeout=args.ready_timeout * 1000,
            )
            shot_def = orig.get("shot")
            if not shot_def:
                # Loud, and with the diagnosis attached. There is no useful
                # partial run here: without the shot definition the port leg
                # would be photographing an arbitrary camera.
                print("\nORIGINAL LEG FAILED — no score can be produced.\n", file=sys.stderr)
                print(f"  {orig.get('error')}\n", file=sys.stderr)
                return 2
            port = capture_port(
                browser, port_url, shot_def, args.w, args.h, args.settle, port_png,
                ready_timeout=float(args.ready_timeout),
            )
        finally:
            browser.close()

    report = {
        "shot": args.shot,
        "definition": shot_def,
        "viewport": [args.w, args.h],
        "settle_frames": args.settle,
        "angle": "gl",
        "urls": {"original": original_url, "port": port_url},
        "original": {k: v for k, v in orig.items() if k != "logs"},
        "port": {k: v for k, v in port.items() if k != "logs"},
        "pins": audit_pins(port, orig, shot_def, args.w, args.h),
        "town": town_comparison(orig, port),
        "region_source": str(PROBE_JS.relative_to(REPO)) if regions else region_note,
        "images": {
            "original": str(orig_png.relative_to(REPO)),
            "port": str(port_png.relative_to(REPO)),
            "diff": str(diff_png.relative_to(REPO)),
        },
    }
    both = orig.get("ok") and port.get("ok") and orig_png.exists() and port_png.exists()
    report["score"] = (
        score(orig_png, port_png, diff_png, regions, args.tol)
        if both
        else {
            "comparable": False,
            "reason": "a leg did not complete, so there is nothing to compare. "
                      f"original ok={orig.get('ok')} ({orig.get('error')}); "
                      f"port ok={port.get('ok')} ({port.get('error')})",
        }
    )

    report_json.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2))

    # The human-readable summary. The pins come FIRST, deliberately: a reader
    # who sees the number before the provenance has already formed an opinion
    # about a measurement that may not mean anything.
    print("\n" + "=" * 70, file=sys.stderr)
    print(f"  {args.shot}  —  what this run could pin", file=sys.stderr)
    print("=" * 70, file=sys.stderr)
    for axis, v in report["pins"].items():
        print(f"  {axis:<22}{v['state']}", file=sys.stderr)
    town = report["town"]
    if town.get("comparable"):
        r = town["ratios_port_over_original"]
        print(
            f"\n  town (port / original)   static_tris {r['static_tris']}   "
            f"instances {r['instances']}   draw_calls {r['draw_calls']}",
            file=sys.stderr,
        )
    else:
        print(f"\n  town NOT COMPARABLE: {town.get('reason')}", file=sys.stderr)

    s = report["score"]
    if s.get("comparable"):
        print(
            f"\n  meanDelta {s['meanDelta']}/255   changedPct {s['changedPct']}%   "
            f"maxDelta {s['maxDelta']}\n"
            f"  meanLuma  original {s['meanLuma']['original']}  port "
            f"{s['meanLuma']['port']}  ratio {s['meanLuma']['ratio']}",
            file=sys.stderr,
        )
    else:
        print(f"\n  SCORE NOT COMPARABLE: {s.get('reason')}", file=sys.stderr)

    notes = port.get("notes", [])
    unpinned = [
        a for a, v in report["pins"].items() if v["state"] in ("UNPINNED", "DIVERGENT")
    ]
    for note in notes:
        print(f"  ! {note}", file=sys.stderr)
    if unpinned:
        print(
            "\n  READ THIS BEFORE QUOTING THE NUMBER: "
            + ", ".join(unpinned)
            + " could not be pinned, so the delta above mixes the parity gap with "
            "whatever those axes drifted by. See `pins` in the report for why each one "
            "failed.",
            file=sys.stderr,
        )
    print(f"\n  {report_json}\n  {diff_png}", file=sys.stderr)
    return 0 if (orig.get("ok") and port.get("ok")) else 1


if __name__ == "__main__":
    raise SystemExit(main())
