//! [`SynNormalizer`] — Rust source normalizer.
//!
//! See the parent module docs (`crate::parser`) and the O5 ADR for the
//! full rule set. This module is the implementation site.

use std::path::Path;

use dry_core::domain::{FilePath, NormalizedForm};
use dry_core::ports::{NormalizeError, NormalizerPort, PlaceholderPolicy};

use super::walker::walk_file;

/// Cargo integration-test root directory names. A source file with any
/// of these as a path component is integration-test code — all of its
/// forms classify as [`dry_core::domain::FormKind::Test`] regardless of
/// `#[test]` markers (dry-rs#108).
///
/// This Cargo-convention heuristic lives in the dry4rs adapter, NOT in
/// `dry-core`: it is Rust/Cargo-specific (dry4ts has no `tests/`
/// convention), so the language-agnostic core stays free of it.
const INTEGRATION_TEST_ROOTS: &[&str] = &["tests", "benches"];

/// Does `path` live under a Cargo integration-test root (`tests/` or
/// `benches/`)?
///
/// Matches on a PATH COMPONENT (so `src/tests_helpers.rs` is NOT a
/// match — `tests_helpers` is not the `tests` component), and works
/// regardless of separator style: `Path::components` normalises both
/// `/` and `\` on the respective platforms, and we additionally split
/// raw components on backslashes so a Windows-style path captured on a
/// Unix host (or vice versa) still classifies correctly (see global
/// memory: Rust emits backslashes on Windows runners).
fn is_integration_test_path(path: &Path) -> bool {
    path.components().any(|component| {
        let raw = component.as_os_str().to_string_lossy();
        raw.split(['/', '\\'])
            .any(|segment| INTEGRATION_TEST_ROOTS.contains(&segment))
    })
}

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
        path: &FilePath,
    ) -> Result<Vec<NormalizedForm>, NormalizeError> {
        let file = syn::parse_file(source).map_err(|err| NormalizeError::Parse {
            message: err.to_string(),
            span: None,
        })?;
        // Path-based integration-test classification (dry-rs#108): every
        // form in a `tests/` / `benches/` file is test-harness code,
        // even without `#[test]` markers (cucumber step modules, BDD
        // world fixtures). Seed the walk's test-context flag accordingly;
        // attribute-based detection (`#[test]`, `#[given]`, …) still
        // applies on top inside the walker.
        let in_test_file = is_integration_test_path(path.as_path());
        Ok(walk_file(&file, in_test_file))
    }

    /// The v0.1 placeholder policy — opaque, versioned default.
    ///
    /// The O5 ADR pins the placeholder vocabulary as hard-coded at
    /// v0.1; the returned policy has no configuration surface to
    /// branch on. v0.2+ extends.
    fn placeholder_policy(&self) -> PlaceholderPolicy {
        PlaceholderPolicy::v0_1_default()
    }

    /// Wire-envelope `tool` identity for this adapter: `"dry4rs"`.
    fn tool_name(&self) -> &'static str {
        "dry4rs"
    }

    /// Wire-envelope `tool_version` for this adapter. Resolved against
    /// `dry4rs`'s own `CARGO_PKG_VERSION` (NOT `dry-core`'s) so the
    /// envelope reports the adapter binary's version.
    fn tool_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Wire-envelope `language` identity for this adapter: `"rust"`.
    fn language(&self) -> &'static str {
        "rust"
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
    fn tool_name_is_dry4rs() {
        // Locks the wire-envelope `tool` value for the Rust adapter.
        let n = SynNormalizer::new();
        assert_eq!(n.tool_name(), "dry4rs");
    }

    #[test]
    fn tool_version_resolves_to_dry4rs_pkg_version() {
        // Adapter MUST resolve `tool_version()` against its own
        // CARGO_PKG_VERSION, not dry-core's. The macro expansion
        // happens inside this crate, so the constant is `dry4rs`'s
        // version. Verifies the override site (not the trait default).
        let n = SynNormalizer::new();
        assert_eq!(n.tool_version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn language_is_rust() {
        // Locks the wire-envelope `language` value for the Rust adapter.
        let n = SynNormalizer::new();
        assert_eq!(n.language(), "rust");
    }

    #[test]
    fn integration_test_path_detection(/* dry-rs#108 */) {
        use std::path::Path;

        use super::is_integration_test_path;

        // Positive: a path component named `tests` / `benches`.
        assert!(is_integration_test_path(Path::new(
            "crates/foo/tests/it.rs"
        )));
        assert!(is_integration_test_path(Path::new(
            "crates/foo/benches/bench.rs"
        )));
        assert!(is_integration_test_path(Path::new(
            "crates/foo/tests/bdd_world/steps.rs"
        )));
        // Negative: ordinary src code.
        assert!(!is_integration_test_path(Path::new(
            "crates/foo/src/lib.rs"
        )));
        // Negative: `tests` as a substring of a file/dir name, not a
        // standalone component.
        assert!(!is_integration_test_path(Path::new(
            "crates/foo/src/tests_helpers.rs"
        )));
        assert!(!is_integration_test_path(Path::new(
            "crates/foo/src/integration_benches.rs"
        )));
    }

    #[test]
    fn normalize_seeds_test_kind_for_tests_tree(/* dry-rs#108 */) {
        // A plain helper fn (no `#[test]`) under `tests/` classifies as
        // Test via the path heuristic; the same source under `src/`
        // stays Production.
        let n = SynNormalizer::new();
        let in_tests = n
            .normalize("fn helper() {}", &path("crates/foo/tests/it.rs"))
            .unwrap();
        assert_eq!(in_tests[0].kind, dry_core::domain::FormKind::Test);
        let in_src = n
            .normalize("fn helper() {}", &path("crates/foo/src/lib.rs"))
            .unwrap();
        assert_eq!(in_src[0].kind, dry_core::domain::FormKind::Production);
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
