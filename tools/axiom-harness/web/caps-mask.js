/*
 * caps-mask.js — THE hostile-environment mask, and the single source of truth
 * for what "denying a capability" means in this repo.
 *
 * ONE FILE, LOADED VERBATIM BY BOTH CONSUMERS. The developer harness
 * (`index.html`) loads it with a `<script src>` that the harness server injects
 * as the first thing in `<head>`; the Playwright suite loads the SAME bytes with
 * `context.add_init_script(path=…)`. Neither one owns a copy. That is not tidiness:
 * a harness whose denial differs from the suite's denial is a harness that lies,
 * and a lying harness is worse than no harness — you would ship a fallback that
 * was only ever proven against a fiction.
 *
 * WHY A SCRIPT AND NOT A BROWSER FEATURE. There is no way to take a rendering
 * API away from a document from the outside. Measured, not assumed:
 *
 *   - No `<iframe sandbox>` token removes WebGPU, WebGL2, WebGL1, Canvas2D or
 *     CSS 3D. `sandbox="allow-scripts"` still has every one of them.
 *   - No CSP directive and no Permissions-Policy feature covers any of them.
 *   - Patching `iframe.contentWindow` before assigning `.src` does nothing: the
 *     navigation installs a fresh realm and the patch is discarded.
 *
 * What the platform *can* take away is JavaScript itself (`sandbox` without
 * `allow-scripts`, `java_script_enabled=False`, `script-src 'none'`) — and that
 * is a different rung, tested separately. Everything between "all APIs present"
 * and "no JS at all" has to be simulated in-realm, which is this file. The two
 * launch flags that genuinely remove a rung at the browser level
 * (`--disable-webgl2`, `--disable-3d-apis`) are used by the suite as an
 * independent second opinion on the same rungs; a disagreement between the two
 * layers is a real bug in the detection logic, not a harness artifact.
 *
 * TIMING IS PART OF THE CONTRACT. This must run before the page's first script.
 * As an init script that is guaranteed. In the harness it is guaranteed by the
 * server injecting it ahead of everything else in `<head>`. The CSS 3D denial in
 * particular installs its stylesheet IMMEDIATELY and re-attaches it through a
 * `MutationObserver` on `document`: waiting for `DOMContentLoaded` there is a
 * silent no-op, because by then the page has already measured its own layout and
 * decided what it can do.
 *
 * Every token is applied inside its own try/catch. One denial that cannot be
 * installed in this browser must not silently cancel the other nine — it is
 * recorded in `window.__AXIOM_MASK_ERRORS` instead, so the harness and the suite
 * can both see that it did not take.
 *
 * Repo tooling: outside the engine dependency graph, the Coverage Law and the
 * Branchless Law. It keeps ordinary control flow on purpose — it is a
 * simulation of a broken browser and must read like one.
 */
