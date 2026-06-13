//! Crate-id derivation — [`CrateIdResolver`] (dry-rs#141, folds in the
//! spike #112 decision).
//!
//! `crate_id` is the crate (Rust) / package (TS) a form belongs to. It
//! is **FilePath-vs-analysis-root work**: the per-file
//! [`NormalizerPort::normalize`](crate::ports::NormalizerPort::normalize)
//! surface deliberately sees only the source string + its path, never the
//! surrounding workspace layout, so the crate axis cannot be resolved in
//! the adapter walker. It is derived here in `dry-core`, keeping the
//! adapter free of workspace-discovery I/O and language-agnostic for the
//! future `dry4ts` join. (The complementary `module_path` axis IS
//! intra-file AST context and is supplied by the adapter walker.)
//!
//! ## Derivation order (spike #112)
//!
//! For each source file, the crate id resolves to the FIRST of:
//!
//! 1. the **nearest-ancestor `Cargo.toml`'s `[package].name`** — walk up
//!    from the file's directory to the filesystem root, reading the first
//!    `Cargo.toml` that carries a `[package]` table with a `name` key (a
//!    workspace-root `Cargo.toml` with only `[workspace]` and no
//!    `[package]` is skipped, so a file under `crates/foo/src/` resolves
//!    to `foo`, not the workspace name);
//! 2. else the **top directory segment of the file path relative to the
//!    analysis root** (e.g. `crates/foo/src/x.rs` analyzed from the repo
//!    root yields `crates`) — a best-effort grouping for non-Cargo trees;
//! 3. else `None`.
//!
//! A single-directory run with no `Cargo.toml` (the file path has no
//! directory segment under the root) resolves to `None`, so the
//! downstream `crate_aware=false` path never drops every pair (a single
//! crate would otherwise make every form same-crate or no-crate and the
//! scope predicate would have nothing meaningful to gate on).
//!
//! ## Caching (the bench guard)
//!
//! The nearest-`Cargo.toml` walk is keyed by the file's PARENT directory
//! and memoized: the rust-analyzer 23k-form corpus spreads across only a
//! few hundred directories, so the resolver performs at most a few
//! hundred `Cargo.toml` reads regardless of form count. `crate_id` is a
//! `String` per form (cloned from the cached value) — shipped as-is per
//! the build plan; interning is deferred unless a bench regression
//! demands it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::domain::FilePath;

/// Resolves a source file's `crate_id` from its path versus the analysis
/// roots, memoizing the nearest-`Cargo.toml` walk per directory.
///
/// Construct with [`CrateIdResolver::new`] (supplying the analysis
/// roots), then call [`CrateIdResolver::resolve`] once per form's source
/// path. The resolver caches `Cargo.toml` lookups so a corpus with many
/// forms-per-file (or many files-per-directory) pays the filesystem cost
/// once per directory, not once per form.
pub struct CrateIdResolver {
    /// Analysis roots (the positional paths the run targets), absolutized
    /// once at construction so the relative-segment fallback is stable
    /// regardless of the process cwd.
    roots: Vec<PathBuf>,
    /// `parent directory -> Option<crate name>` memo for the
    /// nearest-`Cargo.toml` walk. `None` records "walked to root, found
    /// no `[package].name`" so a miss is not re-walked.
    cargo_cache: HashMap<PathBuf, Option<String>>,
}

impl CrateIdResolver {
    /// Build a resolver for the supplied analysis `roots`.
    ///
    /// Roots are absolutized via [`std::path::absolute`] (no filesystem
    /// touch — it only prepends the cwd to a relative path), matching the
    /// config-loader's discovery discipline. A root that fails to
    /// absolutize is kept as-is (degraded but never panicking).
    #[must_use]
    pub fn new(roots: &[FilePath]) -> Self {
        let roots = roots
            .iter()
            .map(|r| std::path::absolute(r.as_path()).unwrap_or_else(|_| r.as_path().to_path_buf()))
            .collect();
        Self {
            roots,
            cargo_cache: HashMap::new(),
        }
    }

