// Axiom asset-stream demo — pool worker (classic Web Worker).
//
// One of N identical background workers. The main thread (the wasm app) posts a
// single job at a time: {id, locator}. This worker fetches the blob and runs a
// CPU-bound placeholder "decode" pass over the bytes — the stand-in for future
// real wasm decode — entirely off the main thread, then reports {id, ok} back.
// The main thread frees this worker and hands it the next queued job.
//
// This is plain app/tooling JS (not the engine spine, not the @axiom/client SDK),
// so ordinary control flow is fine here.

// Tunable cost of the placeholder decode. Sized so each job takes a handful of ms
// — enough to be genuine off-main-thread CPU work without slowing the e2e test.
const DECODE_PASSES = 400000;

self.onmessage = async (event) => {
  const { id, locator } = event.data;
  try {
    const response = await fetch(locator);
    if (!response.ok) {
      throw new Error("HTTP " + response.status + " for " + locator);
    }
    const bytes = new Uint8Array(await response.arrayBuffer());

    // Placeholder decode: a deterministic CPU pass over the bytes. Real decoders
    // (mesh/texture/audio) will replace this — the point is it runs HERE, on the
    // worker, never on the main thread that keeps the page responsive.
    let checksum = 0 >>> 0;
    const len = bytes.length || 1;
    for (let pass = 0; pass < DECODE_PASSES; pass++) {
      checksum = (checksum + bytes[pass % len] + pass) >>> 0;
    }

    self.postMessage({ id, ok: true, bytes: bytes.length, checksum });
  } catch (err) {
    self.postMessage({ id, ok: false, error: String(err) });
  }
};
