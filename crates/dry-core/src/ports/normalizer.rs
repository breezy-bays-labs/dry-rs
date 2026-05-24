//! `NormalizerPort` — language-specific source-to-`NormalizedForm`.
//!
//! Implemented per source language:
//!   - `dry4rs::parser::SynNormalizer` — Rust adapter via `syn` (lands
//!     with PR 5).
//!   - `dry4ts::parser::*` — TypeScript adapter joining at v0.6+
//!     (parser-library choice between `swc_ecma_parser` and
//!     `oxc_parser` is open as of 2026-05).
//!
//! **IO boundary**: core opens the file; the adapter parses bytes
//! (`&str`). Cleaner test ergonomics (no disk fixtures) and rayon-ready
//! parallelism control from the comparison-engine call site. The
//! wrapper that reads the file owns its own error type (lands with the
//! comparison engine and file walker in later PRs); [`NormalizeError`]
//! here is adapter-only.
//!
//! **Single port, not three**: per [`super`] module docs, file
//! enumeration and reporter dispatch are idiomatic Rust free functions.
//! `NormalizerPort` is the *only* port trait because parser adapters
//! are the *only* polymorphism axis in the architecture — one trait
//! per language source.
//!
//! **Object-safe** (`&self`); usable as `Box<dyn NormalizerPort>`. No
//! `Send + Sync` bound on the trait itself — those add at the
//! comparison-engine call site if/when rayon parallelism arrives. The
//! v0.5 size-bucketed parallel comparison-engine path is the
//! anticipated bound site (see roadmap §"Performance at scale").
//!
//! **Cross-language schema** — [`NormalizedForm`] (with `kind`,
//! `fingerprint_set`, `identifier_set`, `qualified_name`, `span`,
//! `node_count`, `line_count`) is the shared IR every adapter
//! produces. The O8 ADR (`adr-normalized-form-schema.md`) pins the
//! field semantics; every adapter honors the same `node_count`
//! heuristic so threshold semantics stay unified across languages.

use crate::domain::{FilePath, NormalizedForm};

/// Parse a single source file into language-agnostic
/// [`NormalizedForm`]s.
///
/// Each adapter crate implements this trait for its target source
/// language. The comparison engine in [`crate::comparison`] (lands
/// with PR 6) is generic over `N: NormalizerPort`; the run loop in
/// [`crate::cli`] takes a concrete adapter at the binary entry point.
///
/// # Object safety
///
/// The trait is object-safe — methods take `&self`, no `Self: Sized`
/// requirement, no generic methods. `Box<dyn NormalizerPort>` is a
/// usable shape for runtime adapter selection (the comparison-engine
/// generic over `N: NormalizerPort` is the v0.1 call-site shape).
pub trait NormalizerPort {
    /// File extensions this adapter handles, including the leading
    /// dot (`".rs"`, `".ts"`, `".tsx"`, …).
    ///
    /// The file walker (`crate::adapters::source::enumerate`, lands
    /// in PR 7) filters by this list before invoking `normalize`. The
    /// `'static` return lifetime lets callers store the slice in
    /// `'static`-bounded data structures (e.g., a global registry
    /// keyed by extension); adapters typically return a static
    /// constant like `&[".rs"]`, which Rust promotes to `&'static
    /// [&'static str]` automatically.
    fn extensions(&self) -> &'static [&'static str];

    /// Normalize the source file into the cross-language IR.
    ///
    /// `source` is the file's raw bytes (the wrapper owning file I/O
    /// reads them before calling this); `path` is the location the
    /// bytes came from. The adapter does NOT open `path` — that
    /// would re-introduce I/O on the port surface. The orchestrator
    /// that calls `normalize` is the one that associates each
    /// returned `NormalizedForm` (and any returned `NormalizeError`)
    /// with the `FilePath` — `FormRef` (which DOES carry `file:
    /// FilePath`) is constructed at the reporter / `Match` boundary,
    /// not by the adapter; the wrapper that holds `path` annotates
    /// returned errors with file context above the trait surface.
    ///
    /// The adapter may consult `path` to compute language-specific
    /// qualified names (e.g., Rust's `path/to/mod.rs` → outer module
    /// name `mod`); other than that, the trait surface treats `path`
    /// as a purely value-bearing companion to `source`.
    ///
    /// # Errors
    ///
    /// Returns [`NormalizeError::Parse`] when the parser cannot
    /// recover a usable projection of the source (whole-file syntax
    /// error that prevents emission of any form). Returns
    /// [`NormalizeError::Unsupported`] when the adapter encounters a
    /// language construct it has not yet been taught to normalize
    /// (the file may emit some forms before this fires; the adapter
    /// chooses whether to short-circuit or recover). I/O failures
    /// are not part of this trait's surface — they are owned by the
    /// wrapper that read `source` before calling here.
    fn normalize(
        &self,
        source: &str,
        path: &FilePath,
    ) -> Result<Vec<NormalizedForm>, NormalizeError>;

    /// The per-language identifier-handling policy this adapter
    /// applies (O5 — placeholder substitution rules).
    ///
    /// At v0.1 [`PlaceholderPolicy`] is an opaque stub — the O5 ADR
    /// (`adr-smart-normalization-rules.md`, lands with PR 5) extends
    /// the type with concrete configuration knobs. Returning a
    /// `PlaceholderPolicy` from a port method now reserves the
    /// surface so PR 5's expansion is purely additive.
    fn placeholder_policy(&self) -> PlaceholderPolicy;
}

