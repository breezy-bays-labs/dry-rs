//! Resolved relatedness-scoping predicate — [`ResolvedScope`].
//!
//! `ResolvedScope` is the concrete, fully-resolved form of the
//! `[scope]` config cascade ([`crate::domain::ScopeConfig`] +
//! [`crate::domain::LanguageConfig`]). Where the config tier carries
//! `Option<bool>` knobs (unset = inherit), `ResolvedScope` carries four
//! concrete `bool`s plus a `crate_aware` runtime fact. The cascade
//! resolver ([`crate::cli::EffectiveConfig::resolve`]) collapses the
//! `Option<bool>` knobs (per-language `Some` shadows shared `Some`; both
//! `None` resolves to the all-true default); the run loop supplies
//! `crate_aware` (derived from whether ANY form's crate-id was
//! resolvable this run).
//!
//! The predicate [`ResolvedScope::allows`] decides whether a pair of
//! forms is *allowed to cluster*, by the structural relationship between
//! their two [`crate::domain::StructuralLocation`]s. It is threaded into
//! the comparison engine at v0.x by dry-rs#124 (PR 11) — this PR
//! (dry-rs#123) defines the type, the config cascade, and the predicate;
//! it is NOT yet wired into `compare_with`.
//!
//! Per the relatedness-scoping ADR
//! (`ops/decisions/dry-rs/adr-relatedness-scoping-model.md`):
//!
//! - The four axes (crate × module, within × across) are **orthogonal**
//!   and map 1:1 to the user's mental model.
//! - The predicate is **symmetric**: `allows(a, b) == allows(b, a)`.
//! - `crate_aware == false` (no derivable crate-id this run, e.g. a
//!   single-dir run with no `Cargo.toml`) forces the two crate axes to
//!   **no-op** so a single-dir run never silently drops every pair
//!   (without crate-ids, both forms' `crate_id` is `None`, which would
//!   otherwise read as "same crate" and prune everything when
//!   `within_crate == false`).

use crate::domain::StructuralLocation;

/// Fully-resolved relatedness-scoping predicate.
///
/// Four concrete `bool` axes plus a `crate_aware` runtime flag. Built by
/// [`crate::cli::EffectiveConfig::resolved_scope`] from the resolved
/// [`crate::domain::ScopeConfig`] cascade; consumed by the comparison
/// engine (dry-rs#124).
///
/// `Default` is **all axes `true`** (and `crate_aware == true`), so a
/// `ResolvedScope::default()` clusters every pair exactly as the engine
/// did before scoping landed — the no-op identity.
///
/// Result-struct convention (AGENTS.md): no `#[non_exhaustive]`; it
/// evolves via constructors and `Default`.
///
/// `clippy::struct_excessive_bools` is allowed here deliberately: the
/// four scope axes are orthogonal toggles that map 1:1 to the user's
/// mental model (and to the locked `[scope]` config knobs + the
/// `Envelope.scope` wire shape per the relatedness-scoping ADR).
/// Collapsing them into a bitflag / enum would force the unnatural
/// product the ADR explicitly rejects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ResolvedScope {
    /// Allow clustering pairs whose two forms share a crate / package.
    pub within_crate: bool,
    /// Allow clustering pairs whose two forms live in different crates.
    pub across_crate: bool,
    /// Allow clustering pairs whose two forms share a module path.
    pub within_module: bool,
    /// Allow clustering pairs whose two forms live in different modules.
    pub across_module: bool,
    /// Whether ANY form's crate-id was resolvable this run. When
    /// `false`, [`ResolvedScope::allows`] treats the two crate axes as
    /// no-ops (always allowed) so a single-dir run — where every form's
    /// `crate_id` is `None` — never drops every pair.
    pub crate_aware: bool,
}

impl Default for ResolvedScope {
    /// All axes allowed, crate-aware — the no-op identity that clusters
    /// every pair (engine behaves exactly as it did before scoping).
    fn default() -> Self {
        Self {
            within_crate: true,
            across_crate: true,
            within_module: true,
            across_module: true,
            crate_aware: true,
        }
    }
}

