/*
 * harness.js — the developer UI.
 *
 * It owns three honest facts and refuses to pretend about a fourth:
 *
 *   1. WHAT IS DENIED. The rung matrix comes from `rungs.json`, the same file
 *      `e2e/test_capability_ladder.py` reads. The UI does not know the ladder;
 *      it renders it. If the suite and the harness ever disagreed about what
 *      "the webgl1 rung" means, one of them would be proving a ladder nobody
 *      ships — so neither of them owns the definition.
 *
 *   2. THE TOGGLES REBOOT THE FRAME. The mask has to run before the game's
 *      first script, and nothing can retro-fit that: patching
 *      `iframe.contentWindow` before assigning `.src` was measured and does not
 *      survive the navigation (`injected:false`). So every change remounts the
 *      iframe with a new `?deny=` and the game restarts. That is a property of
 *      the platform, not a shortcut.
 *
 *   3. THE FALLBACK, NOT THE DENIAL. Anyone can turn something off. The reason
 *      the readout exists is to show which rung the game LANDED on — the
 *      resilient page's own tier, over `postMessage`, and the engine's full
 *      detection report, obtained by running the real `detectTier()` inside the
 *      frame's masked realm.
 *
 * And the fourth: the launch-flag rungs (`--disable-webgl2`, `--disable-3d-apis`,
 * `--use-angle=swiftshader`) and CPU throttling CANNOT be applied from a page. A
 * page cannot relaunch its own browser, and there is no in-page equivalent of a
 * DevTools CPU throttle. The harness prints the exact command line rather than
 * offering a switch that would quietly do nothing — a control that lies is worse
 * than no control.
 *
 * Repo tooling: outside the engine dependency graph and its laws.
 */

const GAME_PATH = "/resilient.html";

/** How long to keep asking the frame for its report. The engine's own detection
 * budget is 2.5s and a throttled machine is slower still. */
const REPORT_TIMEOUT_MS = 15000;
const REPORT_POLL_MS = 150;

const matrix = await (await fetch("/__harness/rungs.json")).json();

const el = (id) => document.getElementById(id);

const state = {
  crossCuts: new Set(),
  flags: new Set(),
  noJs: false,
  rung: matrix.rungs[0].tier,
};

// ── the controls ────────────────────────────────────────────────────────────

const radio = (rung) => {
  const label = document.createElement("label");
  label.className = "hh-check";
  const input = document.createElement("input");
  input.type = "radio";
  input.name = "rung";
  input.value = rung.tier;
  input.checked = rung.tier === state.rung;
  input.addEventListener("change", () => {
    state.rung = rung.tier;
    mount();
  });
  const text = document.createElement("span");
  text.innerHTML = `<strong>${rung.tier}</strong> <span class="hh-deny">${
    rung.deny.length === 0 ? "denies nothing" : `denies ${rung.deny.join(", ")}`
  }</span>`;
  label.append(input, text);
  return label;
};

const check = (cut) => {
  const label = document.createElement("label");
  label.className = "hh-check";
  label.title = cut.note;
  const input = document.createElement("input");
  input.type = "checkbox";
  input.value = cut.id;
  input.addEventListener("change", () => {
    input.checked ? state.crossCuts.add(cut.id) : state.crossCuts.delete(cut.id);
    mount();
  });
  const text = document.createElement("span");
  text.innerHTML = `${cut.label} <span class="hh-deny">${cut.deny.join(", ")}</span>`;
  label.append(input, text);
  return label;
};

const flagCheck = (flag) => {
  const label = document.createElement("label");
  label.className = "hh-check hh-check--inert";
  label.title = flag.why;
  const input = document.createElement("input");
  input.type = "checkbox";
  input.value = flag.id;
  input.addEventListener("change", () => {
    input.checked ? state.flags.add(flag.id) : state.flags.delete(flag.id);
    renderCommandLine();
  });
  const text = document.createElement("span");
  text.innerHTML = `${flag.label} <span class="hh-deny">expects tier ${flag.expectTier}</span>`;
  label.append(input, text);
  return label;
};

