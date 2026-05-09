//! Port traits for the dry structural duplication detector.
//!
//! Houses `NormalizerPort` — the single fundamental adapter
//! abstraction. Each language crate (`dry4rs`, future `dry4ts`)
//! implements this trait; the comparison engine in
//! [`crate::comparison`] is generic over `N: NormalizerPort`.
//!
//! File enumeration and reporting are **free functions**, not traits.
//! Idiomatic Rust: testability via direct fixture inputs is
//! sufficient, the trait abstraction does not earn its complexity
//! when there is no polymorphism axis. See
//! `crate::adapters::source::enumerate` and
//! `crate::adapters::reporters` (modules land in PR 7).
//!
//! Per-port error enums derive `thiserror::Error` and carry
//! `#[non_exhaustive]` per the wire-format ADR.
//!
//! The actual trait definitions land in PR 4 (port traits + error
//! types) — the same PR closes O8 (`NormalizedForm` cross-language
//! schema), O11 (identifier-aware secondary representation for
//! rename signal), and O12 (cross-language node-counting heuristic)
//! via the dedicated `adr-normalized-form-schema.md`.
