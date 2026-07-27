"""The capability-ladder matrix: take something away from the browser, prove the
game still pays out.

The page under test implements ONE ladder, top to bottom::

    webgpu → webgl2 → webgl1 → canvas2d → css3d → form

The first five rungs are ``@axiom/web-engine``'s probed render tiers; ``form`` is
the served document itself — nine ``<button type="submit">`` inside a real
``<form method="POST">``, which needs no script and no stylesheet and is
therefore the one rung nothing can fail to reach.

Every row asserts TWO things, because either one alone is a lie:

  (a) which rung the page landed on — ``window.__axiomTier``, cross-checked
      against the engine's own report on ``window.__renderProbe``, and against
      whether a board actually MOUNTED at that rung;
  (b) that the pick still reached ``POST /api/pick`` and came back as a result
      the player can read.

A row that only checked (a) would pass on a page that detects its way down to
css3d and then does nothing. A row that only checked (b) would pass on a page
that ignored the denial entirely. The pair is the claim.

WHERE THE DENIALS COME FROM. There is no browser feature that removes a
rendering API from a document — measured, not assumed: no ``<iframe sandbox>``
token, no CSP directive and no Permissions-Policy feature covers WebGPU, WebGL2,
WebGL1, Canvas2D or CSS 3D. So the render rungs are masked in-realm by
``tools/axiom-harness/web/caps-mask.js``, loaded here VERBATIM through
``context.add_init_script`` — the same bytes the developer harness loads. Neither
side owns a copy, because a harness whose denial differs from the suite's denial
is proving a ladder nobody ships. The rung matrix itself
(``tools/axiom-harness/web/rungs.json``) is shared the same way.

TWO LAYERS FOR THE SAME RUNG, ON PURPOSE. ``webgl1`` and ``canvas2d`` are tested
BOTH through the mask and through real Chromium launch flags (``--disable-webgl2``
kills WebGL2 and leaves WebGL1 alive; ``--disable-3d-apis`` kills both — measured).
Testing one rung at two layers is nearly free, and a disagreement between them is
a real bug in the detection logic rather than a harness artifact. (``--disable-gpu``
is deliberately absent: it does NOT disable WebGL, it falls back to SwiftShader,
so using it to mean "no GPU" would quietly test nothing.)

WHAT THE MASK CANNOT DO, AND WHAT IS USED INSTEAD. Killing JavaScript is a real
browser capability, so the no-JS rungs do not go anywhere near the mask:
``java_script_enabled=False`` is the faithful "the user turned JS off" simulation
(``<noscript>`` renders), and a CSP ``script-src 'none'`` injected with
``page.route`` is a DIFFERENT rung with a different observable (``<noscript>``
does NOT render there, because scripting is enabled — it is merely forbidden).
Both are tested, and both must produce a NATIVE form POST.

MACHINE-INDEPENDENCE. Cross-cut rows do not hardcode a tier: they assert against
a baseline measured on the machine running the suite, so "blocking canvas
readback must not move the tier" means the same thing on a laptop with a GPU and
in a VDI session without one. The one rung that can genuinely be unavailable —
``webgpu`` — is SKIPPED with the probe's own explanation rather than weakened
into an assertion that would always pass.

Run with ``make e2e-ladder``. The suite starts its own server
(``tools/axiom-harness``, which embeds the real ``axiom-chest-server``), so it is
self-contained; set ``AXIOM_LADDER_REUSE=1`` to reuse one already listening.

Repo tooling: outside the engine dependency graph, the Coverage Law and the
Branchless Law.
"""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import pytest
from playwright.sync_api import Browser, BrowserContext, Page, Playwright, Request, expect

REPO_ROOT = Path(__file__).resolve().parent.parent
HARNESS_DIR = REPO_ROOT / "tools" / "axiom-harness"
MASK_JS = HARNESS_DIR / "web" / "caps-mask.js"
MATRIX = json.loads((HARNESS_DIR / "web" / "rungs.json").read_text(encoding="utf-8"))

PORT = int(os.environ.get("AXIOM_LADDER_PORT", "8091"))
# localhost, never a LAN address: `navigator.gpu` is secure-context gated, and on
# a plain-http non-localhost origin the webgpu rung would silently report absent —
# the suite would "pass" while measuring nothing.
BASE_URL = f"http://localhost:{PORT}"
GAME = "/resilient.html"
PICK_ENDPOINT = "/api/pick"
PICK = 4  # "Chest 5" — the centre chest, which also carries the nameplate.
BOARD_CANVAS = "#chest-board canvas"
# The engine's own budget is 2.5s; under a 20x CPU throttle everything after it
# is slow too, so waits are generous rather than tight.
SETTLE_MS = 20_000

