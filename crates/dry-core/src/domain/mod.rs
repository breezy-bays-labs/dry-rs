//! Domain types for the dry structural duplication detector.
//!
//! Houses the core value types that flow through the comparison
//! engine and serialize to the wire envelope: [`NormalizedForm`],
//! [`Fingerprint`], [`Match`], [`Score`], [`Span`], [`FilePath`],
//! [`FormKind`], [`Tier`], [`Severity`], [`Report`], [`Summary`].
//! It also carries the ordered-tree IR the anti-unification pass
//! operates over ([`NormalizedTree`], [`LeafToken`], [`LeafClass`]) and
//! the anti-unification result types that pass produces ([`Template`],
//! [`TemplateNode`], [`Hole`], [`HoleKind`], [`Substitution`],
//! [`SubElement`], [`Divergence`], [`DistinctValue`]) — POD here in the
//! domain so [`Match`] can carry a [`Template`] without `domain`
//! depending on the `comparison` engine that fills it.
//!
//! Per the hexagonal layering ADR (`ops/decisions/dry-rs/adr-hexagonal-layout.md`,
//! filed in PR 2), this module must not import external crates other
//! than `serde` derive (for wire-format round-tripping) and `thiserror`
//! (for `std::error::Error` derive on the constructor-validation
//! errors). The module performs no I/O — every type is POD with
//! canonical constructors.
//!
//! Wire-format discipline (per the nested-envelope ADR
//! `ops/decisions/dry-rs/adr-nested-json-envelope.md`, filed in PR 2):
//!
//! - Every public *enum* in this module carries `#[non_exhaustive]`
//!   (consumer pattern-match concern).
//! - Result *structs* ([`Match`], [`Report`], [`Summary`], etc.) do
//!   not carry `#[non_exhaustive]` — they evolve via constructors
//!   (`Foo::new`, `Foo::try_new`, `Foo::default`) and serde versioning.
//! - The three reserved score slots on [`Match`] use `#[serde(default)]`
//!   **without** `skip_serializing_if = "Option::is_none"`, because the
//!   v0.1 wire contract requires them visible as `null`, not omitted.
//!
//! The cross-language `NormalizedForm.node_count` semantics will be
//! pinned in the O8 ADR landing with PR 4.

pub mod config;
mod enums;
mod form;
mod identity;
mod r#match;
mod report;
mod score;
mod span;
mod summary;
mod template;
mod tree;

pub use config::{Config, GateConfig, LanguageConfig, OutputConfig, ScopeConfig, WalkConfig};
pub use enums::{FormKind, Severity, Tier};
pub use form::{FormRef, NormalizedForm, StructuralLocation};
pub use identity::{FilePath, Fingerprint};
pub use r#match::Match;
pub use report::Report;
pub use score::{Score, ScoreError};
pub use span::{LineColumn, Span, SpanError};
pub use summary::Summary;
pub use template::{
    DistinctValue, Divergence, Hole, HoleId, HoleKind, SubElement, Substitution, Template,
    TemplateNode,
};
pub use tree::{LeafClass, LeafToken, NormalizedTree};
