//! Domain types for the dry structural duplication detector.
//!
//! Houses the core value types that flow through the comparison
//! engine and serialize to the wire envelope: `NormalizedForm`,
//! `Fingerprint`, `Match`, `Score`, `Span`, `FilePath`, `FormKind`,
//! `Tier`, `Severity`, `Report`, `Summary`.
//!
//! Per the hexagonal layering ADR (`ops/decisions/dry-rs/adr-hexagonal-layout.md`,
//! filed in PR 2), this module must not import external crates other
//! than `serde` derive, and must perform no I/O. Domain types are
//! POD-only with canonical constructors.
//!
//! Wire-format discipline: every public *enum* in this module carries
//! `#[non_exhaustive]` (consumer pattern-match concern); result
//! *structs* (`Match`, `Report`, `Summary`, etc.) do not — they evolve
//! via constructors and serde versioning per the nested-envelope ADR
//! (`ops/decisions/dry-rs/adr-nested-json-envelope.md`, filed in PR 2).
//!
//! The actual type definitions land in PR 3 (domain core types) and
//! the open question on cross-language `NormalizedForm.node_count`
//! semantics is folded into the O8 ADR (PR 4).