RUNGS = MATRIX["rungs"]
CROSS_CUTS = MATRIX["crossCuts"]
LAUNCH_FLAGS = MATRIX["launchFlags"]

# Everything a row needs to judge a page, read in one round trip after the page
# has settled. `__renderProbe` is the engine's whole DetectionReport; `__axiomTier`
# is the rung the PAGE committed to, which can be lower (a backend that detected
# fine and then failed to construct).
INFO_JS = """
() => ({
  denied: window.__AXIOM_DENIED ?? null,
  drawn: document.querySelector('#chest-board canvas') !== null,
  maskErrors: window.__AXIOM_MASK_ERRORS ?? null,
  probeDetails: window.__renderProbe
    ? Object.fromEntries(Object.entries(window.__renderProbe.probes).map(([t, p]) => [t, p.detail]))
    : null,
  probeOutcomes: window.__renderProbe
    ? Object.fromEntries(Object.entries(window.__renderProbe.probes).map(([t, p]) => [t, p.outcome]))
    : null,
  probeSource: window.__renderProbe?.source ?? null,
  probeTier: window.__renderProbe?.tier ?? null,
  readback: window.__renderProbe?.readback ?? null,
  ready: window.__axiomReady === true,
  tier: window.__axiomTier ?? null,
})
"""

# THE BACKBONE. Nine `<button type="submit" name="pick" value="0..8">` inside a
# real `<form method="POST" action="/api/pick">`, present, enabled, laid out and
# visible. This is the single most valuable assertion in the suite, because it is
# the one thing every rung has in common: the form is never removed, disabled,
# rebuilt or replaced — the engine's canvas goes BEHIND the buttons and the CSS 3D
# chests are built INSIDE them. A rung that painted beautifully while quietly
# swapping the controls out for divs would pass every tier assertion and be
# unusable to the browser this build exists for.
BACKBONE_JS = """
() => {
  const form = document.getElementById('pick-form');
  const buttons = [...document.querySelectorAll('#pick-form button.resilient-chest')];
  const shown = (b) => (b.checkVisibility ? b.checkVisibility() : b.offsetParent !== null);
  return {
    action: form && form.getAttribute('action'),
    count: buttons.length,
    enabled: buttons.filter((b) => !b.disabled).length,
    laidOut: buttons.filter((b) => b.getBoundingClientRect().width > 4).length,
    method: form && (form.getAttribute('method') || '').toUpperCase(),
    names: [...new Set(buttons.map((b) => b.name))],
    submits: buttons.filter((b) => b.type === 'submit').length,
    values: buttons.map((b) => b.value).join(','),
    visible: buttons.filter(shown).length,
  };
}
"""

# Watch every tier the page PUBLISHES, not just the one it settles on. A
# transport that turns out to be lying is announced the same way the first claim
# was — and only there: by the time the native submit has navigated, the document
# that knew is gone. The binding hands each value straight to Python, so it
# survives the unload.
TIER_WATCH_JS = """
window.__axiomTierSeen = [];
const record = () => {
  const tier = document.documentElement.dataset.axiomTier;
  if (tier && window.__axiomTierSeen[window.__axiomTierSeen.length - 1] !== tier) {
    window.__axiomTierSeen.push(tier);
    try { window.__axiomTierEvent(tier); } catch (error) { /* binding not installed */ }
  }
};
new MutationObserver(record).observe(document, {
  attributeFilter: ["data-axiom-tier"],
  attributes: true,
  subtree: true,
});
"""


def _port_open(port: int) -> bool:
    with socket.socket() as sock:
        sock.settimeout(0.3)
        return sock.connect_ex(("127.0.0.1", port)) == 0


def _wait_http(url: str, timeout: float) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            urllib.request.urlopen(url, timeout=1)  # noqa: S310 (localhost dev server)
            return True
        except OSError:
            time.sleep(0.3)
    return False


