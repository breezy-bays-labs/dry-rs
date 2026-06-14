# dry-rs Agent Notes

Cross-provider agent operating guide. Both Claude Code and Codex
should read this before touching code.

## Repo identity

- `dry-rs` is a Rust **workspace** in `breezy-bays-labs` org. Public
  visibility from day one; **no crates.io publish, no GitHub Release
  tarballs, no `cargo install` path** until per-crate v1.0 gates trip.
  Tags exist for git pinning only — internal versions like `v0.1.0` do
  not trigger any workflow.
- The release gate is **decoupled per-crate**: `dry4rs` v1.0 ships
  when CLI/JSON contract stabilizes; `dry-core` graduates to v1.0
  only when both `dry4rs` and `dry4ts` consume it. Until those gates
  trip, mokumo is the sole consumer via composite GitHub Action.

## Architecture

Hexagonal (ports & adapters), strict dependency direction enforced by
Cargo crate boundaries. The hexagonal-layering ADR (filed in PR 2 at
`ops/decisions/dry-rs/adr-hexagonal-layout.md`) carries the full
layering invariant.

```
dry-core (no AST libs)
    ^
dry4rs (depends on dry-core; adds syn, proc-macro2, quote)
    ^
dry4ts (depends on dry-core; adds swc_ecma_parser or oxc, napi-rs)  [v0.6+]
```

| Crate | Purpose | Allowed deps |
|-------|---------|--------------|
| `dry-core` | Domain types, port traits, comparison engine, generic CLI surface, language-agnostic adapters (file walker, reporters), config-file loader, `init`-time annotated example + JSON schema emitter | `serde` (derive), `serde_json`, `walkdir`, `ignore`, `globset`, `comfy-table`, `askama` (Template derive, markdown/HTML reporters), `base64` (HTML reporter `#dry-data` island), `clap` (derive), `clap_complete`, `thiserror`, `toml`, `toml_edit`, `documented`, `schemars` |
| `dry4rs` | Rust-source parser adapter + binary | `dry-core`, `syn`, `proc-macro2` (with `span-locations` feature), `xxhash-rust` (with `xxh3` feature) |
| `dry4ts` | TypeScript-source parser adapter + binary | `dry-core`, `swc_ecma_parser` *or* `oxc_parser`, `napi-rs`, `xxhash-rust` (with `xxh3` feature) |
| `dry-examples` | Curated DRY-violation corpus + cross-tool benchmark harness (no library logic; fixtures under `examples/<tier>/<fixture>/main.rs` + snapshot harness in `tests/snapshots.rs`; `publish = false`, `autoexamples = false`) | (none) |

**Never import inward.** `dry-core` must stay free of AST libraries
(`syn`, `swc_*`, `oxc_*`, `tree-sitter*`, `proc-macro2`, `quote`).
Enforcement: structural (`dry-core/Cargo.toml` does not list them) +
source-level (`dry-core AST-library purity` CI job rejects matching
`use` lines in `crates/dry-core/src/`).

## Working rules

- **TDD** — tests before implementation for all domain and adapter code.
- **Domain purity** — `crates/dry-core/src/domain/` must never import
  external crates (other than `serde` derive) or perform I/O.
- **Self-referential test** — once the comparison engine lands, dry4rs
  must analyze its own source as an integration test (the
  `self-check` CI job, lands with PR 9).
- **Symmetric dogfood** — dry4rs's CI also runs `crap4rs` and
  `scrap4rs` against its own production code (`crap-self` /
  `scrap-self` jobs, gated on the production-code CC ladder).
