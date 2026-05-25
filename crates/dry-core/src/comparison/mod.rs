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
//! ## Pass 1 — hash-bucket clustering
//!
//! Forms are grouped by a canonical bucket key derived from
//! `fingerprint_set`. The bucket key is an **XOR-fold of the set's
//! `u64` elements** (chosen for order-independence and zero
//! allocations; the multiplicity ambiguity is irrelevant because
//! `HashSet<u64>` already deduplicates elements). Buckets of size
//! ≥ 2 have their fingerprint sets compared for **structural
//! equality** — XOR collisions (two different sets that XOR to the
//! same key) are rejected, and the offending forms re-enter Pass 2
//! for normal pairwise Jaccard. Verified clusters surface as a
//! single n-ary [`Match`] with `score == 1.0` and tier
//! [`Tier::AutoRefactor`].
//!
//! ## Empty `fingerprint_set` policy
//!
//! [`jaccard`] returns `0.0` when either set is empty (including
//! both being empty). Two empty forms have no shared structure,
//! and reporting a perfect match between them would be a
//! pathological "empty == empty" advisory; dropping them to 0.0
//! filters them out of every threshold tier > 0.0. Pass 1 also
//! treats empty fingerprint sets as non-clustering: even if every
//! form has the empty set (and thus the same XOR bucket key),
//! they are not emitted as an `auto_refactor` cluster — empty
//! forms have no structure to match.
//!
//! ## Threshold validation
//!
//! Callers MUST pass a threshold in the half-open interval
//! `(0.0, 1.0]`. The CLI surface (`dry_core::cli`, PR 8) is the
//! input-validation boundary; this function does not return
//! `Result` and only `debug_assert!`s the contract.

use std::collections::{BTreeMap, HashSet};

use crate::domain::{FilePath, FormRef, Match, NormalizedForm, Tier};

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

    let mut matches: Vec<Match> = Vec::new();
    let mut claimed: HashSet<usize> = HashSet::new();

    // Pass 1 — hash-bucket clustering. Verified exact matches are
    // emitted and their indices are marked `claimed`. XOR
    // collisions and partial-bucket mismatches leave their indices
    // unclaimed so Pass 2 (sliding-window) can compare them
    // pairwise.
    pass1_hash_bucket(forms, &mut matches, &mut claimed);

    let _ = threshold; // Pass 2 lands in the next commit.
    matches
}

/// Pass 1 — hash-bucket clustering. Groups forms by an XOR-fold of
/// their `fingerprint_set`, verifies each grouped pair has a
/// structurally-equal set, and emits a single n-ary [`Match`] per
/// verified bucket. Verified-cluster indices land in `claimed`;
/// unverified ones leave their indices unclaimed for Pass 2.
fn pass1_hash_bucket(
    forms: &[NormalizedForm],
    matches: &mut Vec<Match>,
    claimed: &mut HashSet<usize>,
) {
    // BTreeMap (not HashMap) keeps Pass 1's emit order
    // deterministic before the final sort. Cheap insurance against
    // debugging surprises if a future refactor relies on emit order.
    let mut buckets: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, form) in forms.iter().enumerate() {
        // Empty fingerprint sets do not cluster — they have no
        // structure to match (see "Empty fingerprint_set policy"
        // in the module doc). Leave them unclaimed; Pass 2's
        // Jaccard returns 0.0 against any empty side and filters
        // them out naturally.
        if form.fingerprint_set.is_empty() {
            continue;
        }
        let key = bucket_key(&form.fingerprint_set);
        buckets.entry(key).or_default().push(i);
    }

    for (_key, indices) in buckets {
        if indices.len() < 2 {
            continue;
        }

        // Verify the bucket is a true cluster (XOR collisions are
        // possible — Pass 1's verification step is what makes the
        // XOR bucket key safe). Group by the first index's
        // fingerprint_set; if at least 2 forms match, emit them as
        // an exact cluster. Forms that don't match the first set
        // are LEFT UNCLAIMED so Pass 2 can compare them pairwise
        // against each other (the rare colliding pair) and against
        // the verified-cluster members (also a rare 2nd-order
        // collision).
        //
        // The conservative drop-on-mismatch behavior is correct
        // because verified Pass 1 emits cover ALL exact matches
        // for the canonical group; non-canonical members will be
        // re-discovered by Pass 2's pairwise scan.
        let first = &forms[indices[0]].fingerprint_set;
        let verified: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&i| forms[i].fingerprint_set == *first)
            .collect();

        if verified.len() < 2 {
            // No exact cluster survived verification (extremely
            // rare 2-element bucket where the sets differ); leave
            // both indices unclaimed for Pass 2.
            continue;
        }

        // Emit a single n-ary match for the verified cluster.
        let forms_refs: Vec<FormRef> = verified.iter().map(|&i| form_ref_for(&forms[i])).collect();
        matches.push(Match::new(forms_refs, 1.0, Tier::AutoRefactor));

        for i in verified {
            claimed.insert(i);
        }
    }
}