@pytest.fixture(scope="session")
def ladder_base_url():
    """Serve the game, the API and the engine bundle from ONE origin.

    One origin is not a convenience: a native ``<form method="POST">`` has no
    CORS story at all — no preflight to grant, no header to add — so a
    cross-origin harness would delete the zero-JS rung rather than test it.
    ``tools/axiom-harness`` embeds the real ``axiom-chest-server``, so nothing
    else has to be running first.
    """
    if os.environ.get("AXIOM_LADDER_REUSE") and _port_open(PORT):
        yield BASE_URL
        return

    # `resilient.html` ships its own import map pointing at the engine's BUILT
    # `dist/`, so a missing or stale dist does not fail loudly — every scripted
    # rung simply never boots and the suite would report the form rung as "the
    # ladder", which is the most misleading result this suite could produce.
    # The build is sub-second; running it unconditionally removes the failure
    # mode instead of documenting it.
    subprocess.run(  # noqa: S603
        ["npm", "--prefix", "packages/axiom-web-engine", "run", "build"],
        check=True,
        cwd=REPO_ROOT,
        shell=sys.platform == "win32",
    )

    server = subprocess.Popen(  # noqa: S603
        ["node", str(HARNESS_DIR / "src" / "main.ts"), "--port", str(PORT)],
        cwd=REPO_ROOT,
        shell=sys.platform == "win32",
    )
    try:
        assert _wait_http(f"{BASE_URL}{GAME}", 60), f"the harness server never came up on {BASE_URL}"
        yield BASE_URL
    finally:
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill()


@pytest.fixture(scope="session")
def baseline(ladder_base_url: str, browser: Browser) -> dict:
    """What this machine does with NOTHING denied.

    Every cross-cut is judged against this rather than against a hardcoded tier,
    which is what makes those rows mean the same thing on a workstation with a
    GPU and in a remote-desktop session without one.
    """
    page = browser.new_page()
    try:
        page.goto(f"{ladder_base_url}{GAME}", wait_until="load")
        _settle(page)
        return page.evaluate(INFO_JS)
    finally:
        page.close()


def _arm(context: BrowserContext, deny: list[str]) -> tuple[Page, list[str]]:
    """A page whose realm is masked before its first script runs.

    Context-level, not page-level: measured, ``context.add_init_script`` reaches
    same-origin frames, opaque-origin sandboxed frames and cross-origin frames
    alike, before any of their scripts. A page-level script would miss exactly
    the frames a harness cares about.
    """
    seen: list[str] = []
    context.expose_binding("__axiomTierEvent", lambda _source, tier: seen.append(tier))
    context.add_init_script(path=str(MASK_JS))
    context.add_init_script(script=f"window.__axiomMask({json.dumps(deny)});")
    context.add_init_script(script=TIER_WATCH_JS)
    return context.new_page(), seen


def _settle(page: Page) -> None:
    """Wait for the page's own positive ready signal. A timeout here is a real
    failure — a ladder that never finishes choosing is a ladder that hangs."""
    page.wait_for_function("() => window.__axiomReady === true", timeout=SETTLE_MS)


def _info(page: Page) -> dict:
    _settle(page)
    info = page.evaluate(INFO_JS)
    assert info["maskErrors"] in (None, []), f"a deny token did not apply: {info['maskErrors']}"
    return info


def _assert_form_backbone(page: Page) -> None:
    """Assert the thing every rung has in common, on every row.

    Called BEFORE the pick, because a resolved round legitimately disables the
    chests — the invariant is about the board you are offered, not the one you
    already played.
    """
    form = page.evaluate(BACKBONE_JS)
    assert form["method"] == "POST", f"the board is not a POST form: {form}"
    assert form["action"] == PICK_ENDPOINT, f"the board posts to {form['action']!r}, not {PICK_ENDPOINT}"
    assert form["count"] == 9, f"expected nine chests, found {form['count']}"
    assert form["submits"] == 9, f"only {form['submits']} of the chests are real submit controls"
    assert form["enabled"] == 9, f"only {form['enabled']} of the chests are enabled"
    assert form["visible"] == 9, f"only {form['visible']} of the chests are visible"
    assert form["laidOut"] == 9, f"only {form['laidOut']} of the chests have a usable box"
    assert form["names"] == ["pick"], f"the chests submit under {form['names']}, not 'pick'"
    assert form["values"] == "0,1,2,3,4,5,6,7,8", f"the chest values are {form['values']}"


