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

- New CI job `bot-context-drift` (closes #26): mechanical enforcement
  of AGENTS.md ↔ Cargo.toml dep-table consistency. The AGENTS.md "For
  automated code reviewers" section (landed via #24) grounds AI bots
  (CodeRabbit, gemini-code-assist) in the project's per-crate
  allowed-deps rules; the per-crate dep table under `## Architecture`
  is the authoritative source. This lint catches drift bidirectionally:
  - **missing-in-table** — a crate's Cargo.toml has a dep AGENTS.md
    doesn't list (table is stale / dep added unilaterally).
  - **extra-in-table** — AGENTS.md lists a dep Cargo.toml doesn't
    have (aspirational entry / dep removed without table update).

  Single source of truth in `scripts/bot-context-drift.py`; new
  isolated workflow file `.github/workflows/bot-context-drift.yml` and
  a `bot-context-drift` pre-push hook in `lefthook.yml` both invoke
  it. **Scope**: internal consistency only (option (c) of #26's
  Discovery section) — cross-repo verification against the source
  ADRs in the private `ops` vault is deferred (needs a deploy key +
  cross-repo CI access, out of scope at v0.1). Current-state audit
  flagged two pre-existing AGENTS.md drifts, both fixed in this PR
  (extra-in-table: `regex` for `dry-core` + `quote` for `dry4rs` —
  both aspirational entries never landed in `Cargo.toml`).

- **#20 — Mutation testing CI (Track C sibling-coherence).** New
  `.github/workflows/mutants.yml` runs `cargo mutants` on every PR
  against `crates/dry-core/src/comparison/mod.rs` — the load-bearing
  Jaccard + hash-bucket clustering math. Scoped at v0.1 to one file:
  the comparison engine is where mutating real logic earns its keep
  per AGENTS.md "Comparison algorithm contract"; the rest of the
  workspace is dominated by serde-shape constructors that mutation
  testing covers trivially or that wire-envelope snapshots already
  guard. Mirrors crap4rs's `.cargo/mutants.toml` shape (sibling-
  coherence track, per the same vision the PR 2 CHANGELOG documents).
  - **`.cargo/mutants.toml`** — `exclude_re` patterns for surviving
    mutants, each carrying a `tracked: dry-rs#36` or
    `tracked: dry-rs#37` reference per
    `~/.claude/rules/exclusions.md`:
    - #36 covers 2 mutants in `pass2_sliding_window` that signal
      real test gaps (break-math boundary at L346,
      threshold-equality at L350) — plug + delete the exclude in
      the same PR.
    - #37 covers 7 mutants that are equivalent by design — the
      `pass1_hash_bucket` partition predicate that infinite-loops
      on the `!=` flip (L296), the unreachable `score == 1.0`
      defensive guard inside `pass2_sliding_window`'s
      `debug_assert!(false, ...)` branch (L361, 4 mutants), the
      `jaccard` `||` → `&&` short-circuit whose fallthrough
      produces `0.0` for every single-empty input (L483 — proved
      via trace table in #37), and the `jaccard` "iterate over
      smaller set" optimization invariant (L488). Issue #37 picks
      between an ADR + `exclude_re` shape and an in-source
      `#[cfg_attr(test, mutants::skip)]` shape.
  - **Workflow shape.** Per-PR + `workflow_dispatch`; skip-on-docs
    (`**/*.md`, `docs/**`); 30 min timeout (local run ~2 min; budget
    is the cargo-mutants-recommended backstop for proptest-shrinker
    tail latency); separate cache key (`dry-rs-mutants`) from the
    main `dry-rs` rust-cache so eviction doesn't churn. `--in-place`
    + `--no-shuffle` for deterministic, allocation-free runs.
    `mutants.out/` uploaded as artifact on failure (7-day retention)
    for triage. SHA-pinned actions matching the post-#16 ci.yml
    convention so the `unpinned-uses` zizmor audit stays green.
  - **Scope deliberately narrow at v0.1.** Widening (to add
    `crates/dry4rs/src/normalizer.rs`, or the file walker, or the
    JSON envelope builder) is a follow-up decision — adding files
    multiplies CI time linearly and the burn-vs-signal trade-off is
    file-specific (the normalizer's mutants are dominated by parser
    detail; the comparison engine's mutants are dominated by
    algorithm correctness, which is exactly what mutation testing
    targets).
- **PR 9 (#10) — self-referential test + symmetric dogfood + composite
  action**. Closes the v0.1 walking skeleton. Three layers:
  - **Self-referential test** at `crates/dry4rs/tests/self_check.rs`.
    Spawns the dry4rs binary via `CARGO_BIN_EXE_dry4rs` against the
    workspace's own `crates/` tree and snapshots a stable subset
    (`total_forms` + `matches_count` + `by_tier`) of the JSON wire
    envelope via insta. The snapshot is intentionally fragile —
    source edits that change form counts WILL fail the test; that's
    the gate, not a bug. Cargo-insta-review accept-on-the-PR is the
    intended workflow when a change is deliberate.
    - Belt-and-suspenders test verifies the v0.1 wire envelope's
      top-level locked keys (`schema_version` / `tool` / `language` /
      `tool_version` / `timestamp` / `threshold_mode` / `result`) on a
      real end-to-end production invocation, in addition to the
      dry-core synthesized-fixture wire-envelope snapshot.
    - dry4rs gains an `insta` dev-dep (mirrors dry-core; uses the
      workspace `json` feature).
  - **`.dry-rs-ignore.toml`** schema document at repo root. v0.1
    ships with the schema header + ZERO `[[allowed_match]]` entries
    by design — wiring this file into the gate is a v0.2 deliverable
    that requires per-match fingerprints to ride the wire envelope.
    Documents the forward-looking format (fingerprints + reason +
    optional `until` date) so contributors know the shape; the v0.1
    gate remains the `dry-self` snapshot.
  - **CI workflow** at `.github/workflows/self-test.yml`. Three jobs
    (`dry-self` / `crap-self` / `scrap-self`) triggered on push to
    main and pull_request; concurrency mirrors `ci.yml`.
    - `dry-self` runs the self-check integration test.
    - `crap-self` runs the public `breezy-bays-labs/crap-rs`
      composite action against the workspace's production code
      (SHA-pinned to `34b05488` ⇒ `crap4rs-v0.6.0`). Lands as
      `analysis-gate: false` (measurement-only) — a local probe
      surfaced 8 functions over the default cognitive threshold of 15;
      flipping the gate to `true` on first contact would dishonestly
      attribute pre-existing complexity debt to the PR adding the
      gate. `tracked: dry-rs#42` sequences the refactor + gate-flip
      so the gate state changes in the PR that actually causes the
      change.
    - `scrap-self` is a PR-9 stub — scrap-rs does not yet publish a
      composite action under `.github/actions/`. The job surfaces the
      gap in CI output + tracks the lift (`tracked: dry-rs#40`).
  - **Composite action** at `.github/actions/scorecard/action.yml`.
    Consumed by mokumo CI as
    `breezy-bays-labs/dry-rs/.github/actions/scorecard@v0.1.0`.
    Inputs: `paths` (default `.`), `threshold` (default `0.85`),
    `format` (default `json`), `output-path` (default
    `dry-rs-report.json`), `extensions` (default empty), and
    `fail-on-findings` (default `true`). Outputs: `report-path`,
    `findings-count`.
    - Self-builds dry4rs from the action ref via `actions/checkout@v4`
      pulling `github.action_repository` at `github.action_ref` into
      `__dry-rs-source/`, then `cargo build --release -p dry4rs
      --locked`. No binstall path until per-crate v1.0 (see
      `CLAUDE.md` "v0.x -> v1.0 transition" + `adr-hexagonal-layout.md`).
    - Renders a text summary onto `$GITHUB_STEP_SUMMARY` (KPIs +
      collapsible text-report block) for in-PR visibility.
    - `outputs.row-json` deliberately deferred to a follow-up
      (dry-rs#39) — the `RowCommon.tool: "dry4rs"` row shape for
      mokumo consumption is a cross-tool coordination decision
      across crap4rs / scrap-rs / dry-rs and should not be designed
      in isolation.

- **PR 8 (#9) — CLI surface** in `dry_core::cli`. The v0.1 entry point
  for every adapter binary in the workspace; dry4rs's `main.rs`
  becomes a 5-line entry calling `dry_core::cli::run::<SynNormalizer>()`.
  - **`Args` (clap derive)** — top-level argument struct with global
    flags: `--threshold` (parser-validates (0.0, 1.0]), `--format
    text|json` (markdown/html/sarif deferred to later waves),
    `--threshold-mode strict|default|lenient`, `--top N` (view-shaping
    only), `--only-failing` (view-shaping only), `--no-fail` (suppress
    non-zero exit; `result.passed` still authoritative),
    `--include-ignored` (walker bypass for fixture corpora),
    `--completions <SHELL>` (clap_complete-driven shell-completion
    generation, short-circuits the analyzer pipeline). All flags are
    `global = true` so they propagate to subcommands.
  - **`Command` enum** — v0.1 subcommands: `report` (implicit default)
    / `stats` / `check` carry per-command positional `paths`;
    `ignore <fingerprint>` / `ignored` / `cleanup` are skeletal v0.1
    stubs (full `.dry-rs-ignore.toml` UX lands at v0.2 per roadmap).
    `#[non_exhaustive]` per the AGENTS.md enums-yes-structs-no rule.
  - **`Format` / `ThresholdMode` enums** — `#[non_exhaustive]` value
    enums; `Format` rejects `markdown` / `html` / `sarif` at parse
    time so users get an actionable error instead of silent
    fall-through.
  - **`AnalysisConfig`** — full v0.1 configuration surface: `roots`,
    `threshold`, `format`, `output` (new `OutputDestination` enum;
    `Stdout` only at v0.1, `--output PATH` reserved for v0.2),
    `extensions`, `include_ignored`, `threshold_mode`. Builder chain
    (`with_threshold` / `with_format` / `with_threshold_mode` /
    `with_output` / `with_extensions` / `with_include_ignored`) lets
    library callers embed `dry-core` directly without going through
    clap. `DEFAULT_THRESHOLD = 0.85` constant aligns with
    `comparison::REVIEW_FIRST_FLOOR`; a cross-module sanity test
    fails CI if either constant drifts.
  - **Generic `run<N: NormalizerPort + Default>() -> ExitCode`** — the
    full analyzer pipeline: clap parse → `--completions` short-circuit
    → allowlist-subcommand short-circuit → AnalysisConfig → walker
    `enumerate` → per-file read+normalize → comparison engine →
    Report (truthful gate) → optional ViewProjection (shapeable
    display) → subcommand-dispatched output → exit-code derivation.
  - **Truthful-gate-vs-shapeable-display** (per
    `adr-nested-json-envelope.md`): `result.*` is unfiltered;
    `view.*` is `None` when no shaping flag is set (skip_serializing_if
    omits the field, matching the wire-envelope snapshot lock).
    `--top N` / `--only-failing` populate `view.*` only; `view.passed`
    mirrors `result.passed` for symmetry.
  - **Exit codes**:
    - `ExitCode::SUCCESS` when `report.passed == true` OR `--no-fail`.
    - `ExitCode::FAILURE` when `report.passed == false` AND no `--no-fail`.
    - `ExitCode::from(2)` on clap arg errors (clap's built-in exit) or
      walker `NoRoots` (the CLI defaults paths to `.`, so this is
      unreachable in practice; the variant exists for completeness).
      Per-file source parse errors are diagnostics emitted on stderr,
      NOT gate failures — the comparison engine continues with whatever
      forms normalized successfully.
  - **`NormalizerPort` extension** — three new methods with sensible
    defaults: `tool_name()` (default `"dry"`; SynNormalizer returns
    `"dry4rs"`), `tool_version()` (default `dry-core`'s
    `CARGO_PKG_VERSION`; adapter overrides with its own crate's
    constant), `language()` (default `"unknown"`; SynNormalizer
    returns `"rust"`). These supply the wire envelope's `tool` /
    `tool_version` / `language` fields without `dry-core` hard-coding
    adapter identity.
  - **Allowlist subcommand short-circuit** — `ignore <fingerprint>` /
    `ignored` / `cleanup` bypass the analyzer pipeline at v0.1 and
    return SUCCESS with a stderr deferral note pointing at the v0.2
    `.dry-rs-ignore.toml` landing. The dispatch arm in
    `dispatch_output` is a defensive no-op fallback.
  - **ISO-8601 timestamp formatter** — `format_unix_seconds_iso8601`
    formats `SystemTime::now()` via Howard Hinnant's days-from-epoch
    algorithm (u64 variant — no pre-1970 cases possible) so no
    `chrono` / `time` / `jiff` dep is required for envelope metadata.
  - Workspace deps added: `clap` (with `derive` feature), `clap_complete`.
    Both listed in `dry-core`'s allowed-deps row per the
    hexagonal-layout ADR.
  - Tests: 28 unit tests on `cli/args.rs` + `cli/run.rs` covering
    clap parsing, threshold validation, view-projection building,
    timestamp formatting, threshold-mode labels; 25 integration tests
    at `crates/dry-core/tests/cli_args.rs` (clap try_parse_from round
    trips) + `crates/dry4rs/tests/cli_pipeline.rs` (end-to-end binary
    invocations proving the truthful-gate invariant against the real
    wire output) + `crates/dry4rs/tests/binary_smoke.rs` (`--help` /
    `--version` / `check` / invalid-threshold exit codes).
  - Explicit v0.1 deferrals (documented in PR body):
    `.dry-rs-ignore.toml` allowlist file (lands v0.2); `dry4rs.toml`
    3-layer config (lands v0.2); `--exclude` flag (lands v0.2 with
    allowlist); `markdown` / `html` / `sarif` reporters (lands v0.2 /
    v0.3 / v0.4 per roadmap); `--explain` subcommand (lands at v0.3+).
  - **Comparison engine extension — `compare_with_paths`**: new public
    entry point taking parallel `forms: &[NormalizedForm]` +
    `paths: &[FilePath]` slices so emitted `Match.forms[].file` carries
    real source paths instead of the `qualified_name`-derived stub the
    library-facing `compare()` falls back to. The CLI run loop tracks
    `(form, path)` pairs during normalization and threads both. Object-
    safe `PathResolver` trait (synthetic vs indexed strategies) keeps
    the engine's two passes parameterized without forcing a type
    parameter on every helper. Closes the gap in `form_ref_for`'s
    pre-PR-8 docstring ("PR 8's run loop wires real paths at the higher
    layer"). The legacy `compare()` keeps the synthetic resolver for
    backward compat with existing comparison-engine unit tests.

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

- **#50 — MSRV-gate SHA was actually @stable.** `ci.yml`'s MSRV job
  pinned `dtolnay/rust-toolchain@29eef336…` with a `# tracks @1.85
  branch` comment, but `29eef336` is actually the `@stable` branch
  HEAD ("toolchain: stable") — the real `@1.85` HEAD is `c56a35af…`
  ("toolchain: 1.85.1"). The MSRV gate was structurally vacuous,
  exercising whatever `@stable` resolved to instead of pinning to
  the 1.85 toolchain. Corrected the SHA in the MSRV job; the other
  six `dtolnay/rust-toolchain@29eef336…` pins are legitimately
  tracking `@stable` and unchanged. Surfaced by the #32 SHA-pin-sweep
  agent during PR #49 (in audit-only scope there; tracked here as
  a focused fix). Branch-pinned actions need per-branch SHA
  verification — dependabot's grouped bumps can't distinguish
  trailing-comment intent from the underlying ref.
- **#26 current-state audit** — two AGENTS.md per-crate dep-table
  drifts caught by the bot-context-drift lint on first run, fixed in
  the same PR:
  - **`dry-core` allowed-deps row** — removed `regex` (aspirational
    entry; never added to `crates/dry-core/Cargo.toml [dependencies]`).
    The trailing prose enumeration in the "thiserror IS allowed in
    dry-core" rule (under "For automated code reviewers") drops
    `regex` in lockstep.
  - **`dry4rs` allowed-deps row** — removed `quote` (aspirational
    entry from the pre-PR-5 bootstrap; never added to
    `crates/dry4rs/Cargo.toml [dependencies]`). The AST-purity ban
    list still rejects `quote` in `dry-core` (where it belongs as a
    procedural-macro AST helper); the row removal documents that
    `dry4rs` itself doesn't yet consume it. Either dep can be added
    cleanly later — re-adding to the table will trip the
    bot-context-drift gate until Cargo.toml matches.

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

- **PR 8 (#9) bot-review follow-up** — five findings on PR #34
  (CodeRabbit + Gemini):
  - **`clap_complete` in `dry-core` allowed-deps table** — CodeRabbit
    caught that the workspace `clap_complete` dependency landed in
    `crates/dry-core/Cargo.toml` without an AGENTS.md per-crate
    dep-table entry. Amended the table to add `clap_complete`
    alongside `clap` (derive) — both belong to the v0.1 CLI surface.
  - **`compare_with_paths` length check elevated to release builds**
    — the docstring promised "Panics (debug only)" via
    `debug_assert_eq!`, but `IndexedPathResolver::path_for` indexes
    `paths[i]` unconditionally — release builds panicked with a
    cryptic `index out of bounds` from deep inside the engine.
    Promoted the check to `assert_eq!` so the contract panics in both
    builds with the argument lengths in the message.
    `compare_with_paths_panics_in_debug_on_length_mismatch` test
    renamed to drop the "_in_debug" suffix.
  - **`Args::analysis_paths()` returns empty for non-analysis
    subcommands** — `ignore` / `ignored` / `cleanup` previously
    fell through to `vec![PathBuf::from(".")]` if `analysis_paths()`
    was called (defensive callers might walk the cwd by mistake).
    Added `Command::is_analysis()` predicate and updated
    `analysis_paths()` to return `Vec::new()` when the subcommand is
    non-analysis. Regression test:
    `analysis_paths_returns_empty_for_non_analysis_subcommands`.
  - **JSON-envelope tests use key-based assertions** —
    `cli_pipeline.rs` previously used `stdout.contains("\"view\"")`,
    which would false-positive on any string containing `"view"`.
    Switched both assertions to `serde_json::Value::get("view")`.
  - **Stale doc path in `binary_smoke.rs`** — module doc pointed at
    `crates/dry-core/tests/cli_pipeline.rs`; the tests live in
    `crates/dry4rs/tests/cli_pipeline.rs`.

  Gemini's MEDIUM-priority observation about `fs::read_to_string`
  being a potential bottleneck on extremely large files is filed as
  a separate `priority:later` issue rather than addressed in this PR
  — the suggestion itself frames it as a v0.2+ deferral.

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
- New CI job `bdd-tracked-lint` (closes #22): mechanical enforcement
  of `~/.claude/rules/exclusions.md` — every test/coverage suppression
  (`#[ignore]`, `#[cfg(skip_*)]`, `it.skip(...)`, `--exclude` flag,
  `exclude`/`skip` config-array assignment, `TODO: re-enable` /
  `FIXME: skip` comment marker, `if: false` workflow hard-disable)
  must carry a `tracked: <repo>#<n>` or `adr: <path>` reference in an
  adjacent comment (window: 3 lines above, 1 line below). Single
  source of truth in `scripts/tracked-lint.py`; new isolated workflow
  file `.github/workflows/bdd-tracked-lint.yml` and a `tracked-lint`
  pre-push hook in `lefthook.yml` both invoke it. Workflow name
  preserved as `bdd-tracked-lint` for cross-tool sibling consistency
  with crap4rs's job (where the scope is narrower — BDD feature files
  only). Current-state audit: zero violations across 43 scanned files.
  Known scope gaps documented in the script's module docstring: soft-
  disable `if:` expressions (precise classifier requires GHA
  expression-language evaluation) and commented-out test bodies
  without TODO/FIXME markers (no mechanical signal in the rule).

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
- **#42 — `crap-self` flipped to a real production-code CC ladder
  gate.** Refactored the 7 production-code functions above the
  default cognitive threshold of 15 surfaced by the PR 9 measurement-
  only baseline; covered the 8th hotspot (`emit_stats` — already
  CC < 15 but CRAP 17.90 from 65.5% coverage) with five new
  in-process unit tests against a stub `NormalizerPort`, dropping
  CRAP to 12.00 at 100% line coverage. Post-refactor crap4rs probe:
  `429 functions | 0 above threshold (15) | worst: 12.0 | PASS`.
  Behaviour is byte-identical pre- vs post-refactor (wire-envelope
  insta snapshot, comparison-engine unit + property tests, walker
  integration tests, CLI pipeline integration tests all green); the
  self-check summary snapshot updates from `total_forms: 526` to
  `total_forms: 604` (additive new helper functions + extra test
  helpers; `matches_count` rises 43 → 46 from incidental structural
  overlap between the new walker per-variant helpers, with
  `by_tier.auto_refactor` 5 → 6 and `by_tier.review_first` 38 → 40).
  Refactor approach per function:
  - **`pass2_sliding_window`** (CRAP 15.19 → 8.00): extracted
    `sort_unclaimed_by_node_count`, `try_emit_pass2_match`, and
    `resolve_pass2_score` so the engine reads as a small driver
    over three named operations.
  - **`pass1_hash_bucket`** (CRAP 18.00): extracted
    `group_forms_by_bucket_key`, `emit_clusters_for_bucket`, and
    `emit_pass1_cluster` to lift the bucket-iteration loop out of
    the dispatch.
  - **`enumerate`** (CRAP 28.96): extracted `build_walker`,
    `collect_matching_file`, `extension_is_allowed`, and
    `unreadable_warning` to separate walker construction from the
    per-entry classification.
  - **`Walker::visit_item`** (CRAP 21.01): per-variant extraction
    (`visit_mod_item`, `visit_impl_item`, `visit_trait_item`) per
    the canonical Rust AST-visitor pattern.
  - **`FormEmitter::hash_pat`** (CRAP 16.43): per-variant extraction
    (`hash_pat_ident`, `hash_pat_path`, `hash_pat_lit`,
    `hash_pat_seq`, `hash_pat_tuple_struct`, `hash_pat_struct`,
    `hash_pat_reference`, `hash_pat_type`) with the
    `Tuple`/`Slice`/`Or` arms collapsed onto a generic
    `hash_pat_seq` helper over the punctuation token type.
  - **`FormEmitter::hash_expr`** (CRAP 30.02, the highest hotspot):
    category-grouped dispatch via `hash_expr_dispatch` →
    `hash_expr_value` / `hash_expr_operator` / `hash_expr_call_like`
    / `hash_expr_control` / `hash_expr_collection` / `hash_expr_wrap`
    / `hash_expr_block_like`, with each of the ~28 covered `syn::Expr`
    variants getting its own small helper. The Tuple/Array arms
    share a generic `hash_expr_seq` helper over the comma-punctuated
    sub-expression sequences.
  Q1 (resolution): the `arb_report` proptest helper in
  `crates/dry-core/tests/adapters_proptest.rs` (CRAP 42.00 from 0%
  coverage as test code) is now excluded via a new repo-root
  `crap4rs.toml` that the composite action passes to crap4rs as
  `--config`. Production-code gates are scoped to production code;
  the test-code structural-smell ladder is scrap-rs's domain
  (`scrap-self` wire-up tracked: dry-rs#40).
  Q2 (resolution): `Walker::visit_item` extraction dropped CRAP
  from 21.01 to ≤ 7.00 (`Walker::visit_trait_item`'s 7.00 is the
  worst surviving leg, well under threshold). The
  ADR-allowlist escape hatch was not needed.
  - **`.github/workflows/self-test.yml`**:
    `analysis-gate: false` → `analysis-gate: true`, paired with
    `config: crap4rs.toml`. Step rename: "(production-code dogfood,
    measurement-only)" → "(production-code CC ladder)".
  - **`crap4rs.toml`** (new, repo root): `exclude = ["**/tests/**"]`
    with rationale + Q1 cross-reference documented inline.

### Security

- **Issue #32 — SHA-pin sweep for self-test / mutants / scorecard.**
  Brings the workflows + composite action added after PR #38 up to the
  same SHA-pin discipline by converging on the LATEST pins already
  enforced in `ci.yml`. Prepares dry-rs for the org-wide "Require
  actions to be pinned to a full-length commit SHA" setting at
  https://github.com/organizations/breezy-bays-labs/settings/actions
  (deferred until all sibling repos close their SHA-pin issues).
  - **`.github/workflows/self-test.yml`** — 2× `actions/checkout`
    pins lifted `34e11487 → de0fac2e` (v4 → v6.0.2). Other refs
    (`dtolnay/rust-toolchain`, `Swatinem/rust-cache`,
    `taiki-e/install-action`) already matched `ci.yml`.
  - **`.github/workflows/mutants.yml`** — `actions/checkout` lifted
    `34e11487 → de0fac2e` (v4 → v6.0.2); `actions/upload-artifact`
    lifted `ea165f8d → 043fb46d` (v4 → v7.0.1).
  - **`.github/actions/scorecard/action.yml`** — `actions/checkout`
    lifted `34e11487 → de0fac2e` (v4 → v6.0.2). The
    `dtolnay/rust-toolchain` + `Swatinem/rust-cache` pins were already
    current.
  - **Dependabot** — `.github/dependabot.yml` already configured for
    the `github-actions` ecosystem (PR #38). Future bumps land
    automatically across all 5 workflow + composite-action files in
    grouped weekly PRs.
  - **Verification**: `pipx run 'zizmor>=1.5,<2' .github/` reports
    "No findings to report" — zero `unpinned-uses` audit findings.
  - Pre-existing on main, NOT in this PR's scope:
    `.github/workflows/ci.yml`'s `dtolnay/rust-toolchain@1.85`
    SHA-pin `29eef336` is actually the `@stable` HEAD commit
    ("toolchain: stable"); the real `@1.85` branch HEAD is `c56a35af`
    ("toolchain: 1.85.1"). The MSRV-gate job consequently exercises
    @stable, not @1.85. Surfaced here so the orchestrator can file
    a follow-up `ci(msrv)` fix.

- **Issue #16 — zizmor + dependabot supply-chain hardening.** Brings
  dry-rs to the org-wide supply-chain bar established by crap4rs#264
  and scrap-rs#38.
  - **`.github/workflows/ci.yml` hardened**: workflow-level
    `permissions: contents: read` (least-privilege default; closes
    workflow-wide `excessive-permissions` audit leg);
    `persist-credentials: false` on every `actions/checkout` step (CI
    never pushes from this workflow); all third-party actions
    SHA-pinned with tag comments preserved for refresh discoverability
    (`actions/checkout@34e1148...`, `actions/upload-artifact@ea165f8...`,
    `dtolnay/rust-toolchain` stable + 1.85 branch SHAs,
    `Swatinem/rust-cache@e18b497...`, `taiki-e/install-action@d9be7d8...`).
    `EmbarkStudios/cargo-deny-action` was already SHA-pinned.
  - **`.github/workflows/zizmor.yml` new** — dedicated supply-chain
    audit workflow running `pipx run 'zizmor>=1.5,<2' .github/` on
    every workflow / composite action / dependabot file change
    (separate workflow rather than a job in ci.yml per parallel-agent
    dispatch convention). Includes a top-of-file future-rule comment
    documenting the release.yml cache-poisoning constraint that
    activates at per-crate v1.0 prep (tag-triggered jobs MUST NOT
    restore or save build caches).
  - **`.github/dependabot.yml` new** — weekly Monday polls on both
    `cargo` and `github-actions` ecosystems. Grouped PRs (minor +
    patch into one weekly PR per ecosystem) keep reviewer load
    proportional to delta volume; majors land as separate PRs for
    human review. Semver-aware cooldown on cargo (default: 7d,
    major: 14d, minor: 7d, patch: 7d). `npm` ecosystem deliberately
    deferred to v0.6+ when dry4ts joins the workspace with a
    `napi-rs` Node binding.
  - **Local verification**: `pipx run 'zizmor>=1.5,<2' .github/`
    reports zero findings across `ci.yml`, `zizmor.yml`,
    `dependabot.yml`. The zizmor gate caught one `dependabot-cooldown`
    finding on first attempt (missing `default-days` alongside
    semver-aware keys) — exactly the kind of regression the gate is
    designed to surface.
