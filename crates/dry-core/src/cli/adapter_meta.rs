//! `AdapterMeta` — identity/data struct supplied by each adapter
//! binary at startup.
//!
//! Per the cross-tool config-file ADR (`ops/decisions/org/adr-config-
//! file-pattern.md`, D1), each adapter binary (`dry4rs`, future
//! `dry4ts`) supplies an [`AdapterMeta`] struct value at startup;
//! [`crate::cli::run`] and downstream consumers (parsers, reporters,
//! error renderers) receive `&AdapterMeta` through the call chain.
//!
//! **NOT a trait with associated consts**. Trait+consts is for
//! capabilities (`NormalizerPort`); identity data belongs in a struct
//! that multiple consumers read freely. See memory
//! `feedback_rust_trait_vs_struct_for_data`.
//!
//! All fields are `&'static` so the type is `Copy` and a
//! `const`-friendly. Adapter binaries declare a `const DRY4RS_META:
//! AdapterMeta = AdapterMeta { ... };` and pass `&DRY4RS_META` into
//! [`crate::cli::run`].
//!
//! Per ADR D8 — result structs do NOT carry `#[non_exhaustive]`; they
//! evolve via constructors + serde versioning + additive field
//! additions. [`AdapterMeta`] consumers can construct via the struct
//! literal (the 13 fields are public).

/// Identity/data struct supplied by each adapter binary at startup.
///
/// The shared minimum spans 13 `&'static`-typed fields (per cross-tool
/// ADR D1). Tools MAY extend with tool-specific fields (e.g.,
/// crap-rs's `default_metric`); dry-rs has no tool-specific fields at
/// v0.1.
///
/// # Field semantics
///
/// - `tool_name` — clap binary name (`"dry4rs"`). Drives `--help` /
///   `--version` output.
/// - `display_name` — human-readable language label (`"Rust"`).
///   Reserved for future reporter use (CLI banners, scorecard).
/// - `tool_version` — short version string (`env!("CARGO_PKG_VERSION")`).
/// - `long_version` — long-form version (often same as `tool_version`
///   at v0.1; reserved for git-SHA / build metadata in future).
/// - `about` — short help summary (one line).
/// - `long_about` — long help body (paragraphs; rendered on `--help`).
/// - `after_help` — text shown after the help body. May be empty.
/// - `config_file_name` — the file name `discover_config` walks for
///   (e.g. `"dry4rs.toml"`). Adapter-name-agnostic plumbing flows
///   exclusively through this field (per ADR D7).
/// - `extensions` — file extensions to walk by default (without the
///   leading dot). Overridable by `[walk] extensions = [...]` in
///   TOML or future CLI flag.
/// - `tool_info_uri` — URL to the tool's home (README / docs).
/// - `rule_help_uri` — URL to threshold / rule documentation.
/// - `default_excludes` — glob patterns merged with user excludes
///   (additive, never displaces user input). Empty at v0.1.
/// - `forced_excludes` — glob patterns the user cannot override
///   (e.g., `target/**`). Empty at v0.1.
#[derive(Debug, Clone, Copy)]
pub struct AdapterMeta {
    /// clap binary name used at startup. Drives `--help` / `--version`
    /// rendering. MUST be non-empty.
    pub tool_name: &'static str,
    /// Human-readable language label (e.g., `"Rust"` for `dry4rs`,
    /// `"TypeScript"` for future `dry4ts`). MUST be non-empty.
    pub display_name: &'static str,
    /// Short version string, typically `env!("CARGO_PKG_VERSION")`.
    /// MUST be non-empty.
    pub tool_version: &'static str,
    /// Long-form version string. Often the same as `tool_version` at
    /// v0.1; reserved for git-SHA / build metadata in future. MUST
    /// be non-empty.
    pub long_version: &'static str,
    /// Short help summary (one line). MUST be non-empty.
    pub about: &'static str,
    /// Long help body (paragraphs; rendered on `--help`). MUST be
    /// non-empty.
    pub long_about: &'static str,
    /// Text shown after the help body. May be empty.
    pub after_help: &'static str,
    /// The file name `discover_config` walks for (e.g.,
    /// `"dry4rs.toml"`). Adapter-name-agnostic plumbing flows
    /// exclusively through this field (per ADR D7). MUST be non-empty.
    ///
    /// The loader (`dry_core::adapters::config::discover_config`) lands
    /// in Stage 2 of dry-rs#71 and consumes this field.
    pub config_file_name: &'static str,
    /// File extensions to walk by default (without the leading dot).
    /// Consumers may convert to `Vec<String>` via
    /// [`AdapterMeta::extensions_owned`]. MUST be non-empty (every
    /// adapter has at least one language extension).
    pub extensions: &'static [&'static str],
    /// URL to the tool's home (README / docs). MUST be non-empty.
    pub tool_info_uri: &'static str,
    /// URL to threshold / rule documentation. MUST be non-empty.
    pub rule_help_uri: &'static str,
    /// Glob patterns merged with user excludes (additive). Empty at
    /// v0.1; reserved for future per-adapter default-exclude lists.
    pub default_excludes: &'static [&'static str],
    /// Glob patterns the user cannot override (e.g., `target/**`).
    /// Empty at v0.1; reserved for future forced-exclude lists.
    pub forced_excludes: &'static [&'static str],
}