def _pick(page: Page, native: bool, index: int = PICK) -> Request:
    """Press a chest and prove the pick left the browser.

    ``expect_request`` rather than ``expect_navigation``: it settles before the
    navigation completes, so the native-form rows do not race the unload.
    """
    with page.expect_request(
        lambda r: PICK_ENDPOINT in r.url and r.method == "POST", timeout=SETTLE_MS
    ) as caught:
        page.locator(f'button.resilient-chest[value="{index}"]').click()
    request = caught.value

    assert request.is_navigation_request() is native, (
        f"expected a {'native form navigation' if native else 'in-place'} POST, got "
        f"is_navigation_request={request.is_navigation_request()} ({request.resource_type})"
    )
    body = request.post_data_json
    assert body is not None, "the POST carried no readable body"
    # A native form POST is urlencoded (Playwright parses it into a dict of
    # strings); fetch/XHR send JSON (a number). Both are the same pick, and both
    # must be readable.
    assert int(body["pick"]) == index, f"the POST carried {body!r}, not chest {index}"
    return request


def _assert_result_rendered(page: Page, native: bool, index: int = PICK) -> None:
    """The player can read the outcome.

    Both renderings say the same sentence — the server's result page and the
    in-place panel share ``describeOutcome``'s words — so one assertion covers
    both paths, which is also the point: there is no second copy of the game
    logic for the no-JS tier to drift from.
    """
    if native:
        page.wait_for_load_state("load")
        assert page.url.endswith(PICK_ENDPOINT), f"the form POST did not land on the result page: {page.url}"
        expect(page.locator("table.resilient-board")).to_be_visible(timeout=SETTLE_MS)
        assert page.locator("table.resilient-board tbody tr").count() == 9
    else:
        expect(page.locator("#outcome")).to_be_visible(timeout=SETTLE_MS)
        assert page.locator("#outcome .resilient-board-list li").count() == 9
    expect(page.locator(".resilient-prize")).to_contain_text(f"chest {index + 1}")


# ── the vocabulary itself ─────────────────────────────────────────────────────


def test_mask_implements_every_token_the_matrix_asks_for(ladder_base_url: str, context: BrowserContext) -> None:
    """The cheapest guard against the one failure that would make the whole suite
    worthless: a matrix row naming a denial that silently does nothing.

    ``caps-mask.js`` records an unknown token in ``__AXIOM_MASK_ERRORS`` rather
    than throwing, precisely so this can be an assertion instead of a row that
    quietly passes. (The tokens are only *listed* here, not applied together —
    ``webgpu`` and ``webgpu-adapter`` are mutually exclusive by construction: one
    removes the API the other needs to answer "no adapter".)
    """
    wanted = sorted({token for row in [*RUNGS, *CROSS_CUTS] for token in row["deny"]})
    page, _ = _arm(context, [])
    page.goto(f"{ladder_base_url}{GAME}", wait_until="load")

    implemented = page.evaluate("() => window.__axiomMask.tokens")
    missing = set(wanted) - set(implemented)
    assert not missing, f"rungs.json asks for deny tokens caps-mask.js does not implement: {sorted(missing)}"


# ── the render ladder, masked ─────────────────────────────────────────────────


@pytest.mark.parametrize("rung", RUNGS, ids=[r["tier"] for r in RUNGS])
def test_render_rung_selects_its_tier(rung: dict, ladder_base_url: str, context: BrowserContext) -> None:
    """Denying every tier above ``rung`` makes the page land on exactly ``rung``,
    and makes it DRAW there — the engine rungs mount a canvas behind the buttons,
    css3d builds its chests inside them. "The detector said webgl1" and "a board
    came up at webgl1" are different facts and both are checked."""
    page, _ = _arm(context, rung["deny"])
    page.goto(f"{ladder_base_url}{GAME}", wait_until="load")
    info = _info(page)

    _assert_form_backbone(page)
    assert info["denied"] == rung["deny"]
    assert info["probeSource"] == "probe", "no override should be in play on this row"
    assert info["tier"] not in rung["deny"], f"the page landed on a tier it was denied: {info}"

    if info["tier"] != rung["tier"] and rung["tier"] == "webgpu":
        # The top rung is the one rung a machine can genuinely fail to offer.
        # Saying so, with the probe's own words, beats weakening the assertion
        # into one that would pass everywhere and prove nothing.
        pytest.skip(
            "this machine has no working WebGPU, so the top rung cannot be observed here — "
            f"the probe said: {info['probeDetails']['webgpu']}"
        )

    assert info["tier"] == rung["tier"], f"expected {rung['tier']}, got {info['tier']} — probes: {info['probeDetails']}"
    assert info["probeTier"] == rung["tier"], "the page and the engine disagree about the rung"
    assert info["drawn"] is (rung["drawn"] == "engine"), (
        f"rung {rung['tier']} should be drawn by {rung['drawn']}; canvas present = {info['drawn']}"
    )
    for denied in rung["deny"]:
        assert info["probeOutcomes"][denied] in {"fail", "skipped"}, (
            f"{denied} was denied but its probe reported "
            f"{info['probeOutcomes'][denied]}: {info['probeDetails'][denied]}"
        )


