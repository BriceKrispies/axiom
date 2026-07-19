// verify.mjs — the keystone determinism check. Compares the TRANSPILED hash01
// (GDScript -> JS via gd2ts) against the canonical vec.ts algorithm. If every
// value matches, determinism is preserved across the whole round trip
// (TS -> GDScript -> JS), which is what makes a JS build of the game replay
// identically to the Godot build.
import { hash01 } from "./out/math_util.mjs";

// The reference: the original apps/axiom-home-run web/src/vec.ts hash01.
function ref(seed, keys) {
  let h = (seed | 0) ^ 0x9e3779b9;
  for (const k of keys) {
    h = Math.imul(h ^ (k | 0), 0x85ebca6b);
    h ^= h >>> 13;
    h = Math.imul(h, 0xc2b2ae35);
    h ^= h >>> 16;
  }
  return (h >>> 8) / 16777216;
}

let n = 0, fails = 0;
for (let seed = 0; seed < 40; seed++) {
  for (let a = 0; a < 40; a++) {
    for (let b = 0; b < 25; b++) {
      n++;
      const got = hash01(seed, [a, b]);
      const exp = ref(seed, [a, b]);
      if (got !== exp) {
        if (fails < 5) console.log(`MISMATCH seed=${seed} keys=[${a},${b}]  got=${got}  exp=${exp}`);
        fails++;
      }
    }
  }
}
console.log(`\n${n} cases, ${fails} mismatches`);
console.log(fails === 0
  ? "PASS — transpiled GDScript hash01 is BIT-IDENTICAL to the TS original."
  : "FAIL — determinism broke in the transpile.");
console.log("sample: hash01(1,[0,5]) =", hash01(1, [0, 5]), " ref =", ref(1, [0, 5]));
process.exit(fails === 0 ? 0 : 1);
