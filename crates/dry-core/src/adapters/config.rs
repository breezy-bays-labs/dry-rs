//! Config-file loader for any `<adapter>.toml` file.
//!
//! Per the cross-tool config-file ADR (`ops/decisions/org/adr-config-
//! file-pattern.md`):
//!
//! - D2 — `discover_config(start, file_name)` walks `start.ancestors()`
//!   to filesystem root looking for `start.join(file_name)`. Returns
//!   the first hit; `Ok(None)` if no ancestor contains the file;
//!   `Err` only on filesystem permission errors (NOT `NotFound`,
//!   which is the "no config" path).
//! - D4 — `load_config` deserializes with `#[serde(deny_unknown_
//!   fields)]` so typos surface at parse time with a clear `path:line:
//!   key` message.
//! - D5 — typed [`ConfigError`] enum derives `thiserror::Error` +
//!   `#[non_exhaustive]`; `dry-core` stays `anyhow`-free.
//! - D6 — schema POD types ([`Config`], [`GateConfig`][gc],
//!   [`OutputConfig`][oc], [`WalkConfig`][wc]) live in
//!   `dry-core::domain::config`; this module is the loader only.
//! - D7 — ZERO double-quoted adapter-binary-name string literals
//!   (`"dry4rs.toml"`, `"dry4rs"`, `"dry4ts.toml"`, `"dry4ts"`) appear
//!   in this file or its tests. Adapter-name plumbing flows
//!   exclusively through `discover_config`'s `file_name: &str`
//!   parameter (supplied by `&meta.config_file_name` at the binary
//!   boundary). The layer-4 ast-purity gate
//!   (`scripts/check-config-ast-purity.sh`) enforces this.
//!
//! [gc]: crate::domain::GateConfig
//! [oc]: crate::domain::OutputConfig
//! [wc]: crate::domain::WalkConfig

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::domain::Config;

/// Typed loader errors.
///
/// `#[non_exhaustive]` per ADR D5 + D8 — future variants may land
/// (e.g., `InvalidGlob` once allowlist support arrives at v0.2)
/// without breaking exhaustive matches in consumers.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Filesystem error reading the config file (permission denied,
    /// I/O failure). The `path` is the resolved absolute path the
    /// loader attempted to read.
    #[error("failed to read config file {}", path.display())]
    Io {
        /// Path the loader attempted to read.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// TOML parse error — invalid syntax OR unknown key (the loader
    /// runs with `#[serde(deny_unknown_fields)]` so typos in TOML
    /// surface here, not silently fall back to defaults).
    #[error("failed to parse config file {}", path.display())]
    Parse {
        /// Path the loader was parsing.
        path: PathBuf,
        /// Underlying TOML deserialization error.
        #[source]
        source: toml::de::Error,
    },

    /// Field value outside acceptable range (e.g., a future
    /// `threshold > 1.0`). Reserved for post-parse validation that the
    /// serde schema can't express. Empty at v0.1; reserved for v0.2+.
    #[error("invalid value in {file_line}: {message}")]
    InvalidValue {
        /// `path:line` location of the offending key.
        file_line: String,
        /// Human-readable explanation.
        message: String,
    },
}

/// Walk upward from `start` looking for `start.join(file_name)`.
///
/// Returns `Ok(Some(path))` on the first hit, `Ok(None)` when no
/// ancestor contains the file (this is the "no config present" path
/// — NOT an error per ADR D2). Returns `Err(ConfigError::Io)` only
/// on a filesystem permission error or unrelated I/O failure (rare;
/// `Path::exists` itself swallows `NotFound`).
///
/// The walk extends to the filesystem root (no `[workspace]` stop
/// criterion per ADR D2). This matches `rustfmt`'s discipline —
/// users running an analyzer from a deep subdirectory expect the
/// config at the workspace root to apply.
///
/// `file_name` is the adapter's config-file name (e.g.,
/// `meta.config_file_name`). Adapter-name-agnostic API by design —
/// ast-purity gate forbids double-quoted adapter-name literals in
/// this file (per ADR D7).
///
/// # Errors
///
/// Returns [`ConfigError::Io`] when [`Path::try_exists`] reports a
/// filesystem error other than "file not found". `NotFound`
/// translates to a continued walk; only structural errors propagate.
pub fn discover_config(start: &Path, file_name: &str) -> Result<Option<PathBuf>, ConfigError> {
    for ancestor in start.ancestors() {
        let candidate = ancestor.join(file_name);
        match candidate.try_exists() {
            Ok(true) => return Ok(Some(candidate)),
            Ok(false) => {}
            Err(err) => {
                return Err(ConfigError::Io {
                    path: candidate,
                    source: err,
                });
            }
        }
    }
    Ok(None)
}

/// Read + parse the config file at `path`.
///
/// Strict deserialization — unknown keys fail with
/// [`ConfigError::Parse`] (per ADR D4). The caller is responsible for
/// validating that `path` exists; missing-file errors surface as
/// [`ConfigError::Io`] with the underlying `NotFound`.
///
/// # Errors
///
/// - [`ConfigError::Io`] on filesystem read failure (missing file,
///   permission denied, etc.).
/// - [`ConfigError::Parse`] on TOML syntax error or unknown key.
pub fn load_config(path: &Path) -> Result<Config, ConfigError> {
    let contents = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_config(path, &contents)
}

/// Parse the supplied TOML contents into a [`Config`].
///
/// Separated from [`load_config`] for testability — callers that
/// already have the bytes in memory (CI fixtures, programmatic
/// embedding) can parse without going through the filesystem.
///
/// # Errors
///
/// [`ConfigError::Parse`] on TOML syntax error or unknown key.
pub fn parse_config(path: &Path, contents: &str) -> Result<Config, ConfigError> {
    toml::from_str(contents).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_returns_ok_none_when_file_absent() {
        // Use a tempdir to guarantee no ancestor contains the
        // synthetic file name.
        let tmp = tempfile::tempdir().expect("tempdir");
        let result = discover_config(tmp.path(), "synthetic-config.toml")
            .expect("discover_config must not error on absent file");
        assert!(
            result.is_none(),
            "absent file should produce Ok(None), got: {result:?}"
        );
    }

    #[test]
    fn parse_config_rejects_unknown_keys() {
        let path = std::path::Path::new("synthetic.toml");
        let bad = "[gate]\nnonsense_key = true\n";
        let err = parse_config(path, bad).expect_err("unknown key must reject");
        let msg = err.to_string();
        assert!(msg.contains("failed to parse"), "msg: {msg}");
    }

    #[test]
    fn load_config_surfaces_io_error_on_missing_path() {
        let missing = std::path::Path::new("/nonexistent/path/to/file.toml");
        let err = load_config(missing).expect_err("missing path must produce ConfigError::Io");
        match err {
            ConfigError::Io { .. } => {}
            other => panic!("expected Io error, got: {other:?}"),
        }
    }
}
