//! Source-position coordinate types — [`LineColumn`] and [`Span`].
//!
//! Per the hexagonal layering ADR (constructor pattern for domain
//! structs, [`Span::try_new`] validates inverted ranges) and the
//! orchestrator-stated coordinate convention for v0.1:
//!
//! - **`line`**: 1-indexed (mirrors `proc_macro2::LineColumn::line`).
//!   Line 1 is the first line; line 0 is reserved for "no information"
//!   placeholders only — adapters MUST emit real positions.
//! - **`column`**: 0-indexed (mirrors `proc_macro2::LineColumn::column`).
//!   Column 0 is the first character of the line.
//! - **End coordinate inclusivity**: a [`Span`] is end-**inclusive**.
//!   `start == end` represents a single source position; a one-character
//!   token `x` at line 1 column 0 has `start == end == LineColumn { line: 1, column: 0 }`.
//!
//! The O9 ADR (`adr-span-coordinate-semantics.md`) will canonicalize
//! these as part of the orchestrator's closeout deliverables. This
//! module ships the v0.1 working convention; downstream PRs use these
//! types verbatim.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A 1-indexed line / 0-indexed column position in a source file.
///
/// Mirrors `proc_macro2::LineColumn` so adapter conversion at PR 5
/// (`dry4rs::parser`) is a field-for-field copy with no off-by-one
/// translation.
///
/// # Examples
///
/// ```
/// use dry_core::domain::LineColumn;
/// let p = LineColumn::new(1, 0);
/// assert_eq!(p.line, 1);
/// assert_eq!(p.column, 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LineColumn {
    /// 1-indexed line number. Line 1 is the first line in the file.
    pub line: u32,
    /// 0-indexed column number. Column 0 is the first character of the
    /// line.
    pub column: u32,
}

impl LineColumn {
    /// Construct a [`LineColumn`] from a 1-indexed line and 0-indexed
    /// column.
    #[must_use]
    pub const fn new(line: u32, column: u32) -> Self {
        Self { line, column }
    }
}

/// An end-inclusive source range bounded by two [`LineColumn`] values.
///
/// Constructed via [`Span::try_new`], which rejects inverted ranges
/// (`start > end`). The end position is **inclusive** — a one-character
/// token at line 1, column 0 has `start == end == LineColumn { line: 1, column: 0 }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Span {
    /// Start position, inclusive.
    pub start: LineColumn,
    /// End position, inclusive.
    pub end: LineColumn,
}

impl Span {
    /// Construct a [`Span`] after validating that `start <= end`.
    ///
    /// # Errors
    ///
    /// Returns [`SpanError::InvertedRange`] when `start > end` under
    /// the `(line, column)` lexicographic ordering.
    ///
    /// # Examples
    ///
    /// ```
    /// use dry_core::domain::{LineColumn, Span};
    /// let span =
    ///     Span::try_new(LineColumn::new(1, 0), LineColumn::new(3, 12)).unwrap();
    /// assert_eq!(span.start.line, 1);
    /// assert_eq!(span.end.line, 3);
    /// ```
    pub fn try_new(start: LineColumn, end: LineColumn) -> Result<Self, SpanError> {
        if start > end {
            return Err(SpanError::InvertedRange { start, end });
        }
        Ok(Self { start, end })
    }
}

/// Errors produced when constructing a [`Span`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SpanError {
    /// `start > end` under the lexicographic ordering on `(line, column)`.
    #[error(
        "span start {start:?} is greater than end {end:?} \
         (Span is end-inclusive; start must be <= end)"
    )]
    InvertedRange {
        /// The offending start position.
        start: LineColumn,
        /// The offending end position.
        end: LineColumn,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_column_new_stores_fields() {
        let p = LineColumn::new(7, 3);
        assert_eq!(p.line, 7);
        assert_eq!(p.column, 3);
    }

    #[test]
    fn line_column_origin_is_one_zero() {
        // Documents the convention: line 1, column 0 is the first
        // character of the first line.
        let p = LineColumn::new(1, 0);
        assert_eq!(p.line, 1);
        assert_eq!(p.column, 0);
    }

    #[test]
    fn span_try_new_accepts_equal_start_and_end() {
        // A one-character token has start == end (end-inclusive).
        let pos = LineColumn::new(1, 0);
        let span = Span::try_new(pos, pos).expect("equal start/end is a single position");
        assert_eq!(span.start, pos);
        assert_eq!(span.end, pos);
    }

    #[test]
    fn span_try_new_accepts_multi_line_range() {
        let start = LineColumn::new(1, 2);
        let end = LineColumn::new(3, 12);
        let span = Span::try_new(start, end).expect("forward range is valid");
        assert_eq!(span.start, start);
        assert_eq!(span.end, end);
    }

    #[test]
    fn span_try_new_rejects_inverted_line() {
        let start = LineColumn::new(5, 0);
        let end = LineColumn::new(2, 0);
        let err = Span::try_new(start, end).expect_err("start > end must error");
        assert_eq!(err, SpanError::InvertedRange { start, end });
    }

    #[test]
    fn span_try_new_rejects_inverted_column_same_line() {
        let start = LineColumn::new(1, 10);
        let end = LineColumn::new(1, 3);
        let err =
            Span::try_new(start, end).expect_err("start col > end col on same line must error");
        assert_eq!(err, SpanError::InvertedRange { start, end });
    }

    #[test]
    fn span_error_renders_useful_message() {
        let err = SpanError::InvertedRange {
            start: LineColumn::new(3, 0),
            end: LineColumn::new(1, 0),
        };
        let msg = err.to_string();
        assert!(msg.contains("end-inclusive"), "msg: {msg}");
        assert!(msg.contains("greater"), "msg: {msg}");
    }
}
