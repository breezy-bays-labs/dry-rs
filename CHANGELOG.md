# Changelog

All notable changes to this project will be documented in this file. The
format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

dry-rs follows a deliberate **no-public-release** policy through v0.x —
mokumo is the sole consumer; tags exist for git pinning only. The
release gate is **decoupled per-crate**:

- `dry4rs` v1.0 ships when its CLI/JSON contract stabilizes (depends
  on `dry-core` v0.x).
- `dry-core` graduates to v1.0 only when both `dry4rs` AND `dry4ts`
  consume it (cross-language abstraction validated).
- `dry4ts` v1.0 ships with TS CLI + JSON parity with dry4rs.

See `ops/workspace/dry-rs/20260508-dry-rs-roadmap/roadmap.md` for the
full release roadmap.

## [Unreleased]

### Added

- **PR 6 (#7)** — comparison engine in `dry_core::comparison`. Single
  module per the O6 "no detector taxonomy at v0.1" rule. Two-tier
  detection:
  - **Pass 1 — hash-bucket clustering**: forms whose `fingerprint_set`
    is byte-identical (verified by structural equality, not bucket-key
    equality alone) surface as an n-ary `Match` with `score == 1.0`
    and tier `AutoRefactor`. XOR-fold bucket keys keep the pass
    allocation-free; the verification step filters the rare XOR
    collision.
  - **Pass 2 — sliding-window Jaccard**: unclaimed forms sorted
    ascending by `node_count`; inner loop breaks at
    `forms[j].node_count > forms[i].node_count / threshold` (Jaccard
    upper bound). `node_count` is a heuristic proxy for set size per
    the O8 ADR — when set size and node_count align the break is
    exact; when they diverge the break is conservative.
  - **Threshold tier assignment**: named constants
    `AUTO_REFACTOR_FLOOR = 0.95` and `REVIEW_FIRST_FLOOR = 0.85`
    route Pass 2 emits to the correct tier. Pass 1 emits land at
    `AutoRefactor` directly.
  - **Empty `fingerprint_set` policy**: `jaccard` returns 0.0 when
    either set is empty; Pass 1 skips empty-set forms entirely
    (they all XOR to 0 but have no structure to share).
  - **Deterministic output sort**: `Vec<Match>` is sorted by
    `(forms[0].file, forms[0].span.start, -score)` with
    `f64::total_cmp` for a total order on score.
  - **Threshold validation**: `debug_assert!` rejects values outside
    `(0.0, 1.0]`. CLI surface (PR 8) is the production
    input-validation boundary.
  - Property tests at `crates/dry-core/tests/comparison_proptest.rs`
    pin Jaccard reflexivity / symmetry / boundedness / totality,
    the break-math invariant against `|fingerprint_set|`, the
    threshold gate, the tier floor table, and the canonical sort
    order. Unit tests in `crates/dry-core/src/comparison/mod.rs`
    cover hand-crafted Pass-1-and-Pass-2 scenarios including
    XOR-collision handling, mixed exact+near input, threshold=1.0
    behaviour, and the debug-only threshold panics.

### Fixed

- **PR 6 (#7) Gemini review follow-up** — two findings on PR #33:
  - **Multi-cluster Pass 1 buckets** — a single Pass 1 bucket can
    XOR-collide multiple distinct equal-set clusters (e.g. `{1, 2}`
    and `{4, 7}` both fold to `3`). The original loop emitted only
    the canonical cluster and left the others unclaimed, which made
    Pass 2 legitimately encounter `score == 1.0` pairs and trip its
    `debug_assert!(false)` defensive guard. Pass 1 now iterates over
    the bucket until every equal-set cluster has been emitted; the
    Pass 2 guard remains as a defense-in-depth fallback.
    Regression tests:
    `pass1_emits_distinct_clusters_for_xor_colliding_sets`,
    `pass1_leaves_xor_colliding_singletons_for_pass2`,
    `pass1_emits_cluster_and_leaves_singleton_in_same_bucket`.
  - **Sort-comparator clone** — `sort_matches_for_output` cloned
    `FilePath` (a `PathBuf` newtype) inside the `sort_by` closure,
    which fires O(n log n) times. Switched to borrow-only sort
    keys.

- **PR 7 (#8)** — language-agnostic adapters in `dry_core::adapters`:
  - `source::enumerate(&AnalysisConfig)` — file walker via the `ignore`
    crate (honors `.gitignore` / `.ignore` / `.git/info/exclude` like
    `rg` / `fd`). Deterministically sorted; `include_ignored` switch
    for fixtures; skip-on-read-error policy via
    `SourceWarning::Unreadable` accumulated alongside the path list.
  - `reporters::json::render(&Report, EnvelopeMeta)` — locked v0.1
    nested wire envelope (`schema_version` / `tool` / `tool_version` /
    `language` / `timestamp` / `threshold_mode` / `result` / `view?` /
    `delta?` / `diagnostics?`). The envelope struct lives in
    `adapters/reporters/json/envelope.rs` (NOT a domain type, per
    `adr-nested-json-envelope.md`). Caller-supplied timestamp keeps
    the wire-envelope snapshot byte-stable.
  - `reporters::text::render(&Report)` — comfy-table output grouped
    by tier (`auto_refactor` -> `review_first` -> `advisory`), ASCII
    Markdown preset, NO ANSI color (color is a CLI-flag concern at
    PR 8).
  - `reporters::github_annotations::render(&Report)` — GitHub Actions
    workflow-command lines per Match. Tier->severity:
    `auto_refactor -> ::error::`, `review_first -> ::warning::`,
    `advisory -> ::notice::`. Two-tier GHA escape (message-data vs
    property-value) prevents POSIX-path delimiters in `file=` from
    corrupting the runner's parse.
  - `cli::AnalysisConfig` — minimal v0.1 config the walker consumes
    (`roots`, `extensions`, `include_ignored`). PR 8 extends with
    clap-derive surface for `--threshold` / `--format` / `--top` etc.
  - Mechanical wire-shape lock at
    `crates/dry-core/tests/wire_envelope_snapshot.rs`: the executable
    schema document for the v0.1 envelope. Three-null reserved slots
    on Match, top-level key ordering, BTreeMap summary ordering, and
    `view`/`delta`/`diagnostics` skip-when-None all locked.
  - Insta snapshots for the github-annotations reporter
    (`crates/dry-core/tests/github_annotations_snapshot.rs`) lock all
    three tier mappings and the property-value escape rules.
  - Property tests at `crates/dry-core/tests/adapters_proptest.rs`
    cover: `enumerate` determinism across runs; `json::render`
    totality; reserved-score-slot null emission; text-reporter
    ANSI-cleanness; one annotation per Match with locked severity
    prefix + required property keys.
  - Workspace deps added: `walkdir`, `ignore`, `comfy-table` (all on
    `dry-core`'s allowed list per `adr-hexagonal-layout.md`); promoted
    `serde_json` from dev-only to runtime; added `tempfile` as a
    dev-dep for walker fixtures.
- **PR 5 (#6)** — `dry4rs::parser::SynNormalizer` implementing
  `NormalizerPort` for Rust source via `syn`:
  - Walks the `syn` AST depth-first; emits one `NormalizedForm` per
    top-level `ItemFn` / `ImplItemFn` / `TraitItemFn`-with-default
    (closures + nested fns appear as opaque `Closure` / `NestedFn`
    marker tokens at v0.1; separate-form emission for them lands at
    v0.2+).
  - Per-subform Merkle-style fingerprinting via
    `xxhash_rust::xxh3::Xxh3` (cross-toolchain stable; see § "Changed"
    below). Each visited subtree emits one `u64`; children's `u64`s
    fold into their parent's hash, so structurally-equivalent subtrees
    produce identical `u64`s at every level of granularity.
  - Closed `NormalizedToken` placeholder vocabulary at v0.1: `Var`,
    `Ident`, `TypeParam`, `Lifetime`/`LifetimeStatic`, `Op`, `Kw`,
    `Modifier`, literals (`LitInt`/`LitFloat`/`LitStr`/`LitBool`/
    `LitChar`/`LitByte`/`LitByteStr`), `MacroCall`, `Attr`, `PathSeg`,
    `Closure`, `NestedFn`. Projection through `NormalizedToken`
    decouples the fingerprint vocabulary from syn's enum layout.
  - 17-construct handling per the O5 ADR
    (`adr-rust-normalization-rules.md`): function-shape emission
    (`fn` / `impl Trait for Type` / trait default body), match arms,
    generics (collapsed type params, preserved bounds), lifetimes
    (collapsed except `'static`), attributes (preserve `#[test]` /
    `#[inline]` / `#[cold]` / `#[must_use]` / `#[no_mangle]` /
    `#[repr(...)]`; strip `#[derive(...)]` / `#[doc(...)]` /
    `#[allow(...)]` / `#[warn(...)]` / `#[cfg(...)]` /
    `#[deprecated(...)]`), opaque macro calls, closures, async / const
    / unsafe modifiers, where clauses, type aliases (no form), trait
    objects, module paths.
  - `FormKind::Test` detection: `#[test]` / `#[*::test]` attribute OR
    enclosing `#[cfg(test)] mod` context. `FormKind::Doctest` reserved
    at v0.1 (no extraction).
  - O11 identifier emission rule: every encountered identifier emits
    one entry into `NormalizedForm.identifier_set` in walk order;
    attribute names are NOT recorded (they are language vocabulary,
    not renameable identifiers). The v0.2+ rename-signal consumer
    converts to multiset / set at the comparison boundary.
  - Skip-on-parse-error: `syn::parse_file` errors become
    `NormalizeError::Parse`; no panic, no unwrap.
  - 55 new tests (47 integration + 8 property) at
    `crates/dry4rs/tests/normalizer_integration.rs` and
    `crates/dry4rs/tests/normalizer_proptest.rs`. Property tests pin
    determinism, identifier_set walk-order stability, span-locations
    feature activeness, never-panic on arbitrary source.
  - Workspace deps added: `syn` (with `full` + `extra-traits`
    features), `proc-macro2` (with the LOAD-BEARING `span-locations`
    feature; CI's `proc-macro2 span-locations enforcement` job +
    lefthook pre-push enforce structurally), `xxhash-rust` (with
    `xxh3` feature; see § "Changed"). All adapter-scoped per the
    hexagonal-layout ADR; `dry-core` remains AST-library-pure.
- Initial workspace bootstrap: `crates/dry-core` skeleton (lib only,
  AST-library-pure) with hexagonal module layout (`domain/`, `ports/`,
  `comparison/`, `adapters/`, `cli/`); `crates/dry4rs` skeleton (lib +
  bin) with re-export pattern from `dry-core` and 5-line `main()` entry.
- CI workflow with format / clippy / test matrix (Linux + macOS arm64 +
  macOS x86_64) / coverage / MSRV / cargo-deny / ast-purity / docs jobs.
- Repo chrome: README, AGENTS.md, CLAUDE.md, CONTRIBUTING.md, dual
  MIT/Apache-2.0 license, lefthook git hooks, deny.toml supply-chain
  policy.
- Architecture decisions filed (private ops vault):
  - ADR: Hexagonal layering invariant for dry-rs
    (`ops/decisions/dry-rs/adr-hexagonal-layout.md`)
  - ADR: Nested JSON envelope with `schema_version` + multi-score
    `Match` shape locked at v0.1 with null defaults; `score` is pure
    Jaccard at all versions per dry4clj precedent (Scenario A locked)
    (`ops/decisions/dry-rs/adr-nested-json-envelope.md`)
- Open-question tracking issues filed (`type:design` + `priority:soon`,
  blocked by #3): #11 — O5 smart-normalization rules for ~17 Rust syn
  constructs (resolved by PR 5); #12 — O8 NormalizedForm cross-language
  schema (folds O11 + O12, resolved by PR 4); #13 — O9 Span coordinate
  semantics (resolved by PR 3 closeout-deliverable ADR).
- New CI job `span-locations-check`: mechanical enforcement of the
  `proc-macro2` `span-locations` requirement via `cargo metadata + jq`.
  Vacuously passes at PR 2 time (no `proc-macro2` deps yet); activates
  structurally when PR 5 lands the syn-based normalizer. Mirrored as a
  pre-push hook in `lefthook.yml`.
- Bot context availability infrastructure (closes #24): `AGENTS.md`
  carries a new "For automated code reviewers" section distilling the
  load-bearing rules from private ADRs (allowed deps per crate,
  AST-purity scope, locked wire shapes, `#[non_exhaustive]`
  discipline, `const fn` + `Drop` Rust-1.61 rule); `.coderabbit.yaml`
  carries path-scoped `path_instructions` for
  `crates/dry-core/src/domain/**` and `crates/dry-core/**` so
  CodeRabbit grounds its suggestions in the rules; `.gemini/styleguide.md`
  mirrors the rules in prose for `gemini-code-assist`;
  `.gemini/config.yaml` sets noise thresholds. Drift CI deferred to
  #26 (`priority:later`). Pattern tracked at org level in
  `ops/decisions/org/adr-bot-context-availability.md`.

### Changed

- AST-library purity grep regex extended to cover `rustc_ast` and
  `rustc_parse` (CAO Finding 3.1 from the PR 2 ADR audit). Closes the
  rustc-private parser leak vector. The regex update touches three
  files in lockstep: `.github/workflows/ci.yml` (`ast-purity` job),
  `lefthook.yml` (`ast-purity` pre-push hook), and the
  `deny.toml` AST-library policy comment.
- PR 5 fingerprint hash pulled forward to cross-toolchain stable
  `xxhash_rust::xxh3::Xxh3` (was: `std::hash::DefaultHasher`). The
  stdlib reserves the right to change `DefaultHasher`'s SipHash-1-3
  bit pattern in any new toolchain release; cross-toolchain stability
  is required for the v0.3+ `--delta` baseline comparison feature
  (persisted baselines outlive MSRV bumps). Decision trajectory in
  the amended O5 ADR (`ops/decisions/dry-rs/adr-rust-normalization-rules.md`
  § "Hashing — `xxh3` via `xxhash-rust`") + hexagonal-layout ADR
  per-crate dep table footnote [²]. `dry-core` is NOT amended —
  hashing happens in adapters; the comparison engine consumes
  pre-computed `HashSet<u64>` and never hashes. `deny.toml` adds
  `BSL-1.0` to the allow list (xxhash-rust's license; OSI-approved,
  FSF-recognized, MIT/X11-style permissive). `AGENTS.md` per-crate
  dep table and bot-context section (`.coderabbit.yaml`,
  `.gemini/styleguide.md`) updated in lockstep to pre-empt regression
  suggestions.
