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
//! ## Pass 2 — sliding-window Jaccard
//!
//! Remaining forms (those not claimed by Pass 1) are sorted
//! ascending by `node_count`. For each pair `(i, j)` with `i < j`,
//! the inner loop breaks when
//! `forms[j].node_count > forms[i].node_count / threshold` — no
//! later `k > j` can clear the threshold either (Jaccard upper
//! bound `min/max >= t` ⟹ `max <= min/t`).
//!
//! `node_count` is a **proxy** for fingerprint-set cardinality —
//! the O8 ADR keeps them decoupled (`node_count` is per-leaf,
//! `fingerprint_set` is per-subform Merkle-folded). When set size
//! and `node_count` align, the break math is exact; when they
//! diverge, the engine's break is conservative (the true Jaccard
//! upper bound is set-size-based). The trade-off is deliberate:
//! sorting by `node_count` is `O(N log N)` on `u32`, and the
//! sliding-window can prune most pairs without computing Jaccard.
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
//! ## Deterministic output ordering
//!
//! Returned matches are sorted by
//! `(forms[0].file, forms[0].span.start, -score)`. `Match`
//! derives only `PartialEq` (because of `f64`), so the sort key
//! is computed against `f64::total_cmp` for the score component.
//! This is the canonical ordering every reporter inherits;
//! changing it is a wire-output change (callers may pin against
//! it in snapshot tests) and requires the same discipline as a
//! `schema_version` discussion (see [[adr-nested-json-envelope]]).
//!
//! ## Threshold validation
//!
//! Callers MUST pass a threshold in the half-open interval
//! `(0.0, 1.0]`. The CLI surface (`dry_core::cli`, PR 8) is the
//! input-validation boundary; this function does not return
//! `Result` and only `debug_assert!`s the contract.

use std::collections::{BTreeMap, HashSet};
use std::hash::BuildHasher;

use crate::domain::{FilePath, FormRef, Match, NormalizedForm, Tier};

/// Floor below which a score-tier is downgraded from
/// [`Tier::AutoRefactor`] — pinned at `0.95` per the roadmap's
/// threshold-tier vocabulary. Scores at or above this floor route
/// to [`Tier::AutoRefactor`].
pub const AUTO_REFACTOR_FLOOR: f64 = 0.95;

/// Floor below which a score-tier is downgraded from
/// [`Tier::ReviewFirst`] — pinned at `0.85` per the roadmap.
/// Scores at or above this floor (but below [`AUTO_REFACTOR_FLOOR`])
/// route to [`Tier::ReviewFirst`].
pub const REVIEW_FIRST_FLOOR: f64 = 0.85;

/// Compare a slice of normalized forms and return all matches whose
/// Jaccard similarity meets or exceeds `threshold`.
///
/// The implementation runs two passes:
///
/// 1. **Hash-bucket clustering** — forms whose `fingerprint_set` is
///    structurally identical surface as an n-ary match with score
///    `1.0` (tier [`Tier::AutoRefactor`]). XOR-bucket collisions are
///    rejected via a structural-equality verification step before
///    emission.
/// 2. **Sliding-window Jaccard** — remaining pairs whose Jaccard
///    similarity clears `threshold` surface as binary matches with
///    the computed score and a tier from the floor table.
///
/// The returned `Vec<Match>` is sorted deterministically by
/// `(forms[0].file, forms[0].span.start, -score)`.
///
/// # Panics (debug only)
///
/// Panics in debug builds when `threshold` is not in the half-open
/// interval `(0.0, 1.0]`. Release builds skip the assertion and
/// behave unspecified for out-of-range input; the CLI surface
/// (`dry_core::cli`, PR 8) is the input-validation boundary.
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

    // Pass 2 — sliding-window Jaccard over forms NOT claimed by
    // Pass 1. Sorted ascending by `node_count` with the
    // break-math shortcut.
    pass2_sliding_window(forms, threshold, &claimed, &mut matches);

    sort_matches_for_output(&mut matches);
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

    for (_key, mut indices) in buckets {
        if indices.len() < 2 {
            continue;
        }

        // A single bucket can hold multiple distinct equal-set
        // clusters that XOR-fold to the same key (rare but legal:
        // e.g. `{1, 2}` and `{4, 7}` both fold to `3`). Iterate
        // until every cluster within the bucket is emitted so Pass
        // 2 never has to handle a score==1.0 pair. Singletons
        // (canonical with no equal partner in the bucket) drop out
        // of `cluster` naturally and stay unclaimed for Pass 2's
        // pairwise scan.
        while indices.len() >= 2 {
            let canonical_idx = indices[0];
            let canonical_set = &forms[canonical_idx].fingerprint_set;
            let (cluster, leftover): (Vec<usize>, Vec<usize>) = indices
                .iter()
                .copied()
                .partition(|&i| forms[i].fingerprint_set == *canonical_set);

            if cluster.len() >= 2 {
                let forms_refs: Vec<FormRef> =
                    cluster.iter().map(|&i| form_ref_for(&forms[i])).collect();
                matches.push(Match::new(forms_refs, 1.0, Tier::AutoRefactor));
                for i in cluster {
                    claimed.insert(i);
                }
            }
            // Canonical (and any singleton from this partition step)
            // landed in `cluster` — drop it from the working set
            // either way; `leftover` is the rest of the bucket.
            indices = leftover;
        }
    }
}

