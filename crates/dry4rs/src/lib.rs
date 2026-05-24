//! `dry4rs` — Rust-source adapter for the dry structural duplication
//! detector.
//!
//! Owns the [`parser`] module — the syn-based
//! [`dry_core::ports::NormalizerPort`] implementation. Domain types,
//! port traits, comparison engine, reporters, file walker, and the
//! entire CLI surface live in [`dry_core`]; this crate provides only
//! what is genuinely Rust-source-specific (the [`syn`] AST walk and
//! the typed-placeholder fingerprinting rule set).
//!
//! For consumer convenience the `dry_core` modules are re-exported
//! here, so downstream code that wants the full analyzer surface can
//! depend on `dry4rs` alone. This makes `dry4rs`'s public API a
//! strict superset of `dry_core`'s — every module added to `dry_core`
//! becomes immediately public on `dry4rs`. Re-exported types are
//! identical (no newtype wrap): `dry4rs::domain::Match` and
//! `dry_core::domain::Match` are the same type.

#![warn(missing_docs)]
#![warn(clippy::pedantic, clippy::cargo)]

pub mod parser;

pub use dry_core::{adapters, cli, comparison, domain, ports};

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_compiles() {
        assert!(env!("CARGO_PKG_VERSION").starts_with("0."));
    }
}
