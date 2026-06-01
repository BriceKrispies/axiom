//! The rotating-cube scene expressed as **data** — and a serializable
//! *document* (not only a Rust literal).
//!
//! `CubeSliceDriver` interprets a `SceneContent` value generically each tick:
//! it spawns the nodes, attaches an engine `Spin` (so rotation is animated by
//! the engine, not app code), and registers the materials. A different scene is
//! a different `SceneContent` value — and because the type round-trips through
//! the kernel `Reflect` binary format ([`SceneContent::to_bytes`] /
//! [`SceneContent::from_bytes`]), that value can come from a file, a network
//! fetch, or an agent — i.e. a scene can be authored without recompiling.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};
use axiom_math::Vec3;

/// One cube: which axis it spins about, how far along x it sits, how many ticks
/// a full revolution takes, and its material colour (linear RGBA).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CubeSpec {
    pub spin_axis: Vec3,
    pub offset_x: f32,
    pub period_ticks: u32,
    pub color: [f32; 4],
}

/// The camera: how far back it sits on +z, and its perspective intrinsics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CameraSpec {
    pub offset_z: f32,
    pub fovy_radians: f32,
    pub near: f32,
    pub far: f32,
}

/// The single directional light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LightSpec {
    pub direction_world: Vec3,
    pub color: Vec3,
    pub intensity: f32,
}

/// A whole rotating-cube scene as data.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SceneContent {
    pub clear_color: [f32; 4],
    pub cubes: Vec<CubeSpec>,
    pub camera: CameraSpec,
    pub light: LightSpec,
}

/// Write a linear-RGBA quadruple.
fn write_rgba(color: &[f32; 4], writer: &mut BinaryWriter) {
    for component in color {
        component.reflect_write(writer);
    }
}

/// Read a linear-RGBA quadruple.
fn read_rgba(reader: &mut BinaryReader<'_>) -> KernelResult<[f32; 4]> {
    Ok([
        f32::reflect_read(reader)?,
        f32::reflect_read(reader)?,
        f32::reflect_read(reader)?,
        f32::reflect_read(reader)?,
    ])
}

impl Reflect for CubeSpec {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "CubeSpec",
        &[
            FieldSchema::new("spin_axis", "Vec3"),
            FieldSchema::new("offset_x", "f32"),
            FieldSchema::new("period_ticks", "u32"),
            FieldSchema::new("color", "Vec4"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.spin_axis.reflect_write(writer);
        self.offset_x.reflect_write(writer);
        self.period_ticks.reflect_write(writer);
        write_rgba(&self.color, writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Ok(CubeSpec {
            spin_axis: Vec3::reflect_read(reader)?,
            offset_x: f32::reflect_read(reader)?,
            period_ticks: u32::reflect_read(reader)?,
            color: read_rgba(reader)?,
        })
    }
}

impl Reflect for CameraSpec {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "CameraSpec",
        &[
            FieldSchema::new("offset_z", "f32"),
            FieldSchema::new("fovy_radians", "f32"),
            FieldSchema::new("near", "f32"),
            FieldSchema::new("far", "f32"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.offset_z.reflect_write(writer);
        self.fovy_radians.reflect_write(writer);
        self.near.reflect_write(writer);
        self.far.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Ok(CameraSpec {
            offset_z: f32::reflect_read(reader)?,
            fovy_radians: f32::reflect_read(reader)?,
            near: f32::reflect_read(reader)?,
            far: f32::reflect_read(reader)?,
        })
    }
}

impl Reflect for LightSpec {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "LightSpec",
        &[
            FieldSchema::new("direction_world", "Vec3"),
            FieldSchema::new("color", "Vec3"),
            FieldSchema::new("intensity", "f32"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.direction_world.reflect_write(writer);
        self.color.reflect_write(writer);
        self.intensity.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Ok(LightSpec {
            direction_world: Vec3::reflect_read(reader)?,
            color: Vec3::reflect_read(reader)?,
            intensity: f32::reflect_read(reader)?,
        })
    }
}

impl Reflect for SceneContent {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "SceneContent",
        &[
            FieldSchema::new("clear_color", "Vec4"),
            FieldSchema::new("cubes", "Vec<CubeSpec>"),
            FieldSchema::new("camera", "CameraSpec"),
            FieldSchema::new("light", "LightSpec"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        write_rgba(&self.clear_color, writer);
        writer.write_u32(self.cubes.len() as u32);
        for cube in &self.cubes {
            cube.reflect_write(writer);
        }
        self.camera.reflect_write(writer);
        self.light.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        let clear_color = read_rgba(reader)?;
        let count = reader.read_u32()?;
        // Do not pre-allocate from an untrusted count; a truncated document
        // simply fails on the next read.
        let mut cubes = Vec::new();
        for _ in 0..count {
            cubes.push(CubeSpec::reflect_read(reader)?);
        }
        Ok(SceneContent {
            clear_color,
            cubes,
            camera: CameraSpec::reflect_read(reader)?,
            light: LightSpec::reflect_read(reader)?,
        })
    }
}

impl SceneContent {
    /// Serialize this scene to the kernel binary document format.
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        self.reflect_write(&mut writer);
        writer.into_bytes()
    }

    /// Load a scene from a document produced by [`Self::to_bytes`]. A truncated
    /// or malformed document is a clean error, never a panic.
    pub(crate) fn from_bytes(bytes: &[u8]) -> KernelResult<Self> {
        SceneContent::reflect_read(&mut BinaryReader::new(bytes))
    }
}

/// The demo scene: three cubes on distinct spin axes, a pulled-back camera, and
/// one white directional light. This value *is* the app's content.
pub(crate) fn demo_scene() -> SceneContent {
    SceneContent {
        clear_color: [0.05, 0.06, 0.08, 1.0],
        cubes: vec![
            CubeSpec {
                spin_axis: Vec3::UNIT_Y,
                offset_x: -2.6,
                period_ticks: 360,
                color: [0.85, 0.25, 0.25, 1.0], // red
            },
            CubeSpec {
                spin_axis: Vec3::UNIT_X,
                offset_x: 0.0,
                period_ticks: 360,
                color: [0.30, 0.80, 0.35, 1.0], // green
            },
            CubeSpec {
                spin_axis: Vec3::new(1.0, 1.0, 0.0),
                offset_x: 2.6,
                period_ticks: 360,
                color: [0.30, 0.50, 0.95, 1.0], // blue
            },
        ],
        camera: CameraSpec {
            offset_z: 8.0,
            fovy_radians: std::f32::consts::FRAC_PI_3,
            near: 0.1,
            far: 100.0,
        },
        light: LightSpec {
            direction_world: Vec3::new(0.3, -1.0, 0.4),
            color: Vec3::ONE,
            intensity: 1.0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_scene_round_trips_through_a_document() {
        let original = demo_scene();
        let bytes = original.to_bytes();
        let loaded = SceneContent::from_bytes(&bytes).expect("the document decodes");
        assert_eq!(original, loaded);
    }

    #[test]
    fn a_truncated_document_is_rejected_not_panicked() {
        let bytes = demo_scene().to_bytes();
        assert!(SceneContent::from_bytes(&bytes[..bytes.len() / 2]).is_err());
        assert!(SceneContent::from_bytes(&[]).is_err());
    }
}
