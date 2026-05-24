//! Bounded similarity score in `[0.0, 1.0]` — [`Score`] + [`ScoreError`].
//!
//! The constructor [`Score::try_new`] rejects `NaN`, infinities, and
//! values outside the closed unit interval. Once constructed, a
//! [`Score`] is guaranteed to hold a finite `f64` in `[0.0, 1.0]`.
//!
//! Per the wire-format ADR (`adr-nested-json-envelope.md`), the
//! `score` field on `Match` is **pure Jaccard at all schema
//! versions** — Uncle Bob's mathematical anchor inherited verbatim
//! from dry4clj. Composite or rename-aware scoring lives in separate
//! fields (`structural_score`, `composite_score`).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A similarity score bounded to `[0.0, 1.0]`.
///
/// Constructed via [`Score::try_new`], which rejects `NaN`, infinities,
/// and out-of-range values. The inner `f64` is exposed via
/// [`Score::value`].
///
/// # Examples
///
/// ```
/// use dry_core::domain::Score;
/// let s = Score::try_new(0.92).unwrap();
/// assert!((s.value() - 0.92).abs() < f64::EPSILON);
/// assert!(Score::try_new(f64::NAN).is_err());
/// assert!(Score::try_new(-0.1).is_err());
/// assert!(Score::try_new(1.5).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Score(f64);

impl Score {
    /// Construct a [`Score`] after validating the value lies within
    /// `[0.0, 1.0]` and is finite.
    ///
    /// # Errors
    ///
    /// Returns [`ScoreError::Nan`] when `value.is_nan()`.
    /// Returns [`ScoreError::OutOfRange`] for infinities and any value
    /// outside the closed interval `[0.0, 1.0]`.
    pub fn try_new(value: f64) -> Result<Self, ScoreError> {
        if value.is_nan() {
            return Err(ScoreError::Nan);
        }
        if !(0.0..=1.0).contains(&value) {
            return Err(ScoreError::OutOfRange { value });
        }
        Ok(Self(value))
    }

    /// The inner `f64`, guaranteed finite and in `[0.0, 1.0]` by
    /// construction.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }
}

impl std::fmt::Display for Score {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Match the float's natural Display; consumers that want a
        // fixed precision should format the result of `value()`.
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// Errors produced when constructing a [`Score`].
#[derive(Debug, Clone, Copy, PartialEq, Error)]
#[non_exhaustive]
pub enum ScoreError {
    /// The provided value was `NaN`. `NaN` has no meaningful
    /// similarity interpretation; producers must emit a real number.
    #[error("score must be a finite number; got NaN")]
    Nan,
    /// The provided value was finite but lay outside `[0.0, 1.0]`
    /// (including positive or negative infinity).
    #[error("score must lie in [0.0, 1.0]; got {value}")]
    OutOfRange {
        /// The offending value (may be ±∞ or a finite out-of-range
        /// number).
        value: f64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_new_accepts_zero() {
        let s = Score::try_new(0.0).expect("0.0 is a valid score (lower bound)");
        assert!((s.value() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn try_new_accepts_one() {
        let s = Score::try_new(1.0).expect("1.0 is a valid score (upper bound)");
        assert!((s.value() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn try_new_accepts_typical_jaccard() {
        let s = Score::try_new(0.6).expect("0.6 is a valid score");
        assert!((s.value() - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn try_new_rejects_nan() {
        assert_eq!(Score::try_new(f64::NAN), Err(ScoreError::Nan));
    }

    #[test]
    fn try_new_rejects_negative() {
        assert_eq!(
            Score::try_new(-0.5),
            Err(ScoreError::OutOfRange { value: -0.5 })
        );
    }

    #[test]
    fn try_new_rejects_above_one() {
        assert_eq!(
            Score::try_new(1.5),
            Err(ScoreError::OutOfRange { value: 1.5 })
        );
    }

    #[test]
    fn try_new_rejects_positive_infinity() {
        assert!(matches!(
            Score::try_new(f64::INFINITY),
            Err(ScoreError::OutOfRange { .. })
        ));
    }

    #[test]
    fn try_new_rejects_negative_infinity() {
        assert!(matches!(
            Score::try_new(f64::NEG_INFINITY),
            Err(ScoreError::OutOfRange { .. })
        ));
    }

    #[test]
    fn display_renders_value() {
        let s = Score::try_new(0.25).unwrap();
        assert_eq!(s.to_string(), "0.25");
    }

    #[test]
    fn score_error_renders_nan_message() {
        assert_eq!(
            ScoreError::Nan.to_string(),
            "score must be a finite number; got NaN"
        );
    }

    #[test]
    fn score_error_renders_out_of_range_message() {
        let msg = ScoreError::OutOfRange { value: 1.5 }.to_string();
        assert!(msg.contains("1.5"), "msg: {msg}");
        assert!(msg.contains("[0.0, 1.0]"), "msg: {msg}");
    }
}