- **`explore` is a dev tool, never a gate** (dry-rs#151) — the
  `explore` subcommand runs the SAME analysis as `report`, renders the
  `Mode::Explore` HTML explorer to `std::env::temp_dir()/dry-explore-
  <tool_name>.html`, prints the path, opens it (`$BROWSER` → `open` →
  `xdg-open`), and ALWAYS exits 0 (no `--no-fail` / `result.passed`
  derivation). The browser launch is suppressed by `--no-open` OR the
  `$DRY_NO_OPEN` env escape so CI / tests never spawn a browser; the
  temp file is written REGARDLESS. Per dry-rs#114 the EXPLORE scope
  change model is a binary re-run (re-invoke `explore` with new scope
  flags) — there is NO precomputed runtime toggle; the frontend
  auto-focuses the hottest cluster (cardinality × score × span-lines)
  client-side. Do NOT suggest precomputing scope combinations or
  removing the `--no-open` / `$DRY_NO_OPEN` gate.
- **No release workflow during v0.x** — `release.yml` arrives at
  per-crate v1.0 prep. Tags are git-pinning markers only.
- **No `tools.toml` Warden pin** in mokumo during v0.x — mokumo
  consumes dry-rs via composite action ref (`@v0.x.0`); the action
  self-builds dry4rs from the ref.
- **No direct push to main** — branch + PR for all work after the
  initial bootstrap commit.
- **Worktrees** for parallel work: `git worktree add ../dry-rs-issue-N -b feat/topic-name`.
- **Property tests required** for the Jaccard score formula and the
  sliding-window break math invariant.
- **Regression files committed** — any `proptest-regressions/` dirs
  go into git, never gitignored. Commit the regression file + fix in
  the same PR.

## Comparison algorithm contract

The comparison engine has three stages:

1. **Hash-bucket pass** — clusters forms by `fingerprint_set` hash for
   exact-match detection in O(N); emits n-ary matches (score 1.0).
2. **Sliding-window Jaccard pass** — sorts forms ascending by
   `node_count`, runs Jaccard over the window. Inner loop breaks when
   `forms[j].node_count > forms[i].node_count / threshold` (Jaccard
   upper bound `min/max >= t` ⟹ `max <= min/t`). Collects the
   `>= threshold` pairwise edges.
3. **Clique carving** (dry-rs#97; semantics pinned in the ops ADR
   `adr-cluster-output.md`) — carves the edge graph into maximal
   cliques per connected component (prefer-larger greedy). Every
   intra-cluster pair carries a COMPUTED Jaccard `>= threshold`; a
   missing edge blocks membership and is never fabricated as 0.0.
   Cluster score = the minimum intra-clique pair score (generalizes
   the Pass 1 score-1.0-as-group-min precedent); tier routes by the
   weakest pair. **Edge conservation**: every collected edge is
   represented exactly once — absorbed in a clique or emitted as a
   residual binary match; a form may appear in multiple matches.
   Determinism derives from form identity `(file, span)` +
   `f64::total_cmp`, never input indices or `HashSet` iteration;
   cluster membership is stable across walker orderings. Components
   above `CLUSTER_COMPONENT_CAP` (512) fall back to pairwise
   passthrough. Do NOT suggest connected-component (transitive
   closure) grouping — chaining hands agents clusters containing
   below-threshold pairs; do NOT suggest a separate cluster wire
   type — the locked `Match.forms: Vec<FormRef>` is the N-ary shape.

The threshold tier vocabulary is fixed: `auto_refactor` (>= 0.95) /
`review_first` (>= 0.85) / `advisory` (>= threshold).

## Cross-language node-counting heuristic

`NormalizedForm.node_count` drives the sliding-window break. The O8
ADR (NormalizedForm cross-language schema, filed in PR 4) establishes
a normalized counting heuristic that all adapters honor — if syn
(Rust) counts every punctuation mark while swc/oxc (TS) groups them,
a 0.8 threshold feels wildly different across languages. v0.1
establishes the heuristic for syn; dry4ts honors it when it joins.
This is folded into the O8 ADR per the roadmap (originally surfaced as
O12, the Gemini callout 2026-05-09).

## For automated code reviewers

> Stable section header. AI code-review bots (`gemini-code-assist`,
> `coderabbitai`, future tools) read this section to ground their
> suggestions in the project's load-bearing decisions. The rules below
> mirror conclusions from ADRs in the private `ops` vault. See also
> [`.coderabbit.yaml`](./.coderabbit.yaml) and
> [`.gemini/styleguide.md`](./.gemini/styleguide.md) for bot-specific
> configuration.

### Allowed dependencies per crate

The per-crate dep table at the top of this file (under "Architecture")
is **authoritative**. Do NOT suggest removing any crate listed there.
In particular:

- **`thiserror` IS allowed in `dry-core`**. Suggestions to replace
  `#[derive(thiserror::Error)]` with manual `Display` + `Error` impls
  on grounds of "domain purity" are based on a misread of this
  project's rules. The "domain purity" rule scopes only to AST
  libraries (see below); `thiserror`, `serde`, `serde_json`, `clap`,
  `walkdir`, `ignore`, `globset`, `comfy-table`, `askama`, `base64`,
  `toml`, `toml_edit`, `documented`, and `schemars` are explicitly
  permitted in `dry-core`.
- **`toml` IS allowed in `dry-core::adapters::config`** (config-file
  loader landed in dry-rs#71 per
  `ops/decisions/org/adr-config-file-pattern.md` D6). Suggestions to
  move the loader out of `dry-core` or to drop `toml` for a
  hand-rolled parser misread the cross-tool ADR. The
  `dry-core::domain::config` POD types stay parser-agnostic and use
  `serde` derives only; the loader uses `toml::from_str` +
  `toml::to_string_pretty` (the latter for the round-trip property
  test).
- **`askama` IS allowed in `dry-core`** (markdown reporter landed in
  dry-rs#91; HTML reporter landed in dry-rs#147, epic #111). The
  `#[derive(Template)]` macro lives on reporter-side view structs in
  `dry-core::adapters::reporters::markdown` /
  `dry-core::adapters::reporters::html`, NOT in `domain/`. askama
  is a compile-time templating library — it generates rendering code
  from `.md` / `.html` templates under `crates/dry-core/templates/`,
  type-checked against the struct fields. It is in the proc-macro
  chain allowlist (the Template derive), NOT an AST library, so the
  `dry-core AST-library purity` gate does not reject it (the gate
  greps only `syn`, `quote`, `proc-macro2`, `swc_*`, `oxc_*`,
  `tree-sitter*`, `rustc_ast`, `rustc_parse`). Suggestions to
  hand-roll the markdown with `format!` / `write!` instead misread
  the sibling-coherence precedent (crap4rs#260 ships its HTML/markdown
  reporters through askama). Mirrors crap4rs's `askama = "0.16"` pin.
- **`schemars` IS allowed in `dry-core`** (JSON schema emitter
  landed in dry-rs#78). `#[derive(JsonSchema)]` lives on the same
  POD config types in `dry-core::domain::config` alongside the
  existing `Serialize` / `Deserialize` / `DocumentedFields` derives;
  the schema emitter (`adapters::config_schema_gen`) consumes
  `schema_for!(Config)`. Single source of truth = the annotated
  `Config` struct; one edit propagates to docs.rs, `dry.example.toml`,
  AND `dry.schema.json`. A byte-identical sync test
  (`crates/dry4rs/tests/dry_schema_sync.rs`) keeps the committed
  schema aligned with the live emitter output.

### AST-purity scope

The "no inward import" rule in this file's Architecture section
applies to **AST libraries only**, not all external crates. The
rejected import set is exactly:

```
syn, quote, proc-macro2, swc_*, oxc_*, tree-sitter*, rustc_ast, rustc_parse
```

Any other external crate from the allowed-deps table is fair game in
`dry-core`. The CI job that enforces this is named
`dry-core AST-library purity`; it greps the rejected set only.

### Locked wire shapes (do NOT suggest changes)

The JSON wire envelope is locked at v0.1. Multi-score `Match` shape:

```rust
pub struct Match {
    pub forms: Vec<FormRef>,
    pub score: f64,
    #[serde(default)] pub structural_score: Option<f64>,           // reserved-then-derived
    #[serde(default)] pub rename_count: Option<u32>,               // reserved-then-derived
    #[serde(default)] pub rename_density: Option<f64>,             // reserved-then-derived
    pub tier: Tier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Template>,                                // additive (dry-rs#132)
}
```

- **`score: f64`** is a primitive on the wire. Do NOT suggest replacing
  with the `Score` newtype. `Score` is the input-validation gate at
  comparison-engine boundaries; `Match` carries the raw wire value.
- The three reserved score slots use bare `#[serde(default)]` ONLY,
  never `#[serde(skip_serializing_if = "Option::is_none")]`. The wire
  contract requires `null`, not omission. **They are now "reserved-
  then-derived" (dry-rs#132), NOT "null at v0.1":** they emit `null`
  until a `template` is attached, then `Match::with_template` DERIVES
  them from the template's holes (`rename_count` = count of pure-rename
  holes; `rename_density` = `rename_count / total_holes`;
  `structural_score` = `score` lifted toward 1.0 by the pure-rename
  fraction, always `>= score`). A populated slot is therefore EXPECTED,
  not a violation — do NOT flag a non-null reserved slot.
- **`Match.template: Option<Template>` (dry-rs#132)** is the additive
  anti-unification field, appended at the END (after `tier`). It uses
  `#[serde(default, skip_serializing_if = "Option::is_none")]` so it is
  **OMITTED** when `None` — this keeps the v0.1 wire snapshot
  byte-identical when the feature is off. **This OMISSION is the
  OPPOSITE of the reserved-slot null rule above, and is DELIBERATE.** A
  code-review bot may "helpfully" suggest normalizing one to the other
  (drop `skip_serializing_if` on `template` to match the reserved slots,
  or add it to the reserved slots to match `template`) — reject BOTH
  suggestions. The reserved slots emit `null`; `template` is omitted.
  The two serde shapes coexist on one struct by design (see the
  `feedback_bot-suggestion-contract-stricting` memory). `Template` and
  its component POD types (`TemplateNode`, `HoleId`, `Hole`, `HoleKind`,
  `Substitution`, `SubElement`, `Divergence`, `DistinctValue`) live in
  `dry-core::domain::template` (so `domain::Match` can carry a
  `Template` without `domain` depending on the `comparison` engine that
  fills it); the LGG algorithm stays in `comparison::antiunify`, which
  re-exports the types so `comparison::Template` etc. keep resolving.
- **`NormalizedForm.fingerprint_set: HashSet<u64>`** is intentionally
  raw, not wrapped in `Fingerprint`. The newtype's role is reporter-
  side identity (zero-padded hex display); the set is hot-path input
  to Jaccard intersection. Newtypes are zero-cost so the choice is
  ergonomic, not performance-driven — but it IS deliberate.
- **`Span { start: LineColumn, end: LineColumn }`** and
  **`LineColumn { line: u32, column: u32 }`** carry the canonical
  source-position contract. Locked at v0.1 per the
  `adr-span-coordinate-semantics.md` council verdict (2026-05-26):
  - **Line is 1-based**, mirrors `proc_macro2::LineColumn::line`.
  - **Column is 0-based** on the wire and in the domain, mirrors
    `proc_macro2::LineColumn::column`. The text reporter and the
    GitHub annotations reporter convert column to 1-based via
    `saturating_add(1)` at the boundary; do NOT suggest unifying
    these surfaces — the per-surface split is deliberate and
    documented.
  - **End-inclusive on BOTH line and column.** A one-character token
    has `start == end`. This diverges from rustc diagnostic JSON
    (inclusive line, exclusive column); see the ADR for the
    multi-consumer rationale. Do NOT suggest matching rustc.
  - **No `byte_offset` field at v0.1.** The future amendment
    (deferred to the first real consumer — v0.4 SARIF reporter
    likely) lands `byte_offset: Option<usize>` on `LineColumn` (NOT
    `byte_range` on `Span`), with a `Span::with_byte_range(start,
    end, range)` constructor helper absorbing the `-1` adjustment
    from proc-macro2's half-open `byte_range()`. Do NOT pre-add the
    field at v0.1.
  - **Field order on `LineColumn` is load-bearing**: derived
    `PartialOrd` produces lexicographic ordering on `(line, column)`;
    `Span::try_new`'s inverted-range validation depends on it.
    Reordering or renaming silently breaks the validator.
- **`LanguageConfig` (dry-rs#78) carries per-language overrides** for
  `[rust]` / `[typescript]` in `dry.toml`:

  ```rust
  pub struct LanguageConfig {
      #[serde(skip_serializing_if = "Option::is_none")]
      pub threshold: Option<f64>,
      #[serde(skip_serializing_if = "Option::is_none")]
      pub threshold_mode: Option<ThresholdMode>,
      #[serde(skip_serializing_if = "Option::is_none")]
      pub format: Option<Format>,
      #[serde(skip_serializing_if = "Option::is_none")]
      pub title: Option<String>,
      #[serde(skip_serializing_if = "Option::is_none")]
      pub subtitle: Option<String>,
      #[serde(skip_serializing_if = "Option::is_none")]
      pub include_ignored: Option<bool>,
      #[serde(skip_serializing_if = "Option::is_none")]
      pub extensions: Option<Vec<String>>,
  }
  ```

  Cascade rule (locked at v0.1): per-language `Some(v)` shadows
  shared `[gate]`/`[output]`/`[walk]` `Some(v)`; per-language `None`
  falls back to the shared value; both `None` resolves `None` and
  the next precedence tier applies (`AdapterMeta` default →
  compiled-in fallback). Resolved by
  `dry_core::cli::EffectiveConfig::resolve(&Config, &AdapterMeta)`
  — exhaustive destructure on BOTH `LanguageConfig` AND every shared
  section struct is the compile-time guard against adding a knob to
  one side and not the other.
- **`Config.rust` / `Config.typescript`** use
  `#[serde(skip_serializing_if = "LanguageConfig::is_default")]`
  so empty tables omit from re-serialized TOML output. Do NOT
  suggest replacing with `Option<LanguageConfig>` — the default-
  struct pattern matches the shared sections (`[gate]`, `[output]`,
  `[walk]`) for consistency.
- **`OutputConfig.title` / `OutputConfig.subtitle`** (dry-rs#78)
  are `Option<String>` with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`.
  Scorecard labels rendered by external consumers (e.g., the
  dry-scorecard GitHub Action's sticky PR-comment header); replaces
  the consumer-side `comment-preamble` action input. The same
  fields appear on the wire envelope as `Envelope.title` /
  `Envelope.subtitle` (also `Option<String>`, skip_serializing_if
  Option::is_none, declared at the END of the struct to keep the
  v0.1 snapshot byte-identical when unset).
- **`AdapterMeta.language: Language`** (dry-rs#78) is a typed
  runtime enum that selects which `[rust]` / `[typescript]` section
  of the unified config the adapter reads. `#[non_exhaustive]`,
  not serialized on the wire, not a clap value enum. Decoupled from
  `display_name` (human-readable text) — `language` is matched on
  by `EffectiveConfig::resolve` without string compares.
- **`Envelope.scope: Option<ScopeApplied>` (dry-rs#124)** is the
  relatedness-scoping echo, appended at the END of `Envelope` (after
  `subtitle`) with `#[serde(skip_serializing_if = "Option::is_none")]`
  so the v0.1 snapshot stays byte-identical when the run loop does not
  populate it (the library-facing `Envelope::new` constructor leaves it
  `None`; the CLI run loop always sets it). `ScopeApplied` is a result
  struct (NO `#[non_exhaustive]`) carrying five `bool`s —
  `within_crate` / `across_crate` / `within_module` / `across_module`
  + the runtime `crate_aware` flag — a flat projection of
  `domain::ResolvedScope`. `clippy::struct_excessive_bools` is allowed
  (orthogonal axes, the user's mental model, NOT a bitflag candidate —
  same rationale as `ResolvedScope`). Do NOT suggest collapsing the
  five bools into a bitflag/enum, and do NOT suggest dropping
  `skip_serializing_if`. The comparison engine consumes the predicate
  through an internal `CompareCtx { resolver, scope }`; the public
  facades `compare()` / `compare_with_paths()` stay the stable entry
  points (default all-true scope = no-op), and the scoped run-loop entry
  is `compare_with_paths_scoped(forms, paths, threshold, scope)`.
- **`Envelope.mode: Option<Mode>` and `Envelope.capabilities:
  Option<Capabilities>` (dry-rs#147, epic #111)** are the HTML-explorer
  presentation hints, appended at the END of `Envelope` (AFTER `scope`),
  EACH `#[serde(default, skip_serializing_if = "Option::is_none")]` so
  they are OMITTED when `None`. Only the HTML reporter populates them;
  JSON / text / markdown leave them `None`, keeping the v0.1 wire snapshot
  byte-identical (the all-additive-fields-None case asserts both omit).
  Do NOT suggest dropping `skip_serializing_if` (that would break the
  byte-identical-when-off lock) and do NOT suggest matching the reserved
  score slots' bare-`#[serde(default)]`-null shape — the omission is
  deliberate, the same coexisting-serde-shapes rule as `template` /
  `scope`. `Mode` is an enum (`Report` / `Explore`, `#[non_exhaustive]`,
  `snake_case` wire tags) — `report --format html` emits `Mode::Report`;
  the `explore` subcommand (dry-rs#151) emits `Mode::Explore`.
  `Capabilities` is a result struct (NO
  `#[non_exhaustive]`; evolves via `Capabilities::report()` /
  `Capabilities::showcase()` / `Capabilities::new(...)`) carrying five
  orthogonal feature flags — `overview` / `clusters` / `substitution_grid`
  / `d_slider` / `scope_banner` — telling the frontend which payload-backed
  views are renderable. **As of the showcase PR (dry-rs#149), the run-loop
  HTML path emits `Capabilities::showcase()` — ALL FIVE flags `true`**
  (the frontend now renders the template skeleton + substitution grid +
  d-slider + scope banner). `Capabilities::report()` (overview + clusters
  only, the three showcase flags `false`) survives as the bare-reporter
  constructor for tests / future callers; do NOT flag a non-false
  `substitution_grid` / `d_slider` / `scope_banner` in the HTML island as
  a regression — the showcase flip is deliberate. The flags are
  capability advertisements ("the reporter CAN render this view"), NOT
  per-cluster guarantees: an exact-dup cluster (null / 0-hole `template`)
  still degrades to the concrete shared form with no grid even when
  `substitution_grid == true`. `clippy::struct_excessive_bools` (struct) +
  `clippy::fn_params_excessive_bools` (the `new` constructor) are allowed
  — orthogonal flags, the frontend's mental model, NOT a bitflag
  candidate (same rationale as `ScopeApplied` / `ResolvedScope`). Do NOT
  suggest collapsing the five bools. `Mode` + `Capabilities` live in
  `dry-core::adapters::reporters::json::envelope` alongside `ScopeApplied`
  (serialization-layer companion types, not domain types). The HTML
  reporter (`dry-core::adapters::reporters::html`) injects the FULL
  serialized envelope ONCE into a `<script id="dry-data"
  type="application/json">…</script>` island, **base64-encoded** (`base64`
  dep): base64's `[A-Za-z0-9+/=]` alphabet has zero HTML-special chars, so
  the island injects with askama's DEFAULT escaping (a no-op) — NO `|safe`,
  no `</script>` break-out possible. Do NOT suggest adding `|safe` to the
  island or hand-rolling a `</`→`<\/` sanitizer — base64 makes both
  unnecessary, and `|safe` would re-open the raw-injection surface. The
  client JS base64-decodes (`atob` + `TextDecoder`, UTF-8-safe) and builds
  ALL DOM via `document.createElement` + `textContent` (a tiny `h(...)`
  helper) — NO `innerHTML`/`outerHTML`/`document.write`, so user-controlled
  values are inert by construction (do NOT suggest reintroducing `innerHTML`
  for brevity). See `ops/decisions/dry-rs/adr-dual-mode-html-reporter.md`.

### `#[non_exhaustive]` discipline — enums YES, structs NO

- Every public **enum** in `dry-core::domain` carries `#[non_exhaustive]`
  (`Tier`, `Severity`, `FormKind`, `NormalizeError`, `SpanError`,
  `ScoreError`, `ThresholdMode`, `Format`, `LeafClass`, plus the
  anti-unification enums `TemplateNode` and `HoleKind` — dry-rs#132).
  The `Language` enum on `dry-core::cli::adapter_meta` (dry-rs#78) also
  carries `#[non_exhaustive]` — new language variants land additively.
  The `Mode` enum on `dry-core::adapters::reporters::json::envelope`
  (dry-rs#147) also carries `#[non_exhaustive]` — new presentation modes
  land additively.
- Public **result structs** (`Match`, `Score`, `Span`, `LineColumn`,
  `Fingerprint`, `Report`, `Summary`, `NormalizedForm`, `FormRef`,
  `AdapterMeta`, `Config`, `GateConfig`, `OutputConfig`, `WalkConfig`,
  `LanguageConfig`, `Envelope`, `ScopeApplied`, `Capabilities`,
  `AnalysisConfig`, `EffectiveConfig`, `NormalizedTree`, `LeafToken`,
  plus the anti-unification result structs `Template`, `Hole`, `HoleId`,
  `Substitution`, `SubElement`, `Divergence`, `DistinctValue` —
  dry-rs#132) do NOT carry `#[non_exhaustive]`. They evolve via constructor pattern (`Foo::new`,
  `Foo::try_new`, `Foo::default`, builder methods) and serde versioning
  (`#[serde(default)]`, `#[serde(rename = ...)]`,
  `#[serde(skip_serializing_if = ...)]`).

Do NOT suggest adding `#[non_exhaustive]` to a result struct, and do
NOT suggest removing it from an enum.

### Rust version + `const fn` rules

The workspace pins `rust-version = "1.85"` (edition 2024) — see
root `Cargo.toml`. Two non-obvious consequences:

- **`const fn` accepts parameters that implement `Drop`** since Rust
  1.61. A `pub const fn new(forms: Vec<FormRef>, ...)` is valid: the
  function may not invoke drop in const context, but moving the
  parameter into a returned struct field is fine. Do NOT suggest
  removing `const` on these constructors.
- **`proc-macro2` MUST carry the `span-locations` feature flag** in
  every adapter that uses it (`dry4rs`, future Rust adapters).
  Without it, `Span::start()` / `Span::end()` silently return
  `LineColumn { line: 0, column: 0 }`. Enforced by the
  `span-locations-check` CI job + lefthook pre-push.
- **`xxhash-rust` with the `xxh3` feature is the fingerprint hash**
  for every adapter normalizer (`dry4rs`, future `dry4ts`). Do NOT
  suggest swapping for `std::hash::DefaultHasher`, `siphasher`,
  `ahash`, `fxhash`, or `blake3` — `xxh3` is the load-bearing choice
  because (a) its `u64` output is cross-toolchain stable per upstream
  contract (the stdlib reserves the right to change `DefaultHasher`'s
  bit pattern in any new toolchain), and (b) cross-version stability
  is required for the v0.3+ `--delta` baseline comparison feature
  (persisted baselines outlive MSRV bumps). All adapters MUST share
  the same hash algorithm because the comparison engine compares
  `HashSet<u64>` set-equality on raw values. `dry-core` itself does
  NOT depend on `xxhash-rust` — hashing happens in adapters; the
  comparison engine consumes pre-computed `HashSet<u64>` and never
  hashes. See `adr-rust-normalization-rules.md` § "Hashing" and
  `adr-hexagonal-layout.md` per-crate dep table footnote [²].

### Config file pattern (cross-tool canonical shape)

dry4rs ships a `dry.toml` auto-discovery + parsing path per the
cross-tool ADR (`ops/decisions/org/adr-config-file-pattern.md`).
Per-tool variances live in
`ops/decisions/dry-rs/adr-dry4rs-config-file.md`. Key invariants
automated code reviewers should NOT challenge:

- **`discover_config(start, file_name)` is adapter-name-agnostic**.
  The loader source + tests contain ZERO double-quoted `"dry.toml"`
  / `"dry4rs"` string literals (layer-4 ast-purity gate enforces
  this via `scripts/check-config-ast-purity.sh`). Do NOT suggest
  inlining the literal "for clarity" — adapter-name plumbing flows
  exclusively through `meta.config_file_name`.
- **`ConfigError` is a typed `thiserror::Error` enum with
  `#[non_exhaustive]`**. Do NOT suggest replacing with
  `anyhow::Result`; `dry-core` stays `anyhow`-free per the
  hexagonal layering ADR.
- **`Config` POD types live in `dry-core::domain::config`**, NOT in
  `adapters/`. The loader (`load_config`, `parse_config`) lives in
  `adapters/`. Do NOT suggest moving the schema types into the
  adapters layer; the domain/adapter split is the load-bearing
  layering invariant.
- **Strict-on-unknown-keys is INTENTIONAL**
  (`#[serde(deny_unknown_fields)]`). Typos surface at parse time
  with a `path:line:key` message; do NOT suggest relaxing this for
  "forward-compat" — additive forward-compat comes from
  `#[serde(default)]` on every field, not from silent fallback.
- **Precedence chain is CLI > config > `AdapterMeta` default >
  compiled-in fallback**. Do NOT suggest reordering. The merger
  (`dry_core::cli::run::merge_effective_inputs`) is the single
  authoritative source; the `Args::config` field is `Option<PathBuf>`
  (None = auto-discovery; Some(p) = explicit-path, missing-is-error).
- **`AdapterMeta` is a struct value passed by `&AdapterMeta`**, NOT
  a trait with associated consts. See memory
  `feedback_rust_trait_vs_struct_for_data`. The crap-rs and scrap-rs
  sibling repos (carrying the `crap4rs` and `scrap4rs` adapter
  binaries respectively) both use the struct-value pattern; the
  approach is correct.

### How to engage substantively

If a suggestion contradicts one of the rules above, the contradiction
is with the ADR (not the code). The right reply is:

- Quote the relevant rule from this section.
- Cite the ADR (`adr-hexagonal-layout.md` for deps + layering;
  `adr-nested-json-envelope.md` for wire shapes;
  `adr-normalized-form-schema.md` for `NormalizedForm` cross-language
  semantics).
- If you believe the ADR is wrong, surface that as a meta-discussion
  in the PR — but do not auto-apply the suggestion in the meantime.

Fair-game suggestions (these the project welcomes):

- Missing doc comments, doc-comment formatting fixes.
- `must_use`, `inline`, `repr(transparent)`, `cold` attribute additions
  with clear rationale.
- Missing `?` on `Result`, unnecessary `clone()`.
- `derive` macros where appropriate (`PartialOrd`/`Ord` for sortable
  refs, `Default` for zero-value types).
- Test coverage gaps, missing edge cases.
- Documentation typos and unclear wording.

### When this section is wrong

This section is curated, not auto-generated. Rules listed here may
drift from the source ADRs. The `bot-context-drift` CI job + pre-push
hook catch AGENTS.md ↔ Cargo.toml dep-table drift mechanically; other
ADR consistency (wire envelope shapes, layering rules, normalization
heuristics) still relies on review. An inconsistency between this
section and the ADRs is a bug — the ADRs win, and this section needs
updating. See
[dry-rs#24](https://github.com/breezy-bays-labs/dry-rs/issues/24) for
the implementation epic.

**Mechanical drift detection** lives in `scripts/bot-context-drift.py`
+ `.github/workflows/bot-context-drift.yml`. As of dry-rs#26, the lint
verifies AGENTS.md's per-crate dep table matches the actual
`Cargo.toml` files; cross-repo ADR drift detection is future work (the
source ADRs live in a private ops vault, and the public CI does not
yet have cross-repo read access). The bidirectional check catches both
missing-in-table (Cargo.toml dep absent from the table) and
extra-in-table (table lists an aspirational dep never landed). Run
locally with `python3 scripts/bot-context-drift.py`; the same script
fires as a pre-push hook (`lefthook.yml`) and as a CI job.

## Cross-references

- **Roadmap**: `ops/workspace/dry-rs/20260508-dry-rs-roadmap/roadmap.md`
  (in the private ops vault — full architecture, wave plan, PR scope,
  open questions queue)
- **Pipeline note**: `ops/pipelines/dry-rs/dry-rs-20260508-roadmap-research.md`
- **Improvements catalog**: `ops/workspace/dry-rs/20260508-dry-rs-roadmap/dry-rs-improvements-catalog.md`
- **CLI harmonization** (cross-tool):
  `ops/workspace/dry-rs/20260508-dry-rs-roadmap/cli-harmonization.md`
- **Sibling — production-code complexity**:
  [crap4rs](https://github.com/breezy-bays-labs/crap4rs)
- **Sibling — test-code structural smells**:
  [scrap-rs](https://github.com/breezy-bays-labs/scrap-rs)
- **Modeled on**: [unclebob/dry4clj](https://github.com/unclebob/dry4clj) (Clojure)
