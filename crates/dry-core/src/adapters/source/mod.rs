//! Language-agnostic file walker — [`enumerate`].
//!
//! Walks one or more roots, honoring `.gitignore` / `.ignore` /
//! `.git/info/exclude` like `rg` and `fd` (via the [`ignore`] crate
//! that powers both). Filters by extension via
//! [`crate::cli::AnalysisConfig::extensions`] and sorts output for
//! deterministic downstream processing.
//!
//! The walker is a free function, not a port trait — the hexagonal
//! layering ADR's "Three module-roster divergences from scrap-rs"
//! section names file enumeration as one of the deliberately
//! trait-free seams in `dry-core`. Per-adapter extension filtering
//! arrives via `AnalysisConfig`; there is no polymorphism axis here.
//!
//! The walker DOES NOT read any file's contents. It only enumerates
//! paths. Reading is the orchestrator's job (the comparison-engine
//! run loop owns file I/O and threads the bytes into the
//! [`crate::ports::NormalizerPort::normalize`] call).

use std::ffi::OsStr;
use std::path::PathBuf;

use ignore::WalkBuilder;
use thiserror::Error;

use crate::cli::AnalysisConfig;
use crate::domain::FilePath;

/// Enumerate every source file under the roots in `config`, honoring
/// `.gitignore` / `.ignore` rules unless `config.include_ignored` is
/// `true`.
///
/// Returns a deterministically-sorted [`Vec<FilePath>`] — the inner
/// walker sorts directory entries by file name and the final result is
/// sorted by full path before return, so two back-to-back calls
/// produce identical output regardless of filesystem ordering. This
/// determinism is contract-grade; the property test
/// `enumerate_is_deterministic_across_runs` in
/// `crates/dry-core/tests/adapters_proptest.rs` locks it.
///
/// **Skip-on-read-error policy**: when the walker encounters a path it
/// cannot enumerate (permission denied, broken symlink, etc.), it
/// records a [`SourceWarning::Unreadable`] entry on the returned
/// [`SourceOutcome::warnings`] vector and continues. The walker never
/// fails the whole enumeration on a single unreadable path. The
/// orchestrator decides whether to surface warnings to the user.
///
/// # Errors
///
/// Returns [`SourceError::NoRoots`] when `config.roots` is empty. The
/// walker is never invoked with zero work; the CLI surface (PR 8)
/// ensures `--` input passes a default root.
pub fn enumerate(config: &AnalysisConfig) -> Result<SourceOutcome, SourceError> {
    if config.roots.is_empty() {
        return Err(SourceError::NoRoots);
    }

    let allowed_exts: Vec<&OsStr> = config
        .extensions
        .iter()
        .map(|e| OsStr::new(e.as_str()))
        .collect();
    let builder = build_walker(config);

    let mut files: Vec<PathBuf> = Vec::new();
    let mut warnings: Vec<SourceWarning> = Vec::new();
    for entry in builder.build() {
        match entry {
            Ok(e) => collect_matching_file(&e, &allowed_exts, &mut files),
            Err(err) => warnings.push(unreadable_warning(&err)),
        }
    }

    // The walker's `sort_by_file_name` only sorts entries within each
    // directory; cross-root determinism requires a final pass.
    files.sort();
    let files = files.into_iter().map(FilePath::from).collect();
    Ok(SourceOutcome { files, warnings })
}

/// Build the `ignore`-crate walker, seeding it with `config.roots`
/// and applying `include_ignored` overrides.
///
/// The `ignore` crate's `WalkBuilder` accepts multiple roots via
/// `.add(path)`. We seed with the first root then extend with the
/// rest so a multi-root config builds a single walker (the
/// alternative — one walker per root, concatenated — would
/// double-yield files that appear under overlapping roots).
fn build_walker(config: &AnalysisConfig) -> WalkBuilder {
    let first_root = &config.roots[0];
    let mut builder = WalkBuilder::new(first_root.as_path());
    for additional in &config.roots[1..] {
        builder.add(additional.as_path());
    }
    if config.include_ignored {
        // Disable every ignore-rule source so fixtures inside `target/`
        // or `node_modules/` enumerate. `parents` controls
        // .gitignore-discovery upward from the root; the three booleans
        // gate per-file ignore sources.
        builder
            .git_ignore(false)
            .git_exclude(false)
            .git_global(false)
            .ignore(false)
            .parents(false)
            .hidden(false);
    }
    builder.sort_by_file_name(std::cmp::Ord::cmp);
    builder
}