/// Compute the bucket key for a fingerprint set. **XOR-fold** of
/// the set's `u64` elements — order-independent, allocation-free,
/// and `fold(empty) == 0` (the empty-set case is filtered before
/// this function is called, so the value-zero key is benign).
///
/// XOR ignores multiplicity by construction; this is safe because
/// `HashSet<u64>` already deduplicates elements. The known
/// degenerate collision pattern is two different sets with the same
/// XOR result; Pass 1's structural-equality verification step
/// rejects those before emitting a [`Match`].
fn bucket_key(set: &HashSet<u64>) -> u64 {
    set.iter().fold(0u64, |acc, &x| acc ^ x)
}

/// Project a [`NormalizedForm`] to the reporter-friendly
/// [`FormRef`]. The file path is synthesized as `qualified_name`
/// joined with `::` because the comparison engine has no access to
/// the source `FilePath` (it isn't on `NormalizedForm` — see
/// `adr-normalized-form-schema.md`). PR 8's run loop wires real
/// paths at the higher layer; this stub is the deterministic
/// fallback at v0.1.
fn form_ref_for(form: &NormalizedForm) -> FormRef {
    let synthesized = if form.qualified_name.is_empty() {
        std::path::PathBuf::from("<unknown>")
    } else {
        std::path::PathBuf::from(form.qualified_name.join("::"))
    };
    FormRef::new(FilePath::from(synthesized), form.span, form.kind)
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

    fn make_form(fps: &[u64], node_count: u32) -> NormalizedForm {
        use crate::domain::{FormKind, LineColumn, Span};
        NormalizedForm::new(
            FormKind::Production,
            fps.iter().copied().collect(),
            Span::try_new(LineColumn::new(1, 0), LineColumn::new(1, 0)).unwrap(),
            node_count,
            1,
        )
    }

    #[test]
    fn bucket_key_is_xor_fold() {
        let a = set(&[0x1, 0x2, 0x4]);
        // 0x1 ^ 0x2 ^ 0x4 = 0x7
        assert_eq!(bucket_key(&a), 0x7);
        // Order independence.
        let b = set(&[0x4, 0x2, 0x1]);
        assert_eq!(bucket_key(&a), bucket_key(&b));
    }

    #[test]
    fn bucket_key_for_empty_set_is_zero() {
        let empty: HashSet<u64> = HashSet::new();
        assert_eq!(bucket_key(&empty), 0);
    }

    #[test]
    fn pass1_emits_n_ary_match_for_identical_fingerprint_sets() {
        // Two forms with byte-identical fingerprint sets emit one
        // auto_refactor match with score 1.0.
        let forms = vec![make_form(&[1, 2, 3], 3), make_form(&[3, 2, 1], 3)];
        let matches = compare(&forms, 0.85);
        assert_eq!(matches.len(), 1);
        assert!((matches[0].score - 1.0).abs() < f64::EPSILON);
        assert_eq!(matches[0].tier, Tier::AutoRefactor);
        assert_eq!(matches[0].forms.len(), 2);
    }

    #[test]
    fn pass1_emits_single_match_for_triple_cluster() {
        // Three forms in one exact-match bucket emit ONE n-ary
        // match, not three pairwise matches.
        let forms = vec![
            make_form(&[1, 2, 3], 3),
            make_form(&[1, 2, 3], 3),
            make_form(&[1, 2, 3], 3),
        ];
        let matches = compare(&forms, 0.85);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].forms.len(), 3);
    }

    #[test]
    fn pass1_does_not_cluster_empty_fingerprint_sets() {
        // Empty fingerprint sets don't share structure — even
        // though they all hash to the same XOR bucket key (0),
        // Pass 1 skips them.
        let forms = vec![make_form(&[], 0), make_form(&[], 0)];
        let matches = compare(&forms, 0.85);
        assert!(
            matches.is_empty(),
            "empty fingerprint sets must not cluster"
        );
    }

    #[test]
    fn pass1_does_not_emit_singleton_cluster() {
        // A bucket of size 1 (no duplicate form) does not emit a
        // match.
        let forms = vec![make_form(&[1, 2, 3], 3)];
        let matches = compare(&forms, 0.85);
        assert!(matches.is_empty());
    }

    #[test]
    fn pass1_xor_collision_does_not_emit_false_match() {
        // Two structurally-different sets that XOR to the same key
        // must NOT be reported as an exact match. {1, 2, 3} XORs to
        // 0 (1 ^ 2 ^ 3 == 0); {0} also XORs to 0. Pass 1's
        // structural-verification rejects the false cluster.
        //
        // Pass 2 (sliding-window) will still pairwise-compare them,
        // but with threshold 0.85 the Jaccard score is 0.0 here
        // (disjoint sets) so no match is emitted at all.
        let forms = vec![make_form(&[1, 2, 3], 3), make_form(&[0], 1)];
        let matches = compare(&forms, 0.85);
        // Pass 2 not yet implemented (next commit), so for now
        // only assert that Pass 1 doesn't produce a false match.
        // We re-verify after Pass 2 lands.
        for m in &matches {
            assert!(
                (m.score - 1.0).abs() > f64::EPSILON,
                "no score-1.0 match should be emitted across XOR-colliding non-equal sets, got: {m:?}"
            );
        }
    }
}