impl ResolvedScope {
    /// True when ALL four axes are allowed — the scope cannot disallow
    /// ANY pair, so [`ResolvedScope::allows`] is unconditionally `true`
    /// regardless of `crate_aware` or the two locations.
    ///
    /// This is the cheap O(1) short-circuit the comparison engine checks
    /// before running the O(k²) per-bucket scope scan: on the unrestricted
    /// (default) scope, no pair can ever be pruned, so the engine takes
    /// the fast n-ary path with no `allows()` calls at all.
    ///
    /// `crate_aware` is intentionally NOT consulted: it only ever WIDENS
    /// the allowed set (it forces the two crate axes to no-op when no
    /// crate-id was derivable). When all four axes are already `true`,
    /// `crate_aware` cannot tighten anything, so a scope with every axis
    /// `true` permits every pair whether `crate_aware` is `true` or
    /// `false`.
    #[must_use]
    pub const fn permits_all(&self) -> bool {
        self.within_crate && self.across_crate && self.within_module && self.across_module
    }

    /// Decide whether the pair `(a, b)` is allowed to cluster.
    ///
    /// Evaluates the two axes (crate, module) independently and requires
    /// BOTH to pass: a pair must clear the crate axis AND the module
    /// axis. For each axis the relevant knob is consulted by the
    /// within/across relationship between the two locations:
    ///
    /// - **Same crate** → gated by `within_crate`.
    /// - **Different crate** → gated by `across_crate`.
    /// - **Same module path** → gated by `within_module`.
    /// - **Different module path** → gated by `across_module`.
    ///
    /// **`crate_aware == false`** short-circuits the crate axis to
    /// always-allowed (a no-op): without resolvable crate-ids, every
    /// form's `crate_id` is `None`, which would otherwise read as "same
    /// crate" and prune everything when `within_crate == false`. The
    /// module axis is unaffected by `crate_aware`.
    ///
    /// Symmetric: `allows(a, b) == allows(b, a)` — both the crate-id
    /// comparison (`==`) and the module-path comparison (`==`) are
    /// symmetric.
    #[must_use]
    pub fn allows(&self, a: &StructuralLocation, b: &StructuralLocation) -> bool {
        self.crate_axis_allows(a, b) && self.module_axis_allows(a, b)
    }

    /// Crate axis of the predicate. A no-op (always `true`) when
    /// `crate_aware == false`. Takes `self` by value — [`ResolvedScope`]
    /// is `Copy`.
    fn crate_axis_allows(self, a: &StructuralLocation, b: &StructuralLocation) -> bool {
        if !self.crate_aware {
            return true;
        }
        if a.crate_id == b.crate_id {
            self.within_crate
        } else {
            self.across_crate
        }
    }