/// Pass 2 — sliding-window Jaccard over unclaimed forms. Sorts
/// candidates ascending by `node_count`, then for each pair
/// `(i, j)` with `i < j` the inner loop breaks when
/// `forms[j].node_count > forms[i].node_count / threshold`. Emits
/// one binary [`Match`] per pair clearing `threshold`; tier is
/// assigned by the score (Pass 2 cannot emit `score == 1.0` —
/// those land in Pass 1).
fn pass2_sliding_window(
    forms: &[NormalizedForm],
    threshold: f64,
    claimed: &HashSet<usize>,
    matches: &mut Vec<Match>,
) {
    // Project to unclaimed indices and sort ascending by
    // (node_count, original_index). The secondary sort key keeps
    // the iteration order deterministic when node_counts tie.
    let mut sorted: Vec<usize> = (0..forms.len()).filter(|i| !claimed.contains(i)).collect();
    sorted.sort_by_key(|&i| (forms[i].node_count, i));

    for outer_pos in 0..sorted.len() {
        let i = sorted[outer_pos];
        // f64 cast on u32 is exact for valid node counts (well
        // below 2^53). The CLI-side gate (PR 8) clamps inputs.
        let bound = f64::from(forms[i].node_count) / threshold;
        for &j in &sorted[outer_pos + 1..] {
            let nj = f64::from(forms[j].node_count);
            // Break math: strict inequality. No later k > j
            // (sorted ascending by node_count) can clear the
            // threshold.
            if nj > bound {
                break;
            }
            let score = jaccard(&forms[i].fingerprint_set, &forms[j].fingerprint_set);
            if score < threshold {
                continue;
            }
            // Pass 1 emits ALL equal-set clusters within a bucket
            // (including XOR-colliding distinct clusters), so any
            // form with an equal-set partner has been claimed and
            // Pass 2 should never see `score == 1.0`. The de-rating
            // to AUTO_REFACTOR_FLOOR is a defensive fallback if a
            // future refactor regresses Pass 1's exhaustive emit
            // — a downgrade to AutoRefactor's floor is safer than
            // a double-emit or a panic in release.
            let final_score = if (score - 1.0).abs() < f64::EPSILON {
                debug_assert!(
                    false,
                    "Pass 2 emitted a score-1.0 pair the bucket clustering missed: \
                     forms[{i}] and forms[{j}]"
                );
                AUTO_REFACTOR_FLOOR // de-rate to the floor, but keep tier consistent
            } else {
                score
            };
            let tier = tier_for(final_score, threshold);
            let forms_refs = vec![form_ref_for(&forms[i]), form_ref_for(&forms[j])];
            matches.push(Match::new(forms_refs, final_score, tier));
        }
    }
}

/// Assign a tier from a score and the caller's threshold gate.
///
/// - `score >= 0.95` ⟹ [`Tier::AutoRefactor`].
/// - `score >= 0.85` ⟹ [`Tier::ReviewFirst`].
/// - `score >= threshold` ⟹ [`Tier::Advisory`].
///
/// Callers MUST only pass scores that already cleared the
/// threshold gate. The `1.0` exact-match path is handled by Pass 1
/// directly; this helper is the Pass 2 path.
fn tier_for(score: f64, threshold: f64) -> Tier {
    debug_assert!(
        score >= threshold,
        "tier_for() called with score={score} below threshold={threshold}"
    );
    if score >= AUTO_REFACTOR_FLOOR {
        Tier::AutoRefactor
    } else if score >= REVIEW_FIRST_FLOOR {
        Tier::ReviewFirst
    } else {
        Tier::Advisory
    }
}