    /// Resolve `path`'s `crate_id` per the spike-#112 order
    /// (nearest-`Cargo.toml` `[package].name`, else top directory segment
    /// under the analysis root, else `None`).
    ///
    /// `&mut self` so the per-directory `Cargo.toml` memo persists across
    /// calls within one run.
    #[must_use]
    pub fn resolve(&mut self, path: &FilePath) -> Option<String> {
        let abs =
            std::path::absolute(path.as_path()).unwrap_or_else(|_| path.as_path().to_path_buf());
        if let Some(name) = self.nearest_cargo_package_name(&abs) {
            return Some(name);
        }
        self.top_segment_under_root(&abs)
    }

    /// Walk up from `file`'s parent directory to the filesystem root,
    /// returning the `[package].name` of the first `Cargo.toml` that
    /// carries one. Memoized on the parent directory.
    fn nearest_cargo_package_name(&mut self, file: &Path) -> Option<String> {
        let parent = file.parent()?.to_path_buf();
        if let Some(cached) = self.cargo_cache.get(&parent) {
            return cached.clone();
        }
        let resolved = walk_up_for_package_name(&parent);
        self.cargo_cache.insert(parent, resolved.clone());
        resolved
    }

    /// Best-effort fallback: the first path component of `file` relative
    /// to whichever analysis root contains it.
    ///
    /// Returns `None` when the file is directly under the root (no
    /// intervening directory segment) — the single-directory / no-crate
    /// case that keeps `crate_aware=false` downstream.
    fn top_segment_under_root(&self, file: &Path) -> Option<String> {
        for root in &self.roots {
            let Ok(rel) = file.strip_prefix(root) else {
                continue;
            };
            // The first component is the top directory segment. The LAST
            // component is the file name itself, so a relative path with a
            // single component (file directly under the root) yields no
            // directory segment -> None.
            let mut components = rel.components();
            let first = components.next()?;
            // A trailing file-name-only path has exactly one component;
            // require at least a second component (so `first` is a real
            // directory, not the file) — `?` short-circuits to None when
            // the file sits directly under the root.
            components.next()?;
            return Some(first.as_os_str().to_string_lossy().into_owned());
        }
        None
    }
}

/// Walk `dir` and its ancestors looking for the nearest `Cargo.toml`
/// with a `[package].name`. Pure over the filesystem; no caching (the
/// caller memoizes).
fn walk_up_for_package_name(dir: &Path) -> Option<String> {
    for ancestor in dir.ancestors() {
        let manifest = ancestor.join("Cargo.toml");
        if let Ok(contents) = std::fs::read_to_string(&manifest)
            && let Some(name) = package_name_from_manifest(&contents)
        {
            return Some(name);
        }
        // A `Cargo.toml` with no `[package].name` (a virtual workspace
        // manifest) does NOT stop the walk — keep climbing so a file
        // under `crates/foo/` resolves to `foo` rather than aborting at
        // the workspace root.
    }
    None
}

