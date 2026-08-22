//! **`_updateRooms`** — the coarse interior volumes the indirect gate tests
//! against, and the world-to-level transform that addresses them.
//!
//! Skylight does not reach the middle of a closed room. Without an indirect
//! floor inside one, a doorway reads as a hole cut in a card, because the room
//! is brighter than the street outside it. The gate that fixes that needs to
//! know where the rooms *are*, and this is where that is computed — **once**,
//! when the world first appears, and then never again.
//!
//! # Why footprints and not geometry
//!
//! The volumes are the enterable buildings' own footprints, which the world
//! subsystem already publishes. Per-room geometry is not needed because the
//! gate keys off *depth inside the footprint*, and a wall's outer skin is at
//! depth 0 while its inner skin is one thickness in.
//!
//! # The yaw is recovered, not assumed
//!
//! The level is authored on one yaw, so a world position reaches level space
//! through a 2D rotation. Rather than reading that angle from the world
//! subsystem, `_updateRooms` recovers it by transforming **two** level-space
//! points and taking the difference:
//!
//! ```js
//! const o  = world.levelToWorld(0, 0, 0, this._tmpV3);
//! const ex = world.levelToWorld(1, 0, 0, this._tmpV3b);
//! const c = ex.x - ox, sn = ex.z - oz;
//! const inv = 1 / Math.max(1e-6, Math.hypot(c, sn));
//! ```
//!
//! That keeps it correct if the world subsystem ever re-authors its transform,
//! which is the sort of decision worth carrying across a port intact.
//!
//! # `Math.hypot` is not `sqrt(x*x + y*y)`
//!
//! V8's `Math.hypot` is a **max-scaled Kahan-compensated** sum, and this port
//! has measured the disagreement with the naive root at 25-41% of bit patterns
//! depending on the input distribution. [`hypot2`] is V8's algorithm.
//!
//! It is transcribed here rather than reached for because the app's
//! `crate::jsmath` lives in `apps/shmup` and a module may not depend on an app.
//! **That is a structural signal, not a convenience**: the JS builtin
//! primitives are used by both the app tier and (now) the spine, so their
//! correct home is a layer — the kernel, on the same argument that put
//! `Meters`/`Radians` there. Raised for the orchestrator; see
//! `docs/work-manifests/shmup-port/notes/render-frame-graph.md` §7.

/// `MAX_ROOMS` from `src/render/materialpatch.js` — the uniform array's length,
/// and therefore the hard ceiling on how many interior volumes a level can
/// publish. The source's loop is `if (n >= rooms.length) break`.
pub(crate) const MAX_ROOMS: usize = 10;

/// The floor of every room volume, in metres: below the ground slab, so the
/// floor plate itself counts as interior.
pub(crate) const ROOM_FLOOR_Y: f64 = -0.8;

/// How far under the roof deck (or under a setback's terrace) a room's ceiling
/// sits.
pub(crate) const ROOM_CEILING_INSET: f64 = 0.06;

/// `b.roofY ?? 12` — the roof height assumed for a building that does not
/// publish one.
pub(crate) const DEFAULT_ROOF_Y: f64 = 12.0;

/// `Math.hypot(x, y)`, as V8 implements it: scale by the largest magnitude,
/// then sum the squares with a Kahan compensation, then unscale.
///
/// The naive `(x * x + y * y).sqrt()` disagrees on a large fraction of inputs,
/// measured five times independently across this port at 25-41%.
///
/// V8 substitutes `1` for a zero scale rather than returning early, so the
/// all-zero case still runs the sum and still yields `+0.0`. A `NaN` argument
/// never becomes the scale (`NaN > max` is false) and instead reaches the sum
/// and poisons it, which is also what V8 does.
pub(crate) fn hypot2(x: f64, y: f64) -> f64 {
    let ax = x.abs();
    let ay = y.abs();
    let infinite = (ax == f64::INFINITY) | (ay == f64::INFINITY);
    // `n > max` is false for NaN, so a NaN never wins the scale.
    let largest = [0.0_f64, ax][usize::from(ax > 0.0)];
    let largest = [largest, ay][usize::from(ay > largest)];
    let scale = [largest, 1.0][usize::from(largest == 0.0)];

    let (sum, _) = [ax, ay].iter().fold((0.0_f64, 0.0_f64), |(sum, comp), &v| {
        let n = v / scale;
        let summand = n * n - comp;
        let preliminary = sum + summand;
        ((preliminary), (preliminary - sum) - summand)
    });
    [sum.sqrt() * scale, f64::INFINITY][usize::from(infinite)]
}

