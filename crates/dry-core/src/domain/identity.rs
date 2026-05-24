//! Identity newtypes — [`FilePath`] and [`Fingerprint`].
//!
//! Both are pure value types with no I/O semantics: [`FilePath`]
//! wraps a `PathBuf` but never reads from disk, and [`Fingerprint`]
//! wraps the hashed subform value that the comparison engine clusters
//! on.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A workspace- or file-relative path attached to a domain value.
///
/// `FilePath` is a pure newtype around `PathBuf` — it carries no I/O
/// semantics. The file walker (`crate::adapters::source::enumerate`,
/// lands in PR 7) produces `FilePath`s; the comparison engine and
/// reporters consume them. Display renders the underlying path via
/// [`Path::display`], which is lossy on non-UTF-8 paths but never
/// panics.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use dry_core::domain::FilePath;
/// let p = FilePath::from(PathBuf::from("src/lib.rs"));
/// assert_eq!(p.to_string(), "src/lib.rs");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FilePath(PathBuf);

impl FilePath {
    /// Construct a [`FilePath`] from any value convertible into a
    /// `PathBuf`.
    #[must_use]
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self(path.into())
    }

    /// Borrow the underlying path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl From<PathBuf> for FilePath {
    fn from(value: PathBuf) -> Self {
        Self(value)
    }
}

impl From<&Path> for FilePath {
    fn from(value: &Path) -> Self {
        Self(value.to_path_buf())
    }
}

impl AsRef<Path> for FilePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl std::fmt::Display for FilePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0.display(), f)
    }
}

/// A hashed subform value used by the comparison engine to cluster
/// structurally-equivalent fragments.
///
/// `Fingerprint` is an opaque `u64` newtype; consumers do not depend
/// on the specific hash family. The wire shape is a plain `u64`.
///
/// # Examples
///
/// ```
/// use dry_core::domain::Fingerprint;
/// let fp = Fingerprint::new(0xDEAD_BEEF);
/// assert_eq!(fp.value(), 0xDEAD_BEEF);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Fingerprint(u64);

impl Fingerprint {
    /// Wrap an already-hashed `u64` as a [`Fingerprint`].
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The inner `u64`.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for Fingerprint {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hex rendering is canonical for fingerprints in human-facing
        // contexts (debug logs, terminal reporters). The wire format
        // uses the integer; this Display is only for human output.
        write!(f, "{:016x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_path_from_pathbuf_round_trips() {
        let p = FilePath::from(PathBuf::from("crates/dry-core/src/lib.rs"));
        assert_eq!(p.as_path(), Path::new("crates/dry-core/src/lib.rs"));
    }

    #[test]
    fn file_path_from_path_ref_clones_into_pathbuf() {
        let raw = Path::new("src/lib.rs");
        let p = FilePath::from(raw);
        assert_eq!(p.as_path(), raw);
    }

    #[test]
    fn file_path_from_str_via_new_round_trips() {
        let p = FilePath::new("src/lib.rs");
        assert_eq!(p.as_path(), Path::new("src/lib.rs"));
    }

    #[test]
    fn file_path_display_renders_the_path() {
        let p = FilePath::new("src/lib.rs");
        assert_eq!(p.to_string(), "src/lib.rs");
    }

    fn extension_is_rs<P: AsRef<Path>>(p: P) -> bool {
        p.as_ref().extension().is_some_and(|e| e == "rs")
    }

    #[test]
    fn file_path_as_ref_path_works_with_path_api() {
        // Exercise the `AsRef<Path>` impl through a function that
        // takes one.
        let p = FilePath::new("crates/dry-core/src/domain/identity.rs");
        assert!(extension_is_rs(&p));
    }

    #[test]
    fn file_path_serde_round_trips() {
        let original = FilePath::new("src/lib.rs");
        let json = serde_json::to_string(&original).unwrap();
        let back: FilePath = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn fingerprint_new_stores_value() {
        let fp = Fingerprint::new(42);
        assert_eq!(fp.value(), 42);
    }

    #[test]
    fn fingerprint_from_u64_stores_value() {
        let fp = Fingerprint::from(0xABCD);
        assert_eq!(fp.value(), 0xABCD);
    }

    #[test]
    fn fingerprint_display_renders_padded_hex() {
        let fp = Fingerprint::new(0xDEAD_BEEF);
        assert_eq!(fp.to_string(), "00000000deadbeef");
    }

    #[test]
    fn fingerprint_serde_emits_integer() {
        let fp = Fingerprint::new(0xDEAD_BEEF);
        let json = serde_json::to_string(&fp).unwrap();
        // Wire format is the integer, not the hex string — Display is
        // for humans only.
        assert_eq!(json, "3735928559");
    }

    #[test]
    fn fingerprint_serde_round_trips() {
        let fp = Fingerprint::new(12_345);
        let json = serde_json::to_string(&fp).unwrap();
        let back: Fingerprint = serde_json::from_str(&json).unwrap();
        assert_eq!(back, fp);
    }

    #[test]
    fn fingerprint_equality_uses_inner_value() {
        assert_eq!(Fingerprint::new(7), Fingerprint::new(7));
        assert_ne!(Fingerprint::new(7), Fingerprint::new(8));
    }
}
