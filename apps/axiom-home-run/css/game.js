/*
 * game.js — the MINIMAL JavaScript of the pure-CSS Home Run!. Everything visual
 * and everything in motion is CSS; this file does only what CSS cannot:
 *
 *   input               keyboard/pointer events            (session intent)
 *   seeded pitch math   hash01 + selectPitch — direct port of pitch.ts
 *   outcome decision    a timing/position mapping of swing.ts' contact model
 *   state sequencing    class/custom-property flips on discrete events
 *   HUD text            score / pitch / message numbers    (readHud)
 *
 * Every constant is a mapping from the engine app's constants.ts (60Hz ticks →
 * milliseconds, world units → the CSS file's 20px/u).
 */
(() => {
  "use strict";
  const $ = (id) => document.getElementById(id);
  const body = document.body;
  const params = new URLSearchParams(location.search);

  // ── constants.ts mapping ──────────────────────────────────────────────────
  const S = 20;                     // px per world unit (style.css --u)
  const U0 = 800, V0 = 1000;        // floor px of world (0,0) (home plate)
  const u = (x) => U0 - x * S;      // world +X = screen-left, like the engine
  const v = (z) => V0 - z * S;
  const PITCHES = 10;
  const RELEASE = { x: 0, y: 1.12, z: 9.7 };        // PITCH_RELEASE
  const CATCHER_Z = -2.2;
  const BATTER = { min: 0.55, max: 1.35, start: 0.95 };
  const SWEET_R = 0.88;                              // bat sweet spot (SWEET_SPOT_R)
  const OFF_BARREL = 0.47;                           // beyond the grip/tip reach
  const SWING_LEAD_MS = 130;        // press → bat-at-plate ≈ 8 ticks (OMEGA_SWING)
  const STRIKE_MS = 190, REWIND_MS = 950;            // swing.ts timings
  const WINDUP_MS = 800;                             // WINDUP_TICKS 48
  const GAP_MS = [420, 1000];                        // GAP_TICKS 25 + 0..35
  const RESULT_MS = 1420, HOMER_RESULT_MS = 2500;    // RESULT_TICKS / HOMER_…
  const ZONE = { halfX: 0.45, low: 0.4, high: 1.3 }; // STRIKE_ZONE_*
  const SCORE = { ball: 0, miss: 0, foul: 0, weak: 25, grounder: 50, popup: 50, clean: 100, homer: 500 };
  const CLEAN_BONUS = 1, HOMER_BONUS = 2, MULT_CAP = 4;

  // PITCH_PROFILES — verbatim from constants.ts.
  const PROFILES = [
    { name: "SLOW BALL", speed: 12.5, targetX: 0, targetY: 0.95, tier: "easy" },
    { name: "FASTBALL", speed: 17, targetX: 0, targetY: 0.95, tier: "easy" },
    { name: "HEATER", speed: 23, targetX: 0, targetY: 1.0, tier: "hard" },
    { name: "SINKER", speed: 12, targetX: 0, targetY: 0.72, tier: "medium" },
    { name: "RISER", speed: 24, targetX: 0, targetY: 1.1, tier: "hard" },
    { name: "INSIDE", speed: 16.5, targetX: 0.34, targetY: 0.9, tier: "medium" },
    { name: "OUTSIDE", speed: 16.5, targetX: -0.34, targetY: 0.9, tier: "medium" },
  ];

  // FIELDER_SPOTS — verbatim from constants.ts (the one scene loop JS builds).
  const SPOTS = [
    { r: 1.7, x: -6.9, z: 7.9 }, { r: 1.7, x: -3.4, z: 11.8 },
    { r: 1.7, x: 3.4, z: 11.8 }, { r: 1.7, x: 6.9, z: 7.9 },
    { r: 2.4, x: 12.5, z: 17.5 }, { r: 2.4, x: 6.8, z: 22.5 },
    { r: 2.4, x: 0, z: 24.5 }, { r: 2.4, x: -6.8, z: 22.5 },
    { r: 2.4, x: -12.5, z: 17.5 }, { r: 0.7, x: 2.2, z: 10 },
  ];

  // ── vec.ts hash01 — verbatim port (the seeded pitch sequence) ─────────────
  const hash01 = (seed, ...keys) => {
    let h = (seed | 0) ^ 0x9e3779b9;
    for (const k of keys) {
      h = Math.imul(h ^ (k | 0), 0x85ebca6b);
      h ^= h >>> 13;
      h = Math.imul(h, 0xc2b2ae35);
      h ^= h >>> 16;
    }
    return (h >>> 8) / 16777216;
  };

  // ── pitch.ts selectPitch — verbatim port (same seed → same round) ────────
  const pitchPool = (i) => {
    if (i < 2) return PROFILES.filter((p) => p.tier === "easy");
    if (i < 5) return PROFILES.filter((p) => p.tier !== "hard");
    const weighted = [];
    for (const p of PROFILES) for (let k = 0; k < (p.tier === "hard" ? 2 : 1); k += 1) weighted.push(p);
    return weighted;
  };
  const selectPitch = (sd, i) => {
    const pool = pitchPool(i);
    const p = pool[Math.min(pool.length - 1, Math.floor(hash01(sd, i, 1) * pool.length))];
    const speed = p.speed * (1 + (hash01(sd, i, 2) - 0.5) * 2 * 0.04);
    return {
      name: p.name, speed,
      mph: Math.round(speed * 3.4),
      targetX: p.targetX + (hash01(sd, i, 3) - 0.5) * 2 * 0.18,
      targetY: p.targetY + (hash01(sd, i, 4) - 0.5) * 2 * 0.09,
    };
  };
  const gapMs = (sd, i) => GAP_MS[0] + hash01(sd, i, 5) * (GAP_MS[1] - GAP_MS[0]);

  // ── build the fielders from the spot table (CSS cannot loop data) ────────
  const fieldersEl = $("fielders");
  const fielders = SPOTS.map((s, i) => {
    const spot = document.createElement("div");
    spot.className = "f-spot";
    spot.style.left = `${u(s.x)}px`;
    spot.style.top = `${v(s.z)}px`;
    const ring = document.createElement("div"); // buildPatrolCircles
    ring.className = "patrol";
    const d = s.r * 1.9 * S;
    ring.style.cssText = `left:${-d / 2}px;top:${-d / 2}px;width:${d}px;height:${d}px;`;
    spot.appendChild(ring);
    const chase = document.createElement("div");
    chase.className = "f-chase";
    const wander = document.createElement("div");
    wander.className = "f-wander";
    // Seeded, unsynchronized wander (fielders.ts wanderPos → CSS durations/phases).
    wander.style.setProperty("--wu", `${(2.8 + hash01(1, i, 11) * 2.6).toFixed(2)}s`);
    wander.style.setProperty("--wv", `${(3.6 + hash01(1, i, 12) * 3).toFixed(2)}s`);
    wander.style.setProperty("--wd", `${(-hash01(1, i, 13) * 6).toFixed(2)}s`);
    wander.style.setProperty("--wr", `${Math.round(s.r * 0.6 * S)}px`);
    const cut = document.createElement("div");
    cut.className = "f-cutout";
    wander.appendChild(cut);
    chase.appendChild(wander);
    spot.appendChild(chase);
    fieldersEl.appendChild(spot);
    return { spot: s, chase };
  });

  // ── session state ─────────────────────────────────────────────────────────
  const seed = Number(params.get("seed") || 1) | 0;
  const el = {
    message: $("message"), score: $("score"), pitch: $("pitch"), homers: $("homers"),
    streak: $("streak"), mph: $("mph"), best: $("best"), ready: $("ready"), over: $("over"),
    overScore: $("over-score"), overHomers: $("over-homers"), overBest: $("over-best"),
    confetti: $("confetti"),
  };
  const st = {
    phase: "ready", pitchIndex: 0, score: 0, homers: 0, streak: 0, best: 0,
    batterX: BATTER.start, batReady: true, swung: false,
    spec: null, plateAt: 0,
  };
  const setVar = (k, val) => body.style.setProperty(k, val);
  const timers = [];
  const after = (ms, fn) => timers.push(setTimeout(fn, ms));
  const clearTimers = () => { while (timers.length) clearTimeout(timers.pop()); };

  const hud = () => {
    el.score.textContent = String(st.score);
    el.pitch.textContent = `${Math.min(st.pitchIndex + 1, PITCHES)}/${PITCHES}`;
    el.homers.textContent = String(st.homers);
    el.streak.innerHTML = `${Math.min(Math.max(1, st.streak), MULT_CAP)}&times;`;
    el.streak.classList.toggle("up", st.streak > 1);
    el.best.textContent = st.best > 0 ? `${st.best}m` : "—";
  };
  let msgTimer = 0;
  const say = (text, kind, big) => {
    el.message.textContent = text;
    el.message.className = `show ${kind}${big ? " big" : ""}`;
    clearTimeout(msgTimer);
    if (!params.has("static")) msgTimer = setTimeout(() => (el.message.className = ""), big ? 2100 : 1200);
  };
  const popConfetti = () => {
    el.confetti.innerHTML = "";
    for (let i = 0; i < 36; i += 1) {
      const bit = document.createElement("div");
      bit.className = "bit";
      bit.style.left = `${8 + ((i * 37) % 84)}%`;
      bit.style.background = ["#ffd23d", "#ff6a5e", "#6ecbff", "#7fffa8", "#ff9de2"][i % 5];
      bit.style.animationDelay = `${(i % 9) * 0.07}s`;
      bit.style.animationDuration = `${1.3 + (i % 5) * 0.18}s`;
      el.confetti.appendChild(bit);
    }
    setTimeout(() => (el.confetti.innerHTML = ""), 2600);
  };

  // ── the pitch cycle (windup → CSS flight → outcome) ───────────────────────
  const startWindup = () => {
    st.phase = "windup";
    st.swung = false;
    st.spec = selectPitch(seed, st.pitchIndex);
    hud();
    after(gapMs(seed, st.pitchIndex), () => {
      body.classList.add("windup");
      // ?static=1 freezes the game at the wind-up moment (deterministic shots).
      if (!params.has("static")) after(WINDUP_MS, firePitch);
    });
  };

  const firePitch = () => {
    const s = st.spec;
    st.phase = "pitch";
    body.classList.remove("windup");
    body.classList.add("fire");
    after(200, () => body.classList.remove("fire"));
    const flightMs = ((RELEASE.z - CATCHER_Z) / s.speed) * 1000; // engine kinematics
    const plateMs = (RELEASE.z / s.speed) * 1000;
    el.mph.textContent = `${s.mph} MPH ${s.name}`;
    setVar("--bu", `${u(RELEASE.x)}px`);
    setVar("--bv", `${v(RELEASE.z)}px`);
    setVar("--plate-h", `${Math.round(s.targetY * S)}px`);
    setVar("--pitch-ms", `${Math.round(flightMs)}ms`);
    body.classList.add("ball-live");
    requestAnimationFrame(() => requestAnimationFrame(() => {
      body.classList.add("pitching");
      setVar("--bu", `${u(s.targetX * 1.2)}px`);
      setVar("--bv", `${v(CATCHER_Z)}px`);
    }));
    st.plateAt = performance.now() + plateMs;
    after(flightMs, resolveTake);
    // Dev/screenshot affordance (mirrors the engine's ?swingAt): one scripted
    // swing N ms after the pitch fires, on the first pitch only.
    if (params.has("swingAfter") && st.pitchIndex === 0) {
      setTimeout(trySwing, Number(params.get("swingAfter")));
    }
  };

  // A take (or a whiff): umpired at the catcher — pitch.ts isStrike, verbatim.
  const resolveTake = () => {
    if (st.phase !== "pitch") return;
    body.classList.remove("pitching", "ball-live");
    if (st.swung) return resolve("miss", 0, "MISS");
    const s = st.spec;
    const strike = Math.abs(s.targetX) <= ZONE.halfX && s.targetY >= ZONE.low && s.targetY <= ZONE.high;
    resolve(strike ? "miss" : "ball", 0, strike ? "STRIKE" : "BALL");
  };

  /*
   * The contact decision — a MAPPING of swing.ts' swept contact:
   *   timing error   e = (press + SWING_LEAD) − plate arrival    (θ vs sweet)
   *   position error p = |batterX − (targetX + SWEET_R)|         (r vs sweet spot)
   * Windows mirror the engine's tick windows (1 tick ≈ 16.7ms): |e|≤33ms is
   * homer-grade, ≤66 clean, ≤100 weak/grounder/popup, ≤133 foul, else a whiff —
   * all degraded by p exactly like the sweet-spot falloff (p>0.47 = off barrel).
   */
  const trySwing = () => {
    if (!st.batReady) return;
    st.batReady = false;
    st.swung = true;
    body.classList.add("striking");
    body.classList.remove("rewinding");
    setTimeout(() => {                       // untracked: the swing cycle always completes
      body.classList.remove("striking");
      body.classList.add("rewinding");
      setTimeout(() => { st.batReady = true; body.classList.remove("rewinding"); }, REWIND_MS);
    }, STRIKE_MS);
    if (st.phase !== "pitch") return;

    const e = performance.now() + SWING_LEAD_MS - st.plateAt;
    const p = Math.abs(st.batterX - (st.spec.targetX + SWEET_R));
    const ae = Math.abs(e);
    if (ae > 133 || p > OFF_BARREL) return;  // whiff — the pitch sails on

    st.phase = "flight";
    clearTimers();                            // cancels the pending resolveTake
    const posQ = Math.max(0, 1 - p / OFF_BARREL);
    let outcome, dist;
    if (ae <= 33 && posQ > 0.55) { outcome = "homer"; dist = Math.round(96 - ae * 1.1 - (1 - posQ) * 40); }
    else if (ae <= 66 && posQ > 0.4) { outcome = "clean"; dist = Math.round(33 - ae * 0.12); }
    else if (ae <= 100) { outcome = posQ < 0.3 ? "weak" : e < 0 ? "popup" : "grounder"; dist = Math.max(6, Math.round(15 - ae * 0.06)); }
    else { outcome = "foul"; dist = 14; }
    flyBall(outcome, dist, e);
  };

  // The hit flight: spray from the timing sign (early pulls +X, engine batTangent).
  const flyBall = (outcome, dist, e) => {
    const spray = outcome === "foul"
      ? (e < 0 ? 1.1 : -1.1)
      : Math.max(-0.6, Math.min(0.6, -e / 110));
    const lx = Math.sin(spray) * dist;
    const lz = outcome === "foul" ? 4 : Math.cos(spray) * dist;
    const speed = outcome === "homer" ? 38 : outcome === "clean" ? 28 : 16; // exit tiers, u/s
    const ms = Math.max(700, (Math.hypot(lx, lz) / speed) * 1000 * 1.7);
    body.classList.remove("pitching");
    body.classList.add("hit", outcome === "homer" ? "shake-big" : "shake");
    setTimeout(() => body.classList.remove("shake", "shake-big"), 500);
    setVar("--hit-ms", `${Math.round(ms)}ms`);
    setVar("--peak", `${Math.round(Math.min(230, dist * (outcome === "popup" ? 5 : outcome === "grounder" ? 0.5 : 2.2)))}px`);
    setVar("--bu", `${u(lx)}px`);
    setVar("--bv", `${v(Math.max(lz, -3))}px`);
    chaseFielders(lx, lz, outcome);
    const pts = points(outcome, dist);
    after(ms * 0.92, () => {
      body.classList.remove("hit", "ball-live");
      st.score += pts;
      resolve(outcome, dist, label(outcome, pts));
    });
  };

  const chaseFielders = (lx, lz, outcome) => {
    if (outcome === "homer" || outcome === "foul") return;
    for (const f of fielders) {
      if (Math.hypot(lx - f.spot.x, lz - f.spot.z) <= f.spot.r * 2) {
        const cl = (n, lim) => Math.max(-lim, Math.min(lim, n));
        const cx = cl(lx - f.spot.x, f.spot.r * 1.45);
        const cz = cl(lz - f.spot.z, f.spot.r * 1.45);
        f.chase.style.transform = `translate(${-cx * S}px, ${-cz * S}px)`;
        setTimeout(() => (f.chase.style.transform = ""), 2400);
      }
    }
  };

  // ball.ts scoreFor — verbatim port (streak multiplies consecutive homers).
  const points = (outcome, dist) => {
    if (outcome === "homer") {
      st.streak += 1;
      return (SCORE.homer + Math.round(dist * HOMER_BONUS)) * Math.min(st.streak, MULT_CAP);
    }
    st.streak = 0;
    if (outcome === "clean") return SCORE.clean + Math.round(dist * CLEAN_BONUS);
    return SCORE[outcome];
  };
  const label = (outcome, pts) => {
    const text = { homer: "HOME RUN!", clean: "CLEAN HIT", weak: "WEAK HIT", grounder: "GROUNDER", popup: "POP UP", foul: "FOUL" }[outcome];
    const tag = outcome === "homer" && st.streak > 1 ? ` ×${Math.min(st.streak, MULT_CAP)}` : "";
    return pts > 0 ? `${text} +${pts}${tag}` : text;
  };

  const resolve = (outcome, dist, text) => {
    st.phase = "result";
    if (outcome === "miss" || outcome === "ball" || outcome === "foul") st.streak = 0;
    say(text, outcome, outcome === "homer");
    if (outcome === "homer") { st.homers += 1; popConfetti(); }
    if (dist > 0 && outcome !== "foul") st.best = Math.max(st.best, dist);
    hud();
    after(outcome === "homer" ? HOMER_RESULT_MS : RESULT_MS, nextPitch);
  };

  const nextPitch = () => {
    st.pitchIndex += 1;
    body.classList.remove("ball-live", "pitching", "hit");
    if (st.pitchIndex >= PITCHES) {
      st.phase = "over";
      el.overScore.textContent = String(st.score);
      el.overHomers.textContent = String(st.homers);
      el.overBest.textContent = st.best > 0 ? `${st.best}m` : "—";
      el.over.classList.add("show");
      return;
    }
    startWindup();
  };

  const restart = () => {
    clearTimers();
    Object.assign(st, { phase: "ready", pitchIndex: 0, score: 0, homers: 0, streak: 0, best: 0, batterX: BATTER.start, batReady: true });
    body.className = params.has("static") ? "static" : "";
    setVar("--batter-u", `${u(st.batterX)}px`);
    el.over.classList.remove("show");
    el.ready.classList.add("show");
    el.mph.textContent = "—";
    hud();
  };

  // ── input (the one thing CSS can never do) ───────────────────────────────
  const startRound = () => {
    el.ready.classList.remove("show");
    startWindup();
  };
  const press = () => {
    if (st.phase === "ready") return startRound();
    if (st.phase === "over") return restart();
    trySwing();
  };
  const step = (dir) => { // world sign: +1 = screen-left (A), like the engine
    if (st.phase === "over") return;
    st.batterX = Math.min(BATTER.max, Math.max(BATTER.min, st.batterX + dir * 0.1));
    setVar("--batter-u", `${u(st.batterX)}px`);
  };
  addEventListener("keydown", (ev) => {
    if (ev.repeat) return;
    if (ev.code === "Space") { ev.preventDefault(); press(); }
    if (ev.code === "Enter" && st.phase === "over") restart();
    if (ev.code === "KeyA" || ev.code === "ArrowLeft") step(1);
    if (ev.code === "KeyD" || ev.code === "ArrowRight") step(-1);
  });
  $("pad-swing").addEventListener("pointerdown", press);
  $("pad-left").addEventListener("pointerdown", () => step(1));
  $("pad-right").addEventListener("pointerdown", () => step(-1));

  // ── dev/screenshot affordances (mirror the engine harness) ───────────────
  if (params.has("static")) body.classList.add("static");
  if (params.get("auto") === "1") setTimeout(startRound, 400);

  setVar("--batter-u", `${u(st.batterX)}px`);
  hud();
})();