/// `owRoomXf` — the world-to-level 2D transform, as the `vec4` the shader
/// reads: `(cos, sin, tx, tz)`, applied as
/// `lx = x * xf.x + z * xf.y + xf.z`, `lz = -x * xf.y + z * xf.x + xf.w`.
///
/// `origin` is `levelToWorld(0, 0, 0)`'s `(x, z)` and `unit_x` is
/// `levelToWorld(1, 0, 0)`'s. The translation terms are the source's
/// `-(ox * cs + oz * sni)` and `-(-ox * sni + oz * cs)`, in that grouping.
pub(crate) fn room_transform(origin: (f64, f64), unit_x: (f64, f64)) -> [f64; 4] {
    let (ox, oz) = origin;
    let c = unit_x.0 - ox;
    let sn = unit_x.1 - oz;
    let inv = 1.0 / hypot2(c, sn).max(1e-6);
    let cs = c * inv;
    let sni = sn * inv;
    [cs, sni, -(ox * cs + oz * sni), -(-ox * sni + oz * cs)]
}

/// What the world subsystem publishes for one building — the subset
/// `_updateRooms` reads from `world.buildings[]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BuildingFootprint {
    /// `spec.x` — the footprint's centre, in level space.
    pub(crate) x: f64,
    /// `spec.z`.
    pub(crate) z: f64,
    /// `spec.w` — the footprint's full width; the volume stores its half.
    pub(crate) width: f64,
    /// `spec.d` — full depth; likewise halved.
    pub(crate) depth: f64,
    /// `spec.enterable !== true` skips the building entirely.
    pub(crate) enterable: bool,
    /// `spec.collapse === true` skips it: a collapsed shell is open to the sky
    /// and must keep its skylight.
    pub(crate) collapse: bool,
    /// `spec.ruin === true`, for the same reason.
    pub(crate) ruin: bool,
    /// `b.roofY`; `None` takes [`DEFAULT_ROOF_Y`].
    pub(crate) roof_y: Option<f64>,
    /// `b.floorY[spec.setback.from]`, when both the setback and that floor
    /// exist. A setback's terrace is outdoors and sits *inside* the footprint,
    /// so the volume has to stop under it rather than under the roof.
    pub(crate) setback_floor_y: Option<f64>,
}

/// One published interior volume, as the two `vec4`s the shader reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RoomVolume {
    /// `owRooms[n]` — `(x, z, halfWidth, halfDepth)` in level space.
    pub(crate) xz: [f64; 4],
    /// `owRoomsY[n]` — `(floor, ceiling, 0, 0)` in world Y.
    pub(crate) y: [f64; 4],
}

