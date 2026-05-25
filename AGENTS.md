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
| `dry-core` | Domain types, port traits, comparison engine, generic CLI surface, language-agnostic adapters (file walker, reporters) | `serde` (derive), `serde_json`, `walkdir`, `ignore`, `globset`, `comfy-table`, `clap` (derive), `clap_complete`, `thiserror` |
| `dry4rs` | Rust-source parser adapter + binary | `dry-core`, `syn`, `proc-macro2` (with `span-locations` feature), `xxhash-rust` (with `xxh3` feature) |
| `dry4ts` | TypeScript-source parser adapter + binary | `dry-core`, `swc_ecma_parser` *or* `oxc_parser`, `napi-rs`, `xxhash-rust` (with `xxh3` feature) |

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

The comparison engine (lands in PR 6) has two passes:

1. **Hash-bucket pass** — clusters forms by `fingerprint_set` hash for
   exact-match detection in O(N).
2. **Sliding-window Jaccard pass** — sorts forms ascending by
   `node_count`, runs Jaccard over the window. Inner loop breaks when
   `forms[j].node_count > forms[i].node_count / threshold` (Jaccard
   upper bound `min/max >= t` ⟹ `max <= min/t`).

The threshold tier vocabulary is fixed at v0.1: `auto_refactor`
(>= 0.95) / `review_first` (>= 0.85) / `advisory` (>= threshold).

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
  `walkdir`, `ignore`, `globset`, and `comfy-table` are explicitly
  permitted in `dry-core`.

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
    #[serde(default)] pub structural_score: Option<f64>,
    #[serde(default)] pub rename_count: Option<u32>,
    #[serde(default)] pub rename_density: Option<f64>,
    pub tier: Tier,
}
```

- **`score: f64`** is a primitive on the wire. Do NOT suggest replacing
  with the `Score` newtype. `Score` is the input-validation gate at
  comparison-engine boundaries; `Match` carries the raw wire value.
- The three reserved score slots use `#[serde(default)]` ONLY, never
  `#[serde(skip_serializing_if = "Option::is_none")]`. The v0.1
  contract requires `null`, not omission.
- **`NormalizedForm.fingerprint_set: HashSet<u64>`** is intentionally
  raw, not wrapped in `Fingerprint`. The newtype's role is reporter-
  side identity (zero-padded hex display); the set is hot-path input
  to Jaccard intersection. Newtypes are zero-cost so the choice is
  ergonomic, not performance-driven — but it IS deliberate.

### `#[non_exhaustive]` discipline — enums YES, structs NO

- Every public **enum** in `dry-core::domain` carries `#[non_exhaustive]`
  (`Tier`, `Severity`, `FormKind`, `NormalizeError`, `SpanError`,
  `ScoreError`, future `ThresholdMode`, `OutputFormat`).
- Public **result structs** (`Match`, `Score`, `Span`, `Fingerprint`,
  `Report`, `Summary`, `NormalizedForm`, `FormRef`) do NOT carry
  `#[non_exhaustive]`. They evolve via constructor pattern
  (`Foo::new`, `Foo::try_new`, `Foo::default`) and serde versioning
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
drift from the source ADRs. If a CI job named `bot-context-drift`
exists (future) it will catch drift mechanically; until then, an
inconsistency between this section and the ADRs is a bug — the ADRs
win, and this section needs updating. See
[dry-rs#24](https://github.com/breezy-bays-labs/dry-rs/issues/24) for
the implementation epic.

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
