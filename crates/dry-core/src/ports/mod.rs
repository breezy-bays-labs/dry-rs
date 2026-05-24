//! Port traits for the dry structural duplication detector.
//!
//! Houses [`NormalizerPort`] — the single fundamental adapter
//! abstraction. Each language crate (`dry4rs`, future `dry4ts`)
//! implements this trait; the comparison engine in
//! [`crate::comparison`] (lands with PR 6) is generic over
//! `N: NormalizerPort`.
//!
//! File enumeration and reporting are **free functions**, not traits.
//! Idiomatic Rust: testability via direct fixture inputs is
//! sufficient, the trait abstraction does not earn its complexity
//! when there is no polymorphism axis. See
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

pub mod normalizer;

pub use normalizer::{NormalizeError, NormalizerPort, PlaceholderPolicy};