/// Sort matches deterministically by
/// `(forms[0].file, forms[0].span.start, -score)`.
///
/// `Match` derives only `PartialEq` (because of `f64`), so the
/// score key uses `f64::total_cmp` for a total order even on the
/// pathological inputs (`NaN`, `±0.0`); engine-emitted scores are
/// always finite and in `[threshold, 1.0]`, but the total order is
/// the right discipline.
///
/// Matches with empty `forms` lists (which the engine never emits)
/// sort to the start.
fn sort_matches_for_output(matches: &mut [Match]) {
    matches.sort_by(|a, b| {
        // Borrow-only sort keys — `FilePath` wraps `PathBuf` which
        // is non-trivial to clone, and `sort_by` calls the
        // comparator O(n log n) times.
        let key_a = (
            a.forms.first().map(|f| &f.file),
            a.forms.first().map(|f| f.span.start),
        );
        let key_b = (
            b.forms.first().map(|f| &f.file),
            b.forms.first().map(|f| f.span.start),
        );
        key_a
            .cmp(&key_b)
            // Descending score within the same file+span tie:
            // higher-confidence matches first.
            .then_with(|| b.score.total_cmp(&a.score))
    });
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
fn bucket_key<S: BuildHasher>(set: &HashSet<u64, S>) -> u64 {
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
///
/// Generic over [`BuildHasher`] so the function accepts both the
/// default `HashSet<u64>` (used by `NormalizedForm.fingerprint_set`)
/// and any caller-supplied hasher (e.g., `ahash`, `fxhash`).
#[must_use]
pub fn jaccard<S1, S2>(a: &HashSet<u64, S1>, b: &HashSet<u64, S2>) -> f64
where
    S1: BuildHasher,
    S2: BuildHasher,
{
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    // Iterate over the smaller set for the intersection — cheap
    // optimization, semantically equivalent.
    let intersection = if a.len() <= b.len() {
        a.iter().filter(|x| b.contains(x)).count()
    } else {
        b.iter().filter(|x| a.contains(x)).count()
    };
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
    fn pass1_emits_distinct_clusters_for_xor_colliding_sets() {
        // `{1, 2}` and `{4, 7}` both XOR-fold to `3`, so all four
        // forms land in the same Pass 1 bucket. The pre-fix loop
        // would emit ONE cluster (the canonical {1,2} group) and
        // leave the {4,7} pair unclaimed — Pass 2 would then see
        // them with `jaccard == 1.0` and trip its defensive guard.
        //
        // After the multi-cluster Pass 1 loop, both equal-set
        // clusters are emitted as score-1.0 AutoRefactor matches.
        debug_assert_eq!(
            1u64 ^ 2u64,
            4u64 ^ 7u64,
            "test premise: both sets must XOR-collide"
        );
        let forms = vec![
            make_form(&[1, 2], 2),
            make_form(&[1, 2], 2),
            make_form(&[4, 7], 2),
            make_form(&[4, 7], 2),
        ];
        let matches = compare(&forms, 0.85);
        assert_eq!(
            matches.len(),
            2,
            "both XOR-colliding clusters must emit independently; \
             got {matches:?}"
        );
        for m in &matches {
            assert!((m.score - 1.0).abs() < f64::EPSILON);
            assert_eq!(m.tier, Tier::AutoRefactor);
            assert_eq!(m.forms.len(), 2);
        }
    }

    #[test]
    fn pass1_leaves_xor_colliding_singletons_for_pass2() {
        // `{1, 2}` and `{4, 7}` XOR-collide into the same bucket
        // but each appears only once. Pass 1's partition step
        // produces `matched = [canonical]` (size 1) for each
        // iteration — no cluster is emitted, both forms stay
        // unclaimed, and Pass 2 evaluates the pair via Jaccard
        // (which is 0/4 = 0.0 here — filtered).
        let forms = vec![make_form(&[1, 2], 2), make_form(&[4, 7], 2)];
        let matches = compare(&forms, 0.5);
        assert!(
            matches.is_empty(),
            "XOR-colliding singletons with disjoint sets must not emit; \
             got {matches:?}"
        );
    }

    #[test]
    fn pass1_emits_cluster_and_leaves_singleton_in_same_bucket() {
        // Bucket with a 2-form cluster {1,2} plus a singleton
        // {4,7} (XOR-colliding into the same bucket). Pass 1 must
        // emit the cluster and leave the singleton unclaimed.
        let forms = vec![
            make_form(&[1, 2], 2),
            make_form(&[1, 2], 2),
            make_form(&[4, 7], 2),
        ];
        let matches = compare(&forms, 0.85);
        assert_eq!(matches.len(), 1, "only the {{1,2}} cluster should emit");
        assert!((matches[0].score - 1.0).abs() < f64::EPSILON);
        assert_eq!(matches[0].forms.len(), 2);
    }

    #[test]
    fn pass2_emits_match_for_high_jaccard_pair() {
        // Two forms with |A ∩ B| = 4 and |A ∪ B| = 5 — Jaccard 4/5
        // = 0.8. With threshold 0.7 this clears the gate.
        let forms = vec![make_form(&[1, 2, 3, 4], 4), make_form(&[1, 2, 3, 4, 5], 5)];
        let matches = compare(&forms, 0.7);
        assert_eq!(matches.len(), 1);
        assert!(
            (matches[0].score - 0.8).abs() < 1e-9,
            "expected ~0.8, got {}",
            matches[0].score
        );
    }

    #[test]
    fn pass2_filters_out_below_threshold_pairs() {
        // 1/3 Jaccard is below 0.85 — no match emitted.
        let forms = vec![make_form(&[1, 2], 2), make_form(&[2, 3], 2)];
        let matches = compare(&forms, 0.85);
        assert!(matches.is_empty());
    }

    #[test]
    fn pass2_break_math_prunes_distant_node_counts() {
        // node_count = 10 vs node_count = 100 at threshold 0.85:
        // bound = 10 / 0.85 ≈ 11.76, so the 100-node form is
        // strictly beyond and Pass 2's inner loop breaks before
        // computing Jaccard. Even if the sets were identical, no
        // match would be emitted.
        //
        // We use Pass 2-eligible forms (different fingerprint
        // sets) so Pass 1 doesn't claim them.
        let forms = vec![make_form(&[1, 2, 3], 10), make_form(&[1, 2, 4], 100)];
        let matches = compare(&forms, 0.85);
        assert!(
            matches.is_empty(),
            "break math should prune the disproportionate-size pair"
        );
    }

    #[test]
    fn pass2_break_math_keeps_near_node_counts() {
        // Same Jaccard score (0.5), but the node_counts are close
        // enough that the break math doesn't prune. The score 0.5
        // is below the 0.85 default threshold, so we lower the
        // threshold to 0.4 to actually see the match.
        let forms = vec![make_form(&[1, 2, 3], 3), make_form(&[2, 3, 4], 3)];
        let matches = compare(&forms, 0.4);
        assert_eq!(matches.len(), 1);
        assert!((matches[0].score - 0.5).abs() < 1e-9);
        // 0.5 is < 0.85 review_first floor -> Advisory.
        assert_eq!(matches[0].tier, Tier::Advisory);
    }

    #[test]
    fn pass2_tier_assignment_auto_refactor_floor() {
        // Score >= 0.95 -> AutoRefactor (Pass 2 path; not score 1.0).
        // A = {1..=19, 20} (20 elts), B = {1..=19} (19 elts).
        // intersection = 19, union = 20 -> 0.95 exactly.
        let a: Vec<u64> = (1..=20).collect();
        let b: Vec<u64> = (1..=19).collect();
        let forms = vec![make_form(&a, 20), make_form(&b, 19)];
        let matches = compare(&forms, 0.5);
        assert_eq!(matches.len(), 1);
        assert!(
            (matches[0].score - 0.95).abs() < 1e-12,
            "expected 0.95, got {}",
            matches[0].score
        );
        assert_eq!(matches[0].tier, Tier::AutoRefactor);
    }

    #[test]
    fn pass2_tier_assignment_review_first_floor() {
        // Score in [0.85, 0.95) -> ReviewFirst.
        // A = {1..=17, 18, 19} (19 elts), B = {1..=17, 20} (18 elts).
        // intersection = 17, union = 20 -> 0.85 exactly.
        let a: Vec<u64> = (1..=17).chain([18, 19]).collect();
        let b: Vec<u64> = (1..=17).chain([20]).collect();
        let forms = vec![make_form(&a, 19), make_form(&b, 18)];
        let matches = compare(&forms, 0.5);
        assert_eq!(matches.len(), 1);
        assert!(
            (matches[0].score - 0.85).abs() < 1e-12,
            "expected 0.85, got {}",
            matches[0].score
        );
        assert_eq!(matches[0].tier, Tier::ReviewFirst);
    }

    #[test]
    fn pass2_tier_assignment_advisory() {
        // Score >= threshold but < 0.85 -> Advisory.
        // Already covered by pass2_break_math_keeps_near_node_counts;
        // here we add an explicit check at a different threshold.
        let forms = vec![make_form(&[1, 2, 3, 4], 4), make_form(&[3, 4, 5, 6], 4)];
        let matches = compare(&forms, 0.3);
        assert_eq!(matches.len(), 1);
        // 2/6 = 0.333...
        assert_eq!(matches[0].tier, Tier::Advisory);
    }

    fn make_form_with_qualified_name(
        fps: &[u64],
        qname: &[&str],
        node_count: u32,
    ) -> NormalizedForm {
        use crate::domain::{FormKind, LineColumn, Span};
        NormalizedForm::with_context(
            FormKind::Production,
            fps.iter().copied().collect(),
            Vec::new(),
            qname.iter().map(|s| (*s).to_string()).collect(),
            Span::try_new(LineColumn::new(1, 0), LineColumn::new(1, 0)).unwrap(),
            node_count,
            1,
        )
    }

    #[test]
    fn output_sort_by_file_then_span_then_descending_score() {
        // Three exact-match clusters with distinct qualified_names
        // — the engine synthesizes file paths from qualified_name
        // joined with `::`, so we can predict the sort order.
        let forms = vec![
            // Cluster Z (qualified: "zeta")
            make_form_with_qualified_name(&[1, 2, 3], &["zeta"], 3),
            make_form_with_qualified_name(&[1, 2, 3], &["zeta"], 3),
            // Cluster A (qualified: "alpha")
            make_form_with_qualified_name(&[4, 5, 6], &["alpha"], 3),
            make_form_with_qualified_name(&[4, 5, 6], &["alpha"], 3),
            // Cluster M (qualified: "mid")
            make_form_with_qualified_name(&[7, 8, 9], &["mid"], 3),
            make_form_with_qualified_name(&[7, 8, 9], &["mid"], 3),
        ];
        let matches = compare(&forms, 0.85);
        assert_eq!(matches.len(), 3);

        // Sort key is forms[0].file: "alpha" < "mid" < "zeta".
        let file_at = |idx: usize| matches[idx].forms[0].file.to_string();
        assert_eq!(file_at(0), "alpha");
        assert_eq!(file_at(1), "mid");
        assert_eq!(file_at(2), "zeta");
    }

    #[test]
    fn output_is_byte_equal_across_invocations() {
        // Determinism check — running compare() twice on the same
        // input produces identical Vec<Match>.
        let forms = vec![
            make_form_with_qualified_name(&[1, 2, 3, 4], &["foo"], 4),
            make_form_with_qualified_name(&[1, 2, 3, 5], &["bar"], 4),
            make_form_with_qualified_name(&[1, 2, 3, 4], &["foo"], 4),
            make_form_with_qualified_name(&[10, 20], &["baz"], 2),
        ];
        let r1 = compare(&forms, 0.5);
        let r2 = compare(&forms, 0.5);
        assert_eq!(r1, r2);
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
        // No false score-1.0 match should appear.
        for m in &matches {
            assert!(
                (m.score - 1.0).abs() > f64::EPSILON,
                "no score-1.0 match should be emitted across XOR-colliding non-equal sets, got: {m:?}"
            );
        }
    }

    #[test]
    fn pass1_xor_collision_inside_bucket_with_one_real_cluster() {
        // Three forms: two structurally equal (the canonical pair)
        // and one XOR-colliding outlier. Pass 1 must emit one match
        // for the pair and leave the outlier unclaimed. Pass 2 then
        // compares the outlier against members of the verified
        // cluster — Jaccard is 0.0 (disjoint), so no Pass 2 match.
        let forms = vec![
            make_form(&[1, 2, 3], 3), // bucket key 0
            make_form(&[1, 2, 3], 3), // bucket key 0 — canonical pair
            make_form(&[0], 1),       // bucket key 0 — XOR collision
        ];
        let matches = compare(&forms, 0.85);
        assert_eq!(matches.len(), 1, "exactly one Pass 1 cluster expected");
        assert_eq!(matches[0].forms.len(), 2);
        assert!((matches[0].score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn pass1_pure_collision_two_singletons_fall_through_to_pass2() {
        // A 2-element bucket where neither side matches the other:
        // two distinct sets that XOR to the same key. Pass 1 sees
        // a bucket of size 2, verifies fingerprint_set against the
        // first element, finds zero matches, and leaves both
        // unclaimed (the `verified.len() < 2` early-exit branch).
        //
        // Pass 2 then runs over both as unclaimed forms. They are
        // disjoint (Jaccard 0.0) so no match is emitted.
        let forms = vec![
            make_form(&[1, 2, 3], 3), // XOR = 0
            make_form(&[5, 6, 3], 3), // XOR = 0 (5 ^ 6 ^ 3 = 0)
        ];
        // Verify both forms hash to the same bucket key.
        assert_eq!(
            bucket_key(&forms[0].fingerprint_set),
            bucket_key(&forms[1].fingerprint_set),
            "test precondition: XOR collision setup"
        );
        let matches = compare(&forms, 0.85);
        assert!(matches.is_empty(), "disjoint sets should not match");
    }

    #[test]
    fn pass1_and_pass2_coexist_in_same_input() {
        // One Pass 1 exact-match cluster + one Pass 2 near-match
        // pair in the same input. Both surface in the output.
        let forms = vec![
            // Exact match pair (Pass 1)
            make_form_with_qualified_name(&[1, 2, 3], &["exact_a"], 3),
            make_form_with_qualified_name(&[1, 2, 3], &["exact_b"], 3),
            // Near match (Pass 2) — 4/5 Jaccard
            make_form_with_qualified_name(&[10, 20, 30, 40], &["near_a"], 4),
            make_form_with_qualified_name(&[10, 20, 30, 40, 50], &["near_b"], 5),
        ];
        let matches = compare(&forms, 0.7);
        assert_eq!(matches.len(), 2);
        // Sort: "exact_a" < "near_a" alphabetically.
        assert!((matches[0].score - 1.0).abs() < f64::EPSILON);
        assert_eq!(matches[0].tier, Tier::AutoRefactor);
        assert_eq!(matches[0].forms.len(), 2);

        assert!((matches[1].score - 0.8).abs() < 1e-9);
        // 0.8 < 0.85 review_first floor -> Advisory.
        assert_eq!(matches[1].tier, Tier::Advisory);
        assert_eq!(matches[1].forms.len(), 2);
    }

    #[test]
    fn threshold_of_1_0_emits_only_exact_matches() {
        // With threshold = 1.0, Pass 2's filter `score >= threshold`
        // requires score == 1.0 — Pass 1 already emits those, so
        // Pass 2 emits nothing.
        let forms = vec![
            make_form(&[1, 2, 3], 3),
            make_form(&[1, 2, 3], 3),
            make_form(&[1, 2, 4], 3), // 2/4 = 0.5 against the pair — filtered
        ];
        let matches = compare(&forms, 1.0);
        assert_eq!(
            matches.len(),
            1,
            "only the exact-match cluster should survive"
        );
        assert!((matches[0].score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn forms_with_disjoint_fingerprints_emit_no_matches() {
        let forms = vec![
            make_form(&[1, 2, 3], 3),
            make_form(&[100, 200], 2),
            make_form(&[1000], 1),
        ];
        let matches = compare(&forms, 0.5);
        assert!(matches.is_empty());
    }

    #[test]
    #[should_panic(expected = "threshold must lie in")]
    fn threshold_zero_panics_in_debug() {
        // The debug_assert! catches out-of-range threshold in
        // debug builds. Release builds (incl. `cargo build
        // --release`) skip the check; the CLI surface (PR 8) is
        // the production-build input-validation boundary.
        let _ = compare(&[], 0.0);
    }

    #[test]
    #[should_panic(expected = "threshold must lie in")]
    fn threshold_above_one_panics_in_debug() {
        let _ = compare(&[], 1.5);
    }

    #[test]
    #[should_panic(expected = "threshold must lie in")]
    fn threshold_nan_panics_in_debug() {
        let _ = compare(&[], f64::NAN);
    }
}