matrix.rungs.forEach((rung) => el("rungs").append(radio(rung)));
matrix.crossCuts.forEach((cut) => el("crosscuts").append(check(cut)));
matrix.launchFlags.forEach((flag) => el("flags").append(flagCheck(flag)));

el("nojs").addEventListener("change", (event) => {
  state.noJs = event.target.checked;
  mount();
});
el("reboot").addEventListener("click", () => mount());

// ── the deny list ───────────────────────────────────────────────────────────

const denyList = () => {
  const rung = matrix.rungs.find((entry) => entry.tier === state.rung);
  const cuts = matrix.crossCuts.filter((cut) => state.crossCuts.has(cut.id));
  return [...new Set([...rung.deny, ...cuts.flatMap((cut) => cut.deny)])];
};

const renderCommandLine = () => {
  const chosen = matrix.launchFlags.filter((flag) => state.flags.has(flag.id));
  el("cmdline").textContent =
    chosen.length === 0
      ? "# tick one above to get its command line"
      : `chrome ${chosen.flatMap((flag) => flag.flags).join(" ")} "${location.origin}${GAME_PATH}"`;
};

// ── the frame ───────────────────────────────────────────────────────────────

const mount = () => {
  const deny = denyList();
  const url = `${GAME_PATH}?deny=${encodeURIComponent(deny.join(","))}&t=${Date.now()}`;

  el("applied").textContent = [
    `deny  = [${deny.join(", ")}]`,
    `no-JS = ${state.noJs}`,
    `frame = ${url}`,
  ].join("\n");
  el("badge-page").textContent = "page rung: …";
  el("badge-engine").textContent = state.noJs ? "engine tier: n/a (no JS in the frame)" : "engine tier: …";
  el("badge-url").textContent = `deny=${deny.join(",") || "(none)"}`;
  el("report").textContent = "engine detection report: …";

  const frame = document.createElement("iframe");
  frame.className = "hh-frame";
  frame.title = "the game under test";
  // allow-forms is NOT optional: without it the native form POST is silently
  // swallowed and the zero-JS rung stops existing. allow-same-origin keeps the
  // session cookie and lets the harness read the frame back.
  state.noJs && frame.setAttribute("sandbox", "allow-forms allow-same-origin");
  frame.src = url;
  frame.addEventListener("load", () => {
    if (state.noJs) {
      el("badge-page").textContent = "page rung: form (no script ran)";
      el("badge-engine").textContent = "engine tier: n/a (no JS in the frame)";
      el("report").textContent = "engine detection report: n/a — JavaScript is off in the frame.";
      return;
    }
    // READ the report the game produced; do not run a second detection of our
    // own. The page ships its own import map, so a probe injected from here
    // could load a DIFFERENT engine build and report a tier the game never saw —
    // the harness would be the source of the drift it exists to catch.
    let waited = 0;
    const poll = () => {
      const view = frame.contentWindow;
      const report = view && view.__renderProbe;
      if (report) {
        el("badge-engine").textContent = `engine tier: ${report.tier} via ${report.source}`;
        el("report").textContent = JSON.stringify(report, null, 2);
        return;
      }
      waited += REPORT_POLL_MS;
      if (waited >= REPORT_TIMEOUT_MS) {
        el("report").textContent =
          "engine detection report: the frame never published window.__renderProbe — its boot did not reach the detector.";
        return;
      }
      setTimeout(poll, REPORT_POLL_MS);
    };
    poll();
  });

  el("frame-slot").replaceChildren(frame);
};

// The page publishes its rung on EVERY change, not just once: a transport that
// turns out to be lying falsifies the claim made at load, and the downgrade to
// `form` is announced the same way. Showing only the first value would be the
// harness quietly repeating an optimistic claim.
window.addEventListener("message", (event) => {
  const tier = (event.data ?? {}).axiomTier;
  if (typeof tier === "string") {
    el("badge-page").textContent = `page rung: ${tier}`;
  }
});

renderCommandLine();
mount();