/// If `entry` is a file whose extension passes `allowed_exts`, push
/// its path into `files`. Empty `allowed_exts` accepts every
/// extension; non-empty rejects files without an extension.
fn collect_matching_file(
    entry: &ignore::DirEntry,
    allowed_exts: &[&OsStr],
    files: &mut Vec<PathBuf>,
) {
    let Some(ft) = entry.file_type() else { return };
    if !ft.is_file() {
        return;
    }
    let path = entry.path();
    if !extension_is_allowed(path, allowed_exts) {
        return;
    }
    files.push(path.to_path_buf());
}

/// Whether `path`'s extension passes the allow-list.
///
/// Empty `allowed_exts` accepts every extension. Non-empty rejects
/// paths without an extension and paths whose extension is not in
/// the list.
fn extension_is_allowed(path: &std::path::Path, allowed_exts: &[&OsStr]) -> bool {
    if allowed_exts.is_empty() {
        return true;
    }
    let Some(ext) = path.extension() else {
        return false;
    };
    allowed_exts.contains(&ext)
}

/// Build a [`SourceWarning::Unreadable`] from an `ignore` walker error.
fn unreadable_warning(err: &ignore::Error) -> SourceWarning {
    SourceWarning::Unreadable {
        path: extract_error_path(err).unwrap_or_default(),
        message: err.to_string(),
    }
}

/// Walk down [`ignore::Error`]'s recursive variants
/// (`WithPath` / `WithLineNumber` / `WithDepth` / `Partial`) to
/// surface the inner path when one is attached. `ignore`'s public
/// surface exposes the enum but not a convenience accessor; this
/// helper centralizes the extraction so the walker's warning code
/// path stays a one-liner.
fn extract_error_path(err: &ignore::Error) -> Option<PathBuf> {
    match err {
        ignore::Error::WithPath { path, .. } => Some(path.clone()),
        ignore::Error::WithLineNumber { err, .. } | ignore::Error::WithDepth { err, .. } => {
            extract_error_path(err)
        }
        ignore::Error::Loop { ancestor, .. } => Some(ancestor.clone()),
        ignore::Error::Partial(inner) => inner.iter().find_map(extract_error_path),
        _ => None,
    }
}

/// Result of a successful walk: every yielded file plus any
/// non-fatal warnings the walker accumulated mid-traversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOutcome {
    /// Files matched, deterministically sorted.
    pub files: Vec<FilePath>,
    /// Non-fatal walker warnings (unreadable paths, broken symlinks).
    /// Each warning carries the path the walker tried to read; the
    /// orchestrator decides whether to surface them.
    pub warnings: Vec<SourceWarning>,
}

/// A non-fatal walker warning. Emitted when the walker continues past
/// a path it cannot enumerate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourceWarning {
    /// The walker could not read a path (permission denied, broken
    /// symlink, ...). The walker skipped it and continued.
    Unreadable {
        /// Offending path. May be empty when the underlying walker
        /// error carries no path attribution.
        path: PathBuf,
        /// Human-readable description of the failure (the
        /// [`ignore::Error`] string).
        message: String,
    },
}

