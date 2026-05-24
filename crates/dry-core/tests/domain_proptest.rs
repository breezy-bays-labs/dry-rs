//! Property tests for [`dry_core::domain::Span`] and
//! [`dry_core::domain::Score`] constructor invariants.
//!
//! The unit tests in each module cover representative cases; these
//! properties cover the full input space. Regression files generated
//! by `proptest` live in `crates/dry-core/proptest-regressions/` and
//! are committed (never gitignored) per `AGENTS.md`.

use dry_core::domain::{LineColumn, Score, ScoreError, Span, SpanError};
use proptest::prelude::*;

prop_compose! {
    fn arb_line_column()(line in 1u32..1_000_000, column in 0u32..10_000) -> LineColumn {
        LineColumn::new(line, column)
    }
}

proptest! {
    // Property: any strictly-inverted (start > end) pair is rejected
    // by `Span::try_new`, and the returned error names the offending
    // pair.
    #[test]
    fn span_try_new_rejects_every_inverted_range(
        a in arb_line_column(),
        b in arb_line_column(),
    ) {
        // Force `start > end` by ordering deterministically.
        let (start, end) = if a > b { (a, b) } else if a < b { (b, a) } else {
            // Equal positions are not inverted; skip via `prop_assume!`.
            prop_assume!(false);
            unreachable!()
        };
        let err = Span::try_new(start, end).expect_err("start > end must error");
        prop_assert_eq!(err, SpanError::InvertedRange { start, end });
    }

    // Property: any non-inverted (start <= end) pair is accepted.
    #[test]
    fn span_try_new_accepts_every_non_inverted_range(
        a in arb_line_column(),
        b in arb_line_column(),
    ) {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let span = Span::try_new(start, end).expect("start <= end must succeed");
        prop_assert_eq!(span.start, start);
        prop_assert_eq!(span.end, end);
    }

    // Property: every finite value in `[0.0, 1.0]` is accepted by
    // `Score::try_new` and round-trips through `Score::value`.
    #[test]
    fn score_try_new_accepts_every_value_in_unit_interval(value in 0.0f64..=1.0f64) {
        let s = Score::try_new(value).expect("0.0..=1.0 must succeed");
        // `value()` returns exactly the input (no rounding).
        prop_assert!((s.value() - value).abs() < f64::EPSILON);
    }

    // Property: every finite value strictly below 0.0 is rejected
    // with `OutOfRange` (not `Nan`). The carried value equals the
    // input exactly (no rounding inside `Score::try_new`).
    #[test]
    fn score_try_new_rejects_every_negative_value(value in f64::MIN..0.0f64) {
        // `value` is generated strictly below 0.0; guard against the
        // boundary in case the generator includes 0.0 due to fp
        // rounding.
        prop_assume!(value < 0.0);
        match Score::try_new(value) {
            Err(ScoreError::OutOfRange { value: v }) => {
                prop_assert_eq!(v, value);
            }
            other => prop_assert!(false, "expected OutOfRange, got {other:?}"),
        }
    }

    // Property: every finite value strictly above 1.0 is rejected
    // with `OutOfRange`. The carried value equals the input exactly
    // (no rounding inside `Score::try_new`).
    #[test]
    fn score_try_new_rejects_every_value_above_one(value in 1.0f64..f64::MAX) {
        prop_assume!(value > 1.0);
        match Score::try_new(value) {
            Err(ScoreError::OutOfRange { value: v }) => {
                // `Score::try_new` does not transform the input; the
                // carried value must equal the rejected input bit-for-bit.
                prop_assert_eq!(v, value);
            }
            other => prop_assert!(false, "expected OutOfRange, got {other:?}"),
        }
    }
}

// Non-proptest properties — NaN is a single-value space, not worth a
// generator. Cover it deterministically here so the property-test
// crate-level intent (every constructor invariant covered) reads
// completely.

#[test]
fn score_try_new_rejects_nan() {
    assert_eq!(Score::try_new(f64::NAN), Err(ScoreError::Nan));
}

#[test]
fn score_try_new_rejects_positive_infinity() {
    assert!(matches!(
        Score::try_new(f64::INFINITY),
        Err(ScoreError::OutOfRange { .. })
    ));
}

#[test]
fn score_try_new_rejects_negative_infinity() {
    assert!(matches!(
        Score::try_new(f64::NEG_INFINITY),
        Err(ScoreError::OutOfRange { .. })
    ));
}