/// Errors produced by [`NormalizerPort`] implementations.
///
/// `#[non_exhaustive]` — language adapters add variants (e.g. TS
/// module-resolution failure, Rust macro-expansion limitation)
/// without breaking pattern-match callers. I/O failures are not part
/// of this type's surface; they are owned by the wrapper that reads
/// `source` before calling `normalize`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NormalizeError {
    /// Adapter could not parse `source` into any usable form.
    ///
    /// `message` is human-readable; `span` localizes the failure when
    /// the adapter recovers a position (`syn`'s `Error::span()` plus
    /// the `proc-macro2` `span-locations` feature give a real line/col
    /// in dry4rs; `swc`/`oxc` carry source positions on errors). When
    /// the adapter cannot recover a position (whole-file fail-fast,
    /// typical for some `syn` constructs), `span` is `None` and the
    /// detail belongs in `message`.
    #[error("normalize error: {message}")]
    Parse {
        /// Human-readable description of the parse failure.
        message: String,
        /// Source position of the failure, when recoverable. The
        /// adapter that constructs this variant should attach a
        /// position when one is available; a `None` means the failure
        /// is not localizable to a line range.
        span: Option<crate::domain::Span>,
    },
    /// Adapter encountered a language construct it does not yet
    /// support.
    ///
    /// `construct` names the offending shape (e.g. `"macro_rules!"`,
    /// `"async fn in trait"`, `"jsx fragment"`). Adapters return
    /// this when the construct is parseable but its normalization
    /// rule is not yet implemented — `dry4rs` v0.1 returns it for
    /// any construct outside the O5-ADR-scoped initial set; later
    /// PRs widen the supported set and shrink the unsupported one.
    /// `span` localizes the offending construct when the adapter
    /// recovers a position (the common case — the AST node carries
    /// it); `None` for cases where the adapter recognized
    /// unsupported-ness without a single-position anchor (e.g. a
    /// whole-module attribute or a feature-gated cfg block).
    #[error("unsupported construct: {construct}")]
    Unsupported {
        /// Identifier-shaped name of the unsupported construct.
        construct: String,
        /// Source position of the unsupported construct, when
        /// recoverable.
        span: Option<crate::domain::Span>,
    },
}