/// Errors that abort the whole walk.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SourceError {
    /// `config.roots` was empty. The CLI surface (PR 8) defaults at
    /// least one root in; library callers must do likewise.
    #[error("no roots configured for enumeration")]
    NoRoots,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, content).expect("write");
    }

    fn config_for(root: &Path) -> AnalysisConfig {
        AnalysisConfig::new([root.to_path_buf()]).with_extensions(["rs"])
    }

    #[test]
    fn enumerate_rejects_empty_roots() {
        let config = AnalysisConfig::default();
        let err = enumerate(&config).expect_err("empty roots must error");
        assert!(matches!(err, SourceError::NoRoots), "got: {err:?}");
    }

    #[test]
    fn enumerate_yields_only_files_with_matching_extension() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("a.rs"), "fn a() {}");
        write(&dir.path().join("b.toml"), "");
        write(&dir.path().join("nested/c.rs"), "fn c() {}");
        let out = enumerate(&config_for(dir.path())).unwrap();
        let names: Vec<String> = out
            .files
            .iter()
            .filter_map(|p| p.as_path().file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.rs", "c.rs"], "out: {out:?}");
    }

    #[test]
    fn enumerate_returns_sorted_paths() {
        let dir = TempDir::new().unwrap();
        // Intentionally write in reverse-alpha order; result must be
        // ascending alpha.
        write(&dir.path().join("z.rs"), "");
        write(&dir.path().join("m.rs"), "");
        write(&dir.path().join("a.rs"), "");
        let out = enumerate(&config_for(dir.path())).unwrap();
        let names: Vec<String> = out
            .files
            .iter()
            .filter_map(|p| p.as_path().file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.rs", "m.rs", "z.rs"]);
    }

    #[test]
    fn enumerate_skips_ignored_files_by_default() {
        // The `ignore` crate honors `.ignore` directly (no `.git/`
        // required), plus `.gitignore` whenever a git repo is in
        // scope. The fixture uses `.ignore` for cross-platform
        // determinism — no `git init` required in tmp.
        let dir = TempDir::new().unwrap();
        write(&dir.path().join(".ignore"), "ignored.rs\ntarget/\n");
        write(&dir.path().join("kept.rs"), "");
        write(&dir.path().join("ignored.rs"), "");
        write(&dir.path().join("target/build-artifact.rs"), "");
        let out = enumerate(&config_for(dir.path())).unwrap();
        let names: Vec<String> = out
            .files
            .iter()
            .filter_map(|p| p.as_path().file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"kept.rs".to_string()), "names: {names:?}");
        assert!(
            !names.contains(&"ignored.rs".to_string()),
            "ignored.rs must be excluded by .ignore: {names:?}"
        );
        assert!(
            !names.contains(&"build-artifact.rs".to_string()),
            "target/ contents must be excluded: {names:?}"
        );
    }

    #[test]
    fn enumerate_with_include_ignored_returns_ignored_files() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join(".ignore"), "ignored.rs\n");
        write(&dir.path().join("kept.rs"), "");
        write(&dir.path().join("ignored.rs"), "");
        let config = config_for(dir.path()).with_include_ignored(true);
        let out = enumerate(&config).unwrap();
        let names: Vec<String> = out
            .files
            .iter()
            .filter_map(|p| p.as_path().file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        assert!(
            names.contains(&"ignored.rs".to_string()),
            "with include_ignored, ignored.rs must enumerate: {names:?}"
        );
        assert!(names.contains(&"kept.rs".to_string()), "names: {names:?}");
    }

    #[test]
    fn enumerate_returns_all_files_when_extension_list_empty() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("a.rs"), "");
        write(&dir.path().join("b.toml"), "");
        let config = AnalysisConfig::new([dir.path().to_path_buf()]);
        let out = enumerate(&config).unwrap();
        let names: Vec<String> = out
            .files
            .iter()
            .filter_map(|p| p.as_path().file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"a.rs".to_string()), "names: {names:?}");
        assert!(names.contains(&"b.toml".to_string()), "names: {names:?}");
    }

    #[test]
    fn enumerate_walks_multiple_roots_uniquely() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("alpha/a.rs"), "");
        write(&dir.path().join("beta/b.rs"), "");
        let config = AnalysisConfig::new([dir.path().join("alpha"), dir.path().join("beta")])
            .with_extensions(["rs"]);
        let out = enumerate(&config).unwrap();
        let names: Vec<String> = out
            .files
            .iter()
            .filter_map(|p| p.as_path().file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.iter().filter(|n| *n == "a.rs").count(), 1);
        assert_eq!(names.iter().filter(|n| *n == "b.rs").count(), 1);
    }

    #[test]
    fn enumerate_determinism_two_back_to_back_calls_agree() {
        let dir = TempDir::new().unwrap();
        for name in ["q.rs", "a.rs", "m.rs", "x.rs"] {
            write(&dir.path().join(name), "");
        }
        let config = config_for(dir.path());
        let first = enumerate(&config).unwrap();
        let second = enumerate(&config).unwrap();
        assert_eq!(first.files, second.files);
        assert_eq!(first.warnings.is_empty(), second.warnings.is_empty());
    }
}
