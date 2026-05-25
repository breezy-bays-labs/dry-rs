//! [`SynNormalizer`] — Rust source normalizer.
//!
//! See the parent module docs (`crate::parser`) and the O5 ADR for the
//! full rule set. This module is the implementation site.

use dry_core::domain::{FilePath, NormalizedForm};
use dry_core::ports::{NormalizeError, NormalizerPort, PlaceholderPolicy};

use super::walker::walk_file;

/// The Rust adapter that converts Rust source into [`NormalizedForm`]s
/// for the comparison engine.
///
/// Construct via [`SynNormalizer::new`] or [`SynNormalizer::default`].
/// Both are equivalent at v0.1 — the v0.2+ per-language placeholder
/// ADR will add real constructor parameters.
#[derive(Debug, Clone, Default)]
pub struct SynNormalizer {
    // Private zero-sized field reserves field-layout flexibility for
    // v0.2+ extensions (e.g., per-construct policy toggles, doctest
    // extraction) without forcing external callers through struct-update
    // syntax. Mirrors `PlaceholderPolicy`'s shape.
    _private: (),
}

impl SynNormalizer {
    /// Construct a [`SynNormalizer`] with the v0.1 default
    /// configuration.
    ///
    /// At v0.1 the constructor takes no parameters because the
    /// per-construct rules + typed-placeholder vocabulary are
    /// hard-coded (see the O5 ADR). v0.2+ extends via `new(...)` with
    /// real configuration knobs.
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl NormalizerPort for SynNormalizer {
    /// Rust source-file extensions handled by this adapter.
    ///
    /// At v0.1 the only handled extension is `.rs`. The slice is
    /// `'static` so callers can store it in `'static`-bounded data
    /// structures (e.g., a global registry keyed by extension).
    fn extensions(&self) -> &'static [&'static str] {
        &[".rs"]
    }

    /// Normalize a Rust source file into [`NormalizedForm`]s.
    ///
    /// Walks the file's `syn` AST depth-first and emits one form per
    /// function-shaped body (per the O5 ADR's form-emission scope).
    /// Returns `Err(NormalizeError::Parse)` if the file does not parse
    /// as valid Rust.
    ///
    /// # Errors
    ///
    /// Returns [`NormalizeError::Parse`] when `syn::parse_file(source)`
    /// fails. The `message` carries the syn error's description; the
    /// `span` is `None` at v0.1 (syn's `Error::span()` returns a
    /// `proc_macro2::Span` that v0.1 does not convert into a
    /// `domain::Span` for whole-file parse failures — a v0.2+
    /// improvement).
    fn normalize(
        &self,
        source: &str,
        _path: &FilePath,
    ) -> Result<Vec<NormalizedForm>, NormalizeError> {
        let file = syn::parse_file(source).map_err(|err| NormalizeError::Parse {
            message: err.to_string(),
            span: None,
        })?;
        Ok(walk_file(&file))
    }

    /// The v0.1 placeholder policy — opaque, versioned default.
    ///
    /// The O5 ADR pins the placeholder vocabulary as hard-coded at
    /// v0.1; the returned policy has no configuration surface to
    /// branch on. v0.2+ extends.
    fn placeholder_policy(&self) -> PlaceholderPolicy {
        PlaceholderPolicy::v0_1_default()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn path(p: &str) -> FilePath {
        FilePath::from(PathBuf::from(p))
    }

    #[test]
    fn new_and_default_produce_equivalent_normalizers() {
        // SynNormalizer is opaque at v0.1 (private zero-sized field);
        // the only invariant is that both constructors produce a value
        // that implements NormalizerPort with the documented behavior.
        let a = SynNormalizer::new();
        let b = SynNormalizer::default();
        assert_eq!(a.extensions(), b.extensions());
    }

    #[test]
    fn extensions_returns_dot_rs() {
        let n = SynNormalizer::new();
        assert_eq!(n.extensions(), &[".rs"]);
    }

    #[test]
    fn placeholder_policy_is_v0_1_default() {
        let n = SynNormalizer::new();
        assert_eq!(n.placeholder_policy(), PlaceholderPolicy::v0_1_default());
    }

    #[test]
    fn empty_source_produces_empty_form_list() {
        // An empty file parses successfully (zero items); the walker
        // emits no forms. This is the simplest happy path.
        let n = SynNormalizer::new();
        let result = n.normalize("", &path("empty.rs")).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn invalid_source_emits_parse_error_without_panicking() {
        // Skip-on-parse-error contract: invalid Rust returns
        // NormalizeError::Parse with a message; the adapter does not
        // panic. The span is None at v0.1 (whole-file parse failure).
        let n = SynNormalizer::new();
        let err = n
            .normalize("fn this is not valid rust { ;;; }", &path("bad.rs"))
            .expect_err("invalid source must return Err");
        let NormalizeError::Parse { message, span } = err else {
            panic!("expected NormalizeError::Parse, got {err:?}");
        };
        assert!(!message.is_empty());
        assert!(span.is_none());
    }

    #[test]
    fn parse_error_message_contains_syn_diagnostic() {
        // The Parse variant's `message` is syn's error description.
        // We don't pin the exact wording (syn versions change it), but
        // we require that the message is non-empty and human-readable.
        let n = SynNormalizer::new();
        let err = n
            .normalize("fn () {}", &path("syntax.rs"))
            .expect_err("malformed fn must error");
        let NormalizeError::Parse { message, .. } = err else {
            panic!("expected Parse variant");
        };
        // syn's error messages contain lowercase "expected" or some
        // similar diagnostic verb in the current 2.0 series.
        assert!(message.len() > 5, "message too short: {message:?}");
    }
}
