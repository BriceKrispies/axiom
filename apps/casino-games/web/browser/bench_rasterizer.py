#!/usr/bin/env -S uv run --with playwright python
# /// script
# requires-python = ">=3.10"
# dependencies = ["playwright>=1.48"]
# ///
"""
bench_rasterizer.py — a PAIRED, interleaved A/B of the software rasterizer's inner
loop, over the real triangles of a real frame.

Why this exists: on this machine, benchmarking two builds in two browser runs is
worthless. The same unchanged code measured 20.6ms and 34.1ms minutes apart -- the
machine simply runs at different speeds over time, and that 1.7x drift swamps any
optimization worth under 2x. Every cross-process comparison is drift plus signal
with no way to separate them.

The fix is to never compare across time. Both implementations live in ONE page and
run back-to-back on the SAME captured triangles, alternating which goes first, for
many rounds. Each round yields a PAIRED sample whose two halves are microseconds
apart, so drift affects both equally and cancels in the ratio. The report is the
median paired ratio plus a win count -- if B is genuinely faster it wins nearly
every round, regardless of what the clock speed was doing.

The workload is captured from the live app (`--tris`, produced by dumping one
frame's `rasterize` calls), so this measures real geometry, not a synthetic guess.

    uv run apps/casino-games/web/browser/bench_rasterizer.py --tris /tmp/tris.json
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from pathlib import Path

from playwright.sync_api import sync_playwright

BROWSER_ARGS = ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-gl=angle"]

# Both variants, written against flat typed arrays so neither pays a cost the other
# does not. `A` is the shipped per-pixel form; `B` steps the barycentrics per row.
BENCH = """
({ tris, width, height, rounds }) => {
  const n = tris.length / 13;
  const t = Float64Array.from(tris);
  const pixels = new Uint32Array(width * height);
  const depth = new Float32Array(width * height);

  const A = () => {
    for (let i = 0; i < n; i += 1) {
      const o = i * 13;
      const x0=t[o],y0=t[o+1],w0=t[o+2],x1=t[o+3],y1=t[o+4],w1=t[o+5],x2=t[o+6],y2=t[o+7],w2=t[o+8];
      const packed=(255<<24)|(t[o+11]<<16)|(t[o+10]<<8)|t[o+9];
      const area=(x1-x0)*(y2-y0)-(x2-x0)*(y1-y0);
      if (area === 0) continue;
      const inv=1/area;
      const minX=Math.max(0,Math.floor(Math.min(x0,x1,x2)));
      const maxX=Math.min(width-1,Math.ceil(Math.max(x0,x1,x2)));
      const minY=Math.max(0,Math.floor(Math.min(y0,y1,y2)));
      const maxY=Math.min(height-1,Math.ceil(Math.max(y0,y1,y2)));
      if (minX>maxX||minY>maxY) continue;
      for (let y=minY;y<=maxY;y+=1) {
        const py=y+0.5, rowBase=y*width;
        for (let x=minX;x<=maxX;x+=1) {
          const px=x+0.5;
          const l0=((x1-px)*(y2-py)-(x2-px)*(y1-py))*inv;
          const l1=((x2-px)*(y0-py)-(x0-px)*(y2-py))*inv;
          const l2=1-l0-l1;
          if (l0<0||l1<0||l2<0) continue;
          const invW=l0*w0+l1*w1+l2*w2;
          const idx=rowBase+x;
          if (invW<=depth[idx]) continue;
          depth[idx]=invW; pixels[idx]=packed;
        }
      }
    }
  };

  const B = () => {
    for (let i = 0; i < n; i += 1) {
      const o = i * 13;
      const x0=t[o],y0=t[o+1],w0=t[o+2],x1=t[o+3],y1=t[o+4],w1=t[o+5],x2=t[o+6],y2=t[o+7],w2=t[o+8];
      const packed=(255<<24)|(t[o+11]<<16)|(t[o+10]<<8)|t[o+9];
      const area=(x1-x0)*(y2-y0)-(x2-x0)*(y1-y0);
      if (area === 0) continue;
      const inv=1/area;
      const minX=Math.max(0,Math.floor(Math.min(x0,x1,x2)));
      const maxX=Math.min(width-1,Math.ceil(Math.max(x0,x1,x2)));
      const minY=Math.max(0,Math.floor(Math.min(y0,y1,y2)));
      const maxY=Math.min(height-1,Math.ceil(Math.max(y0,y1,y2)));
      if (minX>maxX||minY>maxY) continue;
      const a0=y1-y2, b0=x2-x1, c0=x1*y2-x2*y1;
      const a1=y2-y0, b1=x0-x2, c1=x2*y0-x0*y2;
      const sL0=a0*inv, sL1=a1*inv, pxStart=minX+0.5;
      for (let y=minY;y<=maxY;y+=1) {
        const py=y+0.5, rowBase=y*width;
        let l0=(c0+a0*pxStart+b0*py)*inv;
        let l1=(c1+a1*pxStart+b1*py)*inv;
        for (let x=minX;x<=maxX;x+=1,l0+=sL0,l1+=sL1) {
          const l2=1-l0-l1;
          if (l0<0||l1<0||l2<0) continue;
          const invW=l0*w0+l1*w1+l2*w2;
          const idx=rowBase+x;
          if (invW<=depth[idx]) continue;
          depth[idx]=invW; pixels[idx]=packed;
        }
      }
    }
  };

  const time = (fn) => { depth.fill(0); pixels.fill(0); const s=performance.now(); fn(); return performance.now()-s; };

  // Warm both to their optimized tier before any sample counts.
  for (let i=0;i<4;i+=1) { time(A); time(B); }

  const pairs = [];
  for (let round=0; round<rounds; round+=1) {
    // Alternate which runs first so ordering cannot favour either one.
    const aFirst = round % 2 === 0;
    const first = time(aFirst ? A : B);
    const second = time(aFirst ? B : A);
    pairs.push(aFirst ? { a: first, b: second } : { a: second, b: first });
  }

  // Verify both produce the same framebuffer, so we are not timing a wrong answer.
  depth.fill(0); pixels.fill(0); A();
  const afterA = Uint32Array.from(pixels);
  depth.fill(0); pixels.fill(0); B();
  let mismatched = 0;
  for (let i=0;i<afterA.length;i+=1) { if (afterA[i] !== pixels[i]) mismatched += 1; }
  return { pairs, mismatched, triangles: n };
}
"""


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tris", type=Path, required=True, help="captured frame triangles (JSON)")
    ap.add_argument("--width", type=int, default=480)
    ap.add_argument("--height", type=int, default=300)
    ap.add_argument("--rounds", type=int, default=40)
    args = ap.parse_args(argv)

    records = json.loads(args.tris.read_text(encoding="utf-8"))
    flat: list[float] = []
    for tri in records:
        flat += [
            tri["x0"], tri["y0"], tri["w0"],
            tri["x1"], tri["y1"], tri["w1"],
            tri["x2"], tri["y2"], tri["w2"],
            tri["r"], tri["g"], tri["b"], tri["opacity"],
        ]

    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True, args=BROWSER_ARGS)
        page = browser.new_context().new_page()
        page.goto("about:blank")
        got = page.evaluate(
            BENCH,
            {"tris": flat, "width": args.width, "height": args.height, "rounds": args.rounds},
        )
        browser.close()

    pairs = got["pairs"]
    a_times = [p["a"] for p in pairs]
    b_times = [p["b"] for p in pairs]
    ratios = [p["b"] / p["a"] for p in pairs if p["a"] > 0]
    wins = sum(1 for p in pairs if p["b"] < p["a"])

    print(f"[raster] {got['triangles']:,} real triangles at {args.width}x{args.height}, {len(pairs)} paired rounds")
    print(f"[raster] A per-pixel : median {statistics.median(a_times):7.3f} ms   min {min(a_times):7.3f}")
    print(f"[raster] B stepped   : median {statistics.median(b_times):7.3f} ms   min {min(b_times):7.3f}")
    print(f"[raster] paired ratio B/A: median {statistics.median(ratios):.4f}  (B faster in {wins}/{len(pairs)} rounds)")
    speedup = 1.0 / statistics.median(ratios)
    print(f"[raster] => stepped is {speedup:.3f}x the speed of per-pixel")
    print(f"[raster] framebuffer mismatch between the two: {got['mismatched']} pixels")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
