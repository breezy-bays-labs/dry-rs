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

## What dry-rs catches

A curated corpus of intentional DRY violations lives at
[`crates/dry-examples/`](crates/dry-examples/README.md). The corpus
exists to (a) prevent regressions in detection behavior on every PR
via the `corpus-smoke` CI job and (b) back empirical comparisons
against competitor tools.

### Catalogue

The full per-fixture table — including verdicts from dry4rs and
competitor tools — lives in
[`crates/dry-examples/EXPECTED.md`](crates/dry-examples/EXPECTED.md).

### Cross-tool comparison

The empirical comparison vs `similarity-rs` lives in
[`crates/dry-examples/examples/bench-output.md`](crates/dry-examples/examples/bench-output.md).
Refresh: trigger the
[`refresh-bench`](.github/workflows/refresh-bench.yml)
workflow_dispatch from the Actions tab.

### Example

The simplest fixture — two free functions with identical bodies,
differing only in identifier names:

```rust
// crates/dry-examples/examples/tier_1_exact/identical_signatures/main.rs
fn add_one(x: i32) -> i32 {
    let result = x + 1;
    println!("computed {result}");
    result
}

fn increment(value: i32) -> i32 {
    let result = value + 1;
    println!("computed {result}");
    result
}
```

dry4rs flags this as `review_first` (Jaccard score ~0.86 after
typed-placeholder normalization). The exact captured verdict lives
in the fixture's `expected.json` and is the regression contract.

## Wire envelope

`--format json` emits a nested envelope with `schema_version` mirroring
the scrap-rs / crap4rs ADR pattern. `result.*` is the truthful gate
(cannot be reshaped by `--top` / `--only-failing`); `view.*` is the
shapeable display projection. Multi-score envelope shape locked at
v0.1 with null defaults — `score`, `structural_score`, `rename_count`,
`rename_density` per finding (v0.1 fills only `score`; v0.2+ fills the
others without changing shape).

## Quick start

Commit a `dry.toml` at your repo root ([schema](#schema) below) and drop the workflow snippet into `.github/workflows/dry-scorecard.yml`. dry-rs runs structural-duplication analysis on every PR + push to `main`. The action checks out, builds dry4rs from a pinned source ref, auto-discovers your `dry.toml`, and writes a JSON envelope + text summary to the step summary.

The recommended invocation is **minimal** — pass only `fail-on-findings`; let `dry.toml` drive the analysis knobs. Add `threshold` / `paths` / `format` inputs only when a specific workflow needs to override config.

```yaml
# Templated dry-scorecard workflow — copy this file into your repo's
# `.github/workflows/` directory, commit a `dry.toml` at your repo
# root, push, done.
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

      # Minimal-input invocation — defers to dry.toml. Add
      # `threshold` / `paths` / `format` only to override config
      # for this specific workflow.
      - uses: breezy-bays-labs/dry-rs/.github/actions/scorecard@<sha>  # pin to a dry-rs release SHA
        with:
          # Start with 'false' to observe signal. Promote to 'true' once a clean
          # baseline is established — see "Getting started" note below.
          fail-on-findings: 'false'
```

> **Today**: Rust source analysis via syn. TypeScript support tracked at dry4ts (v0.6+).

> **v0.x build-time cost**: this action self-builds dry4rs from source on every invocation (~1-2 minutes). Per-crate v1.0 unlocks the binstall path.

> **Getting started**: start with `fail-on-findings: 'false'` to observe signal. Promote to `'true'` once you're happy with the baseline.

> **No dry.toml yet?** The action still works — it'll use compiled-in defaults (threshold `0.85`, format `text`, scan `$GITHUB_WORKSPACE`). Commit a `dry.toml` when you want your analysis knobs in version control.

## Configuration

dry4rs auto-discovers a `dry.toml` config file by walking
upward from the analysis path via `Path::ancestors` — the first
matching file found wins. A worked reference is committed at the
repo root ([`dry.toml`](dry.toml)) and exercised end-to-end on every
CI run by the `dry-self-scorecard` and `dry-corpus-scorecard` jobs in
[`.github/workflows/self-test.yml`](.github/workflows/self-test.yml).
Both jobs invoke the bare `dry4rs` binary (no `--config` / `--threshold`
/ `--format` flags) so the auto-discovery + parsing contract is
verified through the production CI surface, not just unit tests.

### Schema

```toml
[gate]
threshold = 0.85          # Jaccard similarity threshold (0.0–1.0]
threshold_mode = "default" # strict | default | lenient

[output]
format = "text"           # text | json

[walk]
include_ignored = false   # walk .gitignore'd directories?
extensions = ["rs"]       # file extensions to analyze (optional)
```

### Precedence

CLI flag values ALWAYS override config. Missing values resolve via:

```text
CLI flag > [config] section value > AdapterMeta default > compiled-in fallback
```

For example, `dry4rs report --threshold 0.95 crates/foo/` uses `0.95`
regardless of what `dry.toml` says. With no `--threshold` flag,
`[gate] threshold = 0.9` from `dry.toml` applies; if neither
supplies a value, the `AdapterMeta`-supplied default kicks in (e.g.,
the `extensions` field defaults to `&["rs"]` for `dry4rs`); the
compiled-in fallback (`REVIEW_FIRST_FLOOR = 0.85` for `threshold`)
applies last.

### Discovery

- **Auto-discovery**: walks up from the first positional analysis
  path (or CWD if none) to filesystem root, looking for
  `dry.toml`.
- **Explicit override**: `dry4rs report --config /custom/path.toml`
  bypasses auto-discovery. Missing explicit path is an error.
- **Missing file is OK**: with no config file present, defaults
  apply.
- **Unknown keys are errors**: typos surface with clear messages
  (path + line + key name).

See [`ops/decisions/org/adr-config-file-pattern.md`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/org/adr-config-file-pattern.md)
for the cross-tool canonical shape and
[`ops/decisions/dry-rs/adr-dry4rs-config-file.md`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/dry-rs/adr-dry4rs-config-file.md)
for dry-rs-specific decisions.

## Usage (v0.x — internal only)

```bash
# From within dry-rs:
cargo run -p dry4rs -- report crates --format json
```

Mokumo CI consumes dry-rs via the composite action published from
this repo. The minimal-input shape (recommended) defers to
`dry.toml`; the explicit-input shape overrides specific values:

```yaml
- uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6.0.2
  with:
    persist-credentials: false
# Recommended — minimal inputs, dry.toml drives.
- uses: breezy-bays-labs/dry-rs/.github/actions/scorecard@<sha>  # pin to a dry-rs release SHA
  with:
    fail-on-findings: 'false'
# OR explicit overrides for a stricter scan.
- uses: breezy-bays-labs/dry-rs/.github/actions/scorecard@<sha>  # pin to a dry-rs release SHA
  with:
    paths: crates/
    threshold: '0.95'
    fail-on-findings: 'false'
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
