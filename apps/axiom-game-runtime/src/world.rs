//! The retained-world component vocabulary + the branchless kind→codec dispatch
//! (SPEC-02) the bridge routes `worldSet`/`worldGet` through.
//!
//! ## The marshalling convention (load-bearing — every later subsystem reuses it)
//! A component crosses the wasm boundary as a `(kind: string, fields: bytes)`
//! pair, never as a structured JS object:
//! - `kind` is the component's [`Reflect`] **schema name** — and it is exactly the
//!   TS `Component.kind` discriminant. The schema name *is* the routing key, so the
//!   engine's `query_dynamic` (which keys the dynamic store by schema name) needs
//!   no second table.
//! - `fields` are exactly the kernel [`Reflect`] wire bytes for that kind's struct:
//!   little-endian scalars; a `String` as a `u32`-LE length prefix then UTF-8
//!   (`BinaryWriter::write_byte_slice`). There is **no** kind tag or schema embedded
//!   in the bytes — the kind travels as the separate string argument. This is
//!   precisely what `T::reflect_write` produces and `T::reflect_read` consumes, so
//!   both ends of the boundary share the *one* engine codec rather than an
//!   app-invented framing. The TS platform edge (`raf-loop.ts`) holds the matching
//!   per-kind field (en/de)coder; this module holds the Rust half.
//!
//! Dispatch is a slice of `(kind, set-fn, get-fn)` rows scanned with
//! `iter().find(...)` — no `match`/`if` — so an unknown kind is a clean no-op
//! (`set` → `false`) / empty read (`get` → `&[]`), exactly as a stale entity is.
//! `worldSpawn(components)` is **not** a method here: the TS edge composes it as
//! spawn-empty + one `worldSet` per component, so the boundary stays scalar / byte
//! / string only and never marshals an array of buffers.

use axiom::prelude::{
    BinaryReader, BinaryWriter, Entity, FieldSchema, KernelResult, Reflect, RunningApp, TypeSchema,
};

/// Serialize a [`Reflect`] value to its field bytes (the boundary `fields` half).
pub fn encode<T: Reflect>(value: &T) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    value.reflect_write(&mut writer);
    writer.into_bytes()
}

/// Write a `String` field in the kernel wire format (length-prefixed UTF-8).
fn write_string(writer: &mut BinaryWriter, value: &str) {
    writer.write_byte_slice(value.as_bytes());
}

/// Read a `String` field written by [`write_string`]. A non-UTF-8 body decodes
/// losslessly for valid input (both ends always write valid UTF-8) and
/// deterministically (replacement chars) for corrupt input — never a panic.
fn read_string(reader: &mut BinaryReader<'_>) -> KernelResult<String> {
    reader
        .read_byte_slice()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

/// A 2D transform: position, rotation (radians), and per-axis scale. Schema name
/// `"Transform"` — the TS `Transform` component kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2D {
    /// X position.
    pub x: f32,
    /// Y position.
    pub y: f32,
    /// Rotation in radians.
    pub rotation: f32,
    /// X scale.
    pub scale_x: f32,
    /// Y scale.
    pub scale_y: f32,
}

impl Reflect for Transform2D {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "Transform",
        &[
            FieldSchema::new("x", "f32"),
            FieldSchema::new("y", "f32"),
            FieldSchema::new("rotation", "f32"),
            FieldSchema::new("scaleX", "f32"),
            FieldSchema::new("scaleY", "f32"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.x.reflect_write(writer);
        self.y.reflect_write(writer);
        self.rotation.reflect_write(writer);
        self.scale_x.reflect_write(writer);
        self.scale_y.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        f32::reflect_read(reader).and_then(|x| {
            f32::reflect_read(reader).and_then(|y| {
                f32::reflect_read(reader).and_then(|rotation| {
                    f32::reflect_read(reader).and_then(|scale_x| {
                        f32::reflect_read(reader).map(|scale_y| Transform2D {
                            x,
                            y,
                            rotation,
                            scale_x,
                            scale_y,
                        })
                    })
                })
            })
        })
    }
}

/// A 2D linear velocity. Schema name `"Velocity"`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Velocity2D {
    /// X velocity.
    pub x: f32,
    /// Y velocity.
    pub y: f32,
}

impl Reflect for Velocity2D {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "Velocity",
        &[FieldSchema::new("x", "f32"), FieldSchema::new("y", "f32")],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.x.reflect_write(writer);
        self.y.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        f32::reflect_read(reader)
            .and_then(|x| f32::reflect_read(reader).map(|y| Velocity2D { x, y }))
    }
}

/// A textured-sprite render component. Schema name `"Sprite"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    /// The texture key.
    pub texture: String,
}

impl Reflect for Sprite {
    const SCHEMA: TypeSchema =
        TypeSchema::new("Sprite", &[FieldSchema::new("texture", "String")]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        write_string(writer, &self.texture);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        read_string(reader).map(|texture| Sprite { texture })
    }
}

