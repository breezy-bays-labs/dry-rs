@AGENTS.md

# CLAUDE.md — dry-rs

Structural duplication detector. Multi-crate Rust workspace:
`crates/dry-core` (lib — language-agnostic core shared by every
adapter binary), and `crates/dry4rs` (lib + bin — Rust-source adapter
via `syn`). A `crates/dry4ts` (lib + bin — TypeScript-source adapter
via `swc` or `oxc`, distributed to npm via `napi-rs`) joins the
workspace at v0.6+.

## Architecture

Hexagonal (ports & adapters), strict dependency direction enforced by
Cargo crate boundaries: `dry-core` lists no AST library in its deps;
adapter crates depend on `dry-core` and add their own parser library.
A wrong inward import is a build error, not a review catch. The
hexagonal-layering ADR (filed in PR 2 at
`ops/decisions/dry-rs/adr-hexagonal-layout.md`) carries the full
layering invariant + per-crate dep table; this section sketches it.

```
dry-core (no AST libs)
    ^
dry4rs  (depends on dry-core; adds syn, proc-macro2, quote)
    ^
dry4ts  (depends on dry-core; adds swc_ecma_parser or oxc, napi-rs)  [v0.6+]
```

## Phased roadmap (v0.1 -> v1.0)

| Phase  | Adds                                                                                  | Release? |
|--------|---------------------------------------------------------------------------------------|----------|
| v0.1   | Walking skeleton (9 PRs): syn-based normalizer, comparison engine, text + JSON output, self-referential test | No — git tag only |
| v0.2   | Allowlist UX (`.dry-rs-ignore.toml`), markdown reporter, per-language placeholder policy ADR | No |
| v0.3   | Multi-score envelope filled (rename signal), HTML reporter, delta block, severity tiering, `--explain` | No |
| v0.4   | APTED complementary mode, cluster-naming heuristics, threshold calibration empirical study, SARIF | No |
| v0.5   | Performance at scale (size-bucketed rayon parallelism, optional MinHash/LSH)          | No |
| v0.6+  | dry4ts joins workspace (swc/oxc + napi-rs), cross-adapter wire-shape verification     | No |
| **v1.0** | **per-crate decoupled gate**: `dry4rs` ships first when CLI/JSON stabilizes; `dry-core` + `dry4ts` ship together when cross-language abstraction validates | **YES** |

## Comparison algorithm (v0.1)

Two-tier exact + near detection:

1. **Hash-bucket clustering** — first pass clusters forms by their
   `fingerprint_set` hash. Exact structural matches surface in O(N)
   without pairwise comparison.
2. **Sliding-window Jaccard** — second pass over remaining forms
   sorted ascending by `node_count`. For each form `forms[i]`, the
   inner loop breaks when `forms[j].node_count > forms[i].node_count /
   threshold` — the Jaccard upper bound is `min/max`, so for threshold
   `t`, the largest comparable form has `node_count <= forms[i].node_count / t`.
3. **Threshold tiers** — `auto_refactor` (>= 0.95) / `review_first`
   (>= 0.85) / `advisory` (>= threshold) drive agentic-quality routing.

The comparison engine lives in `dry-core::comparison` (single module,
not parallel detector modules — the tool has one algorithm).

## Wire envelope

Mirrors scrap-rs's nested JSON envelope. The nested-envelope ADR
(filed in PR 2 at `ops/decisions/dry-rs/adr-nested-json-envelope.md`)
carries the full forward-compat rules; highlights:

- `schema_version: u32` — bumps only on breaking changes; additive
  fields allowed at any time.
- `result.*` is the **truthful gate** (cannot be reshaped by `--top`,
  `--only-failing`, `--no-fail`).
- `view.*` is the **shapeable display** — filtered, sorted, truncated.
- `delta.*`, `diagnostics.*` — additive optional, omitted when not in use.
- **`#[non_exhaustive]` policy**: every public *enum* in
  `dry-core::domain` carries it (consumer pattern-match concern);
  result *structs* (`Match`, `Score`, `Report`, `Summary`, etc.) do
  not — they evolve via constructors (`Foo::new`, `Foo::try_new`,
  `Foo::default`) and serde versioning. Rationale lives in the
  envelope ADR's `#[non_exhaustive] discipline` section.