@pytest.mark.parametrize("rung", RUNGS, ids=[r["tier"] for r in RUNGS])
def test_render_rung_still_pays_out(rung: dict, ladder_base_url: str, context: BrowserContext) -> None:
    """The whole point. Take every drawing API above ``rung`` away and the pick
    still posts in place and still comes back readable.

    This row never skips: it does not depend on what the machine can render, only
    on the page having landed somewhere at or below the rung and still working.
    """
    page, _ = _arm(context, rung["deny"])
    page.goto(f"{ladder_base_url}{GAME}", wait_until="load")
    info = _info(page)

    _assert_form_backbone(page)
    assert info["tier"] not in rung["deny"]
    assert info["tier"] == info["probeTier"], "the page claimed a rung the engine did not report"
    _pick(page, native=False)
    _assert_result_rendered(page, native=False)


# ── the render ladder, at the browser level ───────────────────────────────────


@pytest.mark.parametrize("flag", LAUNCH_FLAGS, ids=[f["id"] for f in LAUNCH_FLAGS])
def test_render_rung_under_launch_flags(
    flag: dict, ladder_base_url: str, playwright: Playwright, browser_type_launch_args: dict
) -> None:
    """The same rungs, denied one layer down — by Chromium itself.

    A second opinion that costs one browser launch. If the mask and the flag
    disagree about which tier a machine should land on, the bug is in the
    detection logic, not in either denial.
    """
    browser = playwright.chromium.launch(
        **{**browser_type_launch_args, "args": [*browser_type_launch_args.get("args", []), *flag["flags"]]}
    )
    try:
        page = browser.new_page()
        page.goto(f"{ladder_base_url}{GAME}", wait_until="load")
        info = _info(page)
        _assert_form_backbone(page)
        assert info["tier"] == flag["expectTier"], (
            f"{' '.join(flag['flags'])} should land on {flag['expectTier']}, "
            f"got {info['tier']} — {info['probeDetails']}"
        )
        _pick(page, native=False)
        _assert_result_rendered(page, native=False)
    finally:
        browser.close()


# ── the cross-cuts ────────────────────────────────────────────────────────────


@pytest.mark.parametrize("cut", CROSS_CUTS, ids=[c["id"] for c in CROSS_CUTS])
def test_cross_cut(cut: dict, ladder_base_url: str, context: BrowserContext, baseline: dict) -> None:
    """The things a locked-down browser does that are not about drawing.

    ``fetch-rejects`` is the row that matters most, and the reason none of this
    is gated on a ``typeof`` check anywhere: ``fetch`` is present, callable and
    fails every time. Feature detection cannot see that. Only a real attempt
    inside try/catch can — and ``transport-lies`` proves what happens when the
    second transport is gone too.
    """
    page, seen = _arm(context, cut["deny"])
    page.goto(f"{ladder_base_url}{GAME}", wait_until="load")
    info = _info(page)

    _assert_form_backbone(page)
    assert info["denied"] == cut["deny"]
    if cut.get("sameTierAsBaseline"):
        assert info["tier"] == baseline["tier"], (
            f"{cut['id']} moved the render tier from {baseline['tier']} to {info['tier']}, and it must not. "
            f"{cut['note']}"
        )
    if "notTier" in cut:
        assert info["tier"] != cut["notTier"], f"{cut['id']}: {cut['note']}"
    if "expectReadback" in cut:
        assert info["readback"] == cut["expectReadback"], f"{cut['id']}: {cut['note']}"

    _pick(page, native=cut["nativePost"])
    _assert_result_rendered(page, native=cut["nativePost"])

    if "afterPickTier" in cut:
        # The downgrade is published exactly like the first claim, so it is
        # observable even though the document that made it is being replaced.
        assert seen[-1] == cut["afterPickTier"], (
            f"{cut['id']} should have admitted it was down to {cut['afterPickTier']}; published {seen}"
        )


