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
