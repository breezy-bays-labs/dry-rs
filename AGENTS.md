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
| `dry-core` | Domain types, port traits, comparison engine, generic CLI surface, language-agnostic adapters (file walker, reporters) | `serde` (derive), `serde_json`, `walkdir`, `ignore`, `globset`, `comfy-table`, `clap` (derive), `thiserror` |
| `dry4rs` | Rust-source parser adapter + binary | `dry-core`, `syn`, `proc-macro2` (with `span-locations` feature), `quote` |
| `dry4ts` | TypeScript-source parser adapter + binary | `dry-core`, `swc_ecma_parser` *or* `oxc_parser`, `napi-rs` |

**Never import inward.** `dry-core` must stay free of AST libraries
(`syn`, `swc_*`, `oxc_*`, `tree-sitter*`, `proc-macro2`, `quote`).
Enforcement: structural (`dry-core/Cargo.toml` does not list them) +
source-level (`ast-purity` CI job rejects matching `use` lines in
`crates/dry-core/src/`).

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