/// A text render component. Schema name `"Text"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    /// The displayed string.
    pub value: String,
}

impl Reflect for Text {
    const SCHEMA: TypeSchema = TypeSchema::new("Text", &[FieldSchema::new("value", "String")]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        write_string(writer, &self.value);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        read_string(reader).map(|value| Text { value })
    }
}

/// A filled-rectangle render component. Schema name `"Rectangle"`. The fill
/// `color` is an opaque packed `u32` (e.g. `0xRRGGBB`) the engine never interprets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rectangle {
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
    /// Packed fill colour.
    pub color: u32,
}

impl Reflect for Rectangle {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "Rectangle",
        &[
            FieldSchema::new("width", "f32"),
            FieldSchema::new("height", "f32"),
            FieldSchema::new("color", "u32"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.width.reflect_write(writer);
        self.height.reflect_write(writer);
        self.color.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        f32::reflect_read(reader).and_then(|width| {
            f32::reflect_read(reader).and_then(|height| {
                u32::reflect_read(reader).map(|color| Rectangle {
                    width,
                    height,
                    color,
                })
            })
        })
    }
}

/// A static-image render component. Schema name `"Image"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// The texture key.
    pub texture: String,
}

impl Reflect for Image {
    const SCHEMA: TypeSchema = TypeSchema::new("Image", &[FieldSchema::new("texture", "String")]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        write_string(writer, &self.texture);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        read_string(reader).map(|texture| Image { texture })
    }
}

/// Decode `bytes` as `T` and set it on `entity` (a stale handle / bad bytes → a
/// clean `false`). One row's `set` half, monomorphized per component type.
fn set_of<T: Reflect>(app: &mut RunningApp, entity: Entity, bytes: &[u8]) -> bool {
    T::reflect_read(&mut BinaryReader::new(bytes))
        .map(|value| app.set_dynamic(entity, value))
        .unwrap_or(false)
}

/// Read `entity`'s `T` and re-encode it to field bytes (`None` on a miss). One
/// row's `get` half, monomorphized per component type.
fn get_of<T: Reflect>(app: &RunningApp, entity: Entity) -> Option<Vec<u8>> {
    app.get_dynamic::<T>(entity).map(|value| encode(&value))
}

/// One row of the kind→codec dispatch table: the schema-name key plus the
/// monomorphized set/get the boundary routes to.
struct ComponentCodec {
    kind: &'static str,
    set: fn(&mut RunningApp, Entity, &[u8]) -> bool,
    get: fn(&RunningApp, Entity) -> Option<Vec<u8>>,
}

/// The closed game-component vocabulary, in a fixed order. The `kind` is the
/// `Reflect` schema name (== the TS `Component.kind`); scanned branchlessly.
const CODECS: &[ComponentCodec] = &[
    ComponentCodec {
        kind: "Transform",
        set: set_of::<Transform2D>,
        get: get_of::<Transform2D>,
    },
    ComponentCodec {
        kind: "Velocity",
        set: set_of::<Velocity2D>,
        get: get_of::<Velocity2D>,
    },
    ComponentCodec {
        kind: "Sprite",
        set: set_of::<Sprite>,
        get: get_of::<Sprite>,
    },
    ComponentCodec {
        kind: "Text",
        set: set_of::<Text>,
        get: get_of::<Text>,
    },
    ComponentCodec {
        kind: "Rectangle",
        set: set_of::<Rectangle>,
        get: get_of::<Rectangle>,
    },
    ComponentCodec {
        kind: "Image",
        set: set_of::<Image>,
        get: get_of::<Image>,
    },
];

/// Set `entity`'s component of `kind` from its field `bytes`. An unknown kind, a
/// stale entity, or undecodable bytes are all a clean `false`.
pub fn world_set(app: &mut RunningApp, entity: Entity, kind: &str, bytes: &[u8]) -> bool {
    CODECS
        .iter()
        .find(|codec| codec.kind == kind)
        .map(|codec| (codec.set)(app, entity, bytes))
        .unwrap_or(false)
}

/// Read `entity`'s component of `kind` as field bytes — an empty buffer on a
/// miss, a dead entity, or an unknown kind (the TS edge maps `&[]` → `undefined`).
pub fn world_get(app: &RunningApp, entity: Entity, kind: &str) -> Vec<u8> {
    CODECS
        .iter()
        .find(|codec| codec.kind == kind)
        .and_then(|codec| (codec.get)(app, entity))
        .unwrap_or_default()
}

/// Resolve a runtime `kind` string to the `&'static str` schema name the engine's
/// `query_dynamic` requires (`None` for an unknown kind). The closed vocabulary
/// lets a borrowed JS string become the engine's static key without leaking.
pub fn static_kind(kind: &str) -> Option<&'static str> {
    CODECS
        .iter()
        .find(|codec| codec.kind == kind)
        .map(|codec| codec.kind)
}

