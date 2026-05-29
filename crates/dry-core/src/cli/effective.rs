//! Per-knob cascade resolution — applies `[rust]` / `[typescript]`
//! overrides on top of shared `[gate]` / `[output]` / `[walk]` values,
//! producing the file-config tier consumed by
//! [`crate::cli::merge_effective_inputs`]. Pure function over
//! `(&Config, &AdapterMeta)`; CLI flags overlay later in the
//! precedence chain (CLI > per-language > shared > [`AdapterMeta`]
//! default > compiled-in fallback).
//!
//! Per dry-rs#78 + the cascade-model contract on
//! [`crate::domain::config::Config`]:
//!
//! - `[rust].X = Some(v)` shadows `[gate]/[output]/[walk].X` for the
//!   `dry4rs` adapter ONLY.
//! - `[rust].X = None` falls back to the shared value.
//! - When BOTH are `None`, the resolved field stays `None` and the
//!   next precedence tier (CLI overlay → `AdapterMeta` default →
//!   compiled-in fallback) applies.
//!
//! The cascade is symmetric for `[typescript]` against the future
//! `dry4ts` adapter; selection of which language section to read is
//! driven by [`crate::cli::Language`] on `&AdapterMeta`.

use crate::cli::adapter_meta::{AdapterMeta, Language};
use crate::domain::{Config, GateConfig, LanguageConfig, OutputConfig, WalkConfig};

/// Cascade-resolved file-config tier — every knob is either the
/// per-language override value, the shared value, or `None`. The
/// [`crate::cli::merge_effective_inputs`] overlay applies the CLI tier
/// on top before constructing the final [`crate::cli::AnalysisConfig`].
///
/// Result-struct convention (AGENTS.md): no `#[non_exhaustive]`; new
/// shared knobs land additively via construction in
/// [`EffectiveConfig::resolve`] and a parallel field on
/// [`LanguageConfig`] (the cascade resolver's exhaustive destructure
/// is the compile-time guard).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EffectiveConfig {
    /// Cascade-resolved `[gate]` knobs.
    pub gate: GateConfig,
    /// Cascade-resolved `[output]` knobs.
    pub output: OutputConfig,
    /// Cascade-resolved `[walk]` knobs.
    pub walk: WalkConfig,
}

