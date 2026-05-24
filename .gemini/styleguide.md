# dry-rs review style guide for Gemini Code Assist

This file primes `gemini-code-assist` with project-specific
architectural rules. The authoritative source is
[`AGENTS.md`](../AGENTS.md) § "For automated code reviewers" — please
read that section before suggesting structural changes. The same rules
live there in fuller detail with rationale.

Treat the rules below as **constraints** on your review output, not as
guidance about what the code should look like. dry-rs has deliberate
architectural decisions documented in ADRs that may differ from
generic Rust best practices.

## Key principles

- **ADR-locked decisions take precedence over generic best practices.**
  When the code does something that looks "unidiomatic" but is
  consistent with the rules below, the rule wins. Surface the rule (not
  the suggested change) in your review.
- **Suggest what's missing, not what's deliberate.** Doc-comment gaps,
  missing `?`, redundant `clone()`, unclear test names — all welcome.
  Restructuring locked types, removing allowed dependencies, narrowing
  hand-picked primitive types — please don't.
- **Quote the rule if you disagree.** If you believe a rule is wrong,
  raise it as meta-discussion in the PR rather than auto-applying a
  contradicting suggestion. The rules trace back to ADRs in the
  private ops vault; they're not accidents.

## Locked wire shapes — do not suggest changes

The v0.1 JSON wire envelope (defined in
`ops/decisions/dry-rs/adr-nested-json-envelope.md`, mirrored in
`AGENTS.md`) locks these shapes:

- `Match.score: f64` — primitive on the wire. Do NOT suggest
  replacing with the `Score` newtype. `Score` validates input at
  boundaries; `Match` carries the raw wire value.
- `Match.structural_score: Option<f64>`,
  `Match.rename_count: Option<u32>`,
  `Match.rename_density: Option<f64>` — three reserved slots, all use
  `#[serde(default)]` ONLY, never
  `#[serde(skip_serializing_if = "Option::is_none")]`. The v0.1
  contract requires `null`, not omission.
- `NormalizedForm.fingerprint_set: HashSet<u64>` — raw `u64`, not
  `HashSet<Fingerprint>`. The `Fingerprint` newtype's role is
  reporter-side identity display (zero-padded hex); the set is
  hot-path input to Jaccard intersection. Both `u64` and `Fingerprint`
  produce identical machine code (zero-cost newtype) — the choice is
  ergonomic, but deliberate.

## Allowed dependencies per crate

The per-crate dep table at the top of `AGENTS.md` is authoritative.
The notable cases AI bots have historically misjudged:

- **`thiserror` IS allowed in `dry-core`.** Do NOT suggest replacing
  `#[derive(thiserror::Error)]` with manual `Display` + `Error` impls
  on "domain purity" grounds. The purity rule does not apply.
- The "domain purity" rule in dry-rs targets **AST libraries only**,
  not all external crates. The rejected import set is exactly: `syn`,
  `quote`, `proc-macro2`, `swc_*`, `oxc_*`, `tree-sitter*`,
  `rustc_ast`, `rustc_parse`. Other crates from the allowed list
  (`serde`, `serde_json`, `clap`, `walkdir`, `ignore`, `globset`,
  `comfy-table`, `regex`) are fully permitted in `dry-core`.

## `#[non_exhaustive]` discipline

- Public **enums** in `dry-core::domain` (`Tier`, `Severity`,
  `FormKind`, `NormalizeError`, `SpanError`, `ScoreError`, etc.) carry
  `#[non_exhaustive]`. Do NOT suggest removing.
- Public **result structs** (`Match`, `Score`, `Span`, `Fingerprint`,
  `Report`, `Summary`, `NormalizedForm`, `FormRef`) do NOT carry
  `#[non_exhaustive]`. They evolve via constructor pattern + serde
  versioning. Do NOT suggest adding.

## Rust version + `const fn` rules

- Workspace pins `rust-version = "1.85"` (edition 2024). Suggestions
  premised on older Rust versions should be re-checked.
- **`const fn` accepts parameters that implement `Drop`** since Rust
  1.61. A `pub const fn new(forms: Vec<FormRef>, ...)` is valid: the
  function may not invoke drop in const context, but moving the
  parameter into a returned struct field is fine. Do NOT suggest
  removing `const` on constructors taking `Vec<T>`, `PathBuf`,
  `Summary`, or other `Drop`-implementing parameters.
- `proc-macro2` (in `dry4rs` and future Rust adapters) MUST carry the
  `span-locations` feature flag. Enforced by CI; do NOT suggest
  removing this flag.

## What is welcome

Reviews that catch:

- Missing or unclear doc comments.
- Doc-comment formatting (missing examples, broken intra-doc links).
- `#[must_use]`, `#[inline]`, `#[repr(transparent)]`, `#[cold]`
  attribute additions with clear rationale.
- Missing `?` on `Result`, unnecessary `clone()`, suboptimal lifetime
  bounds.
- Trait derives where appropriate (`Default`, `PartialOrd`/`Ord` for
  sortable references when sort ordering is meaningful).
- Test coverage gaps, missing edge cases (especially around the
  comparison engine's sliding-window break math and Jaccard
  invariants).
- Typos, unclear wording, inconsistent terminology.

## What is not welcome

- Auto-applying changes that contradict the rules above.
- Suggesting structural reorganization of locked types.
- Removing crates from the allowed-deps list.
- Repeating the same suggestion across many inline comments when a
  single thematic comment would suffice — please prefer one summary
  comment grouping by theme.

## When the rules feel wrong

The rules trace to ADRs in the private ops vault. If you believe a
rule is misplaced, frame your concern as a question or meta-comment
in the PR review summary, not as an inline suggestion to change the
code. The author will engage with the meta-discussion separately from
the line-level review.
