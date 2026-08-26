/**
 * WHEN EVERY GPU PROGRAM BECAME USABLE, AND WHAT THAT COST.
 *
 * The one number this app's boot is made of is serial GPU shader compilation —
 * ~100 programs, ~27 s of it on a cold driver, and it is most of the time
 * between the player getting control and the game settling. Every attempt to
 * shorten it so far has been aimed by indirect evidence and most of them missed:
 * the surface bakes were worth 24 s, the material feature permutations were
 * worth four programs, and a shader that billed 6 023 ms turned out to cost 108.
 *
 * The reason is that the WebGL probe measures BLOCKING time — how long something
 * waited on a program — which is a property of who asked and when, not of the
 * program. A program the driver compiles while nothing is blocked on it appears
 * free, and this app now deliberately arranges for that to be the common case.
 *
 * So measure the driver directly. `isReady()` is a non-blocking completion flag;
 * polling it every frame for every live program gives the moment each one became
 * usable. Sorted, the gaps between consecutive completions ARE the per-program
 * driver cost, because the driver compiles one at a time.
 *
 * That turns "cut the expensive programs" into a list with numbers on it.
 *
 * Dev-only, behind `?progcurve=1`. It costs one `getProgramParameter` per
 * not-yet-ready program per frame and stops asking once a program is ready, so
 * it converges to zero.
 */

export function installProgramCurve(engine) {
  const renderer = engine.ctx.peek('render')?.renderer;
  if (!renderer) return;

  /** program -> record. Keyed on the wrapper three hands out. */
  const seen = new Map();
  const t0 = performance.now();

  const sample = () => {
    const now = performance.now() - t0;
    const programs = renderer.info?.programs ?? [];
    for (const program of programs) {
      let record = seen.get(program);
      if (!record) {
        record = {
          name: program.name || '(unnamed)',
          // three's own cache key, so a permutation can be identified rather
          // than inferred from a name that several programs may share.
          key: String(program.cacheKey ?? ''),
          firstSeen: Math.round(now),
          readyAt: null,
          // Source size: the one static predictor that is free to collect. A
          // program that is expensive BECAUSE it is huge is a shader problem; one
          // that is expensive at ordinary size is a compiler problem, and they
          // have different fixes.
          chars: sourceChars(renderer, program),
        };
        seen.set(program, record);
      }
      if (record.readyAt === null && isReady(program)) record.readyAt = Math.round(now);
    }
    requestAnimationFrame(sample);
  };
  requestAnimationFrame(sample);

  /**
   * The curve, ready to hand across the CDP boundary.
   *
   * `cost` is the gap to the previously-completed program. The driver compiles
   * serially, so a program that lands 900 ms after the one before it took 900 ms
   * — this is the per-program driver time the WebGL probe cannot see.
   */
  window.__PROGCURVE__ = () => {
    const done = [...seen.values()]
      .filter((r) => r.readyAt !== null)
      .sort((a, b) => a.readyAt - b.readyAt);
    let previous = 0;
    const rows = done.map((r) => {
      const cost = r.readyAt - previous;
      previous = r.readyAt;
      return { ...r, cost };
    });
    return {
      total: seen.size,
      ready: rows.length,
      pending: [...seen.values()].filter((r) => r.readyAt === null).map((r) => r.name),
      rows,
    };
  };
}

function isReady(program) {
  try {
    return typeof program.isReady !== 'function' || program.isReady();
  } catch {
    return true;
  }
}

function sourceChars(renderer, program) {
  try {
    const gl = renderer.getContext();
    const length = (shader) => (shader ? gl.getShaderSource(shader)?.length ?? 0 : 0);
    return length(program.vertexShader) + length(program.fragmentShader);
  } catch {
    return null;
  }
}