/// `_updateRooms`'s volume loop.
///
/// Skips anything not enterable, collapsed or ruined; stops at
/// [`MAX_ROOMS`]. The returned length is `owIndirect.value.z`, the count the
/// shader's gate loops to.
pub(crate) fn room_volumes(buildings: &[BuildingFootprint]) -> Vec<RoomVolume> {
    buildings
        .iter()
        .filter(|b| b.enterable & !b.collapse & !b.ruin)
        .take(MAX_ROOMS)
        .map(|b| {
            let roof = b.roof_y.unwrap_or(DEFAULT_ROOF_Y);
            // A setback's floor replaces the roof outright — not a min, and not
            // a clamp: the source assigns over `top`.
            let reference = b.setback_floor_y.unwrap_or(roof);
            RoomVolume {
                xz: [b.x, b.z, b.width * 0.5, b.depth * 0.5],
                y: [ROOM_FLOOR_Y, reference - ROOM_CEILING_INSET, 0.0, 0.0],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        hypot2, room_transform, room_volumes, BuildingFootprint, DEFAULT_ROOF_Y, MAX_ROOMS,
        ROOM_CEILING_INSET, ROOM_FLOOR_Y,
    };

    fn shell() -> BuildingFootprint {
        BuildingFootprint {
            x: 0.0,
            z: 0.0,
            width: 8.0,
            depth: 6.0,
            enterable: true,
            collapse: false,
            ruin: false,
            roof_y: None,
            setback_floor_y: None,
        }
    }

    /// V8's compensated hypot, against the naive root. They agree on the easy
    /// cases and the compensation is what the port depends on elsewhere.
    #[test]
    fn the_hypot_is_v8s_compensated_one() {
        assert_eq!(hypot2(3.0, 4.0), 5.0);
        assert_eq!(hypot2(0.0, 0.0), 0.0);
        assert_eq!(hypot2(-5.0, 0.0), 5.0);
        assert_eq!(hypot2(0.0, -5.0), 5.0);
        // Scaling by the maximum is what keeps a huge pair finite where the
        // naive square would overflow.
        let big = 1.0e200;
        assert!(hypot2(big, big).is_finite());
        assert!((big * big + big * big).sqrt().is_infinite());
        // Infinity short-circuits to infinity even beside a NaN, as V8 does.
        assert_eq!(hypot2(f64::INFINITY, f64::NAN), f64::INFINITY);
        assert_eq!(hypot2(f64::NAN, f64::INFINITY), f64::INFINITY);
        // A NaN never becomes the scale; it poisons the sum instead.
        assert!(hypot2(f64::NAN, 1.0).is_nan());
        assert!(hypot2(1.0, f64::NAN).is_nan());
        // A tiny pair still normalises through the max scale.
        assert!((hypot2(3.0e-200, 4.0e-200) - 5.0e-200).abs() < 1.0e-215);
    }

    /// An identity level transform round-trips a world point to itself.
    #[test]
    fn an_unrotated_level_transform_is_the_identity() {
        let xf = room_transform((0.0, 0.0), (1.0, 0.0));
        assert_eq!(xf, [1.0, 0.0, -0.0, -0.0]);
        let (x, z) = (7.0, -3.0);
        assert_eq!(x * xf[0] + z * xf[1] + xf[2], 7.0);
        assert_eq!(-x * xf[1] + z * xf[0] + xf[3], -3.0);
    }

    /// A rotated, translated level maps its own origin to level `(0, 0)` and
    /// its own unit-x point to level `(1, 0)` — which is what recovering the
    /// yaw from two transformed points is *for*.
    #[test]
    fn the_recovered_yaw_round_trips_the_two_points_it_was_recovered_from() {
        // A level authored 30 degrees off, with its origin at (12, -5).
        let angle: f64 = std::f64::consts::FRAC_PI_6;
        let origin = (12.0, -5.0);
        let unit_x = (origin.0 + angle.cos(), origin.1 + angle.sin());
        let xf = room_transform(origin, unit_x);

        let to_level = |x: f64, z: f64| (x * xf[0] + z * xf[1] + xf[2], -x * xf[1] + z * xf[0] + xf[3]);
        let at_origin = to_level(origin.0, origin.1);
        assert!(at_origin.0.abs() < 1e-12, "origin x was {}", at_origin.0);
        assert!(at_origin.1.abs() < 1e-12, "origin z was {}", at_origin.1);
        let at_unit = to_level(unit_x.0, unit_x.1);
        assert!((at_unit.0 - 1.0).abs() < 1e-12, "unit x was {}", at_unit.0);
        assert!(at_unit.1.abs() < 1e-12, "unit z was {}", at_unit.1);
    }

    /// A degenerate transform (both sample points identical) is floored rather
    /// than dividing by zero, and yields a finite — if meaningless — basis.
    #[test]
    fn a_degenerate_level_transform_is_floored_rather_than_infinite() {
        let xf = room_transform((3.0, 4.0), (3.0, 4.0));
        assert!(xf.iter().all(|v| v.is_finite()));
        assert_eq!(xf[0], 0.0);
        assert_eq!(xf[1], 0.0);
    }

    /// Only enterable, un-collapsed, un-ruined buildings publish a volume, and
    /// a collapsed shell keeps its skylight for a stated reason.
    #[test]
    fn a_collapsed_or_ruined_shell_publishes_no_interior_volume() {
        let buildings = [
            shell(),
            BuildingFootprint { enterable: false, ..shell() },
            BuildingFootprint { collapse: true, ..shell() },
            BuildingFootprint { ruin: true, ..shell() },
            BuildingFootprint { x: 20.0, ..shell() },
        ];
        let volumes = room_volumes(&buildings);
        assert_eq!(volumes.len(), 2, "one open building would read as a cave");
        assert_eq!(volumes[0].xz, [0.0, 0.0, 4.0, 3.0]);
        assert_eq!(volumes[1].xz, [20.0, 0.0, 4.0, 3.0]);
        // The footprint's full extents are halved on the way in.
        assert_eq!(volumes[0].xz[2] * 2.0, shell().width);
    }

    /// The ceiling: under the roof deck, or under a setback's terrace floor,
    /// with the default roof for a building that publishes none.
    #[test]
    fn the_ceiling_stops_under_the_roof_or_under_a_setbacks_terrace() {
        let plain = room_volumes(&[shell()]);
        assert_eq!(
            plain[0].y,
            [ROOM_FLOOR_Y, DEFAULT_ROOF_Y - ROOM_CEILING_INSET, 0.0, 0.0]
        );

        let tall = room_volumes(&[BuildingFootprint { roof_y: Some(21.0), ..shell() }]);
        assert_eq!(tall[0].y[1], 21.0 - ROOM_CEILING_INSET);

        // A setback replaces the roof outright, and is below it here.
        let setback = room_volumes(&[BuildingFootprint {
            roof_y: Some(21.0),
            setback_floor_y: Some(9.0),
            ..shell()
        }]);
        assert_eq!(setback[0].y[1], 9.0 - ROOM_CEILING_INSET);
        // ...and it is an assignment, not a minimum: a setback *above* the roof
        // wins too, because the source writes over `top`.
        let odd = room_volumes(&[BuildingFootprint {
            roof_y: Some(9.0),
            setback_floor_y: Some(21.0),
            ..shell()
        }]);
        assert_eq!(odd[0].y[1], 21.0 - ROOM_CEILING_INSET);
    }

    /// The uniform array is ten deep and the source breaks rather than growing.
    #[test]
    fn no_more_than_ten_volumes_are_published() {
        let many: Vec<BuildingFootprint> = (0..25)
            .map(|i| BuildingFootprint { x: f64::from(i), ..shell() })
            .collect();
        let volumes = room_volumes(&many);
        assert_eq!(volumes.len(), MAX_ROOMS);
        // The first ten, in publication order — not a selection.
        assert_eq!(volumes[9].xz[0], 9.0);
        assert!(room_volumes(&[]).is_empty());
    }
}