    /// Module axis of the predicate. Always evaluated (independent of
    /// `crate_aware`). Takes `self` by value — [`ResolvedScope`] is
    /// `Copy`.
    fn module_axis_allows(self, a: &StructuralLocation, b: &StructuralLocation) -> bool {
        if a.module_path == b.module_path {
            self.within_module
        } else {
            self.across_module
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(crate_id: Option<&str>, module: &[&str]) -> StructuralLocation {
        StructuralLocation {
            crate_id: crate_id.map(str::to_string),
            module_path: module.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// All 32 `ResolvedScope` combinations (5 boolean axes), bit-
    /// decomposed so the truth-table tests stay flat (no deep loop
    /// nesting that would trip the CC ladder).
    fn scope_combos() -> impl Iterator<Item = ResolvedScope> {
        (0u8..32).map(|b| ResolvedScope {
            within_crate: b & 1 != 0,
            across_crate: b & 2 != 0,
            within_module: b & 4 != 0,
            across_module: b & 8 != 0,
            crate_aware: b & 16 != 0,
        })
    }

    #[test]
    fn default_is_all_true_and_crate_aware() {
        let s = ResolvedScope::default();
        assert!(s.within_crate);
        assert!(s.across_crate);
        assert!(s.within_module);
        assert!(s.across_module);
        assert!(s.crate_aware);
    }

    #[test]
    fn default_allows_every_pair() {
        let s = ResolvedScope::default();
        let same = loc(Some("a"), &["m"]);
        let diff = loc(Some("b"), &["n"]);
        assert!(s.allows(&same, &diff));
        assert!(s.allows(&same, &same));
        assert!(s.allows(&diff, &diff));
    }

    #[test]
    fn permits_all_true_only_when_every_axis_is_true() {
        // Default (all four axes true) -> permits_all is true. Kills the
        // `-> false` whole-fn mutant.
        assert!(
            ResolvedScope::default().permits_all(),
            "default (all axes true) must permit all"
        );

        // Each single axis flipped to false -> permits_all is false. Each
        // case isolates one `&&` term, so the per-operator `&& -> ||`
        // mutants (which would keep returning true when one axis is false)
        // are all killed; the `-> true` whole-fn mutant dies on any of
        // these four.
        for tweak in [
            ResolvedScope {
                within_crate: false,
                ..ResolvedScope::default()
            },
            ResolvedScope {
                across_crate: false,
                ..ResolvedScope::default()
            },
            ResolvedScope {
                within_module: false,
                ..ResolvedScope::default()
            },
            ResolvedScope {
                across_module: false,
                ..ResolvedScope::default()
            },
        ] {
            assert!(
                !tweak.permits_all(),
                "any single axis false must make permits_all false: {tweak:?}"
            );
        }

        // `crate_aware` is NOT a permits_all axis: all four axes true with
        // crate_aware=false still permits all (crate_aware only widens).
        let crate_unaware = ResolvedScope {
            crate_aware: false,
            ..ResolvedScope::default()
        };
        assert!(
            crate_unaware.permits_all(),
            "crate_aware must not gate permits_all when all four axes are true"
        );
    }

    #[test]
    fn allows_is_symmetric() {
        // Exhaustively over a small product of axis settings + a couple
        // of location pairs, assert allows(a,b) == allows(b,a).
        let a = loc(Some("crate_a"), &["foo"]);
        let b = loc(Some("crate_b"), &["bar"]);
        let c = loc(Some("crate_a"), &["foo"]);
        let pairs = [(&a, &b), (&a, &c), (&b, &c)];
        for s in scope_combos() {
            for (x, y) in pairs {
                assert_eq!(
                    s.allows(x, y),
                    s.allows(y, x),
                    "allows must be symmetric for {s:?}"
                );
            }
        }
    }

    // ---- Truth table: crate axis (crate_aware = true) ----

    #[test]
    fn within_crate_false_blocks_same_crate_pair() {
        let s = ResolvedScope {
            within_crate: false,
            ..ResolvedScope::default()
        };
        // Same crate, same module: only the within_crate axis is in play
        // (within_module stays true).
        let a = loc(Some("k"), &["m"]);
        let b = loc(Some("k"), &["m"]);
        assert!(
            !s.allows(&a, &b),
            "same-crate pair blocked by within_crate=false"
        );
    }

    #[test]
    fn within_crate_false_allows_cross_crate_pair() {
        let s = ResolvedScope {
            within_crate: false,
            ..ResolvedScope::default()
        };
        // Different crate, same module — across_crate stays true.
        let a = loc(Some("k1"), &["m"]);
        let b = loc(Some("k2"), &["m"]);
        assert!(
            s.allows(&a, &b),
            "cross-crate pair unaffected by within_crate"
        );
    }

    #[test]
    fn across_crate_false_blocks_cross_crate_pair() {
        let s = ResolvedScope {
            across_crate: false,
            ..ResolvedScope::default()
        };
        let a = loc(Some("k1"), &["m"]);
        let b = loc(Some("k2"), &["m"]);
        assert!(
            !s.allows(&a, &b),
            "cross-crate pair blocked by across_crate=false"
        );
    }

    #[test]
    fn across_crate_false_allows_same_crate_pair() {
        let s = ResolvedScope {
            across_crate: false,
            ..ResolvedScope::default()
        };
        let a = loc(Some("k"), &["m"]);
        let b = loc(Some("k"), &["m"]);
        assert!(
            s.allows(&a, &b),
            "same-crate pair unaffected by across_crate"
        );
    }

    // ---- Truth table: module axis ----

    #[test]
    fn within_module_false_blocks_same_module_pair() {
        let s = ResolvedScope {
            within_module: false,
            ..ResolvedScope::default()
        };
        let a = loc(Some("k"), &["m"]);
        let b = loc(Some("k"), &["m"]);
        assert!(
            !s.allows(&a, &b),
            "same-module pair blocked by within_module=false"
        );
    }

    #[test]
    fn within_module_false_allows_cross_module_pair() {
        let s = ResolvedScope {
            within_module: false,
            ..ResolvedScope::default()
        };
        let a = loc(Some("k"), &["m1"]);
        let b = loc(Some("k"), &["m2"]);
        assert!(
            s.allows(&a, &b),
            "cross-module pair unaffected by within_module"
        );
    }

    #[test]
    fn across_module_false_blocks_cross_module_pair() {
        let s = ResolvedScope {
            across_module: false,
            ..ResolvedScope::default()
        };
        let a = loc(Some("k"), &["m1"]);
        let b = loc(Some("k"), &["m2"]);
        assert!(
            !s.allows(&a, &b),
            "cross-module pair blocked by across_module=false"
        );
    }

    #[test]
    fn across_module_false_allows_same_module_pair() {
        let s = ResolvedScope {
            across_module: false,
            ..ResolvedScope::default()
        };
        let a = loc(Some("k"), &["m"]);
        let b = loc(Some("k"), &["m"]);
        assert!(
            s.allows(&a, &b),
            "same-module pair unaffected by across_module"
        );
    }

    // ---- Both axes must pass ----

    #[test]
    fn pair_must_clear_both_axes() {
        // within_crate ok but across_module blocked => disallowed.
        let s = ResolvedScope {
            across_module: false,
            ..ResolvedScope::default()
        };
        let a = loc(Some("k"), &["m1"]);
        let b = loc(Some("k"), &["m2"]); // same crate (ok), diff module (blocked)
        assert!(!s.allows(&a, &b));
    }

    // ---- crate_aware = false: crate axes no-op ----

    #[test]
    fn crate_aware_false_noops_crate_axes_so_none_id_pair_not_dropped() {
        // The single-dir scenario: both forms have crate_id None, which
        // reads as "same crate". With within_crate=false AND
        // crate_aware=true this pair would be dropped — exactly the bug
        // crate_aware guards. With crate_aware=false the crate axis is a
        // no-op, so the pair survives (module axis still true).
        let s = ResolvedScope {
            within_crate: false,
            across_crate: false,
            crate_aware: false,
            ..ResolvedScope::default()
        };
        let a = loc(None, &["m"]);
        let b = loc(None, &["m"]);
        assert!(
            s.allows(&a, &b),
            "crate_aware=false must no-op the crate axes so a None-id pair is NOT dropped"
        );
    }

    #[test]
    fn crate_aware_false_still_applies_module_axes() {
        // crate_aware=false no-ops ONLY the crate axes — the module axis
        // still gates.
        let scope = ResolvedScope {
            within_crate: false,
            across_crate: false,
            within_module: false,
            crate_aware: false,
            ..ResolvedScope::default()
        };
        let same_a = loc(None, &["m"]);
        let same_b = loc(None, &["m"]); // same module -> blocked by within_module=false
        assert!(
            !scope.allows(&same_a, &same_b),
            "module axis must still apply when crate_aware=false"
        );

        let diff_a = loc(None, &["m1"]);
        let diff_b = loc(None, &["m2"]); // diff module -> across_module true (default)
        assert!(
            scope.allows(&diff_a, &diff_b),
            "cross-module pair allowed when across_module true even with crate_aware=false"
        );
    }

    #[test]
    fn crate_aware_true_drops_none_id_pair_when_within_crate_false() {
        // The contrast to the no-op test: with crate_aware=true, two
        // None crate_ids read as same-crate and ARE dropped by
        // within_crate=false. This is the bug crate_aware=false avoids.
        let s = ResolvedScope {
            within_crate: false,
            crate_aware: true,
            ..ResolvedScope::default()
        };
        let a = loc(None, &["m"]);
        let b = loc(None, &["m"]);
        assert!(
            !s.allows(&a, &b),
            "crate_aware=true treats None==None as same-crate (the guarded bug)"
        );
    }
}
