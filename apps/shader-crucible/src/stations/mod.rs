//! The ten stations, and the table that names them.
//!
//! Each station is a distinct, labelled subject: a viewer should be able to point
//! at one body in the scene and say which capability it proves. The table below
//! is the single list of them — the scene lays out its bodies from it, the
//! preparation barrier collects its surfaces from it, and the on-screen and
//! README labels are read off it, so the three can never disagree about what a
//! station is.

pub mod displacement;
pub mod implicit;
pub mod layered;
pub mod lighting;
pub mod live;
pub mod patterns;
pub mod retune;

use axiom_surface::Surface;

/// One station: what it is, what it proves, and the honest caveat that goes on
/// its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Station {
    /// The station number a viewer reads on screen, `1..=10`.
    pub number: u8,
    /// Its short name.
    pub name: &'static str,
    /// The one thing it demonstrates.
    pub proves: &'static str,
    /// The limitation stated beside it, or the empty string when it carries
    /// none. **A station with a caveat states it**; that is the difference
    /// between a demonstration and an advertisement.
    pub caveat: &'static str,
}

/// The ten stations, in the order the scene lays them out.
pub const STATIONS: [Station; 10] = [
    Station {
        number: 1,
        name: "Layered material",
        proves: "mask-driven layering flattening to Mix composition: metal + paint + scratch + dirt",
        caveat: "metallic is carried, digested and reported — and read by no lighting model, so it changes no pixel",
    },
    Station {
        number: 2,
        name: "Live procedural surface",
        proves: "graph -> Surface -> surface_program -> WGSL, evaluated per pixel",
        caveat: "",
    },
    Station {
        number: 3,
        name: "Baked texture",
        proves: "the same graph through TextureOp::Field: one graph, two realisations",
        caveat: "the bake writes LINEAR bytes and the albedo upload path binds them as Rgba8UnormSrgb, so the baked tile reads darker than the live surface",
    },
    Station {
        number: 4,
        name: "Parameter retune",
        proves: "nine tunings, one digest, one program: a retune is a uniform write",
        caveat: "",
    },
    Station {
        number: 5,
        name: "Time-varying displacement",
        proves: "vertex-stage fields on deterministic engine time",
        caveat: "the shadow pass runs no displacement program, so a displaced vertex casts an UNDISPLACED shadow",
    },
    Station {
        number: 6,
        name: "Three lighting models",
        proves: "Unlit / Lambert / LambertSpecular as a closed discriminant, zero extra pipelines",
        caveat: "",
    },
    Station {
        number: 7,
        name: "Implicit surface",
        proves: "a FieldGraph as a ScalarField, marched by implicit_surface_mesh",
        caveat: "the blobs are exp(-k*d^2) and not k/d^2: the algebra has no Div, deliberately",
    },
    Station {
        number: 8,
        name: "Transcendental patterns",
        proves: "marble and wood as authored graphs over Sin and Pow",
        caveat: "",
    },
    Station {
        number: 9,
        name: "Both backends",
        proves: "per-pixel on the GPU arm, per-triangle-centroid on Canvas2D: a reported substitute",
        caveat: "Canvas2D shades ONE sample per triangle, so a fine mask (station 1's scratches) can vanish there entirely — the mesh is deliberately NOT tessellated to hide it",
    },
    Station {
        number: 10,
        name: "Introspection",
        proves: "explain / digest / diff: the graph is machine-readable data, not opaque source",
        caveat: "",
    },
];

/// Every surface the crucible authors, in station order.
///
/// **This is what the preparation barrier is handed.** A station that authors
/// several surfaces contributes all of them; a station that authors none (9 and
/// 10 are reports about the others) contributes nothing.
pub fn all_surfaces() -> Vec<Surface> {
    let mut surfaces = vec![
        layered::layered_material(),
        live::live_surface(),
        retune::retune_surface(),
        displacement::wind_surface(),
        displacement::ripple_surface(),
    ];
    surfaces.extend(lighting::lighting_surfaces());
    surfaces.push(implicit::implicit_surface());
    surfaces.extend(patterns::pattern_surfaces());
    surfaces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_ten_stations_numbered_one_to_ten() {
        assert_eq!(STATIONS.len(), 10);
        STATIONS.iter().enumerate().for_each(|(index, station)| {
            assert_eq!(station.number as usize, index + 1);
            assert!(!station.name.is_empty());
            assert!(!station.proves.is_empty());
        });
    }

    /// **The four limitations each sit on a station's own label.** Not in a
    /// footnote, not in a comment — on the station they affect.
    #[test]
    fn the_four_limitations_are_each_stated_on_a_station() {
        let caveats: Vec<&str> = STATIONS
            .iter()
            .filter(|s| !s.caveat.is_empty())
            .map(|s| s.caveat)
            .collect();
        assert_eq!(caveats.len(), 5, "five caveats: the four limitations plus the sRGB seam");
        assert!(caveats.iter().any(|c| c.contains("UNDISPLACED")));
        assert!(caveats.iter().any(|c| c.contains("ONE sample per triangle")));
        assert!(caveats.iter().any(|c| c.contains("changes no pixel")));
    }

    /// **Every authored surface is legal.** A surface that did not validate would
    /// be rejected at the barrier, and the station would render its fallback with
    /// nobody told why.
    #[test]
    fn every_authored_surface_validates() {
        let surfaces = all_surfaces();
        assert_eq!(surfaces.len(), crate::levers::SURFACE_COUNT);
        surfaces.iter().enumerate().for_each(|(index, surface)| {
            assert_eq!(surface.validate(), Ok(()), "surface {index} is illegal");
            assert!(
                surface.flatten().is_ok(),
                "surface {index} does not flatten into one graph per channel"
            );
        });
    }

    /// **Every station's digest is pinned.** A digest is the identity a program
    /// cache keys on, so it must never move by accident. When one of these
    /// changes, the material changed — check that you meant it, then update the
    /// number.
    #[test]
    fn every_station_digest_is_the_committed_value() {
        let digests: Vec<String> = all_surfaces()
            .iter()
            .map(|s| format!("{:016X}", s.digest().raw()))
            .collect();
        digests
            .iter()
            .enumerate()
            .for_each(|(index, digest)| println!("surface {index}: {digest}"));
        assert_eq!(digests, crate::COMMITTED_DIGESTS.to_vec());
    }

    /// **Each surface is a distinct program.** Authored surfaces that collapsed
    /// to fewer digests would mean two stations were secretly the same material
    /// and the demo was showing one thing twice.
    #[test]
    fn every_surface_is_a_distinct_program() {
        let distinct: std::collections::BTreeSet<u64> =
            all_surfaces().iter().map(|s| s.digest().raw()).collect();
        assert_eq!(distinct.len(), crate::levers::SURFACE_COUNT);
    }
}