/// Per-language identifier-handling policy applied during
/// normalization (O5 — smart normalization rules).
///
/// **v0.1 stub**: `PlaceholderPolicy` is an opaque value type with a
/// private constructor. The O5 ADR (lands with PR 5) extends the
/// type with concrete configuration (which token classes substitute
/// for typed placeholders, identifier-aware vs identifier-stripped
/// emission rules, per-language deltas). PR 4 reserves the surface
/// so PR 5's expansion is purely additive — every public adapter
/// already returns a `PlaceholderPolicy` from
/// [`NormalizerPort::placeholder_policy`].
///
/// The struct deliberately does **NOT** carry `#[non_exhaustive]` —
/// per the wire-envelope ADR's "enums-yes-structs-no" rule, result
/// and configuration structs evolve via constructors (`Foo::new`,
/// `Foo::try_new`, `Foo::default`) and serde versioning. A private
/// zero-sized field (`_private: ()`) reserves field-layout
/// flexibility without forcing external callers through
/// `..Default::default()` struct-update syntax.
///
/// Construct via [`PlaceholderPolicy::v0_1_default`] (explicit,
/// versioned) or [`PlaceholderPolicy::default`] (implements
/// `Default`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlaceholderPolicy {
    // Private zero-sized field — keeps the struct extensible without
    // making external construction a pattern-match surface. Real
    // configuration knobs land with the O5 ADR (PR 5).
    _private: (),
}

impl PlaceholderPolicy {
    /// The v0.1 default policy (opaque stub).
    ///
    /// PR 5's O5 ADR fills in the policy semantics. At v0.1 every
    /// adapter returns this from [`NormalizerPort::placeholder_policy`];
    /// the comparison engine does not yet branch on the policy value.
    #[must_use]
    pub const fn v0_1_default() -> Self {
        Self { _private: () }
    }
}

// Compile-time invariants on the port trait: object-safe (so
// `Box<dyn NormalizerPort>` works), and *deliberately* not `Send + Sync`
// (parallelism bounds belong at the comparison-engine call site, not
// on the port). Mirrors scrap-rs's `TestParserPort` assertion shape.
#[cfg(test)]
static_assertions::assert_obj_safe!(NormalizerPort);
#[cfg(test)]
static_assertions::assert_not_impl_any!(dyn NormalizerPort: Send, Sync);

#[cfg(test)]
mod error_smoke {
    use std::error::Error;

    use super::*;
    use crate::domain::{LineColumn, Span};

    #[test]
    fn parse_error_with_span_displays_message_and_carries_no_source() {
        let span = Span::try_new(LineColumn::new(3, 4), LineColumn::new(3, 4)).unwrap();
        let err = NormalizeError::Parse {
            message: "unexpected token".into(),
            span: Some(span),
        };
        assert_eq!(err.to_string(), "normalize error: unexpected token");
        // `Parse` carries no `#[source]` — `span` localizes; there is
        // no inner error to chain.
        assert!(err.source().is_none());
    }

    #[test]
    fn parse_error_without_span_renders_message() {
        let err = NormalizeError::Parse {
            message: "whole-file parse failure".into(),
            span: None,
        };
        assert_eq!(err.to_string(), "normalize error: whole-file parse failure");
        assert!(err.source().is_none());
    }

    #[test]
    fn unsupported_error_renders_construct_name_without_span() {
        let err = NormalizeError::Unsupported {
            construct: "macro_rules!".into(),
            span: None,
        };
        assert_eq!(err.to_string(), "unsupported construct: macro_rules!");
        assert!(err.source().is_none());
    }

    #[test]
    fn unsupported_error_carries_optional_span() {
        let span = Span::try_new(LineColumn::new(7, 0), LineColumn::new(9, 5)).unwrap();
        let err = NormalizeError::Unsupported {
            construct: "async fn in trait".into(),
            span: Some(span),
        };
        // The Display message intentionally omits span detail — the
        // span lives on the variant for tooling consumers; the
        // human-readable string stays compact.
        assert_eq!(err.to_string(), "unsupported construct: async fn in trait");
        // No `#[source]`; span is positional metadata, not an
        // upstream error to chain.
        assert!(err.source().is_none());
    }

    #[test]
    fn placeholder_policy_default_matches_v0_1_default() {
        // PR 5 extends `PlaceholderPolicy`. At v0.1 the policy is
        // opaque; `Default` and `v0_1_default()` must agree.
        assert_eq!(
            PlaceholderPolicy::default(),
            PlaceholderPolicy::v0_1_default()
        );
    }
}