/// Extract `[package].name` from a parsed `Cargo.toml` string. Returns
/// `None` when the manifest is unparseable or carries no
/// `[package].name` (e.g. a `[workspace]`-only virtual manifest).
fn package_name_from_manifest(contents: &str) -> Option<String> {
    let value: toml::Value = toml::from_str(contents).ok()?;
    value
        .get("package")?
        .get("name")?
        .as_str()
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, content).expect("write");
    }

    fn fp(path: &Path) -> FilePath {
        FilePath::from(path.to_path_buf())
    }

    #[test]
    fn package_name_from_manifest_reads_package_name() {
        let manifest = "[package]\nname = \"my_crate\"\nversion = \"0.1.0\"\n";
        assert_eq!(
            package_name_from_manifest(manifest),
            Some("my_crate".to_string())
        );
    }

    #[test]
    fn package_name_from_manifest_skips_workspace_only_manifest() {
        // A virtual workspace manifest has no [package] table.
        let manifest = "[workspace]\nmembers = [\"crates/*\"]\n";
        assert_eq!(package_name_from_manifest(manifest), None);
    }

    #[test]
    fn package_name_from_manifest_tolerates_garbage() {
        assert_eq!(package_name_from_manifest("this is not = = toml ["), None);
    }

    #[test]
    fn resolve_uses_nearest_cargo_package_name_for_workspace_file() {
        // A file under crates/foo/src/ resolves to the foo crate, NOT the
        // workspace-root virtual manifest (which has no [package]).
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(
            &root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        );
        write(
            &root.join("crates/foo/Cargo.toml"),
            "[package]\nname = \"foo\"\nversion = \"0.1.0\"\n",
        );
        let file = root.join("crates/foo/src/lib.rs");
        write(&file, "fn a() {}");

        let mut resolver = CrateIdResolver::new(&[fp(root)]);
        assert_eq!(resolver.resolve(&fp(&file)), Some("foo".to_string()));
    }

    #[test]
    fn resolve_returns_none_for_single_dir_run_with_no_cargo_toml() {
        // A flat directory of .rs files with no Cargo.toml anywhere: the
        // file sits directly under the root (no intervening directory),
        // so BOTH the Cargo walk and the top-segment fallback miss ->
        // None. This is the crate_aware=false case.
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let file = root.join("a.rs");
        write(&file, "fn a() {}");

        let mut resolver = CrateIdResolver::new(&[fp(root)]);
        assert_eq!(
            resolver.resolve(&fp(&file)),
            None,
            "single-dir no-Cargo.toml run must resolve crate_id to None"
        );
    }

    #[test]
    fn resolve_falls_back_to_top_segment_under_root_when_no_cargo_toml() {
        // No Cargo.toml, but the file is nested: crates/foo/src/x.rs under
        // the repo root falls back to the first directory segment
        // (`crates`).
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let file = root.join("crates/foo/src/x.rs");
        write(&file, "fn a() {}");

        let mut resolver = CrateIdResolver::new(&[fp(root)]);
        assert_eq!(
            resolver.resolve(&fp(&file)),
            Some("crates".to_string()),
            "no-Cargo.toml nested file falls back to the top directory segment"
        );
    }

    #[test]
    fn resolve_memoizes_cargo_lookup_per_directory() {
        // Two files in the same directory resolve to the same crate; the
        // second resolve hits the cache (observable only via behavior:
        // identical result without re-reading). We assert the cache is
        // populated after the first call.
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(
            &root.join("crates/foo/Cargo.toml"),
            "[package]\nname = \"foo\"\n",
        );
        let a = root.join("crates/foo/src/a.rs");
        let b = root.join("crates/foo/src/b.rs");
        write(&a, "fn a() {}");
        write(&b, "fn b() {}");

        let mut resolver = CrateIdResolver::new(&[fp(root)]);
        assert_eq!(resolver.resolve(&fp(&a)), Some("foo".to_string()));
        assert_eq!(resolver.cargo_cache.len(), 1, "one directory cached");
        assert_eq!(resolver.resolve(&fp(&b)), Some("foo".to_string()));
        assert_eq!(
            resolver.cargo_cache.len(),
            1,
            "same-directory second file reuses the cache entry"
        );
    }

    #[test]
    fn resolve_two_crates_yield_distinct_ids() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(
            &root.join("crates/foo/Cargo.toml"),
            "[package]\nname = \"foo\"\n",
        );
        write(
            &root.join("crates/bar/Cargo.toml"),
            "[package]\nname = \"bar\"\n",
        );
        let foo_file = root.join("crates/foo/src/x.rs");
        let bar_file = root.join("crates/bar/src/y.rs");
        write(&foo_file, "fn x() {}");
        write(&bar_file, "fn y() {}");

        let mut resolver = CrateIdResolver::new(&[fp(root)]);
        assert_eq!(resolver.resolve(&fp(&foo_file)), Some("foo".to_string()));
        assert_eq!(resolver.resolve(&fp(&bar_file)), Some("bar".to_string()));
    }
}