- `Option<T>` fields use `#[serde(skip_serializing_if = "Option::is_none")]`.
- **Multi-score envelope shape locked at v0.1** with null defaults:
  `Match` carries `score`, `structural_score`, `rename_count`,
  `rename_density`. v0.1 fills only `score`; v0.2+ fills the others
  without bumping `schema_version`.

## Commands

| Task | Command |
|------|---------|
| Build | `cargo build -p dry4rs` |
| Test | `cargo nextest run` (or `cargo test`) |
| Coverage | `cargo llvm-cov nextest --lcov --output-path lcov.info` |
| Lint | `cargo clippy --all-targets -- -D warnings` |
| Format | `cargo fmt` |
| Quick verify | `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo nextest run` |

## Property test invariants

Filled in as comparison engine + reporters land:

| Function | Key invariants |
|----------|---------------|
| `jaccard()` | `score >= 0.0 && score <= 1.0`, symmetric (`J(A,B) == J(B,A)`), reflexive (`J(A,A) == 1.0`), never panics on empty sets (returns 0.0) |
| Sliding-window break | If inner loop breaks at `j`, then for all `k > j`, `J(forms[i], forms[k]) < threshold` |
| Hash-bucket clustering | If two forms hash to the same bucket, their `fingerprint_set` is structurally equal |
| Detector idempotence | `detect(detect(forms))` produces the same finding set as `detect(forms)` |

## Commit convention

```
feat(domain):  feat(ports):  feat(adapters):  feat(comparison):  feat(cli):
fix(domain):   test:         ci:              docs:               chore:
adr:           closeout:
```

## Worktree setup

```bash
git worktree add ../dry-rs-issue-N -b feat/topic-name
```

Shared target directory once configured under `.cargo/config.toml`
(arrives when worktrees are needed).

## v0.x -> v1.0 transition (decoupled per-crate gate)

The workspace already has the right shape: `dry-core` (lib),
`dry4rs` (lib + bin), with `dry4ts` (lib + bin) joining at v0.6+ when
its parser adapter is ready. Per-crate cadences:

1. **`dry4rs` v1.0** — ships when CLI surface + JSON output schema
   stabilize. Depends on `dry-core` v0.x. Adds `release.yml` mirroring
   crap4rs (tri-platform tarballs, ordered `cargo publish`, GH Release)
   and `[package.metadata.binstall]` to `crates/dry4rs/Cargo.toml`.
2. **`dry-core` v1.0** — graduates only when both `dry4rs` AND `dry4ts`
   consume it (cross-language abstraction validated). Library-facing
   Rust API contract (port traits, types, comparison engine).
3. **`dry4ts` v1.0** — TS CLI + JSON parity with dry4rs. Distributes
   to npm via `napi-rs` — `npm install -D @dry-rs/dry4ts` pulls a
   per-platform native binary. Mokumo migrates from action-ref
   consumption to `bins: dry4rs@1.0.0` + composite action `@v1.0.0`.

The decoupled gate replaces the prior "block dry4rs v1.0 on dry4ts"
pattern — that pattern created perverse pressure to either under-design
the TS parser to unblock the Rust crate or stall public release
indefinitely. Decoupling preserves the load-bearing principle
(`dry-core`'s API only stabilizes when two adapters validate it)
without holding a working tool hostage.

## Cross-references

- **Roadmap** (private ops vault):
  `ops/workspace/dry-rs/20260508-dry-rs-roadmap/roadmap.md`
- **Pipeline note** (private ops vault):
  `ops/pipelines/dry-rs/dry-rs-20260508-roadmap-research.md`
- **Sibling — production-code complexity**:
  [crap4rs](https://github.com/breezy-bays-labs/crap4rs)
- **Sibling — test-code structural smells**:
  [scrap-rs](https://github.com/breezy-bays-labs/scrap-rs)
- **Modeled on**: [unclebob/dry4clj](https://github.com/unclebob/dry4clj) (Clojure)

## Compact instructions

Preserve: hexagonal layering, comparison-engine algorithm (hash-bucket
+ sliding-window Jaccard with bound math), wire envelope invariants,
property test contracts, decoupled per-crate release gate, multi-score
envelope shape locked at v0.1.
Discard: full file contents from old reads, search results not acted
on, completed PR details, intermediate license/visibility deliberations
already documented in the roadmap.
