//! Comparison engine for the dry structural duplication detector.
//!
//! Single module — dry-rs has one algorithm (Jaccard on subform
//! fingerprints), not a detector taxonomy (per O6). Three stages:
//!
//! 1. **Hash-bucket clustering** — first pass clusters forms by their
//!    `fingerprint_set` hash. Exact structural matches surface in O(N)
//!    without pairwise comparison, as n-ary matches.
//! 2. **Sliding-window Jaccard** — second pass over remaining forms
//!    sorted ascending by `node_count`. For each form `forms[i]`, the
//!    inner loop breaks when
//!    `forms[j].node_count > forms[i].node_count / threshold`. This
//!    is the Jaccard upper bound: `J(A,B) <= min(|A|,|B|)/max(|A|,|B|)`,
//!    so for threshold `t`, the largest comparable form has
//!    `node_count <= forms[i].node_count / t`. Collects the
//!    `>= threshold` pairwise edges.
//! 3. **Clique carving** (dry-rs#97, adr-cluster-output) — third
//!    stage groups the Pass 2 edge graph into maximal cliques so a
//!    near-duplication appearing in N places surfaces as ONE n-ary
//!    match. Every intra-cluster pair carries a computed Jaccard
//!    `>= threshold`; leftover edges emit as residual binary matches
//!    (edge conservation — the clustering is a lossless regrouping
//!    of the pairwise output).
//!
//! Threshold tier assignment (`auto_refactor` >= 0.95,
//! `review_first` >= 0.85, `advisory` >= threshold) drives
//! agentic-quality routing; a cluster routes by its WEAKEST pair
//! (score = minimum intra-clique Jaccard, generalizing Pass 1's
//! score-1.0-as-group-min precedent).
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
//! ## Pass 3 — clique carving
//!
//! The Pass 2 edges form a sparse graph (vertices = forms, edge =
//! `>= threshold` pair). Pass 3 carves **maximal cliques** with a
//! prefer-larger-cliques greedy: seed at the highest-score
//! unassigned edge, grow by repeatedly admitting the candidate
//! adjacent to ALL current members that maximizes the minimum edge
//! into the clique. Guarantees, per adr-cluster-output:
//!
//! - **Clique guarantee** — every pair inside an emitted cluster has
//!   a computed Jaccard `>= threshold`; a missing edge (pruned by
//!   the window proxy or computed sub-threshold) blocks membership
//!   and is never fabricated as `0.0`.
//! - **Edge conservation** — every collected edge is represented in
//!   the output exactly once: absorbed inside a clique or emitted as
//!   a residual binary match. Nothing the pairwise output carried is
//!   lost; a form may appear in multiple matches, as before.
//! - **Determinism** — carving order, candidate tie-breaks, and
//!   member order derive from form identity `(file, span)` and
//!   `f64::total_cmp`; cluster membership is stable across walker
//!   orderings. Components larger than `CLUSTER_COMPONENT_CAP`
//!   (private const, 512) fall back to pairwise passthrough.
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

use crate::domain::{FilePath, FormRef, LineColumn, Match, NormalizedForm, ResolvedScope, Tier};

mod antiunify;

pub use antiunify::{
    DistinctValue, Divergence, Hole, HoleId, HoleKind, SubElement, Substitution, Template,
    TemplateNode, antiunify, instantiate,
};

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

/// Largest connected component (in forms) the Pass 3 clique carving
/// will process. Components above the cap fall back to pairwise
/// passthrough — every edge emits as a binary match, exactly the
/// pre-clustering behavior — keeping the engine deterministic and
/// bounded on pathological generated-code families
/// (adr-cluster-output ADR-6). Defense in depth only: the largest
/// component observed across six real corpora (including a
/// 23k-form workspace at a lenient 0.6 threshold) is 110 forms.
const CLUSTER_COMPONENT_CAP: usize = 512;

/// Compare a slice of normalized forms and return all matches whose
/// Jaccard similarity meets or exceeds `threshold`.
///
/// The implementation runs three stages:
///
/// 1. **Hash-bucket clustering** — forms whose `fingerprint_set` is
///    structurally identical surface as an n-ary match with score
///    `1.0` (tier [`Tier::AutoRefactor`]). XOR-bucket collisions are
///    rejected via a structural-equality verification step before
///    emission.
/// 2. **Sliding-window Jaccard** — remaining pairs whose Jaccard
///    similarity clears `threshold` are collected as internal
///    pairwise edges (never emitted directly).
/// 3. **Clique carving** — the Pass 2 edge graph is partitioned into
///    maximal cliques; each clique emits one n-ary match scored by
///    its weakest intra-clique pair, and every edge not absorbed by a
///    clique emits as a residual binary match (edge conservation,
///    dry-rs#97 / adr-cluster-output).
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
    compare_with(
        forms,
        threshold,
        &CompareCtx::new(&SyntheticPathResolver, ResolvedScope::default()),
    )
}

/// Compare with caller-supplied file paths attached to each form by
/// index.
///
/// `paths.len()` MUST equal `forms.len()`; element `paths[i]` is the
/// `FilePath` for `forms[i]`. The returned matches carry real paths
/// on each [`FormRef`], not the synthetic stub that [`compare`] emits.
///
/// This is the CLI run-loop's entry point — the run loop tracks
/// (path, form) pairs during normalization and threads both into the
/// comparison engine. Library callers that don't track paths use
/// [`compare`] (which falls back to a `qualified_name`-derived
/// synthetic path).
///
/// # Panics
///
/// Panics on length mismatch between `forms` and `paths` in both
/// debug AND release builds — `IndexedPathResolver::path_for` indexes
/// `paths[i]` unconditionally, so a mismatch would panic with a
/// cryptic `index out of bounds` deep in the engine. The explicit
/// `assert_eq!` surfaces the contract violation up front with the
/// argument lengths in the message. The threshold-range check is the
/// same debug-only `debug_assert!` as [`compare`].
#[must_use]
pub fn compare_with_paths(
    forms: &[NormalizedForm],
    paths: &[FilePath],
    threshold: f64,
) -> Vec<Match> {
    compare_with_paths_scoped(forms, paths, threshold, ResolvedScope::default())
}

/// Compare with caller-supplied paths AND a relatedness-scoping
/// predicate ([`ResolvedScope`], dry-rs#124).
///
/// Identical to [`compare_with_paths`] except the engine prunes
/// candidate pairs by the supplied `scope`: a pair the predicate
/// disallows (different crate when `across_crate == false`, same module
/// when `within_module == false`, …) never clusters — neither as a
/// Pass 1 exact-match group member nor as a Pass 2 pairwise edge. The
/// default `ResolvedScope` (all axes allowed) reproduces
/// [`compare_with_paths`] exactly (byte-identical output), so this is a
/// strict superset entry point: the CLI run loop calls it with the
/// resolved scope, and the legacy facade delegates here with the
/// all-true default.
///
/// The predicate reads each form's [`crate::domain::StructuralLocation`]
/// (`forms[i].location`) — the engine never mutates it. Pairs are
/// gated on `forms[i].location` vs `forms[j].location`; the wire-visible
/// [`FormRef`] never carries the location (it stays on the engine-
/// internal [`NormalizedForm`]).
///
/// # Panics
///
/// Same as [`compare_with_paths`]: panics on a `forms` / `paths` length
/// mismatch in both debug and release builds; `debug_assert!`s the
/// threshold range.
#[must_use]
pub fn compare_with_paths_scoped(
    forms: &[NormalizedForm],
    paths: &[FilePath],
    threshold: f64,
    scope: ResolvedScope,
) -> Vec<Match> {
    assert_eq!(
        forms.len(),
        paths.len(),
        "compare_with_paths(): forms and paths must be the same length; \
         got forms={} paths={}",
        forms.len(),
        paths.len()
    );
    compare_with(
        forms,
        threshold,
        &CompareCtx::new(&IndexedPathResolver { paths }, scope),
    )
}

/// Engine invocation context — the bundle of strategy objects threaded
/// through the three comparison passes.
///
/// Introduced (dry-rs#124) so the engine grows new orthogonal concerns
/// (path resolution; relatedness scoping; the v0.5 rayon-parallelism
/// param) without stacking positional arguments on every helper. The
/// public facades ([`compare`], [`compare_with_paths`]) build a context
/// internally; only [`compare_with_paths_scoped`] (the CLI run-loop
/// entry) supplies a non-default `scope`.
///
/// Holds borrows (`'a`), so it is constructed per `compare_with` call
/// and never retained.
struct CompareCtx<'a> {
    /// Strategy that maps `forms[i]` to the [`FilePath`] embedded in its
    /// emitted [`FormRef`] (synthetic-from-qualified-name for the
    /// library facade; indexed-from-caller-paths for the CLI run loop).
    resolver: &'a dyn PathResolver,
    /// Relatedness-scoping predicate. [`ResolvedScope::default`] (all
    /// axes allowed) is the no-op identity — every pair is allowed to
    /// cluster, exactly as the pre-scoping engine behaved.
    scope: ResolvedScope,
}

impl<'a> CompareCtx<'a> {
    /// Construct an engine context from a path resolver + scope
    /// predicate.
    const fn new(resolver: &'a dyn PathResolver, scope: ResolvedScope) -> Self {
        Self { resolver, scope }
    }
}

/// Internal entry point parameterized by the engine [`CompareCtx`].
fn compare_with(forms: &[NormalizedForm], threshold: f64, ctx: &CompareCtx) -> Vec<Match> {
    debug_assert!(
        threshold > 0.0 && threshold <= 1.0,
        "compare() threshold must lie in (0.0, 1.0]; got {threshold}"
    );

    let mut matches: Vec<Match> = Vec::new();
    let mut claimed: HashSet<usize> = HashSet::new();

    // Pass 1 — hash-bucket clustering. Verified exact matches are
    // emitted and their indices are marked `claimed`. XOR
    // collisions, partial-bucket mismatches, AND scope-disallowed
    // members leave their indices unclaimed so Pass 2 (sliding-window)
    // can compare them pairwise (where the scope gate applies again).
    pass1_hash_bucket(forms, ctx, &mut matches, &mut claimed);

    // Pass 2 — sliding-window Jaccard over forms NOT claimed by
    // Pass 1. Sorted ascending by `node_count` with the
    // break-math shortcut. Collects the >= threshold pairwise edges
    // (scope-disallowed pairs are skipped before Jaccard) instead of
    // emitting matches directly.
    let edges = pass2_sliding_window(forms, threshold, &claimed, ctx.scope);

    // Pass 3 — clique carving over the collected edge graph
    // (dry-rs#97, adr-cluster-output). UNCHANGED by scoping: the edge
    // graph already contains only scope-allowed pairs, and
    // `grow_clique`'s adjacent-to-all invariant structurally cannot
    // admit a pair with no edge — so a disallowed pair (which has no
    // edge) can never enter a clique (edge conservation). Maximal
    // cliques emit as n-ary matches; leftover edges emit as residual
    // binary matches.
    emit_pass2_clusters(
        forms,
        ctx.resolver,
        threshold,
        &edges,
        CLUSTER_COMPONENT_CAP,
        &mut matches,
    );

    sort_matches_for_output(&mut matches);
    matches
}

/// Path-resolver strategy used by the comparison engine to construct
/// each [`FormRef`]'s `file` field. The library-facing `compare()` uses
/// a synthetic stub derived from `qualified_name`; the CLI run loop
/// uses an indexed strategy that maps `forms[i]` -> caller-supplied
/// `paths[i]`.
///
/// Object-safe (used as `&dyn PathResolver`) — keeps the engine's two
/// passes generic without forcing a type parameter on every helper.
trait PathResolver {
    /// Return the [`FilePath`] to embed in the `FormRef` for the form
    /// at index `i` in the comparison engine's input slice.
    fn path_for(&self, form: &NormalizedForm, i: usize) -> FilePath;
}

/// Library-facing fallback: synthesize a placeholder path from the
/// form's `qualified_name`. Equivalent to the pre-PR-8 behavior; kept
/// so `compare()` (the public legacy entry point) stays usable from
/// the comparison-engine unit tests that don't thread paths.
struct SyntheticPathResolver;

impl PathResolver for SyntheticPathResolver {
    fn path_for(&self, form: &NormalizedForm, _i: usize) -> FilePath {
        let synthesized = if form.qualified_name.is_empty() {
            std::path::PathBuf::from("<unknown>")
        } else {
            std::path::PathBuf::from(form.qualified_name.join("::"))
        };
        FilePath::from(synthesized)
    }
}

/// CLI-facing resolver: pull `paths[i]` for `forms[i]`. The CLI run
/// loop owns the (path, form) pairing during normalization and threads
/// it through to the comparison engine.
struct IndexedPathResolver<'a> {
    paths: &'a [FilePath],
}

impl PathResolver for IndexedPathResolver<'_> {
    fn path_for(&self, _form: &NormalizedForm, i: usize) -> FilePath {
        // Debug-asserted to be in-bounds in `compare_with_paths`.
        self.paths[i].clone()
    }
}

/// Pass 1 — hash-bucket clustering. Groups forms by an XOR-fold of
/// their `fingerprint_set`, verifies each grouped pair has a
/// structurally-equal set, and emits a single n-ary [`Match`] per
/// verified bucket. Verified-cluster indices land in `claimed`;
/// unverified ones leave their indices unclaimed for Pass 2.
fn pass1_hash_bucket(
    forms: &[NormalizedForm],
    ctx: &CompareCtx,
    matches: &mut Vec<Match>,
    claimed: &mut HashSet<usize>,
) {
    let buckets = group_forms_by_bucket_key(forms);
    for (_key, indices) in buckets {
        if indices.len() < 2 {
            continue;
        }
        emit_clusters_for_bucket(forms, ctx, indices, matches, claimed);
    }
}

/// Group every non-empty form's index by its `fingerprint_set`
/// XOR-fold bucket key. Empty fingerprint sets are skipped — they
/// have no structure to match (see "Empty `fingerprint_set` policy"
/// in the module doc) and Pass 2's Jaccard returns 0.0 against any
/// empty side, filtering them out naturally.
///
/// `BTreeMap` (not `HashMap`) keeps Pass 1's emit order deterministic
/// before the final sort. Cheap insurance against debugging
/// surprises if a future refactor relies on emit order.
fn group_forms_by_bucket_key(forms: &[NormalizedForm]) -> BTreeMap<u64, Vec<usize>> {
    let mut buckets: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, form) in forms.iter().enumerate() {
        if form.fingerprint_set.is_empty() {
            continue;
        }
        let key = bucket_key(&form.fingerprint_set);
        buckets.entry(key).or_default().push(i);
    }
    buckets
}

