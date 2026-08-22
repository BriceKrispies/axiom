//! What kind of program a surface names.
//!
//! A [`crate::Surface`] has always meant one thing: a set of channels bound to
//! field graphs, from which the GPU backend *generates* WGSL. That is
//! [`SurfaceKind::Field`], and it is the default and the overwhelming majority.
//!
//! [`SurfaceKind::RuntimeMaterial`] is the second kind. It names the
//! hand-written runtime material shader — the port of Claude-of-Duty's
//! `materials/shader.js` — which the field algebra deliberately cannot express:
//! parallax occlusion mapping is a bounded loop with a linear refine, de-tiling
//! needs `textureGrad` with explicit derivatives, and triplanar is nine texture
//! fetches. The algebra has no loops, no derivatives and no sampling, and its
//! branchlessness is the Branchless Law itself, so those absences are immovable.
//!
//! ## Why a kind on `Surface`, rather than a second mechanism
//!
//! The engine already has exactly one way to say "this material has a program":
//! author a surface, and the draw names its **content digest**. A hand-written
//! program with no surface behind it has no digest and nothing an app can name.
//! Giving `Surface` a kind puts the runtime material *inside* that mechanism
//! instead of alongside it, so content addressing, the preparation barrier, the
//! one-pipeline-per-distinct-program property and the program cap all keep
//! working with no new machinery.
//!
//! ## The parameters are NOT in the digest — except the one that changes the
//! program's *shape*
//!
//! [`crate::Surface::digest`] is structural: it excludes parameter *values* so
//! that retuning one cannot force a recompile. A runtime material follows the
//! same rule — the digest carries only [`SurfaceKind::code`], never the
//! [`MaterialParams`]. Two runtime materials with entirely different parameters
//! are therefore **one program and one pipeline**, differing only in the bytes
//! written to their parameter buffer. That is exactly the behaviour the source
//! has, where every extended material shares one shader and differs by uniforms.
//!
//! **De-tiling is the one exception, and it is not really an exception.** The
//! source gates it with `#ifdef OW_DETILE` — a *compile-time* permutation, not a
//! uniform — because a runtime `t = 0` through the height blend is not
//! bit-identical to omitting the block (measured: 1 ULP on 17.2% of operands).
//! So de-tiled and un-de-tiled are two different programs, and "structural"
//! means precisely that: a value that changes which code exists is structure,
//! however it is spelled in the parameter block.
//!
//! It therefore gets its own code. Without that, two materials differing only in
//! `detile` would share a digest, collide in the program cache, and one of them
//! would silently render the other's shader.

use crate::material_params::MaterialParams;

/// Which program a surface names. See the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SurfaceKind {
    /// Channels bound to field graphs; the backend generates the WGSL.
    #[default]
    Field,
    /// The hand-written runtime material shader, with its authored parameters.
    RuntimeMaterial(MaterialParams),
}

impl SurfaceKind {
    /// The wire code written into a surface's canonical bytes and its digest.
    ///
    /// Stable and order-independent: an added kind takes the next unused number
    /// rather than being inserted, because these are persisted in bytes that
    /// [`crate::Surface::deserialize`] reads back. This is the "an enum used as
    /// a table index is order-dependent" trap in its serialised form.
    pub fn code(self) -> u16 {
        // 0 = field, 1 = runtime material, 2 = runtime material with de-tiling.
        // Codes 1 and 2 are two different programs — see the module doc.
        let material = self.material_params();
        let detiled = material.map_or(false, |p| p.detile_enabled());
        let kind = usize::from(material.is_some()) + usize::from(detiled);
        [0, 1, 2][kind]
    }

    /// The authored parameters, when this is a runtime material.
    pub fn material_params(self) -> Option<MaterialParams> {
        match self {
            SurfaceKind::RuntimeMaterial(params) => Some(params),
            SurfaceKind::Field => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_kind_is_a_field_surface() {
        assert_eq!(SurfaceKind::default(), SurfaceKind::Field);
        assert_eq!(SurfaceKind::default().code(), 0);
    }

    /// The codes are written into bytes a reader decodes, so they are a wire
    /// format, not an implementation detail.
    #[test]
    fn the_wire_codes_are_pinned() {
        assert_eq!(SurfaceKind::Field.code(), 0);
        assert_eq!(
            SurfaceKind::RuntimeMaterial(MaterialParams::default()).code(),
            1
        );
        assert_eq!(
            SurfaceKind::RuntimeMaterial(MaterialParams {
                detile: 0.5,
                ..MaterialParams::default()
            })
            .code(),
            2
        );
    }

    /// The code ignores every parameter that only feeds a uniform — two runtime
    /// materials tuned differently are one program.
    #[test]
    fn the_code_ignores_parameters_that_only_move_uniforms() {
        let a = SurfaceKind::RuntimeMaterial(MaterialParams::default());
        let b = SurfaceKind::RuntimeMaterial(MaterialParams {
            scale: 17.5,
            parallax: 0.4,
            ..MaterialParams::default()
        });
        assert_eq!(a.code(), b.code());
        assert_ne!(a, b, "the kinds still differ; only the code is shared");
    }

    /// ...but `detile` changes which code exists, so it must NOT be ignored.
    /// Two materials differing only in `detile` sharing a digest would collide
    /// in the program cache and one would render the other's shader.
    #[test]
    fn detile_changes_the_code_because_it_changes_the_program() {
        let off = SurfaceKind::RuntimeMaterial(MaterialParams::default());
        let on = SurfaceKind::RuntimeMaterial(MaterialParams {
            detile: 0.35,
            ..MaterialParams::default()
        });
        assert_ne!(off.code(), on.code());
    }

    /// The source's gate is `p.detile > 0 && p.uvMode !== 'triplanar'`, so a
    /// triplanar material never de-tiles however high its `detile` is.
    #[test]
    fn triplanar_never_detiles_however_high_the_amount() {
        let tri = SurfaceKind::RuntimeMaterial(MaterialParams {
            detile: 1.0,
            uv_mode: crate::UvMode::Triplanar,
            ..MaterialParams::default()
        });
        assert_eq!(tri.code(), 1, "triplanar excludes de-tiling");
    }

    #[test]
    fn only_a_runtime_material_yields_parameters() {
        assert_eq!(SurfaceKind::Field.material_params(), None);
        let params = MaterialParams {
            scale: 3.0,
            ..MaterialParams::default()
        };
        assert_eq!(
            SurfaceKind::RuntimeMaterial(params).material_params(),
            Some(params)
        );
    }
}
