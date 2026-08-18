# Port notes — `weapons::ballistics`, `weapons::defs`

## What was ported

- `C:/dev/Claude-of-Duty/src/weapons/ballistics.js:1-166` → `apps/shmup/src/weapons/ballistics.rs`
- `C:/dev/Claude-of-Duty/src/weapons/defs.js:1-320` → `apps/shmup/src/weapons/defs.rs`
- `apps/shmup/src/weapons/mod.rs` gained `pub mod ballistics;` / `pub mod defs;`
  (merged alongside a concurrently-landing `pub mod mathx;` from another agent).
- `apps/shmup/src/lib.rs` gained `pub mod weapons;`, inserted between the
  concurrently-added `rng`/`world` lines without disturbing them.
- Tests: `apps/shmup/tests/weapons_port.rs`.

## defs.rs — data tables

Ported as `const` structs (`WeaponDef`, `RecoilDef`) rather than the source's plain
object literals — one `const` per weapon (`RIFLE`, `SMG`, `PISTOL`), plus
`WEAPON_DEFS: [&WeaponDef; 3]` and `by_id(&str) -> Option<&'static WeaponDef>` for the
source's `WEAPON_DEFS[id]` lookup. Every numeric field is `f64` (a JS `number`
exactly), including spatial constants (`hipPos`, `hipRot`, …) — there was no reason to
narrow those to `f32` and doing so would have made them harder to diff against the
source. The large pose-derivation comment block on the rifle (`defs.js:71-111`) is
carried over verbatim as a doc comment on `RIFLE`, because the numbers only make sense
with the constraints that produced them.

`SPREAD_MODS` became `enum Stance` + `Stance::spread_mod(self) -> f64` instead of a
`HashMap`/array-of-tuples — an exhaustive match reads better than a lookup that could
silently miss a key, and apps are outside the Branchless Law so a `match` is the
idiomatic shape here.

**`DEG2RAD` divergence.** The source imports `DEG` from `mathx.js` and re-exports it
as `DEG2RAD`. `mathx.rs` was being authored by a different concurrent agent at the time
of this port (confirmed to exist by the time this port finished, but not stable when
work started). Rather than take a cross-file dependency on code landing concurrently
from another agent, `defs.rs` restates the constant locally:
`pub const DEG2RAD: f64 = std::f64::consts::PI / 180.0;`, with a comment noting this
and that it is safe to become `pub use crate::weapons::mathx::DEG as DEG2RAD;` once
`mathx.rs` is settled — the value is identical either way.

## ballistics.rs — the physics seam

The source reaches two capabilities off `ctx.peek('physics')`, which does not exist in
this port: `phys.raycast(origin, dir, maxDist, mask)` and `phys.fireBullet({…})` (the
penetration solver). Per the manifest's instruction, this is a trait/callback seam:

```rust
pub trait RaycastWorld {
    fn raycast(&self, origin: Vec3, dir: Vec3, max_dist: f64) -> Option<RaycastHit>;
    fn fire_bullet(&mut self, request: FireBulletRequest);
}
```

`ProjectileSim::spawn` and `ProjectileSim::fixed_update` take
`Option<&mut dyn RaycastWorld>`. Passing `None` reproduces the source's `if (phys) {…}`
guards exactly: rounds still integrate and expire on range/age/altitude with no physics
bound, they just never register a hit. **Nothing implements `RaycastWorld` yet** — that
is deliberately left for whichever future physics capability lands (a layer or module,
per the Layer/Module Law, not an app-tier stand-in). `tests/weapons_port.rs` has a
`MockWorld` used only to exercise the seam in tests.

**One simplification at the seam, documented in code:** the source threads
`phys.MASK?.BULLET` — a constant *physics itself* owns — through every `raycast` call.
That mask table doesn't exist yet (physics isn't ported), so `RaycastWorld::raycast`
drops the mask parameter entirely; the contract is "cast against whatever physics
considers bullet-blocking geometry" and the future physics binding decides what that
means. The **per-projectile** `mask` field (`p.mask`, threaded only to `fireBullet`)
*is* preserved, since that one genuinely varies per shot and isn't the same fixed
constant.