/// Drain every equal-set cluster from a single XOR-bucket.
///
/// A single bucket can hold multiple distinct equal-set clusters
/// that XOR-fold to the same key (rare but legal: e.g. `{1, 2}` and
/// `{4, 7}` both fold to `3`). Iterate until every cluster within
/// the bucket is emitted so Pass 2 never has to handle a
/// `score == 1.0` pair. Singletons (canonical with no equal partner
/// in the bucket) drop out naturally and stay unclaimed for Pass 2's
/// pairwise scan.
fn emit_clusters_for_bucket(
    forms: &[NormalizedForm],
    ctx: &CompareCtx,
    mut indices: Vec<usize>,
    matches: &mut Vec<Match>,
    claimed: &mut HashSet<usize>,
) {
    while indices.len() >= 2 {
        let canonical_set = &forms[indices[0]].fingerprint_set;
        let (cluster, leftover): (Vec<usize>, Vec<usize>) = indices
            .iter()
            .copied()
            .partition(|&i| forms[i].fingerprint_set == *canonical_set);

        if cluster.len() >= 2 {
            emit_pass1_cluster(forms, ctx, &cluster, matches, claimed);
        }
        // Canonical (and any singleton from this partition step)
        // landed in `cluster` — drop it from the working set either
        // way; `leftover` is the rest of the bucket.
        indices = leftover;
    }
}

/// Emit Pass 1 matches for an equal-set cluster (every member's
/// `fingerprint_set` is byte-identical, so every intra-cluster pair has
/// Jaccard 1.0).
///
/// ## Scope sub-partition (dry-rs#124)
///
/// When the relatedness-scoping predicate ([`CompareCtx::scope`]) allows
/// EVERY pair in the cluster (always true under the all-true default),
/// the whole cluster surfaces as ONE n-ary score-1.0 [`Match`] — the
/// fast path, byte-identical to the pre-scoping engine. When the
/// predicate disallows some pair (e.g. two identical forms in different
/// crates under `across_crate == false`), the cluster is sub-partitioned
/// into scope-allowed groups via the SAME clique-carving machinery Pass 3
/// uses ([`emit_pass2_clusters`], over synthetic score-1.0 edges): every
/// emitted group is a scope-allowed clique, leftover allowed pairs emit
/// as residual score-1.0 binary matches, and a member with NO
/// scope-allowed equal partner stays unclaimed (its only equal pairs are
/// disallowed, which Pass 2's gate skips — so no score-1.0 pair ever
/// leaks into Pass 2's defensive guard).
///
/// EVERY member that participates in any scope-allowed exact pair is
/// claimed here, so Pass 2 never re-derives a score-1.0 equal-set pair.
///
/// ## Fast-path short-circuit (Qodo, dry-rs#124)
///
/// Two sequential fast-path gates, cheapest first:
///
/// 1. [`ResolvedScope::permits_all`] — O(1). When every axis is `true`
///    the scope CANNOT disallow any pair, so the default (no-`[scope]`)
///    run never pays the O(k²) `allows()` scan on a large identical
///    bucket. This is the Qodo perf fix: the common case is O(1) per
///    cluster.
/// 2. [`cluster_is_one_scope_allowed_clique`] — O(k²). Only reached when
///    the scope IS restricted; confirms a restricted-scope cluster
///    happens to be wholly mutually-allowed (e.g. an all-same-crate
///    bucket under `across_crate == false`), so it still emits as one
///    n-ary match rather than carving.
///
/// Both gates route to the same [`emit_pass1_fast_path`]; only when BOTH
/// fail does the cluster carve. Kept as two `if`s (not one `||`) so each
/// gate is independently exercised — the cheap O(1) gate is not fused
/// with the expensive scan.
fn emit_pass1_cluster(
    forms: &[NormalizedForm],
    ctx: &CompareCtx,
    cluster: &[usize],
    matches: &mut Vec<Match>,
    claimed: &mut HashSet<usize>,
) {
    // Gate 1 (O(1)): unrestricted scope cannot disallow any pair — take
    // the fast path without ever running the per-pair scan.
    if ctx.scope.permits_all() {
        emit_pass1_fast_path(forms, ctx.resolver, cluster, matches, claimed);
        return;
    }

    // Gate 2 (O(k²)): the scope restricts SOMETHING, but this particular
    // cluster may still be wholly mutually-allowed.
    if cluster_is_one_scope_allowed_clique(forms, ctx.scope, cluster) {
        emit_pass1_fast_path(forms, ctx.resolver, cluster, matches, claimed);
        return;
    }

    // Slow path — some intra-cluster pair is scope-disallowed. Carve the
    // scope-allowed sub-graph with the shared Pass 3 machinery over
    // synthetic score-1.0 edges, then mark every claimed member.
    let edges = scope_allowed_unit_edges(forms, ctx.scope, cluster);
    let before = matches.len();
    emit_pass2_clusters(
        forms,
        ctx.resolver,
        // Threshold below the score (1.0) so `tier_for` routes every
        // emitted exact group / residual to `AutoRefactor`, matching the
        // fast path's tier.
        1.0,
        &edges,
        CLUSTER_COMPONENT_CAP,
        matches,
    );
    mark_emitted_members_claimed(&matches[before..], cluster, forms, ctx.resolver, claimed);
}

/// Emit one n-ary score-1.0 [`Match`] for a whole equal-set cluster and
/// claim every member — the Pass 1 fast path. Byte-identical to the
/// pre-scoping engine, which is what keeps the default-scope output
/// stable.
fn emit_pass1_fast_path(
    forms: &[NormalizedForm],
    resolver: &dyn PathResolver,
    cluster: &[usize],
    matches: &mut Vec<Match>,
    claimed: &mut HashSet<usize>,
) {
    let forms_refs: Vec<FormRef> = cluster
        .iter()
        .map(|&i| form_ref_for(&forms[i], i, resolver))
        .collect();
    matches.push(Match::new(forms_refs, 1.0, Tier::AutoRefactor));
    for &i in cluster {
        claimed.insert(i);
    }
}

/// True when every pair in an equal-set cluster is scope-allowed — the
/// fast-path test that keeps the default-scope (all-true) output
/// byte-identical to the pre-scoping engine. `O(k²)` over the (small)
/// equal-set group; under the all-true default the first pair already
/// short-circuits to `true` for the common single-location group.
fn cluster_is_one_scope_allowed_clique(
    forms: &[NormalizedForm],
    scope: ResolvedScope,
    cluster: &[usize],
) -> bool {
    for (pos, &a) in cluster.iter().enumerate() {
        for &b in &cluster[pos + 1..] {
            if !scope.allows(&forms[a].location, &forms[b].location) {
                return false;
            }
        }
    }
    true
}

/// Build the synthetic score-1.0 edge set over the scope-ALLOWED pairs of
/// an equal-set cluster. Every intra-cluster pair has Jaccard 1.0, so the
/// only filter is the scope predicate; the resulting edges feed the
/// shared Pass 3 carving so exact-match scope sub-partitioning reuses the
/// near-match clique guarantee + edge conservation.
fn scope_allowed_unit_edges(
    forms: &[NormalizedForm],
    scope: ResolvedScope,
    cluster: &[usize],
) -> Vec<PairwiseEdge> {
    let mut edges = Vec::new();
    for (pos, &i) in cluster.iter().enumerate() {
        for &j in &cluster[pos + 1..] {
            if scope.allows(&forms[i].location, &forms[j].location) {
                edges.push(PairwiseEdge { i, j, score: 1.0 });
            }
        }
    }
    edges
}

/// Mark every cluster member that appeared in a freshly-emitted Pass 1
/// scope sub-partition match as `claimed`, so Pass 2 never re-derives the
/// score-1.0 equal-set pairs the sub-partition already represented.
///
/// `emitted` is the slice of matches the carving just pushed; a member's
/// `(file, span)` identity links its [`FormRef`] back to its input index.
/// A member absent from every emitted match (no scope-allowed equal
/// partner) is intentionally left unclaimed.
fn mark_emitted_members_claimed(
    emitted: &[Match],
    cluster: &[usize],
    forms: &[NormalizedForm],
    resolver: &dyn PathResolver,
    claimed: &mut HashSet<usize>,
) {
    for &i in cluster {
        let want = form_ref_for(&forms[i], i, resolver);
        let appears = emitted.iter().any(|m| {
            m.forms
                .iter()
                .any(|f| f.file == want.file && f.span == want.span)
        });
        if appears {
            claimed.insert(i);
        }
    }
}

/// A Pass 2 pairwise edge: two indices into the engine's input slice
/// plus the computed (post-de-rate) Jaccard score. Engine-internal
/// plumbing for the Pass 3 clique carving — never a domain type,
/// never on the wire. Indices are invocation-scoped; the durable
/// identity is the projected [`FormRef`].
struct PairwiseEdge {
    i: usize,
    j: usize,
    score: f64,
}

/// Pass 2 — sliding-window Jaccard over unclaimed forms. Sorts
/// candidates ascending by `node_count`, then for each pair
/// `(i, j)` with `i < j` the inner loop breaks when
/// `forms[j].node_count > forms[i].node_count / threshold`. Collects
/// one [`PairwiseEdge`] per pair clearing `threshold` (Pass 2 cannot
/// produce `score == 1.0` — those land in Pass 1); Pass 3 turns the
/// edge graph into matches.
fn pass2_sliding_window(
    forms: &[NormalizedForm],
    threshold: f64,
    claimed: &HashSet<usize>,
    scope: ResolvedScope,
) -> Vec<PairwiseEdge> {
    let sorted = sort_unclaimed_by_node_count(forms, claimed);
    let mut edges: Vec<PairwiseEdge> = Vec::new();

    for outer_pos in 0..sorted.len() {
        let i = sorted[outer_pos];
        // f64 cast on u32 is exact for valid node counts (well
        // below 2^53). The CLI-side gate (PR 8) clamps inputs.
        let bound = f64::from(forms[i].node_count) / threshold;
        for &j in &sorted[outer_pos + 1..] {
            // Break math: strict inequality. No later k > j
            // (sorted ascending by node_count) can clear the
            // threshold.
            if f64::from(forms[j].node_count) > bound {
                break;
            }
            if let Some(edge) = try_collect_pass2_edge(forms, threshold, i, j, scope) {
                edges.push(edge);
            }
        }
    }
    edges
}

/// Project to unclaimed indices and sort ascending by
/// `(node_count, original_index)`. The secondary sort key keeps the
/// iteration order deterministic when node counts tie.
fn sort_unclaimed_by_node_count(forms: &[NormalizedForm], claimed: &HashSet<usize>) -> Vec<usize> {
    let mut sorted: Vec<usize> = (0..forms.len()).filter(|i| !claimed.contains(i)).collect();
    sorted.sort_by_key(|&i| (forms[i].node_count, i));
    sorted
}

/// Try to collect a Pass 2 edge for the candidate pair `(i, j)`.
///
/// Computes Jaccard, applies the threshold gate, and resolves the
/// effective score (de-rating any unexpected score-1.0 hit per the
/// Pass 1 exhaustive-emit invariant). Returns `None` below the
/// threshold — sub-threshold similarities are never retained, so the
/// edge graph only ever contains `>= threshold` pairs.
fn try_collect_pass2_edge(
    forms: &[NormalizedForm],
    threshold: f64,
    i: usize,
    j: usize,
    scope: ResolvedScope,
) -> Option<PairwiseEdge> {
    // Relatedness-scoping gate (dry-rs#124): a pair the predicate
    // disallows (by crate / module boundary) is pruned BEFORE Jaccard,
    // so a disallowed pair never becomes an edge — and therefore (by
    // Pass 3's adjacent-to-all clique invariant) never enters any
    // cluster. Checked first because it is a cheap field comparison and
    // short-circuits the Jaccard intersection on disallowed pairs.
    if !scope.allows(&forms[i].location, &forms[j].location) {
        return None;
    }
    // `jaccard_score` (not `score`) to stay clear of the `scope` param
    // under clippy::similar_names; the wire field stays `score`.
    let jaccard_score = jaccard(&forms[i].fingerprint_set, &forms[j].fingerprint_set);
    if jaccard_score < threshold {
        return None;
    }
    let final_score = resolve_pass2_score(jaccard_score, i, j);
    Some(PairwiseEdge {
        i,
        j,
        score: final_score,
    })
}

/// Identity key for a form inside the Pass 3 carving: the projected
/// `(file, span.start, span.end)` plus the input index as a final
/// tie-break for degenerate inputs (e.g. unit-test forms sharing one
/// synthetic path and span). On real corpora the `(file, span)`
/// prefix is unique, which is what makes cluster membership and
/// member ordering stable across walker orderings — the input index
/// never decides anything unless identities collide.
type NodeIdent = (FilePath, LineColumn, LineColumn, usize);

fn node_ident(forms: &[NormalizedForm], i: usize, resolver: &dyn PathResolver) -> NodeIdent {
    let form = &forms[i];
    (
        resolver.path_for(form, i),
        form.span.start,
        form.span.end,
        i,
    )
}

/// Pass 3 — clique carving over the Pass 2 edge graph (dry-rs#97,
/// adr-cluster-output).
///
/// Carves **maximal cliques** out of each connected component with a
/// prefer-larger-cliques greedy: seed at the highest-score unassigned
/// edge, then repeatedly admit the candidate adjacent (`>= threshold`)
/// to **all** current members that maximizes the minimum edge into
/// the clique. Every emitted cluster is a clique in the thresholded
/// graph — every intra-cluster pair carries a COMPUTED Jaccard
/// `>= threshold` (the "guaranteed extractability" contract that
/// makes tier routing safe for downstream automation). A missing
/// edge always blocks membership; absence means "not computed", never
/// a fabricated 0.0 (adr-cluster-output ADR-5).
///
/// **Edge conservation** (ADR-2): every collected edge is represented
/// in the output exactly once — absorbed inside a carved clique, or
/// emitted as a residual binary [`Match`] identical to the
/// pre-clustering output. Nothing Pass 2 found is lost; a form may
/// appear in multiple matches, exactly as in the pairwise output this
/// stage replaces.
///
/// **Determinism** (ADR-4): all ordering — carving order, candidate
/// tie-breaks, member order — derives from [`NodeIdent`] and
/// `f64::total_cmp`, never from `HashSet` iteration. Components
/// larger than `component_cap` fall back to pairwise passthrough.
fn emit_pass2_clusters(
    forms: &[NormalizedForm],
    resolver: &dyn PathResolver,
    threshold: f64,
    edges: &[PairwiseEdge],
    component_cap: usize,
    matches: &mut Vec<Match>,
) {
    if edges.is_empty() {
        return;
    }

    let ident = build_node_idents(forms, resolver, edges);
    let adj = build_adjacency(edges);
    let component_size = component_size_by_node(edges, &ident);
    let order = carving_order(edges, &ident);

    let (cliques, clique_of) =
        carve_cliques(edges, &order, &adj, &ident, &component_size, component_cap);

    emit_clique_matches(forms, resolver, threshold, &cliques, &adj, matches);
    emit_residual_matches(
        forms, resolver, threshold, edges, &order, &ident, &clique_of, matches,
    );
}