(function () {
  "use strict";

  var scope = typeof window === "undefined" ? undefined : window;
  if (!scope) {
    return;
  }

  /** How many times the CSS 3D sheet may be re-attached before we stop fighting
   * whatever else is rewriting the head. A cap, not a policy: without it a page
   * with its own MutationObserver could ping-pong with ours forever. */
  var MAX_SHEET_MOVES = 500;

  var applied = [];
  var errors = [];

  /** Canvas context names this mask must refuse, filled in by the tokens and
   * enforced by ONE `getContext` patch installed at the end. Patching once means
   * `webgl2` + `canvas2d` compose instead of each wrapping the other's wrapper. */
  var deniedContexts = Object.create(null);

  var note = function (token, error) {
    errors.push(token + ": " + String(error));
  };

  /** Remove a global constructor so constructor-shaped feature detection
   * (`"WebGL2RenderingContext" in window`) is denied too, not just the factory.
   * Real deployments break exactly this way — a policy that removes the API
   * removes the type with it — and code that only guards the factory then
   * throws later, off the boot path, where nobody is looking. */
  var dropGlobal = function (name) {
    try {
      delete scope[name];
    } catch (error) {
      note("drop " + name, error);
    }
  };

  var securityError = function (what) {
    try {
      return new DOMException(what + " is blocked by policy", "SecurityError");
    } catch (error) {
      var fallback = new Error(what + " is blocked by policy");
      fallback.name = "SecurityError";
      return fallback;
    }
  };

  // ── the tokens ────────────────────────────────────────────────────────────

  /** `webgpu` — the API is not there at all. No launch flag can do this
   * (measured: nothing removes `navigator.gpu`), so it must be JS-level.
   * `navigator.gpu` is an accessor on `Navigator.prototype`, so deleting it from
   * the instance is a no-op; the prototype is the property's real home and
   * removing it there also makes `"gpu" in navigator` false, which is what a
   * feature test actually asks. */
  var denyWebgpu = function () {
    try {
      delete Navigator.prototype.gpu;
    } catch (error) {
      note("webgpu(prototype)", error);
    }
    if (scope.navigator.gpu !== undefined) {
      Object.defineProperty(scope.navigator, "gpu", {
        configurable: true,
        get: function () {
          return undefined;
        },
      });
    }
    deniedContexts.webgpu = true;
    ["GPU", "GPUAdapter", "GPUDevice", "GPUCanvasContext", "GPUQueue"].forEach(dropGlobal);
  };

  /** `webgpu-adapter` — the API is present and answers "no adapter". This is the
   * more faithful shape of a machine with hardware acceleration off (and of
   * `--disable-3d-apis`): `navigator.gpu` exists, so presence-based detection
   * says yes, and only actually asking gets the truth. */
  var denyWebgpuAdapter = function () {
    var gpu = scope.navigator.gpu;
    if (!gpu) {
      throw new Error("navigator.gpu is not present, so its adapter cannot be denied");
    }
    // Patch the PROTOTYPE method, not the instance: native WebGPU methods need
    // the real internal slot, so an `Object.create(gpu)` shim would break every
    // call we are not overriding.
    var proto = Object.getPrototypeOf(gpu);
    Object.defineProperty(proto, "requestAdapter", {
      configurable: true,
      value: function () {
        return Promise.resolve(null);
      },
      writable: true,
    });
  };

  var denyWebgl2 = function () {
    deniedContexts.webgl2 = true;
    dropGlobal("WebGL2RenderingContext");
  };

  var denyWebgl1 = function () {
    deniedContexts.webgl = true;
    deniedContexts["experimental-webgl"] = true;
    dropGlobal("WebGLRenderingContext");
  };

  /** `canvas2d` — no 2D context from either canvas class. The `OffscreenCanvas`
   * half matters: a probe or a worker that finds the element path blocked will
   * cheerfully try the offscreen one, and a mask that only covered
   * `HTMLCanvasElement` would report a capability the real policy had removed.
   * The WebGL constructors go with it, so constructor-based detection sees a
   * consistently context-free browser rather than a half-masked one. */
  var denyCanvas2d = function () {
    deniedContexts["2d"] = true;
    deniedContexts.bitmaprenderer = true;
    dropGlobal("CanvasRenderingContext2D");
    dropGlobal("OffscreenCanvasRenderingContext2D");
    dropGlobal("WebGL2RenderingContext");
    dropGlobal("WebGLRenderingContext");
  };

  /**
   * `css3d` — no depth-sorted 3D transform tree.
   *
   * Two halves, and BOTH are needed. `CSS.supports` is what feature detection
   * asks, and the `!important` sheet is what actually flattens a page that never
   * asked. The sheet is installed synchronously — before `document.head` may
   * even exist, hence the `documentElement` fallback and the observer — and
   * re-attached as the LAST rule source whenever the DOM changes, because a
   * stylesheet the page adds later would otherwise win on order.
   */
  var denyCss3d = function () {
    var css = scope.CSS;
    if (css && typeof css.supports === "function") {
      var real = css.supports.bind(css);
      Object.defineProperty(css, "supports", {
        configurable: true,
        value: function () {
          var text = Array.prototype.join.call(arguments, " ");
          if (/preserve-3d/i.test(text)) {
            return false;
          }
          return real.apply(null, arguments);
        },
        writable: true,
      });
    }

    var sheet = document.createElement("style");
    sheet.setAttribute("data-axiom-mask", "css3d");
    sheet.textContent =
      "*, *::before, *::after { transform-style: flat !important; perspective: none !important; }";

    var moves = 0;
    var attach = function () {
      var host = document.head || document.documentElement;
      if (!host || moves >= MAX_SHEET_MOVES) {
        return;
      }
      if (host.lastChild !== sheet) {
        moves += 1;
        host.appendChild(sheet);
      }
    };
    attach();
    try {
      new MutationObserver(attach).observe(document, { childList: true, subtree: true });
    } catch (error) {
      note("css3d(observer)", error);
    }
  };

  /** `fetch` — the API is gone. `"fetch" in window` is false, which is what the
   * resilient page's transport probe asks before it falls to XHR. */
  var denyFetch = function () {
    dropGlobal("fetch");
  };

  /**
   * `fetch-reject` — the API is present, callable, and always fails.
   *
   * This is the condition that ships broken. A managed browser behind a proxy,
   * or a CSP carrying `connect-src 'none'`, leaves `typeof fetch === "function"`
   * true right up until the request dies. Feature detection cannot see it; only
   * a real attempt inside try/catch can. Every "graceful degradation" that was
   * only ever tested against an ABSENT API is untested against this one.
   */
  var denyFetchResult = function () {
    Object.defineProperty(scope, "fetch", {
      configurable: true,
      value: function () {
        return Promise.reject(new TypeError("Failed to fetch"));
      },
      writable: true,
    });
  };

  var denyXhr = function () {
    dropGlobal("XMLHttpRequest");
  };

  var denyWebsocket = function () {
    dropGlobal("WebSocket");
  };

  var denyWasm = function () {
    dropGlobal("WebAssembly");
  };

  /** `readback` — pixels go in, nothing comes out. Brave's farbling, Firefox's
   * `resistFingerprinting` and Tor's blank canvas all live here, and so does a
   * tainted canvas. The engine's control probe is meant to notice and stop
   * treating pixel evidence as admissible; this is how that gets proven. */
  var denyReadback = function () {
    var throwing = function (what) {
      return function () {
        throw securityError(what);
      };
    };
    var targets = [
      [scope.CanvasRenderingContext2D, "getImageData"],
      [scope.OffscreenCanvasRenderingContext2D, "getImageData"],
      [scope.HTMLCanvasElement, "toDataURL"],
      [scope.HTMLCanvasElement, "toBlob"],
      [scope.OffscreenCanvas, "convertToBlob"],
    ];
    targets.forEach(function (entry) {
      var ctor = entry[0];
      var name = entry[1];
      if (!ctor || !ctor.prototype) {
        return;
      }
      Object.defineProperty(ctor.prototype, name, {
        configurable: true,
        value: throwing(name),
        writable: true,
      });
    });
  };

  var TOKENS = {
    canvas2d: denyCanvas2d,
    css3d: denyCss3d,
    fetch: denyFetch,
    "fetch-reject": denyFetchResult,
    readback: denyReadback,
    wasm: denyWasm,
    webgl1: denyWebgl1,
    webgl2: denyWebgl2,
    webgpu: denyWebgpu,
    "webgpu-adapter": denyWebgpuAdapter,
    websocket: denyWebsocket,
    xhr: denyXhr,
  };

  /** Every token this mask understands, for the harness UI and for a test that
   * wants to assert the vocabulary has not drifted. */
  var TOKEN_NAMES = Object.keys(TOKENS).sort();

  // ── the one getContext patch ──────────────────────────────────────────────

  /**
   * Installed once, after the tokens have filled `deniedContexts`. It returns
   * `null` — exactly what a browser without the context returns — rather than
   * throwing, because a throw is a different failure that real code handles
   * differently, and simulating the wrong one would prove the wrong thing.
   */
  var installContextDenial = function () {
    var names = Object.keys(deniedContexts);
    if (names.length === 0) {
      return;
    }
    [scope.HTMLCanvasElement, scope.OffscreenCanvas].forEach(function (ctor) {
      if (!ctor || !ctor.prototype || typeof ctor.prototype.getContext !== "function") {
        return;
      }
      var real = ctor.prototype.getContext;
      Object.defineProperty(ctor.prototype, "getContext", {
        configurable: true,
        value: function (kind) {
          if (deniedContexts[String(kind).toLowerCase()]) {
            return null;
          }
          return real.apply(this, arguments);
        },
        writable: true,
      });
    });
  };

  // ── the entry point ───────────────────────────────────────────────────────

  /**
   * Apply `denyList` (an array, or a comma/space separated string) to this
   * realm. Returns the tokens that took. Calling it with an empty list is a
   * legitimate no-op: it is the top rung, and the harness still calls it so that
   * `window.__AXIOM_DENIED` is present and empty rather than missing.
   */
  scope.__axiomMask = function (denyList) {
    var list = Array.isArray(denyList) ? denyList : String(denyList || "").split(/[\s,]+/);
    list
      .map(function (token) {
        return String(token).trim().toLowerCase();
      })
      .filter(function (token) {
        return token.length > 0;
      })
      .forEach(function (token) {
        var deny = TOKENS[token];
        if (!deny) {
          note(token, "unknown deny token");
          return;
        }
        try {
          deny();
          applied.push(token);
        } catch (error) {
          note(token, error);
        }
      });

    try {
      installContextDenial();
    } catch (error) {
      note("getContext", error);
    }

    scope.__AXIOM_DENIED = applied.slice();
    scope.__AXIOM_MASK_ERRORS = errors.slice();
    return scope.__AXIOM_DENIED;
  };

  scope.__axiomMask.tokens = TOKEN_NAMES;
})();
