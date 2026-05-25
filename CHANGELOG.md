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