/// Map every runtime `kind` to its `&'static str` schema name, or `None` if any is
/// unknown — an unknown kind makes the whole intersection query empty (no entity
/// can carry a kind the engine was never given).
pub fn static_kinds(kinds: &[&str]) -> Option<Vec<&'static str>> {
    kinds.iter().map(|kind| static_kind(kind)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::prelude::{App, DefaultPlugins, Window};

    fn app() -> RunningApp {
        App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .build()
    }

    #[test]
    fn every_kind_round_trips_through_the_byte_boundary() {
        let mut app = app();
        let e = app.spawn_empty();

        // Each component kind: set from encoded bytes, read back, decode, compare.
        let transform = Transform2D {
            x: 1.0,
            y: 2.0,
            rotation: 0.5,
            scale_x: 3.0,
            scale_y: 4.0,
        };
        assert!(world_set(&mut app, e, "Transform", &encode(&transform)));
        let got =
            Transform2D::reflect_read(&mut BinaryReader::new(&world_get(&app, e, "Transform")))
                .unwrap();
        assert_eq!(got, transform);

        let velocity = Velocity2D { x: -5.0, y: 6.5 };
        assert!(world_set(&mut app, e, "Velocity", &encode(&velocity)));
        assert_eq!(
            Velocity2D::reflect_read(&mut BinaryReader::new(&world_get(&app, e, "Velocity")))
                .unwrap(),
            velocity
        );

        let sprite = Sprite {
            texture: "hero.png".to_string(),
        };
        assert!(world_set(&mut app, e, "Sprite", &encode(&sprite)));
        assert_eq!(
            Sprite::reflect_read(&mut BinaryReader::new(&world_get(&app, e, "Sprite"))).unwrap(),
            sprite
        );

        let text = Text {
            value: "hello world".to_string(),
        };
        assert!(world_set(&mut app, e, "Text", &encode(&text)));
        assert_eq!(
            Text::reflect_read(&mut BinaryReader::new(&world_get(&app, e, "Text"))).unwrap(),
            text
        );

        let rectangle = Rectangle {
            width: 10.0,
            height: 20.0,
            color: 0x00FF_8000,
        };
        assert!(world_set(&mut app, e, "Rectangle", &encode(&rectangle)));
        assert_eq!(
            Rectangle::reflect_read(&mut BinaryReader::new(&world_get(&app, e, "Rectangle")))
                .unwrap(),
            rectangle
        );

        let image = Image {
            texture: "bg.png".to_string(),
        };
        assert!(world_set(&mut app, e, "Image", &encode(&image)));
        assert_eq!(
            Image::reflect_read(&mut BinaryReader::new(&world_get(&app, e, "Image"))).unwrap(),
            image
        );
    }

    #[test]
    fn unknown_kind_is_a_clean_no_op() {
        let mut app = app();
        let e = app.spawn_empty();
        // An unknown kind never sets and always reads empty.
        assert!(!world_set(&mut app, e, "Nope", &[1, 2, 3, 4]));
        assert!(world_get(&app, e, "Nope").is_empty());
        assert_eq!(static_kind("Nope"), None);
        assert_eq!(static_kind("Transform"), Some("Transform"));
    }

    #[test]
    fn undecodable_bytes_and_stale_entity_are_clean_no_ops() {
        let mut app = app();
        let e = app.spawn_empty();
        // Too few bytes for a Transform's five f32s ⇒ a graceful false, no insert.
        assert!(!world_set(&mut app, e, "Transform", &[0, 1, 2]));
        assert!(world_get(&app, e, "Transform").is_empty());
        // A stale entity handle is also a clean false / empty read.
        assert!(!world_set(
            &mut app,
            Entity::from_raw(9999),
            "Transform",
            &encode(&Transform2D {
                x: 0.0,
                y: 0.0,
                rotation: 0.0,
                scale_x: 1.0,
                scale_y: 1.0,
            })
        ));
        assert!(world_get(&app, Entity::from_raw(9999), "Transform").is_empty());
    }

    #[test]
    fn static_kinds_is_all_or_nothing() {
        assert_eq!(
            static_kinds(&["Transform", "Velocity"]),
            Some(vec!["Transform", "Velocity"])
        );
        // One unknown kind collapses the whole list to None (empty query).
        assert_eq!(static_kinds(&["Transform", "ghost"]), None);
    }

    #[test]
    fn corrupt_utf8_decodes_deterministically_without_panic() {
        // A length-prefixed body with invalid UTF-8 decodes to a deterministic
        // replacement-char string rather than panicking.
        let mut w = BinaryWriter::new();
        w.write_byte_slice(&[0xFF, 0xFE]);
        let bytes = w.into_bytes();
        let a = Sprite::reflect_read(&mut BinaryReader::new(&bytes)).unwrap();
        let b = Sprite::reflect_read(&mut BinaryReader::new(&bytes)).unwrap();
        assert_eq!(a, b);
    }
}