/// Node identities for deterministic, permutation-stable ordering —
/// one [`NodeIdent`] per endpoint touched by an edge.
fn build_node_idents(
    forms: &[NormalizedForm],
    resolver: &dyn PathResolver,
    edges: &[PairwiseEdge],
) -> BTreeMap<usize, NodeIdent> {
    let mut ident: BTreeMap<usize, NodeIdent> = BTreeMap::new();
    for e in edges {
        for n in [e.i, e.j] {
            ident
                .entry(n)
                .or_insert_with(|| node_ident(forms, n, resolver));
        }
    }
    ident
}

/// Adjacency map: node -> (neighbor -> edge score). Drives clique
/// growth, score lookups, and the absorbed/residual split.
fn build_adjacency(edges: &[PairwiseEdge]) -> BTreeMap<usize, BTreeMap<usize, f64>> {
    let mut adj: BTreeMap<usize, BTreeMap<usize, f64>> = BTreeMap::new();
    for e in edges {
        adj.entry(e.i).or_default().insert(e.j, e.score);
        adj.entry(e.j).or_default().insert(e.i, e.score);
    }
    adj
}

/// Component size keyed by node (not root) so the carving loop can
/// apply the oversize cap with a plain map lookup — no `&mut parent`
/// borrow inside the hot loop. Union-find is consulted only here.
fn component_size_by_node(
    edges: &[PairwiseEdge],
    ident: &BTreeMap<usize, NodeIdent>,
) -> BTreeMap<usize, usize> {
    let mut parent: BTreeMap<usize, usize> = BTreeMap::new();
    for e in edges {
        uf_union(&mut parent, e.i, e.j);
    }
    let mut size_by_root: BTreeMap<usize, usize> = BTreeMap::new();
    for &n in ident.keys() {
        let root = uf_find(&mut parent, n);
        *size_by_root.entry(root).or_insert(0) += 1;
    }
    let mut by_node: BTreeMap<usize, usize> = BTreeMap::new();
    for &n in ident.keys() {
        let root = uf_find(&mut parent, n);
        by_node.insert(n, size_by_root[&root]);
    }
    by_node
}

/// Carving order: score descending (total order via `total_cmp`),
/// then the ident-ordered endpoint pair. Quantized Jaccard makes
/// exact score ties the common case, so the identity tie-break is
/// load-bearing for byte-stable output.
fn carving_order(edges: &[PairwiseEdge], ident: &BTreeMap<usize, NodeIdent>) -> Vec<usize> {
    // Hold references into `ident` rather than cloning each
    // `NodeIdent` (its `FilePath` wraps a `PathBuf` — a clone per edge
    // is a heap allocation). `&NodeIdent` orders by referent, so the
    // tie-break comparison below is unchanged. `edge_keys` is local —
    // the borrows never escape.
    let edge_keys: Vec<(&NodeIdent, &NodeIdent)> = edges
        .iter()
        .map(|e| {
            let (a, b) = (&ident[&e.i], &ident[&e.j]);
            if a <= b { (a, b) } else { (b, a) }
        })
        .collect();
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by(|&x, &y| {
        edges[y]
            .score
            .total_cmp(&edges[x].score)
            .then_with(|| edge_keys[x].cmp(&edge_keys[y]))
    });
    order
}

/// Carve maximal cliques in carving order. A node belongs to at most
/// one clique; each carved clique's members are sorted by identity so
/// the wire-visible `forms[0]` is stable. Edges whose endpoints land
/// in different cliques (or none) become residuals. Returns the
/// cliques plus the node -> clique-id map the residual split needs.
fn carve_cliques(
    edges: &[PairwiseEdge],
    order: &[usize],
    adj: &BTreeMap<usize, BTreeMap<usize, f64>>,
    ident: &BTreeMap<usize, NodeIdent>,
    component_size: &BTreeMap<usize, usize>,
    component_cap: usize,
) -> (Vec<Vec<usize>>, BTreeMap<usize, usize>) {
    let mut clique_of: BTreeMap<usize, usize> = BTreeMap::new();
    let mut cliques: Vec<Vec<usize>> = Vec::new();
    for &ei in order {
        let e = &edges[ei];
        if component_size[&e.i] > component_cap {
            continue;
        }
        if clique_of.contains_key(&e.i) || clique_of.contains_key(&e.j) {
            continue;
        }
        let mut clique = vec![e.i, e.j];
        grow_clique(&mut clique, adj, ident, &clique_of);
        clique.sort_by(|a, b| ident[a].cmp(&ident[b]));
        let id = cliques.len();
        for &m in &clique {
            clique_of.insert(m, id);
        }
        cliques.push(clique);
    }
    (cliques, clique_of)
}

/// Emit carved cliques as n-ary matches: members already identity-
/// ordered, score = the minimum intra-clique pair score (generalizing
/// Pass 1's score-1.0-as-group-min precedent), tier routed by that
/// weakest pair.
fn emit_clique_matches(
    forms: &[NormalizedForm],
    resolver: &dyn PathResolver,
    threshold: f64,
    cliques: &[Vec<usize>],
    adj: &BTreeMap<usize, BTreeMap<usize, f64>>,
    matches: &mut Vec<Match>,
) {
    for clique in cliques {
        let min_score = min_intra_clique_score(clique, adj);
        let tier = tier_for(min_score, threshold);
        let forms_refs: Vec<FormRef> = clique
            .iter()
            .map(|&m| form_ref_for(&forms[m], m, resolver))
            .collect();
        matches.push(Match::new(forms_refs, min_score, tier));
    }
}

/// The weakest (minimum) pairwise score over every pair in a clique.
fn min_intra_clique_score(clique: &[usize], adj: &BTreeMap<usize, BTreeMap<usize, f64>>) -> f64 {
    let mut min_score = f64::INFINITY;
    for (pos, &a) in clique.iter().enumerate() {
        for &b in &clique[pos + 1..] {
            min_score = min_score.min(adj[&a][&b]);
        }
    }
    min_score
}

/// Residual edges — endpoints not co-members of one clique — emit as
/// binary matches (edge conservation). Covers both cross-clique
/// leftovers and every edge of an oversize (capped) component.
/// Members ordered by identity.
#[allow(clippy::too_many_arguments)]
fn emit_residual_matches(
    forms: &[NormalizedForm],
    resolver: &dyn PathResolver,
    threshold: f64,
    edges: &[PairwiseEdge],
    order: &[usize],
    ident: &BTreeMap<usize, NodeIdent>,
    clique_of: &BTreeMap<usize, usize>,
    matches: &mut Vec<Match>,
) {
    for &ei in order {
        let e = &edges[ei];
        if edge_absorbed(clique_of, e.i, e.j) {
            continue;
        }
        let (a, b) = if ident[&e.i] <= ident[&e.j] {
            (e.i, e.j)
        } else {
            (e.j, e.i)
        };
        let tier = tier_for(e.score, threshold);
        let forms_refs = vec![
            form_ref_for(&forms[a], a, resolver),
            form_ref_for(&forms[b], b, resolver),
        ];
        matches.push(Match::new(forms_refs, e.score, tier));
    }
}

/// True when both endpoints landed in the same carved clique (the edge
/// is already represented by that clique's n-ary match).
fn edge_absorbed(clique_of: &BTreeMap<usize, usize>, i: usize, j: usize) -> bool {
    match (clique_of.get(&i), clique_of.get(&j)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Grow a seeded clique to maximality: repeatedly admit the
/// unassigned candidate adjacent to ALL current members that
/// maximizes the minimum edge score into the clique; ties break on
/// the smaller node identity. The adjacent-to-all check is what
/// maintains the clique invariant — a candidate with any missing
/// edge (never computed or sub-threshold) is rejected.
fn grow_clique(
    clique: &mut Vec<usize>,
    adj: &BTreeMap<usize, BTreeMap<usize, f64>>,
    ident: &BTreeMap<usize, NodeIdent>,
    clique_of: &BTreeMap<usize, usize>,
) {
    while let Some(cand) = best_clique_candidate(clique, adj, ident, clique_of) {
        clique.push(cand);
    }
}

/// Pick the next node to admit into `clique`: among unassigned nodes
/// adjacent to every current member, the one maximizing the minimum
/// edge into the clique (ties → smaller identity). `None` when no
/// admissible candidate remains (the clique is maximal).
fn best_clique_candidate(
    clique: &[usize],
    adj: &BTreeMap<usize, BTreeMap<usize, f64>>,
    ident: &BTreeMap<usize, NodeIdent>,
    clique_of: &BTreeMap<usize, usize>,
) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for &cand in adj[&clique[0]].keys() {
        if clique_of.contains_key(&cand) || clique.contains(&cand) {
            continue;
        }
        let Some(worst) = min_edge_into_clique(clique, adj, cand) else {
            continue;
        };
        if candidate_beats_best(worst, cand, best, ident) {
            best = Some((worst, cand));
        }
    }
    best.map(|(_, cand)| cand)
}

/// Minimum edge score from `cand` into every current clique member,
/// or `None` if `cand` is not adjacent to all of them — a missing
/// edge disqualifies the candidate (it would break the clique
/// invariant).
fn min_edge_into_clique(
    clique: &[usize],
    adj: &BTreeMap<usize, BTreeMap<usize, f64>>,
    cand: usize,
) -> Option<f64> {
    let mut worst = f64::INFINITY;
    for &m in clique {
        let w = *adj[&m].get(&cand)?;
        worst = worst.min(w);
    }
    Some(worst)
}

/// Prefer-larger-clique candidate ranking: a higher minimum edge wins;
/// exact ties (the common case under quantized Jaccard) break on the
/// smaller node identity for deterministic output.
fn candidate_beats_best(
    worst: f64,
    cand: usize,
    best: Option<(f64, usize)>,
    ident: &BTreeMap<usize, NodeIdent>,
) -> bool {
    match best {
        None => true,
        Some((best_worst, best_cand)) => match worst.total_cmp(&best_worst) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => ident[&cand] < ident[&best_cand],
        },
    }
}

/// Union-find `find` with path compression over a sparse
/// `BTreeMap` parent table. A node absent from the table is its own
/// root.
fn uf_find(parent: &mut BTreeMap<usize, usize>, x: usize) -> usize {
    let mut root = x;
    while let Some(&p) = parent.get(&root) {
        if p == root {
            break;
        }
        root = p;
    }
    let mut cur = x;
    while let Some(&p) = parent.get(&cur) {
        if p == root {
            break;
        }
        parent.insert(cur, root);
        cur = p;
    }
    root
}

/// Union by min-root — the smaller index becomes the representative,
/// keeping the structure deterministic.
fn uf_union(parent: &mut BTreeMap<usize, usize>, a: usize, b: usize) {
    let ra = uf_find(parent, a);
    let rb = uf_find(parent, b);
    if ra != rb {
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        // `lo` stays root; `uf_find` reads an absent node as its own
        // root, so no `lo -> lo` self-entry is needed.
        parent.insert(hi, lo);
    }
}