def test_cpu_throttled_20x(ladder_base_url: str, context: BrowserContext, baseline: dict) -> None:
    """A CPU twenty times slower than this one — the thin-client/VDI condition.

    Nothing about the ladder may be gated on being fast, and the detector in
    particular must not be: it is capped, and it must still finish and still pick
    the same rung. There is no in-page equivalent of this, which is also why the
    harness UI lists it as unenforceable rather than offering a switch that would
    quietly do nothing.
    """
    page, _ = _arm(context, [])
    session = context.new_cdp_session(page)
    session.send("Emulation.setCPUThrottlingRate", {"rate": 20})
    try:
        page.goto(f"{ladder_base_url}{GAME}", wait_until="load")
        info = _info(page)
        _assert_form_backbone(page)
        assert info["tier"] == baseline["tier"], "a slow CPU changed which rung the page chose"
        _pick(page, native=False)
        _assert_result_rendered(page, native=False)
    finally:
        session.send("Emulation.setCPUThrottlingRate", {"rate": 1})


# ── the explicit override ─────────────────────────────────────────────────────


@pytest.mark.parametrize("tier", MATRIX["ladder"])
def test_render_override_forces_tier(tier: str, ladder_base_url: str, context: BrowserContext) -> None:
    """``?render=<tier>`` is the support conversation's escape hatch, so it
    outranks the probes AND the crash guard — a machine that crashed once must
    still be askable to try that tier again. Asserted for every rung, including
    ones this machine cannot probe its way to."""
    page, _ = _arm(context, [])
    page.goto(f"{ladder_base_url}{GAME}?render={tier}", wait_until="load")
    info = _info(page)

    _assert_form_backbone(page)
    assert info["probeSource"] == "url", f"?render={tier} was not honoured as a URL override: {info}"
    assert info["probeTier"] == tier
    assert info["tier"] == tier, f"the engine was forced to {tier} but the page came up at {info['tier']}"

    _pick(page, native=False)
    _assert_result_rendered(page, native=False)


# ── the no-JavaScript rungs ───────────────────────────────────────────────────


def test_no_javascript_native_form_post(ladder_base_url: str, browser: Browser) -> None:
    """The user turned JavaScript off — the bottom rung, and the one the whole
    build is shaped around.

    The chests are ordinary submit buttons, the browser serializes the pressed
    one, and the server renders the answer with the same chance engine the
    animated rungs use. ``<noscript>`` renders here; that is the observable which
    distinguishes this rung from the CSP one below.
    """
    context = browser.new_context(java_script_enabled=False)
    try:
        page = context.new_page()
        page.goto(f"{ladder_base_url}{GAME}", wait_until="load")

        assert page.locator("html").get_attribute("data-axiom-tier") is None, "a script ran with JS disabled"
        assert page.locator(BOARD_CANVAS).count() == 0, "a rendered board appeared without script"
        _assert_form_backbone(page)
        expect(page.locator("noscript p")).to_be_visible()

        request = _pick(page, native=True)
        assert request.resource_type == "document", f"expected a document navigation, got {request.resource_type}"
        assert request.post_data_json == {"pick": str(PICK)}, "the native form body should be urlencoded"
        _assert_result_rendered(page, native=True)
    finally:
        context.close()


def test_csp_script_src_none_native_form_post(ladder_base_url: str, context: BrowserContext) -> None:
    """A policy forbade scripts — a DIFFERENT rung from the user turning JS off.

    Scripting is *enabled* here and merely blocked, so ``<noscript>`` does NOT
    render: a page that relied on ``<noscript>`` to explain itself says nothing at
    all in this environment. The form is what carries the player through, exactly
    as it does with JS off, and that is the whole reason the baseline is the
    served document rather than something script builds.
    """

    def with_csp(route) -> None:
        fetched = route.fetch()
        route.fulfill(
            body=fetched.body(),
            headers={
                "cache-control": "no-store",
                "content-security-policy": "script-src 'none'",
                "content-type": "text/html; charset=utf-8",
            },
            status=fetched.status,
        )

    context.route(f"**{GAME}*", with_csp)
    page = context.new_page()
    page.goto(f"{ladder_base_url}{GAME}", wait_until="domcontentloaded")

    assert page.evaluate("() => window.__axiomTier ?? null") is None, "a script ran under script-src 'none'"
    _assert_form_backbone(page)
    assert not page.locator("noscript p").is_visible(), (
        "<noscript> rendered under a CSP — scripting is enabled here, only forbidden, "
        "which is precisely why this rung needs a row of its own"
    )

    request = _pick(page, native=True)
    assert request.resource_type == "document"
    assert request.post_data_json == {"pick": str(PICK)}
    _assert_result_rendered(page, native=True)