impl EffectiveConfig {
    /// Resolve cascade: per-language overrides → shared values.
    /// Knobs unset at BOTH tiers stay `None`.
    ///
    /// # Compile-time guard
    ///
    /// Exhaustive destructure of [`LanguageConfig`] AND every shared
    /// section struct ([`GateConfig`], [`OutputConfig`],
    /// [`WalkConfig`]) — adding a new knob to either side breaks the
    /// compile at this site until cascade behavior is wired. That's
    /// the load-bearing rot prevention from dry-rs#78.
    #[must_use]
    pub fn resolve(config: &Config, meta: &AdapterMeta) -> Self {
        let lang = match meta.language {
            Language::Rust => &config.rust,
            Language::TypeScript => &config.typescript,
        };
        let LanguageConfig {
            threshold: lang_threshold,
            threshold_mode: lang_mode,
            format: lang_format,
            title: lang_title,
            subtitle: lang_subtitle,
            include_ignored: lang_include_ignored,
            extensions: lang_extensions,
        } = lang.clone();
        let GateConfig {
            threshold: shared_threshold,
            threshold_mode: shared_mode,
        } = config.gate.clone();
        let OutputConfig {
            format: shared_format,
            title: shared_title,
            subtitle: shared_subtitle,
        } = config.output.clone();
        let WalkConfig {
            include_ignored: shared_include_ignored,
            extensions: shared_extensions,
        } = config.walk.clone();
        Self {
            gate: GateConfig {
                threshold: lang_threshold.or(shared_threshold),
                threshold_mode: lang_mode.or(shared_mode),
            },
            output: OutputConfig {
                format: lang_format.or(shared_format),
                title: lang_title.or(shared_title),
                subtitle: lang_subtitle.or(shared_subtitle),
            },
            walk: WalkConfig {
                include_ignored: lang_include_ignored.or(shared_include_ignored),
                extensions: lang_extensions.or(shared_extensions),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Format, ThresholdMode};

    const RUST_META: AdapterMeta = AdapterMeta {
        tool_name: "test-adapter",
        display_name: "TestLang",
        tool_version: "0.0.0",
        long_version: "0.0.0",
        about: "test about",
        long_about: "test long about",
        after_help: "",
        config_file_name: "test-adapter.toml",
        example_file_name: "test-adapter.example.toml",
        schema_file_name: "test-adapter.schema.json",
        extensions: &["x"],
        language: Language::Rust,
        tool_info_uri: "https://example.test/info",
        rule_help_uri: "https://example.test/rules",
        default_excludes: &[],
        forced_excludes: &[],
    };

    const TS_META: AdapterMeta = AdapterMeta {
        language: Language::TypeScript,
        ..RUST_META
    };

    #[test]
    fn empty_config_produces_empty_effective() {
        let cfg = Config::default();
        let eff = EffectiveConfig::resolve(&cfg, &RUST_META);
        assert!(eff.gate.is_default());
        assert!(eff.output.is_default());
        assert!(eff.walk.is_default());
    }

    #[test]
    fn per_language_some_shadows_shared_some() {
        let mut cfg = Config::default();
        cfg.gate.threshold = Some(0.85);
        cfg.rust.threshold = Some(0.90);

        let eff = EffectiveConfig::resolve(&cfg, &RUST_META);
        assert_eq!(
            eff.gate.threshold,
            Some(0.90),
            "per-language threshold should shadow shared threshold"
        );
    }

    #[test]
    fn per_language_none_falls_back_to_shared_some() {
        let mut cfg = Config::default();
        cfg.gate.threshold = Some(0.85);
        // rust.threshold stays None

        let eff = EffectiveConfig::resolve(&cfg, &RUST_META);
        assert_eq!(
            eff.gate.threshold,
            Some(0.85),
            "per-language None should fall back to shared Some"
        );
    }

    #[test]
    fn both_none_resolves_to_none() {
        let cfg = Config::default();
        let eff = EffectiveConfig::resolve(&cfg, &RUST_META);
        assert_eq!(
            eff.gate.threshold, None,
            "shared None + per-language None should resolve None"
        );
        assert_eq!(eff.gate.threshold_mode, None);
        assert_eq!(eff.output.format, None);
        assert_eq!(eff.output.title, None);
        assert_eq!(eff.output.subtitle, None);
        assert_eq!(eff.walk.include_ignored, None);
        assert_eq!(eff.walk.extensions, None);
    }

    #[test]
    fn rust_meta_reads_config_rust() {
        let mut cfg = Config::default();
        cfg.rust.threshold = Some(0.90);
        cfg.typescript.threshold = Some(0.70);

        let eff = EffectiveConfig::resolve(&cfg, &RUST_META);
        assert_eq!(
            eff.gate.threshold,
            Some(0.90),
            "Language::Rust must read config.rust, not config.typescript"
        );
    }

    #[test]
    fn typescript_meta_reads_config_typescript() {
        let mut cfg = Config::default();
        cfg.rust.threshold = Some(0.90);
        cfg.typescript.threshold = Some(0.70);

        let eff = EffectiveConfig::resolve(&cfg, &TS_META);
        assert_eq!(
            eff.gate.threshold,
            Some(0.70),
            "Language::TypeScript must read config.typescript, not config.rust"
        );
    }

    #[test]
    fn cascade_covers_every_knob() {
        let mut cfg = Config::default();
        // Shared values.
        cfg.gate.threshold = Some(0.85);
        cfg.gate.threshold_mode = Some(ThresholdMode::Default);
        cfg.output.format = Some(Format::Text);
        cfg.output.title = Some("shared title".to_string());
        cfg.output.subtitle = Some("shared subtitle".to_string());
        cfg.walk.include_ignored = Some(false);
        cfg.walk.extensions = Some(vec!["rs".to_string()]);
        // Rust overrides shadow each one.
        cfg.rust.threshold = Some(0.95);
        cfg.rust.threshold_mode = Some(ThresholdMode::Strict);
        cfg.rust.format = Some(Format::Json);
        cfg.rust.title = Some("rust title".to_string());
        cfg.rust.subtitle = Some("rust subtitle".to_string());
        cfg.rust.include_ignored = Some(true);
        cfg.rust.extensions = Some(vec!["rs".to_string(), "rsi".to_string()]);

        let eff = EffectiveConfig::resolve(&cfg, &RUST_META);
        assert_eq!(eff.gate.threshold, Some(0.95));
        assert_eq!(eff.gate.threshold_mode, Some(ThresholdMode::Strict));
        assert_eq!(eff.output.format, Some(Format::Json));
        assert_eq!(eff.output.title.as_deref(), Some("rust title"));
        assert_eq!(eff.output.subtitle.as_deref(), Some("rust subtitle"));
        assert_eq!(eff.walk.include_ignored, Some(true));
        assert_eq!(
            eff.walk.extensions,
            Some(vec!["rs".to_string(), "rsi".to_string()])
        );
    }

    #[test]
    fn typescript_overrides_do_not_leak_into_rust() {
        let mut cfg = Config::default();
        cfg.typescript.threshold = Some(0.50);
        cfg.typescript.title = Some("ts title".to_string());

        let eff = EffectiveConfig::resolve(&cfg, &RUST_META);
        assert_eq!(
            eff.gate.threshold, None,
            "typescript override must not affect rust adapter"
        );
        assert_eq!(eff.output.title, None);
    }
}