/// Resolve the effective Pass 2 score, de-rating an unexpected
/// score-1.0 hit to [`AUTO_REFACTOR_FLOOR`].
///
/// Pass 1 emits ALL equal-set clusters within a bucket (including
/// XOR-colliding distinct clusters), so any form with an equal-set
/// partner has been claimed and Pass 2 should never see
/// `score == 1.0`. The de-rating is a defensive fallback if a future
/// refactor regresses Pass 1's exhaustive emit — a downgrade to
/// `AutoRefactor`'s floor is safer than a double-emit or a panic in
/// release.
fn resolve_pass2_score(score: f64, i: usize, j: usize) -> f64 {
    if (score - 1.0).abs() < f64::EPSILON {
        debug_assert!(
            false,
            "Pass 2 emitted a score-1.0 pair the bucket clustering missed: \
             forms[{i}] and forms[{j}]"
        );
        AUTO_REFACTOR_FLOOR // de-rate to the floor, but keep tier consistent
    } else {
        score
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

/// Project a [`NormalizedForm`] (at index `i` in the engine's input
/// slice) to the reporter-friendly [`FormRef`]. The file path comes
/// from `resolver.path_for(form, i)`.
///
/// At v0.1 two resolvers exist: [`SyntheticPathResolver`] (the
/// library-facing fallback that derives a placeholder from
/// `qualified_name`) and [`IndexedPathResolver`] (the CLI run loop's
/// strategy that maps `forms[i]` -> `paths[i]`). The trait object
/// keeps the inner-loop callsites identical; static-dispatch via
/// monomorphization would also work but the cost of a vtable call
/// per form is negligible at the call-frequency of two emit sites
/// per match.
fn form_ref_for(form: &NormalizedForm, i: usize, resolver: &dyn PathResolver) -> FormRef {
    FormRef::new(resolver.path_for(form, i), form.span, form.kind)
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
    fn pass2_break_math_does_not_break_when_node_count_equals_bound_exactly() {
        // Pins the STRICT `>` in `pass2_sliding_window`'s break gate
        // (`forms[j].node_count > forms[i].node_count / threshold`).
        // Equality must NOT break — the j-form is exactly at the
        // Jaccard upper bound, so it is still a legal candidate.
        //
        // Constructed so `node_count_j == bound` is EXACT in f64:
        // threshold = 0.5 (exactly representable), forms[i].node_count
        // = 4, so bound = 4.0 / 0.5 = 8.0 (exact); forms[j].node_count
        // = 8.0 (exact). With the correct strict `>`, `8.0 > 8.0` is
        // false -> the loop does NOT break, the pair is evaluated, and
        // its Jaccard (4/5 = 0.8) clears the 0.5 threshold -> 1 match.
        //
        // The `> -> ==` mutant: `8.0 == 8.0` is true -> break -> 0
        // matches (test fails -> mutant killed).
        // The `> -> >=` mutant: `8.0 >= 8.0` is true -> break -> 0
        // matches (test fails -> mutant killed).
        //
        // The fingerprint sets are distinct (XOR bucket keys differ:
        // 1^2^3^4 = 4 vs 4^5 = 1) AND node_count is decoupled from set
        // size, so Pass 1 never claims them and the score sits well
        // above the gate boundary — isolating the break-math `>`.
        let forms = vec![make_form(&[1, 2, 3, 4], 4), make_form(&[1, 2, 3, 4, 5], 8)];
        let matches = compare(&forms, 0.5);
        assert_eq!(
            matches.len(),
            1,
            "node_count == bound (8.0 == 8.0) must NOT break the loop; \
             the pair must be evaluated and emitted, got {matches:?}"
        );
        assert!(
            (matches[0].score - 0.8).abs() < 1e-9,
            "expected ~0.8, got {}",
            matches[0].score
        );
    }

    #[test]
    fn pass2_threshold_gate_emits_when_score_equals_threshold_exactly() {
        // Pins the STRICT `<` in `try_emit_pass2_match`'s threshold
        // gate (`if score < threshold { return; }`). A score that
        // equals the threshold EXACTLY must NOT be filtered out —
        // the gate is "meets or exceeds" (`>= threshold`), so `score
        // == threshold` emits.
        //
        // Constructed so `score == threshold` is EXACT in f64:
        // {1,2,3} vs {2,3,4} share {2,3}; Jaccard = 2/4 = 0.5
        // (exact), and threshold = 0.5 (exact). Node counts are equal
        // (3 == 3) so the break math (bound = 3/0.5 = 6.0, nj = 3.0,
        // 3.0 > 6.0 is false) never prunes the pair — isolating the
        // threshold gate.
        //
        // The `< -> <=` mutant: `0.5 <= 0.5` is true -> early return
        // -> 0 matches (test fails -> mutant killed).
        //
        // Sets are distinct (XOR keys 0 vs 5) so Pass 1 leaves them
        // for Pass 2.
        let forms = vec![make_form(&[1, 2, 3], 3), make_form(&[2, 3, 4], 3)];
        let matches = compare(&forms, 0.5);
        assert_eq!(
            matches.len(),
            1,
            "score == threshold (0.5 == 0.5) must NOT be filtered; \
             the `< threshold` gate is strict, got {matches:?}"
        );
        assert!(
            (matches[0].score - 0.5).abs() < f64::EPSILON,
            "expected exactly 0.5, got {}",
            matches[0].score
        );
        // 0.5 < 0.85 review_first floor -> Advisory.
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

    #[test]
    fn compare_with_paths_uses_caller_supplied_paths_on_form_refs() {
        // Two identical forms in two different files. With
        // `compare_with_paths` each emitted FormRef carries the
        // caller's path; with `compare()` they'd carry the
        // qualified-name fallback ("unknown" since the helpers below
        // use empty qualified names).
        let forms = vec![make_form(&[1, 2, 3], 3), make_form(&[1, 2, 3], 3)];
        let paths = vec![
            FilePath::from(std::path::PathBuf::from("src/alpha.rs")),
            FilePath::from(std::path::PathBuf::from("src/beta.rs")),
        ];
        let matches = compare_with_paths(&forms, &paths, 0.85);
        assert_eq!(matches.len(), 1, "exact-match cluster expected");
        let m = &matches[0];
        assert!((m.score - 1.0).abs() < f64::EPSILON);
        let files: Vec<String> = m.forms.iter().map(|f| f.file.to_string()).collect();
        assert!(
            files.contains(&"src/alpha.rs".to_string()),
            "expected src/alpha.rs in match.forms, got: {files:?}"
        );
        assert!(
            files.contains(&"src/beta.rs".to_string()),
            "expected src/beta.rs in match.forms, got: {files:?}"
        );
    }

    #[test]
    fn compare_with_paths_pass2_emits_correct_paths() {
        // Pass 2 (sliding-window Jaccard) emits matches with FormRef
        // paths that map back to the caller-supplied paths array.
        let forms = vec![make_form(&[1, 2, 3, 4], 4), make_form(&[1, 2, 3, 4, 5], 5)];
        let paths = vec![
            FilePath::from(std::path::PathBuf::from("src/x.rs")),
            FilePath::from(std::path::PathBuf::from("src/y.rs")),
        ];
        let matches = compare_with_paths(&forms, &paths, 0.7);
        assert_eq!(matches.len(), 1);
        let files: Vec<String> = matches[0]
            .forms
            .iter()
            .map(|f| f.file.to_string())
            .collect();
        assert!(files.contains(&"src/x.rs".to_string()));
        assert!(files.contains(&"src/y.rs".to_string()));
    }

    #[test]
    fn compare_with_paths_handles_pass1_xor_collision_with_correct_paths() {
        // Pass 1 emits one cluster per equal-set; XOR-colliding
        // distinct clusters each get their own match. Paths must
        // propagate per index.
        let forms = vec![
            make_form(&[1, 2], 2),
            make_form(&[1, 2], 2),
            make_form(&[4, 7], 2),
            make_form(&[4, 7], 2),
        ];
        let paths = vec![
            FilePath::from(std::path::PathBuf::from("a.rs")),
            FilePath::from(std::path::PathBuf::from("b.rs")),
            FilePath::from(std::path::PathBuf::from("c.rs")),
            FilePath::from(std::path::PathBuf::from("d.rs")),
        ];
        let matches = compare_with_paths(&forms, &paths, 0.85);
        assert_eq!(matches.len(), 2, "two distinct equal-set clusters expected");
        let mut all_files: Vec<String> = matches
            .iter()
            .flat_map(|m| m.forms.iter().map(|f| f.file.to_string()))
            .collect();
        all_files.sort();
        assert_eq!(
            all_files,
            vec!["a.rs", "b.rs", "c.rs", "d.rs"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
            "all four paths should surface across the two clusters"
        );
    }

    #[test]
    #[should_panic(expected = "forms and paths must be the same length")]
    fn compare_with_paths_panics_on_length_mismatch() {
        let forms = vec![make_form(&[1, 2], 2)];
        // Empty paths — the unconditional `assert_eq!` catches the
        // length mismatch in both debug AND release builds. The prior
        // `debug_assert_eq!` left release builds to panic with a
        // cryptic `index out of bounds` from the resolver's
        // `paths[i]`; the explicit `assert_eq!` surfaces the lengths.
        let paths: Vec<FilePath> = Vec::new();
        let _ = compare_with_paths(&forms, &paths, 0.85);
    }

    // --- Pass 2 clique clustering (dry-rs#97, adr-cluster-output) ---

    /// `{1..=10}`, `{1..=9, 11}`, `{1..=9, 12}` — every pair shares 9
    /// of 11 union elements: Jaccard 9/11 ≈ 0.818 for all three pairs.
    fn triangle_forms() -> Vec<NormalizedForm> {
        let a: Vec<u64> = (1..=10).collect();
        let b: Vec<u64> = (1..=9).chain([11]).collect();
        let c: Vec<u64> = (1..=9).chain([12]).collect();
        vec![make_form(&a, 10), make_form(&b, 10), make_form(&c, 10)]
    }

    #[test]
    fn pass2_clusters_triangle_into_single_n_ary_match() {
        // A triangle (all three pairs >= threshold) is a 3-clique and
        // must emit ONE 3-form Match, not three binary matches.
        let matches = compare(&triangle_forms(), 0.8);
        assert_eq!(
            matches.len(),
            1,
            "triangle must collapse into one cluster, got {matches:?}"
        );
        assert_eq!(matches[0].forms.len(), 3);
        // Cluster score is the MINIMUM intra-clique pairwise Jaccard
        // (all pairs are 9/11 here).
        assert!(
            (matches[0].score - 9.0 / 11.0).abs() < 1e-9,
            "expected 9/11, got {}",
            matches[0].score
        );
    }

    #[test]
    fn pass2_chain_emits_clique_plus_residual_pair() {
        // A–B and B–C clear the threshold; A–C does not. Clique
        // semantics must NOT merge all three (the A–C pair would be
        // below threshold inside the cluster). Edge conservation must
        // NOT drop the real B–C relationship either: the output is
        // the carved 2-clique plus the leftover edge as a residual
        // binary match.
        //
        // A = {1..=9, 20}, B = {1..=10}, C = {2..=10, 21}:
        //   A∩B = 9, A∪B = 11 → 0.818 >= 0.8
        //   B∩C = 9, B∪C = 11 → 0.818 >= 0.8
        //   A∩C = 8, A∪C = 12 → 0.667 <  0.8
        let a: Vec<u64> = (1..=9).chain([20]).collect();
        let b: Vec<u64> = (1..=10).collect();
        let c: Vec<u64> = (2..=10).chain([21]).collect();
        let forms = vec![make_form(&a, 10), make_form(&b, 10), make_form(&c, 10)];
        let matches = compare(&forms, 0.8);
        assert_eq!(
            matches.len(),
            2,
            "chain must emit clique + residual pair, got {matches:?}"
        );
        assert!(
            matches.iter().all(|m| m.forms.len() == 2),
            "no 3-form cluster may form across a below-threshold pair: {matches:?}"
        );
    }

    #[test]
    fn pass2_cluster_tier_follows_min_pairwise_score() {
        // 3-clique with heterogeneous pair scores: A–C and B–C are
        // 19/20 = 0.95 (AutoRefactor range) but A–B is 19/21 ≈ 0.905.
        // The cluster routes by its WEAKEST pair: ReviewFirst.
        let a: Vec<u64> = (1..=19).chain([20]).collect();
        let b: Vec<u64> = (1..=19).chain([21]).collect();
        let c: Vec<u64> = (1..=19).collect();
        let forms = vec![make_form(&a, 20), make_form(&b, 20), make_form(&c, 19)];
        let matches = compare(&forms, 0.85);
        assert_eq!(matches.len(), 1, "expected one 3-clique, got {matches:?}");
        assert_eq!(matches[0].forms.len(), 3);
        assert!(
            (matches[0].score - 19.0 / 21.0).abs() < 1e-9,
            "cluster score must be the min pair (19/21), got {}",
            matches[0].score
        );
        assert_eq!(matches[0].tier, Tier::ReviewFirst);
    }

    #[test]
    fn pass2_cluster_members_ordered_by_file_identity() {
        // Cluster members are ordered by (file, span.start), not by
        // input index — the ordering is wire-visible (forms[0] feeds
        // the canonical output sort) and must be stable across walker
        // orderings.
        let paths = vec![
            FilePath::from(std::path::PathBuf::from("z.rs")),
            FilePath::from(std::path::PathBuf::from("a.rs")),
            FilePath::from(std::path::PathBuf::from("m.rs")),
        ];
        let matches = compare_with_paths(&triangle_forms(), &paths, 0.8);
        assert_eq!(matches.len(), 1);
        let files: Vec<String> = matches[0]
            .forms
            .iter()
            .map(|f| f.file.to_string())
            .collect();
        assert_eq!(files, vec!["a.rs", "m.rs", "z.rs"]);
    }

    #[test]
    fn pass2_cluster_membership_is_stable_across_input_permutation() {
        // Same forms, permuted input order (paths permuted alongside):
        // the emitted match set must be identical up to ordering —
        // membership and scores may not depend on input indices.
        let forms = triangle_forms();
        let paths: Vec<FilePath> = ["x.rs", "y.rs", "z.rs"]
            .iter()
            .map(|p| FilePath::from(std::path::PathBuf::from(p)))
            .collect();

        let canonical = |ms: &[Match]| -> Vec<(Vec<String>, u64)> {
            let mut v: Vec<(Vec<String>, u64)> = ms
                .iter()
                .map(|m| {
                    let mut files: Vec<String> =
                        m.forms.iter().map(|f| f.file.to_string()).collect();
                    files.sort();
                    (files, m.score.to_bits())
                })
                .collect();
            v.sort();
            v
        };

        let forward = compare_with_paths(&forms, &paths, 0.8);
        let permuted_forms: Vec<NormalizedForm> =
            vec![forms[2].clone(), forms[0].clone(), forms[1].clone()];
        let permuted_paths = vec![paths[2].clone(), paths[0].clone(), paths[1].clone()];
        let backward = compare_with_paths(&permuted_forms, &permuted_paths, 0.8);

        assert_eq!(canonical(&forward), canonical(&backward));
    }

    #[test]
    fn pass2_path_of_four_conserves_every_edge() {
        // Path A–B–C–D where consecutive pairs clear the threshold
        // and all other pairs are below. Carving yields two 2-cliques
        // and one residual pair — three binary matches that together
        // cover exactly the three collected edges (edge conservation).
        //
        // A = {1..=9, 20}, B = {1..=10}, C = {2..=10, 21},
        // D = {2..=9, 21, 30}:
        //   A–B 9/11, B–C 9/11, C–D 9/11 (all >= 0.8)
        //   A–C 8/12, A–D 8/12, B–D 8/12 (all < 0.8)
        let a: Vec<u64> = (1..=9).chain([20]).collect();
        let b: Vec<u64> = (1..=10).collect();
        let c: Vec<u64> = (2..=10).chain([21]).collect();
        let d: Vec<u64> = (2..=9).chain([21, 30]).collect();
        let forms = vec![
            make_form(&a, 10),
            make_form(&b, 10),
            make_form(&c, 10),
            make_form(&d, 10),
        ];
        let matches = compare(&forms, 0.8);
        assert_eq!(
            matches.len(),
            3,
            "path of four must emit exactly its three edges, got {matches:?}"
        );
        assert!(matches.iter().all(|m| m.forms.len() == 2));
    }

    #[test]
    fn oversize_component_falls_back_to_pairwise_passthrough() {
        // A 4-clique with a component cap of 3 skips carving and
        // emits all six edges pairwise — deterministic defense for
        // pathological generated-code families (adr-cluster-output
        // ADR-6). Exercises `emit_pass2_clusters` directly with a
        // small cap; the production cap is `CLUSTER_COMPONENT_CAP`.
        //
        // Shared core {1..=9} plus one unique element each: every
        // pair is 9/11 ≈ 0.818.
        let sets: Vec<Vec<u64>> = (0..4u64)
            .map(|u| (1..=9).chain([100 + u]).collect())
            .collect();
        let forms: Vec<NormalizedForm> = sets.iter().map(|s| make_form(s, 10)).collect();
        let claimed: HashSet<usize> = HashSet::new();
        let edges = pass2_sliding_window(&forms, 0.8, &claimed, ResolvedScope::default());
        assert_eq!(edges.len(), 6, "4-clique has six edges");

        let mut capped: Vec<Match> = Vec::new();
        emit_pass2_clusters(&forms, &SyntheticPathResolver, 0.8, &edges, 3, &mut capped);
        assert_eq!(
            capped.len(),
            6,
            "cap 3 must passthrough all six edges pairwise, got {capped:?}"
        );
        assert!(capped.iter().all(|m| m.forms.len() == 2));

        let mut uncapped: Vec<Match> = Vec::new();
        emit_pass2_clusters(
            &forms,
            &SyntheticPathResolver,
            0.8,
            &edges,
            CLUSTER_COMPONENT_CAP,
            &mut uncapped,
        );
        assert_eq!(uncapped.len(), 1, "uncapped carving emits one 4-clique");
        assert_eq!(uncapped[0].forms.len(), 4);
    }

    // --- Pass 3 clustering-helper mutation hardening (dry-rs#116) ---

    /// Build the four-form 4-clique fixture (component size 4, every
    /// pair 9/11 ≈ 0.818) used by the component-cap boundary tests.
    fn four_clique_forms() -> Vec<NormalizedForm> {
        (0..4u64)
            .map(|u| make_form(&(1..=9).chain([100 + u]).collect::<Vec<_>>(), 10))
            .collect()
    }

    #[test]
    fn carve_cliques_component_cap_is_a_strict_greater_than_boundary() {
        // Pins the EXACT `>` in `carve_cliques`' oversize guard
        // (`component_size[&e.i] > component_cap`). The component here
        // has exactly 4 nodes.
        //
        // - cap == size (4): correct `4 > 4` is false -> the component
        //   is carved into ONE 4-clique. The `> -> >=` mutant
        //   (`4 >= 4` true) and the `> -> ==` mutant (`4 == 4` true)
        //   both cap it and passthrough six binary edges instead.
        // - cap == size + 1 (5): correct `4 > 5` is false -> carved
        //   (one 4-clique). The `> -> <` mutant (`4 < 5` true) caps it
        //   and passes through six binary edges.
        //
        // Together the two caps kill all three `carve_cliques`
        // relational mutants without ever excluding the boundary.
        let forms = four_clique_forms();
        let claimed: HashSet<usize> = HashSet::new();
        let edges = pass2_sliding_window(&forms, 0.8, &claimed, ResolvedScope::default());
        assert_eq!(edges.len(), 6, "4-clique has six edges");

        // cap == component size -> NOT capped -> one 4-clique.
        let mut at_boundary: Vec<Match> = Vec::new();
        emit_pass2_clusters(
            &forms,
            &SyntheticPathResolver,
            0.8,
            &edges,
            4,
            &mut at_boundary,
        );
        assert_eq!(
            at_boundary.len(),
            1,
            "cap == size (4) must NOT cap; `>` is strict, got {at_boundary:?}"
        );
        assert_eq!(at_boundary[0].forms.len(), 4);

        // cap == component size + 1 -> NOT capped -> one 4-clique.
        let mut above_boundary: Vec<Match> = Vec::new();
        emit_pass2_clusters(
            &forms,
            &SyntheticPathResolver,
            0.8,
            &edges,
            5,
            &mut above_boundary,
        );
        assert_eq!(
            above_boundary.len(),
            1,
            "cap > size must NOT cap; the `< -> cap` mutant would passthrough, \
             got {above_boundary:?}"
        );
        assert_eq!(above_boundary[0].forms.len(), 4);
    }

    #[test]
    fn carve_cliques_skips_nodes_already_assigned_to_a_clique() {
        // Pins the `||` in `carve_cliques`' membership guard
        // (`clique_of.contains_key(&e.i) || clique_of.contains_key(&e.j)`)
        // and, jointly, `best_clique_candidate`'s skip guard.
        //
        // Two disjoint triangles share NO elements, so they form two
        // separate 3-cliques. After the first triangle is carved, its
        // three nodes are in `clique_of`; the second triangle carves
        // independently. The `|| -> &&` mutant only skips an edge when
        // BOTH endpoints are already assigned, which lets an already-
        // carved node re-seed a spurious overlapping clique — changing
        // the emitted match count.
        //
        // Triangle 1: {1..=10}, {1..=9,11}, {1..=9,12} (all 9/11).
        // Triangle 2: {21..=30}, {21..=29,31}, {21..=29,32} (all 9/11).
        let forms = two_disjoint_triangles();
        let matches = compare(&forms, 0.8);
        assert_eq!(
            matches.len(),
            2,
            "two disjoint triangles must carve into exactly two 3-cliques, got {matches:?}"
        );
        assert!(
            matches.iter().all(|m| m.forms.len() == 3),
            "each clique must hold all three triangle members: {matches:?}"
        );
    }

    #[test]
    fn min_intra_clique_score_returns_the_weakest_pair_not_a_constant() {
        // Pins `min_intra_clique_score` against the const-return
        // mutants (`-> 0.0`, `-> 1.0`, `-> -1.0`) and the
        // `clique[pos + 1..]` range arithmetic.
        //
        // A 3-clique with heterogeneous pair scores: the cluster score
        // MUST equal the minimum pair (not 0.0/1.0/-1.0, and not a
        // value poisoned by a bogus self-pair from a `+ -> *`/`+ -> -`
        // range mutation, which would panic on the missing diagonal
        // `adj[&a][&a]`).
        //
        // A = {1..=19, 20}, B = {1..=19, 21}, C = {1..=19}:
        //   A-C = B-C = 19/20 = 0.95; A-B = 19/21 ≈ 0.905 (the min).
        let a: Vec<u64> = (1..=19).chain([20]).collect();
        let b: Vec<u64> = (1..=19).chain([21]).collect();
        let c: Vec<u64> = (1..=19).collect();
        let forms = vec![make_form(&a, 20), make_form(&b, 20), make_form(&c, 19)];
        let matches = compare(&forms, 0.85);
        assert_eq!(matches.len(), 1, "expected one 3-clique, got {matches:?}");
        assert_eq!(matches[0].forms.len(), 3);
        // Exactly the weakest pair (19/21) — rules out every const
        // return AND a self-pair-poisoned min.
        assert!(
            (matches[0].score - 19.0 / 21.0).abs() < 1e-12,
            "cluster score must be the min pair 19/21 ≈ 0.9048, got {}",
            matches[0].score
        );
        assert_eq!(matches[0].tier, Tier::ReviewFirst);
    }

    #[test]
    fn residual_binary_match_members_ordered_by_identity() {
        // Pins the `<=` in `emit_residual_matches`' member-ordering
        // (`ident[&e.i] <= ident[&e.j]`). A single below-clique edge
        // emits as ONE residual binary match whose `forms[0]` is the
        // smaller identity. Distinct file identities (a.rs < b.rs) make
        // the order observable; the `<= -> >` mutant flips forms[0]
        // and forms[1].
        //
        // Two near-duplicate forms (Jaccard 4/5 = 0.8) — one residual
        // edge, no clique. The input order is REVERSED relative to the
        // file identities (index 0 -> b.rs, index 1 -> a.rs) so the
        // helper must reorder by identity, not pass input order through.
        let forms = vec![make_form(&[1, 2, 3, 4], 4), make_form(&[1, 2, 3, 4, 5], 5)];
        let paths = vec![
            FilePath::from(std::path::PathBuf::from("b.rs")),
            FilePath::from(std::path::PathBuf::from("a.rs")),
        ];
        let matches = compare_with_paths(&forms, &paths, 0.7);
        assert_eq!(matches.len(), 1, "one residual binary match expected");
        let files: Vec<String> = matches[0]
            .forms
            .iter()
            .map(|f| f.file.to_string())
            .collect();
        assert_eq!(
            files,
            vec!["a.rs".to_string(), "b.rs".to_string()],
            "residual members must be identity-ordered (a.rs before b.rs)"
        );
    }

    #[test]
    fn min_edge_into_clique_drives_member_selection_not_a_constant() {
        // Pins `min_edge_into_clique` against `-> None` and the
        // const-`Some(_)` mutants. The greedy growth admits the
        // candidate maximizing the MINIMUM edge into the clique; if the
        // function returned a constant or `None`, growth would either
        // reject every candidate (`None` -> no 3-clique forms) or admit
        // on a bogus uniform score (admitting a non-adjacent node and
        // inflating the clique).
        //
        // Seed/grow a 3-clique {A,B,C} (all pairs 19/21 ≈ 0.905). A
        // fourth form D shares only {1..=18} with each, so every D-edge
        // is below threshold and D has NO adjacency. A `-> None` mutant
        // blocks A and C from joining the seed (only a residual pair
        // survives). A const `Some(1.0)` mutant ignores real adjacency
        // and would admit D too (a 4-form clique).
        //
        // A = {1..=18, 19, 20}, B = {1..=18, 19, 21},
        // C = {1..=18, 19, 22}, D = {1..=18, 40, 41}:
        //   A-B, A-C, B-C = 19/21 ≈ 0.905 (>= 0.85);
        //   D shares only {1..=18} (18 elts) with each of A/B/C:
        //   D-A = 18/(20+20-18) = 18/22 ≈ 0.818 < 0.85 -> no edge.
        let a: Vec<u64> = (1..=18).chain([19, 20]).collect();
        let b: Vec<u64> = (1..=18).chain([19, 21]).collect();
        let c: Vec<u64> = (1..=18).chain([19, 22]).collect();
        let d: Vec<u64> = (1..=18).chain([40, 41]).collect();
        let forms = vec![
            make_form(&a, 20),
            make_form(&b, 20),
            make_form(&c, 20),
            make_form(&d, 20),
        ];
        let matches = compare(&forms, 0.85);
        // Exactly one 3-clique {A,B,C}; D shares no >= threshold edge.
        assert_eq!(
            matches.len(),
            1,
            "expected one 3-clique with D excluded, got {matches:?}"
        );
        assert_eq!(
            matches[0].forms.len(),
            3,
            "min_edge_into_clique must admit A and C onto the seed, but not D: {matches:?}"
        );
        assert!(
            (matches[0].score - 19.0 / 21.0).abs() < 1e-12,
            "clique min score must be 19/21, got {}",
            matches[0].score
        );
    }

    #[test]
    fn candidate_beats_best_is_deterministic_on_ties() {
        // Pins `candidate_beats_best` against `-> true`, `-> false`,
        // and the `<` identity tie-break
        // (`ident[&cand] < ident[&best_cand]`). In a triangle the seed
        // edge's two extension candidates tie on minimum edge (all
        // 9/11), so the tie-break MUST pick the smaller identity
        // deterministically AND identically across input permutations.
        //
        // Triangle (all pairs 9/11) with distinct file identities. The
        // emitted members are identity-sorted; the permutation-stability
        // check is what pins the tie-break: a `-> true` mutant admits
        // whichever candidate the `BTreeMap` scan reaches first (still
        // identity-stable here since adj keys are ordered), but a
        // `-> false` mutant never updates `best` and yields `None` from
        // `best_clique_candidate` (the clique never grows past the seed
        // pair) -> two matches instead of one. The `< -> ==/<=/>` tie-
        // break mutants flip member order under permutation.
        let paths = vec![
            FilePath::from(std::path::PathBuf::from("a.rs")),
            FilePath::from(std::path::PathBuf::from("b.rs")),
            FilePath::from(std::path::PathBuf::from("c.rs")),
        ];
        let forward = compare_with_paths(&triangle_forms(), &paths, 0.8);
        assert_eq!(
            forward.len(),
            1,
            "triangle must be one 3-clique (a `candidate_beats_best -> false` \
             mutant would never grow the seed), got {forward:?}"
        );
        let files: Vec<String> = forward[0]
            .forms
            .iter()
            .map(|f| f.file.to_string())
            .collect();
        assert_eq!(
            files,
            vec!["a.rs", "b.rs", "c.rs"],
            "clique members must be identity-ordered deterministically"
        );

        // Permuting the input must not change the emitted member set —
        // the tie-break derives from identity, not input index.
        let permuted_paths = vec![paths[2].clone(), paths[0].clone(), paths[1].clone()];
        let pf = triangle_forms();
        let permuted_forms = vec![pf[2].clone(), pf[0].clone(), pf[1].clone()];
        let backward = compare_with_paths(&permuted_forms, &permuted_paths, 0.8);
        assert_eq!(backward.len(), 1);
        let back_files: Vec<String> = backward[0]
            .forms
            .iter()
            .map(|f| f.file.to_string())
            .collect();
        assert_eq!(files, back_files, "tie-break must be permutation-stable");
    }

    #[test]
    fn edge_absorbed_distinguishes_same_clique_from_cross_clique() {
        // Pins `edge_absorbed`'s match (`(Some(a), Some(b)) => a == b`):
        // the `delete match arm` mutant and the `== -> !=` mutant.
        //
        // Two disjoint triangles -> two cliques (ids 0 and 1). Within a
        // triangle every edge is absorbed (a == b, same clique) and
        // must NOT re-emit as a residual. The `== -> !=` mutant treats
        // same-clique edges as residuals (emitting six spurious binary
        // matches); the deleted arm collapses to the wildcard `false`,
        // also re-emitting every intra-clique edge as a residual.
        // Correct output: exactly two 3-form matches, zero binary
        // residuals.
        let forms = two_disjoint_triangles();
        let matches = compare(&forms, 0.8);
        assert_eq!(
            matches.len(),
            2,
            "absorbed intra-clique edges must NOT re-emit as residuals, got {matches:?}"
        );
        assert!(
            matches.iter().all(|m| m.forms.len() == 3),
            "every emitted match must be a 3-clique, no binary residuals: {matches:?}"
        );
    }

    #[test]
    fn union_find_partitions_two_small_components_independently() {
        // Pins `uf_find` (`-> 0`, `-> 1`, the two `== -> !=`) and
        // `uf_union`'s `!= -> ==` against a TWO-component input.
        //
        // `component_size_by_node` is the only consumer of union-find,
        // and the size feeds the cap check. Two disjoint triangles form
        // two separate 3-node components. With a component cap of 3:
        //   - Correct union-find: each component has size 3, `3 > 3` is
        //     false -> BOTH carve into 3-cliques (two matches).
        //   - A broken `uf_find`/`uf_union` that COLLAPSES the two
        //     components into one (`-> 0`/`-> 1` makes every node share
        //     a root; a corrupted root walk or skipped merge miscounts)
        //     yields a phantom 6-node "component" -> `6 > 3` true ->
        //     capped -> passthrough of all six intra-triangle edges as
        //     binary matches.
        //
        // Six binary residuals vs two 3-cliques is the observable
        // difference. Exercises `emit_pass2_clusters` directly so the
        // cap is controllable (the production cap is 512).
        let forms = two_disjoint_triangles();
        let claimed: HashSet<usize> = HashSet::new();
        let edges = pass2_sliding_window(&forms, 0.8, &claimed, ResolvedScope::default());
        // Each triangle contributes three edges; the two are disjoint.
        assert_eq!(edges.len(), 6, "two triangles contribute six edges total");

        // Cap exactly at the per-component size (3). Correct union-find
        // keeps the components separate, so neither is capped.
        let mut out: Vec<Match> = Vec::new();
        emit_pass2_clusters(&forms, &SyntheticPathResolver, 0.8, &edges, 3, &mut out);
        assert_eq!(
            out.len(),
            2,
            "correct union-find keeps two size-3 components uncapped -> two 3-cliques; \
             a collapsed/miscounted partition would cap a phantom size-6 component into \
             six binary residuals, got {out:?}"
        );
        assert!(
            out.iter().all(|m| m.forms.len() == 3),
            "each component must carve into a 3-clique: {out:?}"
        );
    }

    #[test]
    fn union_find_must_merge_a_triangle_into_one_oversize_component() {
        // Pins `uf_union`'s `!= -> ==` merge guard (and reinforces
        // `uf_find`) from the UNDER-count direction. The two-component
        // test above catches collapse (over-count -> spurious cap);
        // this one catches a union-find that fails to MERGE.
        //
        // A single triangle is ONE component of size 3. With a cap of
        // 2:
        //   - Correct union-find merges all three nodes -> size 3,
        //     `3 > 2` true -> capped -> three binary residuals.
        //   - The `!= -> ==` mutant skips every real merge, so each
        //     node stays its own size-1 component -> `1 > 2` false ->
        //     NOT capped -> one 3-clique. (Likewise a `uf_find`
        //     corruption that mis-roots nodes undercounts here.)
        //
        // One 3-clique vs three binary residuals is the observable
        // difference. Exercises `emit_pass2_clusters` directly to drive
        // the cap below the real component size.
        let forms = triangle_forms();
        let claimed: HashSet<usize> = HashSet::new();
        let edges = pass2_sliding_window(&forms, 0.8, &claimed, ResolvedScope::default());
        assert_eq!(edges.len(), 3, "triangle has three edges");

        let mut out: Vec<Match> = Vec::new();
        emit_pass2_clusters(&forms, &SyntheticPathResolver, 0.8, &edges, 2, &mut out);
        assert_eq!(
            out.len(),
            3,
            "correct union-find merges the triangle into one size-3 component, which \
             exceeds cap 2 -> three binary residuals; a non-merging union-find would \
             leave size-1 components uncapped -> one 3-clique, got {out:?}"
        );
        assert!(
            out.iter().all(|m| m.forms.len() == 2),
            "an over-cap component passes through as binary residuals: {out:?}"
        );
    }

    /// Two element-disjoint triangles (six forms): forms 0–2 share
    /// `{1..=9}` (+ one unique element each) and forms 3–5 share
    /// `{21..=29}` (+ one unique element each). Every intra-triangle
    /// pair is 9/11 ≈ 0.818; every cross-triangle pair is 0.0. Used by
    /// the union-find / edge-absorbed / membership-guard mutation tests
    /// where TWO independent components are the discriminating input.
    fn two_disjoint_triangles() -> Vec<NormalizedForm> {
        let sets: Vec<Vec<u64>> = vec![
            (1..=10).collect(),
            (1..=9).chain([11]).collect(),
            (1..=9).chain([12]).collect(),
            (21..=30).collect(),
            (21..=29).chain([31]).collect(),
            (21..=29).chain([32]).collect(),
        ];
        sets.iter().map(|s| make_form(s, 10)).collect()
    }

    // --- Candidate-selection coverage requiring TWO simultaneous growth
    //     candidates (dry-rs#116, second hardening pass) ---
    //
    // The triangle/two-triangle fixtures above only ever present ONE
    // growth candidate after the seed edge, so `candidate_beats_best`'s
    // comparison arms and `min_edge_into_clique`'s real return value are
    // never exercised. These fixtures are "K4 minus one edge": a seed
    // pair plus two competing candidates NOT adjacent to each other,
    // forcing the greedy to choose between them.

    /// K4-minus-edge with UNEQUAL candidate worsts. Seed P-Q is the
    /// strongest edge; H fits the seed at 0.925, L at 0.857; H-L is
    /// below threshold (0.837) so only one of them can join. The greedy
    /// must admit H (higher minimum edge) and reject L.
    ///
    /// P = {1..=36, 901, 902}, Q = {1..=36, 901, 903},
    /// H = {1..=35, 901, 902, 903, 910}, L = {3..=36, 901, 902, 903,
    /// 920, 921, 922}:
    ///   P-Q 0.949 (seed), P-H = Q-H 0.925, P-L = Q-L 0.857,
    ///   H-L 0.837 (< 0.85, no edge).
    fn k4_minus_edge_unequal() -> Vec<NormalizedForm> {
        let p: Vec<u64> = (1..=36).chain([901, 902]).collect();
        let q: Vec<u64> = (1..=36).chain([901, 903]).collect();
        let h: Vec<u64> = (1..=35).chain([901, 902, 903, 910]).collect();
        let l: Vec<u64> = (3..=36).chain([901, 902, 903, 920, 921, 922]).collect();
        vec![
            make_form(&p, 20),
            make_form(&q, 20),
            make_form(&h, 20),
            make_form(&l, 20),
        ]
    }

    #[test]
    fn greedy_admits_higher_worst_candidate_and_rejects_the_disconnected_one() {
        // Kills `min_edge_into_clique -> Some(0.0/1.0/-1.0)` (a constant
        // return would admit L despite the missing H-L edge -> a 4-form
        // cluster), `candidate_beats_best -> true` and the `< -> ==/>/<=`
        // ranking mutants (which would admit L over H -> wrong member
        // set), and `best_clique_candidate -> None`.
        //
        // Correct carving: seed {P,Q}; admit H (worst 0.925 > L's 0.857);
        // L is not adjacent to H so it cannot join -> clique {P,Q,H}
        // scored by its weakest pair (0.925). L emits as residual pairs.
        let paths = vec![
            FilePath::from(std::path::PathBuf::from("p.rs")),
            FilePath::from(std::path::PathBuf::from("q.rs")),
            FilePath::from(std::path::PathBuf::from("h.rs")),
            FilePath::from(std::path::PathBuf::from("l.rs")),
        ];
        let matches = compare_with_paths(&k4_minus_edge_unequal(), &paths, 0.85);

        // Exactly one 3-clique {P,Q,H}; L is not a clique member.
        let cliques: Vec<&Match> = matches.iter().filter(|m| m.forms.len() == 3).collect();
        assert_eq!(
            cliques.len(),
            1,
            "expected exactly one 3-clique, got {matches:?}"
        );
        let members: Vec<String> = cliques[0]
            .forms
            .iter()
            .map(|f| f.file.to_string())
            .collect();
        assert_eq!(
            members,
            vec!["h.rs", "p.rs", "q.rs"],
            "clique must be {{P,Q,H}} (H, the higher-worst candidate), not L: {matches:?}"
        );
        assert!(
            (cliques[0].score - 0.925).abs() < 1e-9,
            "clique min score must be 0.925 (P-H/Q-H), got {}",
            cliques[0].score
        );
        // L surfaces only as residual binary matches (with P and Q).
        assert!(
            matches.iter().any(|m| {
                let f: Vec<String> = m.forms.iter().map(|x| x.file.to_string()).collect();
                m.forms.len() == 2 && f.contains(&"l.rs".to_string())
            }),
            "L must emit as a residual pair, not be dropped or absorbed: {matches:?}"
        );
    }

    /// K4-minus-edge with EQUAL candidate worsts (the tie case). Seed
    /// P-Q strongest; the two candidates H and L each fit the seed at
    /// the SAME minimum edge (0.895); H-L is below threshold (0.813) so
    /// only one can join. The greedy ties on the minimum edge, so the
    /// identity tie-break (`ident[&cand] < ident[&best_cand]`) decides
    /// which one is admitted.
    ///
    /// A large shared core (`1..=100`) gives the headroom to keep H and
    /// L equally close to the seed while still pulling them apart from
    /// each other: H drops the core's tail (`93..=100`), L drops its head
    /// (`1..=8`), and each adds a private 2-element tail.
    ///
    /// P = {1..=100, 901, 902}, Q = {1..=100, 901, 903},
    /// H = {1..=92, 901, 902, 903, 940, 941},
    /// L = {9..=100, 901, 902, 903, 950, 951}:
    ///   P-Q 0.981 (seed), P-H = Q-H = P-L = Q-L 0.895 (tie),
    ///   H-L 0.813 (< 0.85, no edge).
    fn k4_minus_edge_tie() -> (Vec<NormalizedForm>, Vec<FilePath>) {
        let p: Vec<u64> = (1..=100).chain([901, 902]).collect();
        let q: Vec<u64> = (1..=100).chain([901, 903]).collect();
        let h: Vec<u64> = (1..=92).chain([901, 902, 903, 940, 941]).collect();
        let l: Vec<u64> = (9..=100).chain([901, 902, 903, 950, 951]).collect();
        let forms = vec![
            make_form(&p, 20),
            make_form(&q, 20),
            make_form(&h, 20),
            make_form(&l, 20),
        ];
        let paths = vec![
            FilePath::from(std::path::PathBuf::from("p.rs")),
            FilePath::from(std::path::PathBuf::from("q.rs")),
            FilePath::from(std::path::PathBuf::from("h_candidate.rs")),
            FilePath::from(std::path::PathBuf::from("l_candidate.rs")),
        ];
        (forms, paths)
    }

    #[test]
    fn candidate_tie_break_picks_smaller_identity_deterministically() {
        // Kills the `818 < -> ==/>/<=` identity tie-break mutants AND
        // `candidate_beats_best -> true`. With H and L tied on minimum
        // edge into the seed {P,Q}, the tie-break admits the smaller
        // identity. `h_candidate.rs` < `l_candidate.rs`, so H joins and
        // the clique is {P,Q,H}. A flipped tie-break (`>`/`<=`/`==`)
        // would admit L instead; `-> true` admits whichever the BTreeMap
        // scan reaches last.
        let (forms, paths) = k4_minus_edge_tie();
        let matches = compare_with_paths(&forms, &paths, 0.85);
        let cliques: Vec<&Match> = matches.iter().filter(|m| m.forms.len() == 3).collect();
        assert_eq!(cliques.len(), 1, "expected one 3-clique, got {matches:?}");
        let members: Vec<String> = cliques[0]
            .forms
            .iter()
            .map(|f| f.file.to_string())
            .collect();
        assert_eq!(
            members,
            vec!["h_candidate.rs", "p.rs", "q.rs"],
            "tie-break must admit the smaller identity (h_candidate < l_candidate): {matches:?}"
        );

        // Permuting input order must not change the admitted member —
        // the tie-break is identity-driven, never input-index-driven.
        let permuted_forms = vec![
            forms[3].clone(),
            forms[2].clone(),
            forms[1].clone(),
            forms[0].clone(),
        ];
        let permuted_paths = vec![
            paths[3].clone(),
            paths[2].clone(),
            paths[1].clone(),
            paths[0].clone(),
        ];
        let permuted = compare_with_paths(&permuted_forms, &permuted_paths, 0.85);
        let pcliques: Vec<&Match> = permuted.iter().filter(|m| m.forms.len() == 3).collect();
        assert_eq!(pcliques.len(), 1);
        let pmembers: Vec<String> = pcliques[0]
            .forms
            .iter()
            .map(|f| f.file.to_string())
            .collect();
        assert_eq!(
            members, pmembers,
            "tie-break must be permutation-stable, not input-order-dependent"
        );
    }

    #[test]
    fn best_clique_candidate_skips_a_node_already_in_the_growing_clique() {
        // Kills `774 || -> &&` (the `clique_of.contains_key(&cand) ||
        // clique.contains(&cand)` skip guard). During growth a node
        // already in the CURRENT clique (`clique.contains`, but not yet
        // in `clique_of`) must be skipped. With `&&`, a current member
        // would only be skipped if ALSO in `clique_of` — letting the
        // grower re-examine an already-admitted node, changing the
        // emitted clique or duplicating a member.
        //
        // Use the unequal K4-minus-edge: after admitting H onto {P,Q},
        // the next scan of `adj[&P].keys()` re-encounters Q and H (both
        // current members), which MUST be skipped via `clique.contains`.
        let paths = vec![
            FilePath::from(std::path::PathBuf::from("p.rs")),
            FilePath::from(std::path::PathBuf::from("q.rs")),
            FilePath::from(std::path::PathBuf::from("h.rs")),
            FilePath::from(std::path::PathBuf::from("l.rs")),
        ];
        let matches = compare_with_paths(&k4_minus_edge_unequal(), &paths, 0.85);
        let cliques: Vec<&Match> = matches.iter().filter(|m| m.forms.len() >= 3).collect();
        assert_eq!(
            cliques.len(),
            1,
            "exactly one clique, with each member present once, got {matches:?}"
        );
        let mut members: Vec<String> = cliques[0]
            .forms
            .iter()
            .map(|f| f.file.to_string())
            .collect();
        let unique: std::collections::HashSet<&String> = members.iter().collect();
        assert_eq!(
            unique.len(),
            members.len(),
            "no clique member may appear twice (the skip guard must reject \
             current members): {members:?}"
        );
        members.sort();
        assert_eq!(members, vec!["h.rs", "p.rs", "q.rs"]);
    }

    #[test]
    fn residual_pair_members_ordered_by_identity_in_a_chain() {
        // Kills `722 <= -> >` (the residual member-ordering in
        // `emit_residual_matches`). A chain A-B-C where A-B is the
        // strongest edge seeds the 2-clique {A,B}; B-C is then a
        // cross-clique RESIDUAL (B is in clique 0, C is unassigned) and
        // flows through `emit_residual_matches`'s `ident[&e.i] <=
        // ident[&e.j]` ordering. With distinct identities the residual's
        // forms[0] is the smaller identity; the `<= -> >` mutant flips
        // forms[0] and forms[1].
        //
        // A = {1..=19, 100}, B = {1..=19, 101}, C = {3..=19, 101, 300,
        // 301}:
        //   A-B 0.905 (strongest -> clique {A,B}),
        //   B-C 0.818 (residual), A-C 0.739 (< 0.8, no edge).
        // Identities: B = "b_mid.rs", C = "c_tail.rs" -> residual must
        // be [b_mid.rs, c_tail.rs].
        let a: Vec<u64> = (1..=19).chain([100]).collect();
        let b: Vec<u64> = (1..=19).chain([101]).collect();
        let c: Vec<u64> = (3..=19).chain([101, 300, 301]).collect();
        let forms = vec![make_form(&a, 20), make_form(&b, 20), make_form(&c, 20)];
        let paths = vec![
            FilePath::from(std::path::PathBuf::from("a_head.rs")),
            FilePath::from(std::path::PathBuf::from("b_mid.rs")),
            FilePath::from(std::path::PathBuf::from("c_tail.rs")),
        ];
        let matches = compare_with_paths(&forms, &paths, 0.8);

        // One 2-clique {A,B} + one residual pair {B,C}.
        let residual = matches
            .iter()
            .find(|m| {
                let f: Vec<String> = m.forms.iter().map(|x| x.file.to_string()).collect();
                f.contains(&"c_tail.rs".to_string())
            })
            .expect("B-C residual pair must be emitted");
        assert_eq!(residual.forms.len(), 2, "B-C is a binary residual");
        let files: Vec<String> = residual.forms.iter().map(|f| f.file.to_string()).collect();
        assert_eq!(
            files,
            vec!["b_mid.rs", "c_tail.rs"],
            "residual members must be identity-ordered (b_mid before c_tail): {residual:?}"
        );
    }

    /// Overlapping-adjacency fixture for the `best_clique_candidate`
    /// membership guard (dry-rs#116). A node X is carved into an EARLIER
    /// clique A, then is adjacent (at a lower score) to every member of
    /// a LATER-seeded clique B. The `clique_of.contains_key(&cand)` arm
    /// of the skip guard is the only thing keeping X out of B.
    ///
    /// Layout (threshold 0.3): P,Q,X share `1..=20`; X additionally
    /// carries the bridge `100..=119` that R,S share (`100..=115`). P,Q
    /// have NO bridge element, so they are NOT adjacent to R,S — X is the
    /// UNIQUE bridge between the two cliques.
    ///
    /// Edge scores (descending):
    ///   P-Q 0.913 (A seed) > R-S 0.895 (B seed)
    ///     > P-X = Q-X 0.465 (X joins A) > R-X = S-X 0.372 (X bridges B).
    ///
    /// Carving: A={P,Q,X} seeds at P-Q, admits X. R-S then seeds B; X is
    /// adjacent to both R and S, so it is a growth candidate — but X is
    /// already in `clique_of` (it was carved into A), so the OR-guard
    /// skips it and B stays {R,S}. R-X / S-X emit as cross-clique
    /// residual pairs.
    fn overlapping_bridge_forms() -> (Vec<NormalizedForm>, Vec<FilePath>) {
        // pp,qq carry only the A-core; xx (the bridge) carries A-core +
        // the B-bridge; rr,ss carry only the B-bridge.
        let pp: Vec<u64> = (1..=20).chain([901, 902]).collect();
        let qq: Vec<u64> = (1..=20).chain([901, 903]).collect();
        let xx: Vec<u64> = (1..=20).chain(100..=119).chain([904]).collect();
        let rr: Vec<u64> = (100..=115).chain([2001, 2002]).collect();
        let ss: Vec<u64> = (100..=115).chain([2001, 2003]).collect();
        let forms = vec![
            make_form(&pp, 22),
            make_form(&qq, 22),
            make_form(&xx, 41),
            make_form(&rr, 18),
            make_form(&ss, 18),
        ];
        let paths = ["p.rs", "q.rs", "x.rs", "r.rs", "s.rs"]
            .iter()
            .map(|n| FilePath::from(std::path::PathBuf::from(n)))
            .collect();
        (forms, paths)
    }

    #[test]
    fn carved_node_is_not_re_admitted_to_a_later_clique() {
        // Kills `774 || -> &&` (the `clique_of.contains_key(&cand) ||
        // clique.contains(&cand)` membership skip guard in
        // `best_clique_candidate`). When B={R,S} grows, X is adjacent to
        // both members and would join — but X is already carved into A,
        // so it is in `clique_of` (NOT in the growing clique B). The
        // correct OR-guard skips X via `clique_of.contains_key`. The
        // `&&` mutant requires BOTH conditions, so X (in clique_of but
        // not in B) is NOT skipped -> X is wrongly admitted to B,
        // appearing in TWO n-ary cliques and stealing the R-X / S-X
        // residual edges.
        //
        // The existing disjoint-triangle / K4-minus-edge fixtures cannot
        // catch this: they never present a candidate that is in
        // `clique_of` but absent from the current clique (a candidate
        // already in the current clique is independently rejected by
        // `min_edge_into_clique` returning None for the self-edge, which
        // masks the `&&` flip). Overlapping adjacency is required.
        let (forms, paths) = overlapping_bridge_forms();
        let matches = compare_with_paths(&forms, &paths, 0.3);

        let members_of = |m: &Match| -> Vec<String> {
            let mut v: Vec<String> = m.forms.iter().map(|f| f.file.to_string()).collect();
            v.sort();
            v
        };

        // Exactly one >=3-member clique: {P,Q,X}. The `&&` mutant emits a
        // SECOND ({R,S,X}).
        let nary: Vec<Vec<String>> = matches
            .iter()
            .filter(|m| m.forms.len() >= 3)
            .map(members_of)
            .collect();
        assert_eq!(
            nary.len(),
            1,
            "exactly one n-ary clique expected ({{P,Q,X}}); the && mutant \
             also emits {{R,S,X}}: {matches:?}"
        );
        assert_eq!(nary[0], vec!["p.rs", "q.rs", "x.rs"]);

        // X (x.rs) must appear in exactly ONE n-ary clique — never two.
        let xt = "x.rs".to_string();
        let x_in_nary = matches
            .iter()
            .filter(|m| m.forms.len() >= 3 && members_of(m).contains(&xt))
            .count();
        assert_eq!(
            x_in_nary, 1,
            "a carved node must not be re-admitted to a second clique: {matches:?}"
        );

        // {R,S} stays a binary clique (not absorbed into {R,S,X}); R-X
        // and S-X survive as cross-clique residual pairs.
        let has_pair = |a: &str, b: &str| {
            matches
                .iter()
                .any(|m| m.forms.len() == 2 && members_of(m) == vec![a.to_string(), b.to_string()])
        };
        assert!(
            has_pair("r.rs", "s.rs"),
            "R-S must remain a binary clique, not grow to {{R,S,X}}: {matches:?}"
        );
        assert!(
            has_pair("r.rs", "x.rs") && has_pair("s.rs", "x.rs"),
            "R-X and S-X must emit as residual pairs (X absorbed into B \
             under the mutant would consume them): {matches:?}"
        );
    }

    /// Equal-score carving-order fixture (dry-rs#116) for the canonical
    /// endpoint-pair tie-break in `carving_order`.
    ///
    /// Two TOP edges A-D and B-C carry the SAME score (0.9375) — quantized
    /// Jaccard makes exact ties the common case, and `carving_order`
    /// breaks the tie on the canonical `(smaller_ident, larger_ident)`
    /// endpoint pair. The five identities sort `a < b < c < d < m`, so:
    ///   - correct min-first canonicalization: key(A-D)=(a,d) <
    ///     key(B-C)=(b,c) because `a < b` -> A-D seeds FIRST.
    ///   - the `<= -> >` mutant canonicalizes to (larger, smaller):
    ///     key(A-D)=(d,a), key(B-C)=(c,b); now `c < d` -> B-C seeds first.
    ///
    /// Swing node M is adjacent (at 0.3191, below both top edges) to ALL
    /// of A, B, C, D, and is the ONLY admissible growth candidate for
    /// whichever top pair seeds first (A,B,C,D are pairwise non-adjacent
    /// across the two cores). So M joins the FIRST-seeded clique and is
    /// then locked out of the second:
    ///   - correct: {A,D,M} 3-clique + {B,C} 2-clique (+ B-M, C-M
    ///     residuals).
    ///   - mutant:  {B,C,M} 3-clique + {A,D} 2-clique (+ A-M, D-M
    ///     residuals).
    ///
    /// The 3-clique's membership is the observable difference: M clusters
    /// with {A,D} iff the canonical pair ordering is correct.
    fn equal_score_swing_forms() -> (Vec<NormalizedForm>, Vec<FilePath>) {
        // aa,dd share one core (edge A-D); bb,cc share another (edge
        // B-C); mm is the swing node bridging both.
        let aa: Vec<u64> = (1..=30).chain([900]).collect();
        let dd: Vec<u64> = (1..=30).chain([901]).collect();
        let bb: Vec<u64> = (50..=79).chain([950]).collect();
        let cc: Vec<u64> = (50..=79).chain([951]).collect();
        let mm: Vec<u64> = (1..=15).chain(50..=64).chain([999]).collect();
        // Input order is aa,bb,cc,dd,mm; filenames sort a<b<c<d<m so the
        // identity tie-break is filename-driven, independent of index.
        let forms = vec![
            make_form(&aa, 31),
            make_form(&bb, 31),
            make_form(&cc, 31),
            make_form(&dd, 31),
            make_form(&mm, 31),
        ];
        let paths = ["a.rs", "b.rs", "c.rs", "d.rs", "m.rs"]
            .iter()
            .map(|n| FilePath::from(std::path::PathBuf::from(n)))
            .collect();
        (forms, paths)
    }

    #[test]
    fn equal_score_carving_seeds_in_canonical_pair_order() {
        // Kills `620 <= -> >` in `carving_order` (the canonical
        // endpoint-pair `if a <= b { (a, b) } else { (b, a) }`). Two
        // equal-score top edges A-D and B-C tie on score; the tie-break
        // is the canonical (min-ident, max-ident) pair. The `<= -> >`
        // flip reverses every pair to (max, min), reordering the two
        // equal-score edges so B-C seeds before A-D. The swing node M
        // then joins B-C instead of A-D -> a DIFFERENT clustering.
        //
        // Verified: passes on HEAD, FAILS under a manual `<= -> >` flip
        // at line 620 (the 3-clique becomes {B,C,M} instead of {A,D,M}).
        let (forms, paths) = equal_score_swing_forms();
        let matches = compare_with_paths(&forms, &paths, 0.3);

        let members_of = |mm: &Match| -> Vec<String> {
            let mut v: Vec<String> = mm.forms.iter().map(|f| f.file.to_string()).collect();
            v.sort();
            v
        };

        // Exactly one 3-clique, and it MUST be {A,D,M} — the mutant emits
        // {B,C,M} instead (same shape, different members).
        let three: Vec<Vec<String>> = matches
            .iter()
            .filter(|mm| mm.forms.len() == 3)
            .map(members_of)
            .collect();
        assert_eq!(
            three.len(),
            1,
            "expected exactly one 3-clique, got {matches:?}"
        );
        assert_eq!(
            three[0],
            vec!["a.rs", "d.rs", "m.rs"],
            "swing node M must cluster with the canonical-first pair {{A,D}}; \
             the <=->> mutant flips seeding so M clusters with {{B,C}}: {matches:?}"
        );

        // The losing pair {B,C} stays a binary clique (M did not absorb
        // into it).
        let has_pair = |x: &str, y: &str| {
            matches.iter().any(|mm| {
                mm.forms.len() == 2 && members_of(mm) == vec![x.to_string(), y.to_string()]
            })
        };
        assert!(
            has_pair("b.rs", "c.rs"),
            "B-C must remain a binary clique (M joined A-D, not B-C): {matches:?}"
        );
    }

    // --- Relatedness scoping via CompareCtx (dry-rs#124) ---

    use crate::domain::{Span, StructuralLocation};

    /// Build a form with a structural location (crate + single-segment
    /// module) so the scope predicate has something to gate on. The
    /// `(file, span)` identity stays distinct via the qualified name so
    /// permutation-stability tests can address members by name.
    fn make_located_form(
        fps: &[u64],
        crate_id: Option<&str>,
        module: &str,
        qname: &str,
        node_count: u32,
    ) -> NormalizedForm {
        make_form_with_qualified_name(fps, &[qname], node_count).with_location(StructuralLocation {
            crate_id: crate_id.map(str::to_string),
            module_path: vec![module.to_string()],
        })
    }

    /// The all-true default context — the no-op identity the equivalence
    /// oracle pins against `compare()`.
    fn default_ctx() -> CompareCtx<'static> {
        CompareCtx::new(&SyntheticPathResolver, ResolvedScope::default())
    }

    /// A representative spread of the engine's fixture inputs — exact
    /// (Pass 1), near (Pass 2), cliques + residuals (Pass 3), XOR
    /// collisions, empty, and singletons — each paired with the threshold
    /// the corresponding fixture test uses. The equivalence oracle runs
    /// the no-op-scope engine over every one and asserts byte-identical
    /// output against `compare()`.
    fn equivalence_fixtures() -> Vec<(Vec<NormalizedForm>, f64)> {
        vec![
            (Vec::new(), 0.85),
            (vec![make_form(&[1, 2, 3], 3)], 0.85),
            (
                vec![make_form(&[1, 2, 3], 3), make_form(&[3, 2, 1], 3)],
                0.85,
            ),
            (
                vec![
                    make_form(&[1, 2, 3], 3),
                    make_form(&[1, 2, 3], 3),
                    make_form(&[1, 2, 3], 3),
                ],
                0.85,
            ),
            (vec![make_form(&[], 0), make_form(&[], 0)], 0.85),
            (
                vec![
                    make_form(&[1, 2], 2),
                    make_form(&[1, 2], 2),
                    make_form(&[4, 7], 2),
                    make_form(&[4, 7], 2),
                ],
                0.85,
            ),
            (
                vec![make_form(&[1, 2, 3, 4], 4), make_form(&[1, 2, 3, 4, 5], 5)],
                0.7,
            ),
            (triangle_forms(), 0.8),
            (four_clique_forms(), 0.8),
            (two_disjoint_triangles(), 0.8),
            (
                vec![
                    make_form(&[1, 2, 3], 3),
                    make_form(&[100, 200], 2),
                    make_form(&[1000], 1),
                ],
                0.5,
            ),
            (
                vec![
                    make_form_with_qualified_name(&[1, 2, 3], &["zeta"], 3),
                    make_form_with_qualified_name(&[1, 2, 3], &["zeta"], 3),
                    make_form_with_qualified_name(&[4, 5, 6], &["alpha"], 3),
                    make_form_with_qualified_name(&[4, 5, 6], &["alpha"], 3),
                    make_form_with_qualified_name(&[7, 8, 9], &["mid"], 3),
                    make_form_with_qualified_name(&[7, 8, 9], &["mid"], 3),
                ],
                0.85,
            ),
        ]
    }

    #[test]
    fn equivalence_oracle_default_scope_matches_compare_byte_for_byte() {
        // SAFETY NET: the no-op (all-true) scope must produce output
        // byte-identical to the legacy `compare()` over every engine
        // fixture. `compare_with(forms, t, default_ctx)` exercises the
        // SAME internal entry the public facade uses, only with the
        // explicit default context — so this also pins that the facade's
        // default-context construction is the no-op identity.
        for (forms, threshold) in equivalence_fixtures() {
            let oracle = compare_with(&forms, threshold, &default_ctx());
            let legacy = compare(&forms, threshold);
            assert_eq!(
                oracle, legacy,
                "default-scope compare_with must equal compare() for threshold {threshold}"
            );
        }
    }

    #[test]
    fn equivalence_oracle_holds_even_when_forms_carry_locations() {
        // Locations are present but the default scope allows every pair,
        // so attaching a `StructuralLocation` to each form changes
        // NOTHING — the engine still clusters identically. Guards against
        // the predicate accidentally reading location under the default.
        let located: Vec<NormalizedForm> = vec![
            make_located_form(&[1, 2, 3], Some("a"), "m1", "f0", 3),
            make_located_form(&[1, 2, 3], Some("b"), "m2", "f1", 3),
            make_located_form(&[1, 2, 3, 4], Some("a"), "m1", "f2", 4),
            make_located_form(&[1, 2, 3, 4, 5], Some("b"), "m2", "f3", 5),
        ];
        let unlocated: Vec<NormalizedForm> = vec![
            make_form_with_qualified_name(&[1, 2, 3], &["f0"], 3),
            make_form_with_qualified_name(&[1, 2, 3], &["f1"], 3),
            make_form_with_qualified_name(&[1, 2, 3, 4], &["f2"], 4),
            make_form_with_qualified_name(&[1, 2, 3, 4, 5], &["f3"], 5),
        ];
        assert_eq!(
            compare_with(&located, 0.7, &default_ctx()),
            compare(&unlocated, 0.7),
            "default scope must ignore locations entirely"
        );
    }

    /// Build a `CompareCtx` over the synthetic resolver with the given
    /// scope — locations drive the predicate, so the synthetic resolver's
    /// qualified-name paths are fine for the `FormRef` identities.
    fn scoped_ctx(scope: ResolvedScope) -> CompareCtx<'static> {
        // The resolver is a unit struct; leak a 'static reference via a
        // const so the returned ctx can outlive the call (test-only).
        CompareCtx::new(&SyntheticPathResolver, scope)
    }

    /// Collect every emitted pair `(qname_a, qname_b)` from a match set,
    /// expanding each n-ary match into all its intra-match pairs. Used to
    /// assert "every emitted pair is scope-allowed".
    fn emitted_pairs(matches: &[Match]) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        for m in matches {
            for (pos, a) in m.forms.iter().enumerate() {
                for b in &m.forms[pos + 1..] {
                    let mut ab = [a.file.to_string(), b.file.to_string()];
                    ab.sort();
                    pairs.push((ab[0].clone(), ab[1].clone()));
                }
            }
        }
        pairs
    }

    #[test]
    fn complement_across_crate_false_drops_pass1_cross_crate_exact_pair() {
        // Two byte-identical forms in DIFFERENT crates. Under the default
        // scope they cluster (one Pass 1 score-1.0 match); under
        // across_crate=false the cross-crate pair is disallowed, so NO
        // match is emitted (the lone scope-allowed partner each form has
        // is the other — which is now disallowed).
        let forms = vec![
            make_located_form(&[1, 2, 3], Some("crate_a"), "m", "a", 3),
            make_located_form(&[1, 2, 3], Some("crate_b"), "m", "b", 3),
        ];
        // Default scope: they cluster.
        let baseline = compare_with(&forms, 0.85, &default_ctx());
        assert_eq!(
            baseline.len(),
            1,
            "default scope clusters the cross-crate exact pair"
        );
        assert_eq!(baseline[0].forms.len(), 2);

        // across_crate=false: the cross-crate pair is dropped.
        let scope = ResolvedScope {
            across_crate: false,
            ..ResolvedScope::default()
        };
        let scoped = compare_with(&forms, 0.85, &scoped_ctx(scope));
        assert!(
            scoped.is_empty(),
            "across_crate=false must drop the cross-crate exact pair, got {scoped:?}"
        );
    }

    #[test]
    fn complement_across_crate_false_drops_pass2_cross_crate_near_pair() {
        // Two NEAR-duplicate forms (Jaccard 4/5 = 0.8) in different
        // crates. Default scope -> one Pass 2 residual match; across_crate
        // =false -> the pair is pruned BEFORE Jaccard, so nothing emits.
        let forms = vec![
            make_located_form(&[1, 2, 3, 4], Some("crate_a"), "m", "a", 4),
            make_located_form(&[1, 2, 3, 4, 5], Some("crate_b"), "m", "b", 5),
        ];
        let baseline = compare_with(&forms, 0.7, &default_ctx());
        assert_eq!(
            baseline.len(),
            1,
            "default scope clusters the cross-crate near pair"
        );

        let scope = ResolvedScope {
            across_crate: false,
            ..ResolvedScope::default()
        };
        let scoped = compare_with(&forms, 0.7, &scoped_ctx(scope));
        assert!(
            scoped.is_empty(),
            "across_crate=false must prune the cross-crate near pair, got {scoped:?}"
        );
    }

    #[test]
    fn complement_every_emitted_pair_is_scope_allowed_not_vacuous() {
        // A mixed corpus: a same-crate exact pair (ALLOWED, must survive)
        // and a cross-crate exact pair (DISALLOWED under across_crate
        // =false, must be dropped). Asserts BOTH directions — the allowed
        // pair survives (non-vacuous) AND every emitted pair is allowed.
        //
        // f0,f1 share crate_a (same module) -> within_crate&within_module
        //   allowed -> they cluster.
        // f2,f3 are crate_a vs crate_b -> across_crate=false drops them.
        // f0/f2 share fps too? No — distinct fp sets per pair to isolate.
        let forms = vec![
            make_located_form(&[1, 2, 3], Some("crate_a"), "m", "f0", 3),
            make_located_form(&[1, 2, 3], Some("crate_a"), "m", "f1", 3),
            make_located_form(&[9, 8, 7], Some("crate_a"), "m", "f2", 3),
            make_located_form(&[9, 8, 7], Some("crate_b"), "m", "f3", 3),
        ];
        let scope = ResolvedScope {
            across_crate: false,
            ..ResolvedScope::default()
        };
        let matches = compare_with(&forms, 0.85, &scoped_ctx(scope));

        // Non-vacuous: the same-crate {f0,f1} pair MUST survive.
        let pairs = emitted_pairs(&matches);
        assert!(
            pairs.contains(&("f0".to_string(), "f1".to_string())),
            "the allowed same-crate exact pair must still cluster (non-vacuous), got {pairs:?}"
        );
        // The disallowed cross-crate {f2,f3} pair must be ABSENT.
        assert!(
            !pairs.contains(&("f2".to_string(), "f3".to_string())),
            "the cross-crate pair must be dropped under across_crate=false, got {pairs:?}"
        );

        // Soundness: EVERY emitted pair is scope-allowed. Map qname ->
        // location to re-check the predicate on each surviving pair.
        let loc_of = |name: &str| -> StructuralLocation {
            forms
                .iter()
                .find(|f| f.qualified_name == vec![name.to_string()])
                .map(|f| f.location.clone())
                .expect("emitted pair member must exist in the input")
        };
        for (a, b) in &pairs {
            assert!(
                scope.allows(&loc_of(a), &loc_of(b)),
                "emitted pair ({a},{b}) must be scope-allowed"
            );
        }
    }

    #[test]
    fn complement_within_module_false_drops_pass2_same_module_pair() {
        // Module axis is independent of crate_aware — a same-module near
        // pair is dropped under within_module=false even with crate-ids
        // present.
        let forms = vec![
            make_located_form(&[1, 2, 3, 4], Some("k"), "m", "a", 4),
            make_located_form(&[1, 2, 3, 4, 5], Some("k"), "m", "b", 5),
        ];
        let scope = ResolvedScope {
            within_module: false,
            ..ResolvedScope::default()
        };
        let scoped = compare_with(&forms, 0.7, &scoped_ctx(scope));
        assert!(
            scoped.is_empty(),
            "within_module=false must drop the same-module near pair, got {scoped:?}"
        );
    }

    #[test]
    fn pass1_scope_subpartition_membership_stable_across_permutation() {
        // Three byte-identical forms across two crates:
        //   a,b in crate_a (same module); c in crate_b.
        // Under across_crate=false: a-b allowed, a-c & b-c disallowed.
        // So the ONLY emitted Pass 1 group is {a,b}; c stays unclaimed
        // (its equal partners are all cross-crate -> Pass 2 gate skips
        // them too). The sub-partition membership must be identical
        // regardless of input order.
        let scope = ResolvedScope {
            across_crate: false,
            ..ResolvedScope::default()
        };
        let canonical = |ms: &[Match]| -> Vec<Vec<String>> {
            let mut groups: Vec<Vec<String>> = ms
                .iter()
                .map(|m| {
                    let mut names: Vec<String> =
                        m.forms.iter().map(|f| f.file.to_string()).collect();
                    names.sort();
                    names
                })
                .collect();
            groups.sort();
            groups
        };

        let forward = vec![
            make_located_form(&[1, 2, 3], Some("crate_a"), "m", "a", 3),
            make_located_form(&[1, 2, 3], Some("crate_a"), "m", "b", 3),
            make_located_form(&[1, 2, 3], Some("crate_b"), "m", "c", 3),
        ];
        let permuted = vec![forward[2].clone(), forward[0].clone(), forward[1].clone()];

        let f = compare_with(&forward, 0.85, &scoped_ctx(scope));
        let p = compare_with(&permuted, 0.85, &scoped_ctx(scope));

        assert_eq!(
            canonical(&f),
            vec![vec!["a".to_string(), "b".to_string()]],
            "only the same-crate {{a,b}} group should emit under across_crate=false: {f:?}"
        );
        assert_eq!(
            canonical(&f),
            canonical(&p),
            "Pass 1 scope sub-partition membership must be permutation-stable"
        );
    }

    #[test]
    fn pass1_scope_subpartition_carves_two_same_crate_groups() {
        // Four byte-identical forms: {a,b} in crate_a, {c,d} in crate_b.
        // Under across_crate=false the two same-crate pairs are allowed
        // but cross-crate pairs are not, so the equal-set group carves
        // into TWO score-1.0 cliques ({a,b} and {c,d}) — edge
        // conservation across the scope sub-partition.
        let scope = ResolvedScope {
            across_crate: false,
            ..ResolvedScope::default()
        };
        let forms = vec![
            make_located_form(&[5, 6, 7], Some("crate_a"), "m", "a", 3),
            make_located_form(&[5, 6, 7], Some("crate_a"), "m", "b", 3),
            make_located_form(&[5, 6, 7], Some("crate_b"), "m", "c", 3),
            make_located_form(&[5, 6, 7], Some("crate_b"), "m", "d", 3),
        ];
        let matches = compare_with(&forms, 0.85, &scoped_ctx(scope));
        assert_eq!(
            matches.len(),
            2,
            "two same-crate exact groups must carve out independently, got {matches:?}"
        );
        for m in &matches {
            assert_eq!(m.forms.len(), 2, "each scope-allowed group is a 2-clique");
            assert!(
                (m.score - 1.0).abs() < f64::EPSILON,
                "exact group keeps score 1.0"
            );
            assert_eq!(m.tier, Tier::AutoRefactor);
        }
        // Every emitted pair is same-crate (scope-allowed).
        for (a, b) in emitted_pairs(&matches) {
            let same = (a == "a" && b == "b") || (a == "c" && b == "d");
            assert!(same, "emitted pair ({a},{b}) must be a same-crate pair");
        }
    }

    #[test]
    fn crate_aware_false_noops_crate_axes_in_engine() {
        // End-to-end through the engine: two byte-identical forms with NO
        // crate-id (single-dir run) and across_crate=false BUT
        // crate_aware=false. The crate axis no-ops, so the pair (same
        // module) survives via within_crate/within_module — it is NOT
        // dropped. This is the single-dir guard exercised at the engine
        // boundary, not just on the predicate.
        let forms = vec![
            make_located_form(&[1, 2, 3], None, "m", "a", 3),
            make_located_form(&[1, 2, 3], None, "m", "b", 3),
        ];
        let scope = ResolvedScope {
            within_crate: false,
            across_crate: false,
            crate_aware: false,
            ..ResolvedScope::default()
        };
        let matches = compare_with(&forms, 0.85, &scoped_ctx(scope));
        assert_eq!(
            matches.len(),
            1,
            "crate_aware=false must no-op the crate axes so the None-id pair survives, got {matches:?}"
        );
    }

    // --- Scope sub-partition helper mutation hardening (dry-rs#124) ---

    #[test]
    fn cluster_is_one_scope_allowed_clique_skips_self_pairs_and_reads_real_pairs() {
        // Pins `cluster_is_one_scope_allowed_clique` against BOTH the
        // `-> false` whole-fn mutant and the `cluster[pos + 1..]` range
        // `+ -> *` mutant.
        //
        // Scope: within_crate=false (same-crate pairs disallowed) but
        // across_crate=true (cross-crate allowed). Cluster: two equal-set
        // forms in DIFFERENT crates.
        //   - The only REAL pair (a,b) is cross-crate -> ALLOWED, so the
        //     correct fn returns `true` (kills `-> false`, which returns
        //     `false`).
        //   - The `+ -> *` mutant turns `cluster[0 + 1..]` into
        //     `cluster[0 * 1..]` = `cluster[0..]`, which re-checks the
        //     SELF pair (a,a): same crate -> within_crate=false ->
        //     DISALLOWED -> the mutant returns `false`. The strict `+ 1`
        //     skips self-pairs, so only the mutant trips.
        let scope = ResolvedScope {
            within_crate: false,
            across_crate: true,
            within_module: true,
            across_module: true,
            crate_aware: true,
        };
        let forms = vec![
            make_located_form(&[1, 2, 3], Some("crate_a"), "m", "a", 3),
            make_located_form(&[1, 2, 3], Some("crate_b"), "m", "b", 3),
        ];
        assert!(
            cluster_is_one_scope_allowed_clique(&forms, scope, &[0, 1]),
            "the lone cross-crate pair is allowed (within_crate=false only \
             gates SAME-crate pairs); self-pairs must be skipped by `pos + 1`"
        );

        // Contrast: a SAME-crate cluster under within_crate=false is NOT a
        // scope-allowed clique (the real pair is disallowed) -> false.
        let same_crate = vec![
            make_located_form(&[4, 5, 6], Some("crate_a"), "m", "c", 3),
            make_located_form(&[4, 5, 6], Some("crate_a"), "m", "d", 3),
        ];
        assert!(
            !cluster_is_one_scope_allowed_clique(&same_crate, scope, &[0, 1]),
            "a same-crate real pair is disallowed under within_crate=false"
        );
    }

    #[test]
    fn mark_emitted_members_claimed_requires_exact_file_and_span_identity() {
        // Pins `mark_emitted_members_claimed`'s identity predicate
        // (`f.file == want.file && f.span == want.span`) against the
        // `== -> !=` (file) and `&& -> ||` mutants.
        //
        // Two forms share the SAME synthetic file ("shared") but carry
        // DISTINCT spans. The emitted match contains ONLY form 0's
        // FormRef. The correct predicate claims EXACTLY form 0 (file AND
        // span match). The mutants diverge:
        //   - `&& -> ||`: form 1 matches on file alone (same "shared")
        //     -> wrongly claimed too -> claimed = {0, 1}.
        //   - `== -> !=` (file): form 0's `file != want.file` is false
        //     -> form 0 NOT claimed -> claimed = {} (or {1}).
        let span0 = Span::try_new(LineColumn::new(1, 0), LineColumn::new(2, 0)).unwrap();
        let span1 = Span::try_new(LineColumn::new(10, 0), LineColumn::new(11, 0)).unwrap();
        let forms = vec![
            located_form_with_span(&[1, 2, 3], "shared", span0, 3),
            located_form_with_span(&[1, 2, 3], "shared", span1, 3),
        ];
        let resolver = SyntheticPathResolver;
        // Emitted: one match carrying only form 0's projected FormRef.
        let emitted = vec![Match::new(
            vec![form_ref_for(&forms[0], 0, &resolver)],
            1.0,
            Tier::AutoRefactor,
        )];

        let mut claimed: HashSet<usize> = HashSet::new();
        mark_emitted_members_claimed(&emitted, &[0, 1], &forms, &resolver, &mut claimed);

        assert!(
            claimed.contains(&0),
            "form 0 (exact file+span identity) must be claimed (kills `== -> !=` on file)"
        );
        assert!(
            !claimed.contains(&1),
            "form 1 shares the file but NOT the span -> must NOT be claimed \
             (kills `&& -> ||`)"
        );
    }

    /// Build a located form with an explicit span (so two forms can share
    /// a synthetic file path via qualified name while carrying distinct
    /// `(file, span)` identities). Crate/module default to the
    /// single-location group used by the claim-identity test.
    fn located_form_with_span(
        fps: &[u64],
        qname: &str,
        span: Span,
        node_count: u32,
    ) -> NormalizedForm {
        use crate::domain::FormKind;
        NormalizedForm::with_context(
            FormKind::Production,
            fps.iter().copied().collect(),
            Vec::new(),
            vec![qname.to_string()],
            span,
            node_count,
            1,
        )
    }
}
