//! Rust-source [`dry_core::ports::NormalizerPort`] implementation via
//! `syn`.
//!
//! The [`SynNormalizer`] type implements
//! [`dry_core::ports::NormalizerPort`] for Rust source files. It walks
//! the `syn` AST depth-first, emits one
//! [`dry_core::domain::NormalizedForm`] per function-shaped body
//! (`ItemFn`, `ImplItemFn`, `TraitItemFn` with a default body,
//! `ExprClosure`), and constructs `fingerprint_set` via per-subform
//! typed-placeholder hashing.
//!
//! The full rule set is pinned in the O5 ADR
//! (`ops/decisions/dry-rs/adr-rust-normalization-rules.md`):
//!
//! - **Per-subform fingerprinting** — each subtree visited during the
//!   depth-first walk emits one fingerprint. Mirrors dry4clj's
//!   `(tree-seq sequential? seq form)` traversal.
//! - **Deterministic hashing** — `std::hash::DefaultHasher`
//!   (SipHash-1-3 with a fixed key), NOT `HashMap`'s `RandomState`.
//! - **Form emission scope** — `ItemFn`, `ImplItemFn`, `TraitItemFn`
//!   (default-body only), `ExprClosure`. Containers (`mod`, `impl`,
//!   `trait`) and type definitions emit no form.
//! - **Typed placeholders** — local variables collapse to `Var`,
//!   function / method names preserve as `Ident(<name>)`, concrete
//!   types preserve as `Type(<name>)`, type parameters collapse to
//!   `TypeParam`, lifetimes collapse to `Lifetime` (except `'static`),
//!   literals preserve verbatim, macros are opaque `MacroCall(<name>)`.
//! - **`FormKind::Test` detection** (dry-rs#108) — a form is test code
//!   when ANY of: a recognised test-framework attribute is present
//!   (`#[test]` / `#[tokio::test]`-style, cucumber `#[given]` /
//!   `#[when]` / `#[then]`, `#[rstest]`, `#[test_case]`); an enclosing
//!   `#[cfg(test)] mod`; OR the source file lives under a Cargo
//!   integration-test root (`tests/` / `benches/`). The path heuristic
//!   is Cargo-specific and lives in the adapter (`normalizer`), seeded
//!   into the walk; the attribute / module signals live in the walker.
//!   `FormKind::Doctest` is reserved at v0.1; no extraction.
//! - **Skip-on-parse-error** — `syn::parse_file` errors become
//!   `NormalizeError::Parse`; no panic.

mod normalizer;
mod token;
mod tree;
mod visitor;
mod walker;

pub use normalizer::SynNormalizer;
