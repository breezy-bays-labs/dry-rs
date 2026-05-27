# dry-rs

[![CI](https://github.com/breezy-bays-labs/dry-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/breezy-bays-labs/dry-rs/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Structural duplication detector — Rust workspace.

```text
crates/dry4rs   syn-based analyzer for Rust source
crates/dry-core (v1.0+) language-agnostic domain + ports + comparison engine
crates/dry4ts   (v0.6+) TypeScript analyzer (swc/oxc-based, distributed via napi-rs)
```

dry-rs flags **structural duplication in source code** — Jaccard
similarity over normalized subform fingerprints with depth-first
emission, modeled on Uncle Bob's [dry4clj][dry4clj] (Clojure). It runs
in milliseconds, complements (does not replace) cargo-dupes /
similarity-rs, and is designed to give agentic-CI loops sub-second
feedback on duplication risk.

> **Status: v0.x — pre-release.** Public visibility from day one for
> agent reviews and free GitHub Actions; **no crates.io publish, no
> GitHub Release tarballs, no `cargo install` path** until per-crate
> v1.0 gates trip. dry-rs uses a **decoupled per-crate release gate**:
> `dry4rs` v1.0 ships when its CLI/JSON contract stabilizes;
> `dry-core` graduates to v1.0 only when both `dry4rs` and `dry4ts`
> consume it (cross-language abstraction validated). See [the
> roadmap][roadmap] in the breezy-bays-labs ops repo for the full plan.
>
> During v0.x the only consumer is [mokumo][mokumo] via a composite
> GitHub Action — `dry4rs` rebuilds inside the action on every CI run.

## Architecture

dry-rs is a hexagonal (ports & adapters) workspace. Strict dependency
direction enforced by Cargo crate boundaries: `dry-core` is
language-agnostic and lists no AST library; adapter crates depend on
`dry-core` and add their own parser library. A wrong inward import is
a build error, not a review catch. See [`CLAUDE.md`](CLAUDE.md) and the
hexagonal-layering ADR (filed in PR 2) for the full layering invariant.

```
dry-core (no AST libs)
    ^
dry4rs  (depends on dry-core; adds syn, proc-macro2, quote)
    ^
dry4ts  (depends on dry-core; adds swc_ecma_parser or oxc, napi-rs)  [v0.6+]
```

## Sibling tools

dry-rs is one of three structural-quality sensors in the
**agentic-development sensor suite**:

| Tool        | Repo                                           | What it gates              |
|-------------|------------------------------------------------|----------------------------|
| `crap4rs`   | <https://github.com/breezy-bays-labs/crap4rs>  | production-code complexity (Rust) |
| `crap4ts`   | <https://github.com/breezy-bays-labs/crap4ts>  | production-code complexity (TS)   |
| `scrap4rs`  | <https://github.com/breezy-bays-labs/scrap-rs> | test-code structural smells (Rust) |
| `scrap4ts`  | (in `scrap-rs` workspace, v0.6+)               | test-code structural smells (TS)   |
| `dry4rs`    | this repo                                      | **structural duplication (Rust)**  |
| `dry4ts`    | this repo (v0.6+)                              | **structural duplication (TS)**    |

`crap` answers "how risky is this production function?" — `scrap`
answers "is this test testing real behavior?" — `dry` answers "where
is this code structurally duplicated?"

## Detection algorithm (v0.1)

Jaccard similarity over **subform fingerprints** with depth-first
emission, plus typed-placeholder normalization:

1. Each parser adapter walks its language's AST, normalizing each
   subform (function, method, block, expression chain, etc.) into a
   structural fingerprint set. Identifier-aware secondary
   representation captures rename signal (PR 5 lays the foundation;
   v0.2+ activates via `rename_count` / `rename_density`).
2. The comparison engine clusters exact matches via hash-bucket pass,
   then runs sliding-window Jaccard over forms sorted ascending by
   `node_count` for near-duplicates. Loop-break math:
   `forms[j].node_count > forms[i].node_count / threshold` exits the
   inner loop early using Jaccard's `min/max` upper bound.
3. Findings emit with **threshold tier semantics** —
   `auto_refactor` / `review_first` / `advisory` — for agentic-quality
   routing.

dry4rs uses [syn][syn] for Rust source; dry4ts uses [swc][swc] or
[oxc][oxc] for TypeScript (decided at PR 5 / v0.6).

## Wire envelope

`--format json` emits a nested envelope with `schema_version` mirroring
the scrap-rs / crap4rs ADR pattern. `result.*` is the truthful gate
(cannot be reshaped by `--top` / `--only-failing`); `view.*` is the
shapeable display projection. Multi-score envelope shape locked at
v0.1 with null defaults — `score`, `structural_score`, `rename_count`,
`rename_density` per finding (v0.1 fills only `score`; v0.2+ fills the
others without changing shape).

## Quick start

Drop the workflow below into a Rust repo at `.github/workflows/dry-scorecard.yml` and dry-rs runs structural-duplication analysis on every PR + push to `main`. The action checks out, builds dry4rs from a pinned source ref, scans the paths you point it at, and writes a JSON envelope + text summary to the step summary.

```yaml
# Templated dry-scorecard workflow — copy this file into your repo's
# `.github/workflows/` directory, change `paths:` to point at your
# crates, push, done.
#
# This file lives at `.github/workflows/examples/` (subdirectory) so
# GitHub Actions does NOT auto-trigger it inside this repo. Workflow
# triggers only fire for files DIRECTLY under `.github/workflows/` —
# subdirectories are documentation. dry-rs's own dogfood smoke at
# `.github/workflows/example-smoke.yml` (lands in dry-rs#59) invokes
# the action with identical inputs to keep this file mechanically
# honest.
name: dry Scorecard

on:
  pull_request:
    branches: [main]
  push:
    branches: [main]

permissions:
  contents: read

jobs:
  scorecard:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6.0.2
        with:
          persist-credentials: false

      - uses: breezy-bays-labs/dry-rs/.github/actions/scorecard@<sha>  # pin to a dry-rs release SHA
        with:
          paths: crates/
          extensions: 'rs'
          threshold: '0.85'
          # Start with 'false' to observe signal. Promote to 'true' once a clean
          # baseline is established — see README "Getting started" note.
          fail-on-findings: 'false'
```

> **Today**: Rust source analysis via syn. TypeScript support tracked at dry4ts (v0.6+).

> **v0.x build-time cost**: this action self-builds dry4rs from source on every invocation (~1-2 minutes). Per-crate v1.0 unlocks the binstall path.

> **Getting started**: start with `fail-on-findings: 'false'` to observe signal. Promote to `'true'` once you're happy with the baseline.

## Usage (v0.x — internal only)

```bash
# From within dry-rs:
cargo run -p dry4rs -- --src crates --format json
```

Mokumo CI consumes dry-rs via the composite action published from
this repo (lands at PR 9):

```yaml
- uses: actions/checkout@v4
- uses: breezy-bays-labs/dry-rs/.github/actions/scorecard@v0.1.0
  with:
    src: crates
    config: dry4rs.toml
```

The action builds `dry4rs` from the pinned ref on every run. v1.0 adds
`cargo binstall dry4rs` so consumers can install the binary once and
skip the rebuild.

## Documentation

- [`CLAUDE.md`](CLAUDE.md) — architecture invariants and layering rules
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — how to contribute
- [`AGENTS.md`](AGENTS.md) — agent operating notes
- [`CHANGELOG.md`](CHANGELOG.md) — release notes (sparse during v0.x)

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

[dry4clj]: https://github.com/unclebob/dry4clj
[roadmap]: https://github.com/breezy-bays-labs/ops (private — `ops/workspace/dry-rs/20260508-dry-rs-roadmap/roadmap.md`)
[mokumo]: https://github.com/breezy-bays-labs/mokumo
[syn]: https://github.com/dtolnay/syn
[swc]: https://swc.rs
[oxc]: https://oxc.rs
