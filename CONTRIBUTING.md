# Contributing to dry-rs

dry-rs is a private-org Rust workspace developed by Breezy Bays Labs.
The repo is public for free GitHub Actions / agent reviews; external
contributions are welcome at v1.0+ once the project ships its first
crates.io release.

## Quick start

```bash
git clone git@github.com:breezy-bays-labs/dry-rs.git
cd dry-rs
lefthook install            # wires pre-commit + pre-push hooks
cargo build -p dry4rs
cargo nextest run
```

`lefthook install` is one-time. After that, `cargo fmt --check` runs
on every commit; the full pre-push battery (fmt + ast-purity + pedantic
clippy + tests + cargo-deny + docs-as-errors) runs on every push and
matches CI exactly. See [`lefthook.yml`](lefthook.yml) for what each
hook runs.

## Development loop

| Step | Command |
|------|---------|
| Format | `cargo fmt --all` |
| Lint (with pedantic) | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| Test | `cargo nextest run --workspace --all-targets --locked` |
| Coverage | `cargo llvm-cov nextest --workspace --locked --fail-under-lines 85` |
| Supply chain | `cargo deny check` |
| Doc lint | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items --locked` |
| Quick verify | `lefthook run pre-push` |
| Run binary | `cargo run -p dry4rs -- <args>` (workspace-root `cargo run` is ambiguous once a second binary lands; default to `-p` from the start) |

`#![warn(clippy::pedantic, clippy::cargo)]` lives at the crate root of
each workspace member (`dry-core`, `dry4rs`), so `clippy -D warnings`
enforces pedantic and cargo lints automatically — no extra flag
needed.

CI runs the same chain on every PR. See `.github/workflows/ci.yml` for
the full job set (matrix test on Linux / macOS arm64 / macOS x86_64,
plus MSRV / cargo-deny / cargo doc / coverage / ast-purity gates).

## Branch + PR

- Always branch off `main`; never push directly. The repo enforces
  this for ongoing work. (The genesis commit is the one-time
  exception.)
- Use worktrees for parallel work:
  `git worktree add ../dry-rs-issue-N -b feat/topic-name`.
- Title: `<conventional-prefix>(<scope>): <one-liner>` (e.g.
  `feat(comparison): hash-bucket clustering pass`).
- Body: include `Closes #N` to link to the sub-issue.
- 1 PR closes exactly 1 sub-issue (per
  `ops/standards/issue-hierarchy.md`).

## Architecture discipline

Read [`CLAUDE.md`](CLAUDE.md) and [`AGENTS.md`](AGENTS.md) before
touching code. The hexagonal layering rule is **strict**:

- `dry-core` (lib) houses `domain/`, `ports/`, `comparison/`,
  language-agnostic adapters (file walker, reporters), and the CLI
  surface. No AST library deps allowed (`syn`, `swc_*`, `oxc_*`,
  `tree-sitter*`, `proc-macro2`, `quote`). Enforced structurally
  (`dry-core/Cargo.toml` doesn't list them) and via the `ast-purity`
  CI job (rejects matching `use` lines).
- `dry4rs` is the Rust-source adapter (lib + bin) — depends on
  `dry-core`, adds `syn` for parsing.
- `dry4ts` (planned, v0.6+) is the TypeScript-source adapter —
  depends on `dry-core`, adds `swc`/`oxc` for parsing, distributes to
  npm via `napi-rs`.
- Never import inward. The dep graph runs `dry-core <- dry4rs` (and
  `dry-core <- dry4ts` once it lands).

## Comparison-engine authoring checklist

When extending the comparison engine:

1. Add the change to `dry_core::comparison` (single module — `dry-rs`
   has one algorithm; multiple-detector parallel modules are
   explicitly out of scope per O6).
2. Add a property invariant covering the score-formula effect (see
   CLAUDE.md "Property test invariants" table for the canonical set).
3. Add fixtures under `crates/dry4rs/tests/fixtures/` with explicit
   "should match" / "should not match" assertions tied to threshold
   tier semantics.
4. Update the comparison engine's wire snapshot (`crates/dry-core/tests/wire_envelope_snapshot.rs`)
   if the change affects the JSON envelope.

## Exclusions and tracking-issue rule

Every entry in `dry4rs.toml`'s `exclude = [...]` array, every
`#[ignore]`, every `#[cfg(skip_in_ci)]` MUST carry an inline
`# tracked: dry-rs#<n> -- <reason>` comment OR `# adr: <path>` if
permanent. Quarterly grep audit. See `~/.claude/rules/exclusions.md`
for the full rule.

## Issue discipline

- Every issue gets exactly one `type:*` label
  (`type:feature`/`type:bug`/`type:task`/etc.) and one `priority:*`
  label.
- Sub-issues use `--parent <epic-number>` (native GH sub-issues; not
  manual checkboxes).
- Body skeleton: `## Summary` / `## Acceptance Criteria` /
  `## Context` / `## Discovery`.
- Wire `blocked-by` edges at creation time, not later.

## Release discipline (v0.x)

- **No `cargo publish`** until per-crate v1.0 gates trip.
- **No GH Release** until per-crate v1.0.
- Tags during v0.x exist solely for git-pinning consumers (mokumo's
  composite-action ref). They do not trigger any workflow.
- See `CHANGELOG.md` for the deliberate-no-release policy and the
  decoupled per-crate v1.0 gate definition.

## License

By submitting a PR you agree your contributions are dual-licensed
under MIT OR Apache-2.0.