There is no `THREE.Vector3` and no vector type anywhere in this workspace (checked:
`axiom-kernel` has no `Vec3`/`Vector3`; no crate in the workspace pulls in `glam`).
`ballistics.rs` defines a minimal local `Vec3` (`f64` x/y/z, `Copy`) rather than
inventing a shared math primitive — that decision belongs to a layer if/when multiple
consumers need it, not to one app module.

**Precision:** everything is `f64`, matching a JS `number` exactly, specifically so
`falloff`/`range01` (pure `+ - * /`) golden-check by *exact* equality rather than a
tolerance.

## Source defect found and fixed (not silently)

`spawn`'s pool-exhaustion path (`ballistics.js:67-72`):

```js
if (!p) {
  p = this.live[0];
  if (!p) return null;
  this._retire(p);
}
```

never removes `this.live[0]` before `this.live.push(p)` a few lines later. Because JS
objects are references, the recycled round ends up listed **twice** in `this.live` —
so the next `fixedUpdate` steps it twice in one frame (effectively doubling its
velocity for that tick), and once the first occurrence dies and is spliced out, the
surviving stale entry keeps stepping an already-retired (`alive: false`) object.
`live.length` can grow past `MAX_LIVE` under sustained fire past pool capacity.

Per the recipe's "if fixing is clearly right, fix it, comment why, and cover it"
clause: this is fixed in the port (`self.live.remove(0)` before the retire), with the
reasoning recorded at the call site in `ballistics.rs`. The source's own comment —
"oldest round yields its slot" — only requires the old occupant stop being live and
the slot be reused once, not processed twice while also lingering as a phantom entry,
so this reads as an omission rather than an intended design. Covered by
`the_oldest_round_yields_its_slot_once_the_pool_is_exhausted`, which asserts
`live_count() == MAX_LIVE` after spawning one round past pool capacity — that
assertion fails under the unfixed (literal) translation.

## Golden capture

Captured with a temporary `capture_weapons_tmp.mjs` in `C:/dev/Claude-of-Duty`
(deleted after use, per the recipe), importing `buildRecoilPattern` from the real
`defs.js` and the real `rng.js`, run under Node 24:

- **Recoil patterns** for all three weapons (`rifle` 30 shots, `smg` 32, `pistol` 17):
  read back through the source's `Float32Array`, so the captured numbers are already
  narrowed to `f32` exactly as the source narrows them on write.
  - **Pitch component**: pure `+ - *` over `float()`/`signed()` draws, no trig →
    asserted with **exact `f32` equality**.
  - **Yaw component**: involves `Math.sin` → not bit-guaranteed across libm, so
    asserted with an **absolute tolerance of `1e-6`** on the `f32` value (a few `f32`
    ulps — `f32` relative precision is ~1.2e-7 — enough to absorb any `f64`-level
    `sin()` disagreement between V8 and Rust's libm without masking a genuine drift).
- **Falloff curve**: `1 - (1 - dropoff) * range01²` over 5 `dropoff` values × 7
  `range01` samples (35 points spanning the extremes 0.0/1.0 and the three weapons'
  real `dropoff`s) → pure `+ - *`, asserted with **exact `f64` equality**.

All golden tests pass (`cargo test -p axiom-shmup`: 108 tests total across the
crate, all green).

## Verification

- `cargo test -p axiom-shmup` — pass (108 tests: 22 unit + 53 core_port + 16
  weapons_mathx_port + 17 weapons_port).
- `cargo xtask check-architecture` — pass, no violations.
- Coverage gate not run (per recipe — runs centrally).

## What is NOT ported / left for later

- The Three.js-facing half of `src/weapons/` (weapon controllers, the viewmodel rig,
  `parts.js`, `clips.js`, `hands.js`, `geometry.js`) — draws meshes and reads input,
  neither of which has a home in this port yet.
- Any `RaycastWorld` implementation — physics has not landed as a layer/module.
- Wiring `ProjectileSim`/weapon firing into the `Subsystem`/`Registry`/`Ctx` machinery
  — this port is data + pure simulation only, callable standalone (as the tests do);
  engine wiring is a separate, later step once there's something for it to fire at.