impl AdapterMeta {
    /// Mandatory startup validation. Panics if any required `&'static
    /// str` field is empty.
    ///
    /// Catches `env!()` / build.rs misconfiguration at process start,
    /// not at first reporter call (per ADR D1 + crap-rs precedent).
    /// Optional fields (`after_help`, `default_excludes`,
    /// `forced_excludes`) may be empty.
    ///
    /// # Panics
    ///
    /// Panics on the first empty required field. The panic message
    /// names the field for debugging; this is a programming error,
    /// not a runtime config error, so panicking is the correct
    /// behavior.
    pub fn validate_or_panic(&self) {
        assert!(
            !self.tool_name.is_empty(),
            "AdapterMeta::tool_name must be non-empty"
        );
        assert!(
            !self.display_name.is_empty(),
            "AdapterMeta::display_name must be non-empty"
        );
        assert!(
            !self.tool_version.is_empty(),
            "AdapterMeta::tool_version must be non-empty"
        );
        assert!(
            !self.long_version.is_empty(),
            "AdapterMeta::long_version must be non-empty"
        );
        assert!(
            !self.about.is_empty(),
            "AdapterMeta::about must be non-empty"
        );
        assert!(
            !self.long_about.is_empty(),
            "AdapterMeta::long_about must be non-empty"
        );
        assert!(
            !self.config_file_name.is_empty(),
            "AdapterMeta::config_file_name must be non-empty"
        );
        assert!(
            !self.extensions.is_empty(),
            "AdapterMeta::extensions must contain at least one entry"
        );
        assert!(
            !self.tool_info_uri.is_empty(),
            "AdapterMeta::tool_info_uri must be non-empty"
        );
        assert!(
            !self.rule_help_uri.is_empty(),
            "AdapterMeta::rule_help_uri must be non-empty"
        );
    }

    /// Convert [`extensions`](Self::extensions) into a `Vec<String>`.
    ///
    /// Called by the precedence merger (`merge_effective_inputs`,
    /// landing in Stage 5 of dry-rs#71) only when neither CLI nor
    /// config supplies an extension override; the allocation happens
    /// at most once per run.
    #[must_use]
    pub fn extensions_owned(&self) -> Vec<String> {
        self.extensions.iter().map(|s| (*s).to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic meta for in-module tests. Mirrors `DRY4RS_META`'s
    /// shape but uses `"test-adapter.toml"` for the config file name
    /// (ast-purity-safe per ADR D7).
    const TEST_META: AdapterMeta = AdapterMeta {
        tool_name: "test-adapter",
        display_name: "TestLang",
        tool_version: "0.0.0",
        long_version: "0.0.0",
        about: "test about",
        long_about: "test long about",
        after_help: "",
        config_file_name: "test-adapter.toml",
        extensions: &["x"],
        tool_info_uri: "https://example.test/info",
        rule_help_uri: "https://example.test/rules",
        default_excludes: &[],
        forced_excludes: &[],
    };

    #[test]
    fn validate_or_panic_accepts_well_formed_meta() {
        TEST_META.validate_or_panic();
    }

    #[test]
    #[should_panic(expected = "tool_name")]
    fn validate_or_panic_rejects_empty_tool_name() {
        let bad = AdapterMeta {
            tool_name: "",
            ..TEST_META
        };
        bad.validate_or_panic();
    }

    #[test]
    #[should_panic(expected = "extensions")]
    fn validate_or_panic_rejects_empty_extensions() {
        let bad = AdapterMeta {
            extensions: &[],
            ..TEST_META
        };
        bad.validate_or_panic();
    }

    #[test]
    #[should_panic(expected = "config_file_name")]
    fn validate_or_panic_rejects_empty_config_file_name() {
        let bad = AdapterMeta {
            config_file_name: "",
            ..TEST_META
        };
        bad.validate_or_panic();
    }

    #[test]
    fn extensions_owned_returns_owned_strings() {
        let owned = TEST_META.extensions_owned();
        assert_eq!(owned, vec!["x".to_string()]);
    }

    #[test]
    fn validate_or_panic_tolerates_empty_optional_fields() {
        let meta = AdapterMeta {
            after_help: "",
            default_excludes: &[],
            forced_excludes: &[],
            ..TEST_META
        };
        meta.validate_or_panic();
    }
}
