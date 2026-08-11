//! # Axiom State — the explicit-persistent-state substrate
//!
//! > All persistent engine/game truth is explicit, typed state data. Engine
//! > computation may consume state and produce new state or state changes, but
//! > executable engine machinery may not secretly retain the current state
//! > between invocations.
//!
//! This layer is the engine-wide model for that truth. It gives a game or module
//! a way to declare *what state exists*, hold it as an immutable
//! [`StateSnapshot`], describe changes as an explicit [`StatePatch`], and get a
//! new snapshot back — with deterministic hashing, diffing, serialization and
//! migration for free.
//!
//! ## What it is not
//!
//! It is not an ECS, not a scene graph, and not a mutable `World`. Above all it
//! is not a manager that holds "the current state": there is no
//! `StateStore::current()`, because the whole point is that the owner of the
//! current snapshot lives *outside* the engine machinery. The app owns the
//! snapshot and hands it in; every entry point here takes values and returns
//! values.
//!
//! ```text
//! snapshot N  +  explicit input  +  explicit rules
//!                     |
//!                 computation                    (retains nothing)
//!                     |
//!             patch / new snapshot N+1
//! ```
//!
//! ## What it knows about your game
//!
//! Nothing. A schema declares paths and binds each to a Rust type; what a
//! `ScoreState` *means* is the caller's business. Typing is compile-time — the
//! identity and the value type come from the same key type, so a call site
//! cannot ask for the wrong type under a path — while storage is the value's
//! own `Reflect` bytes under a stable identity. That is what makes the substrate
//! type-agnostic without runtime type erasure: no `TypeId`, no `downcast`, no
//! `dyn Any`, no trait objects.

#![forbid(unsafe_code)]

mod state_decl;
mod state_diff;
mod state_access;
mod state_apply;
mod state_conflict;
mod state_entry;
mod state_error;
mod state_error_code;
mod state_granule;
mod state_id;
mod state_key;
mod state_kind;
mod state_migration;
mod state_op;
mod state_op_kind;
mod state_origin;
mod state_patch;
mod state_patch_builder;
mod state_payload;
mod state_report;
mod state_result;
mod state_schema;
mod state_schema_id;
mod state_sequence;
mod state_snapshot;
mod state_snapshot_builder;
mod state_table;
mod state_shape_id;
mod state_view;

pub use state_decl::StateDecl;
pub use state_diff::{diff, StateChange, StateChangeKind, StateDiff, StateValueRef};
pub use state_error::StateError;
pub use state_error_code::StateErrorCode;
pub use state_granule::StateGranule;
pub use state_id::StateId;
pub use state_key::{CellKey, SequenceKey, StateKey, TableKey};
pub use state_access::StateAccess;
pub use state_apply::{apply, merge};
pub use state_conflict::{StateConflict, StateOpRef};
pub use state_kind::StateKind;
pub use state_migration::{StateMigration, StateMigrationPlan};
pub use state_op::StateOp;
pub use state_op_kind::StateOpKind;
pub use state_origin::StateOrigin;
pub use state_patch::{detect_conflict, StatePatch};
pub use state_patch_builder::StatePatchBuilder;
pub use state_report::{report, StateEntryReport, StateReport};
pub use state_result::StateResult;
pub use state_schema::StateSchema;
pub use state_schema_id::StateSchemaId;
pub use state_sequence::StateSequence;
pub use state_snapshot::StateSnapshot;
pub use state_snapshot_builder::StateSnapshotBuilder;
pub use state_table::StateTable;
pub use state_shape_id::StateShapeId;
pub use state_view::StateView;
