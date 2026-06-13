//! Port traits for the dry structural duplication detector.
//!
//! Houses [`NormalizerPort`] — the fundamental source-to-IR adapter
//! abstraction — and [`TreeDeriverPort`] — the on-demand ordered-tree
//! re-derivation port that feeds anti-unification (epic #107). Each
//! language crate (`dry4rs`, future `dry4ts`) implements BOTH traits;
//! the comparison engine in [`crate::comparison`] is generic over
//! `N: NormalizerPort`, and the run loop's bound widens to
//! `N: NormalizerPort + TreeDeriverPort + Default` when the tree
//! re-derive wires in (PR 7).
//!
//! These two traits are the architecture's TWO genuine polymorphism
//! axes: parsing source to the bag-of-hashes IR
//! ([`NormalizerPort`]) and re-deriving the ordered tree for LGG
//! ([`TreeDeriverPort`]) — syn (Rust) and swc/oxc (TypeScript)
//! produce different bags AND different trees. File enumeration and
//! reporting remain **free functions**, not traits: testability via
//! direct fixture inputs is sufficient, the trait abstraction does not
//! earn its complexity when there is no polymorphism axis. See
//! `crate::adapters::source::enumerate` and
//! `crate::adapters::reporters` (modules land in PR 7).
//!
//! Per-port error enums derive `thiserror::Error` and carry
//! `#[non_exhaustive]` so language adapters can add variants without
//! breaking pattern-match callers — see the wire-envelope ADR
//! (`adr-nested-json-envelope.md`) `#[non_exhaustive]` discipline
//! section.
//!
//! **Cross-language schema** ([`crate::domain::NormalizedForm`])
//! pinned by the O8 ADR (`adr-normalized-form-schema.md`, filed with
//! PR 4 alongside the trait): `identifier_set` + `qualified_name` +
//! `node_count` semantics every adapter honors.
//!
//! Module roster:
//! - `normalizer` — [`NormalizerPort`] + [`NormalizeError`] + [`PlaceholderPolicy`]
//! - `tree` — [`TreeDeriverPort`] (on-demand ordered-tree re-derivation)

pub mod normalizer;
pub mod tree;

pub use normalizer::{NormalizeError, NormalizerPort, PlaceholderPolicy};
pub use tree::TreeDeriverPort;
