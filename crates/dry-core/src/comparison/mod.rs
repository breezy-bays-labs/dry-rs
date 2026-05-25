//! Comparison engine for the dry structural duplication detector.
//!
//! Single module — dry-rs has one algorithm (Jaccard on subform
//! fingerprints), not a detector taxonomy (per O6). Two-tier
//! detection:
//!
//! 1. **Hash-bucket clustering** — first pass clusters forms by their
//!    `fingerprint_set` hash. Exact structural matches surface in O(N)
//!    without pairwise comparison.
//! 2. **Sliding-window Jaccard** — second pass over remaining forms
//!    sorted ascending by `node_count`. For each form `forms[i]`, the
//!    inner loop breaks when
//!    `forms[j].node_count > forms[i].node_count / threshold`. This
//!    is the Jaccard upper bound: `J(A,B) <= min(|A|,|B|)/max(|A|,|B|)`,
//!    so for threshold `t`, the largest comparable form has
//!    `node_count <= forms[i].node_count / t`.
//!
//! Threshold tier assignment (`auto_refactor` >= 0.95,
//! `review_first` >= 0.85, `advisory` >= threshold) drives
//! agentic-quality routing.
//!
//! # Algorithm contract
//!
//! [`compare`] is a pure free function — it takes a slice of
//! [`NormalizedForm`] plus a threshold and returns a deterministic
//! [`Vec<Match>`]. Same input + threshold ⟹ byte-identical output.
//!
//! ## Empty `fingerprint_set` policy
//!
//! [`jaccard`] returns `0.0` when either set is empty (including
//! both being empty). Two empty forms have no shared structure,
//! and reporting a perfect match between them would be a
//! pathological "empty == empty" advisory; dropping them to 0.0
//! filters them out of every threshold tier > 0.0.
//!
//! ## Threshold validation
//!
//! Callers MUST pass a threshold in the half-open interval
//! `(0.0, 1.0]`. The CLI surface (`dry_core::cli`, PR 8) is the
//! input-validation boundary; this function does not return
//! `Result` and only `debug_assert!`s the contract.

use std::collections::HashSet;

use crate::domain::{Match, NormalizedForm};

/// Compare a slice of normalized forms and return all matches whose
/// Jaccard similarity meets or exceeds `threshold`.
///
/// The full implementation lands across subsequent commits in this PR
/// (hash-bucket clustering, sliding-window Jaccard, threshold tier
/// assignment, deterministic output sort). This signature is the
/// public contract.
///
/// # Panics (debug only)
///
/// Panics in debug builds when `threshold` is not in the half-open
/// interval `(0.0, 1.0]`.
#[must_use]
pub fn compare(forms: &[NormalizedForm], threshold: f64) -> Vec<Match> {
    debug_assert!(
        threshold > 0.0 && threshold <= 1.0,
        "compare() threshold must lie in (0.0, 1.0]; got {threshold}"
    );
    let _ = forms;
    Vec::new()
}

/// Jaccard similarity over two fingerprint sets.
///
/// Returns `0.0` when either set is empty (the empty-set policy
/// documented at the module level). The function is total: it
/// never panics, returns a value in `[0.0, 1.0]`, is reflexive
/// on any non-empty input (`jaccard(A, A) == 1.0`), and is
/// symmetric (`jaccard(A, B) == jaccard(B, A)`).
#[must_use]
pub fn jaccard(a: &HashSet<u64>, b: &HashSet<u64>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    // Iterate over the smaller set for the intersection — cheap
    // optimization, semantically equivalent.
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let intersection = small.iter().filter(|x| large.contains(x)).count();
    // |A ∪ B| = |A| + |B| - |A ∩ B|. Both sets are non-empty here,
    // so |A| + |B| >= 2 and intersection <= min(|A|, |B|); union >= 1.
    // No div-by-zero is possible.
    let union = a.len() + b.len() - intersection;
    #[allow(clippy::cast_precision_loss)]
    let score = intersection as f64 / union as f64;
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(values: &[u64]) -> HashSet<u64> {
        values.iter().copied().collect()
    }

    #[test]
    fn compare_with_empty_input_returns_empty_vec() {
        let out = compare(&[], 0.85);
        assert!(out.is_empty());
    }

    #[test]
    fn jaccard_of_two_empty_sets_is_zero() {
        // Empty-set policy: not a perfect match, score is 0.0 so the
        // pair is filtered out of every threshold tier > 0.0.
        let empty: HashSet<u64> = HashSet::new();
        assert!((jaccard(&empty, &empty) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_empty_vs_non_empty_is_zero() {
        let empty: HashSet<u64> = HashSet::new();
        let a = set(&[1, 2, 3]);
        assert!((jaccard(&empty, &a) - 0.0).abs() < f64::EPSILON);
        assert!((jaccard(&a, &empty) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_reflexive_on_non_empty() {
        let a = set(&[1, 2, 3, 4]);
        assert!((jaccard(&a, &a) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_symmetric() {
        let a = set(&[1, 2, 3]);
        let b = set(&[2, 3, 4]);
        let ab = jaccard(&a, &b);
        let ba = jaccard(&b, &a);
        assert!((ab - ba).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_known_overlap() {
        // |A ∩ B| = 2, |A ∪ B| = 4 -> 2/4 = 0.5
        let a = set(&[1, 2, 3]);
        let b = set(&[2, 3, 4]);
        let s = jaccard(&a, &b);
        assert!((s - 0.5).abs() < f64::EPSILON, "expected 0.5, got {s}");
    }

    #[test]
    fn jaccard_disjoint_is_zero() {
        let a = set(&[1, 2, 3]);
        let b = set(&[10, 20, 30]);
        let s = jaccard(&a, &b);
        assert!((s - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_subset_full_overlap() {
        // |A ∩ B| = 2, |A ∪ B| = 3 -> 2/3
        let a = set(&[1, 2]);
        let b = set(&[1, 2, 3]);
        let s = jaccard(&a, &b);
        let expected = 2.0 / 3.0;
        assert!(
            (s - expected).abs() < f64::EPSILON,
            "expected {expected}, got {s}"
        );
    }

    #[test]
    fn jaccard_bounded_in_unit_interval() {
        // Sample a handful of representative cases; the property
        // test in `tests/comparison_proptest.rs` covers the full
        // input space.
        let cases: &[(&[u64], &[u64])] = &[
            (&[], &[]),
            (&[1], &[1]),
            (&[1, 2], &[3, 4]),
            (&[1, 2, 3], &[2, 3, 4]),
            (&[1, 2, 3, 4, 5], &[1, 2]),
        ];
        for (a, b) in cases {
            let s = jaccard(&set(a), &set(b));
            assert!((0.0..=1.0).contains(&s), "score {s} out of bounds");
        }
    }
}
