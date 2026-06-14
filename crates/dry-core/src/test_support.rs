//! Shared `#[cfg(test)]` fixtures for `dry-core` unit tests.
//!
//! This module is compiled ONLY under `cfg(test)` (declared
//! `#[cfg(test)] mod test_support;` in `lib.rs`), so it has zero
//! impact on the production binary. It consolidates the
//! `make_form_ref` / `make_span` builders that were previously
//! copy-pasted across six `domain` / `adapters` / `cli` test modules
//! — duplication that dry4rs flagged against its own source as
//! score-1.0 `auto_refactor` matches (dry-rs#124).
//!
//! Each per-module copy carried slightly different defaults
//! (0-arg vs `(path, line)` vs `(path, start, end)`, and one variant
//! ended its span at column 5 rather than 12). The canonical
//! [`make_form_ref`] takes every varying axis as a parameter; the
//! thin convenience wrappers reproduce each former call shape's EXACT
//! values so no migrated assertion changes.

use crate::domain::{FilePath, FormKind, FormRef, LineColumn, Span};

/// Most-general `FormRef` builder for tests.
///
/// All former copies started the span at column 0 and used
/// [`FormKind::Production`]; only the path, line range, and the END
/// column varied. Those varying axes are parameters here; the fixed
/// axes (`start_col = 0`, `FormKind::Production`) are baked in.
pub(crate) fn make_form_ref(path: &str, start_line: u32, end_line: u32, end_col: u32) -> FormRef {
    FormRef::new(
        FilePath::from(std::path::PathBuf::from(path)),
        Span::try_new(
            LineColumn::new(start_line, 0),
            LineColumn::new(end_line, end_col),
        )
        .unwrap(),
        FormKind::Production,
    )
}

/// Zero-argument default used by `domain::match` and `domain::report`
/// tests: `src/foo.rs`, span `(1,0)..=(3,12)`, `Production`.
pub(crate) fn make_form_ref_default() -> FormRef {
    make_form_ref("src/foo.rs", 1, 3, 12)
}

/// `(path, line)` builder used by the text / markdown reporter tests:
/// span `(line,0)..=(line+2,12)`, `Production`.
pub(crate) fn make_form_ref_at(path: &str, line: u32) -> FormRef {
    make_form_ref(path, line, line + 2, 12)
}

/// `(path, start_line, end_line)` builder used by the
/// github-annotations reporter tests: span `(start,0)..=(end,12)`,
/// `Production`.
pub(crate) fn make_form_ref_lines(path: &str, start_line: u32, end_line: u32) -> FormRef {
    make_form_ref(path, start_line, end_line, 12)
}

/// `(path, line)` builder used by the `cli::run` tests: span
/// `(line,0)..=(line+2,5)`, `Production`. Identical to
/// [`make_form_ref_at`] except the END column is 5, not 12 — the one
/// historically divergent default, preserved so `cli::run` assertions
/// stay byte-stable.
pub(crate) fn make_form_ref_col5(path: &str, line: u32) -> FormRef {
    make_form_ref(path, line, line + 2, 5)
}

/// Canonical span used by `domain::form` and `domain::tree` tests:
/// `(1,0)..=(3,12)`.
pub(crate) fn make_span() -> Span {
    Span::try_new(LineColumn::new(1, 0), LineColumn::new(3, 12)).unwrap()
}
