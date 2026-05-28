# dry-examples

Curated DRY-violation fixture corpus + cross-tool benchmark harness
for the dry-rs detector ecosystem. `publish = false`; never reaches
crates.io.

## What this is

One artifact serving three audiences:

1. **Regression-protective CI guard** — snapshot drift = detection
   behavior change, caught on every PR via the `corpus-smoke` job in
   `.github/workflows/example-smoke.yml`.
2. **Marketing-claim backstop** — empirical "dry-rs catches X,
   competitor Y misses Z" populated from `examples/bench-output.md`
   (run against the same fixtures both tools see).
3. **Future threshold-calibration ground truth** — the v0.4
   threshold-calibration study reads expected verdicts from
   `EXPECTED.md` to validate score-floor proposals.

Distinct from `crates/dry4rs/tests/self_check.rs` (which proves
detection works on real production code by analyzing dry-rs's own
source). This corpus proves detection works on isolated curated
patterns and provides the side-by-side "what dry-rs catches"
documentation surface.

## Layout

```
crates/dry-examples/
├── Cargo.toml
├── README.md                       ← this file
├── EXPECTED.md                     ← cross-fixture verdict catalogue
├── .ignore                         ← '*' — opts corpus out of default walks
├── src/
│   └── lib.rs                      ← empty (docstring only)
├── tests/
│   └── snapshots.rs                ← BLESS-driven snapshot harness
└── examples/
    ├── tier_1_exact/<fixture>/
    │   ├── main.rs                 ← intentionally-duplicated items
    │   ├── README.md               ← what this fixture demonstrates
    │   └── expected.json           ← BLESS-generated golden envelope
    ├── tier_2_renamed/<fixture>/   ← same layout
    ├── tier_3_reordered/<fixture>/ ← same layout
    ├── tier_4_false_positive_bait/<fixture>/ ← same layout
    ├── tier_5_algorithmic/<fixture>/ ← same layout
    ├── edge_cases/<fixture>/       ← same layout (may have multiple .rs files)
    ├── bench.sh                    ← cross-tool benchmark script
    └── bench-output.md             ← committed empirical comparison
```

Multi-file fixtures (today: only `edge_cases/cross_file_duplicate/`
with `producer.rs` + `consumer.rs`) exist per ADR-6 to exercise the
walker's cross-file clustering path. The harness passes the
DIRECTORY path to dry4rs; the walker discovers `.rs` files
recursively.

## Adding a fixture

Five-step recipe:

1. `mkdir crates/dry-examples/examples/<tier>/<name>/`
   (use `snake_case`; `<tier>` is one of `tier_1_exact`,
   `tier_2_renamed`, `tier_3_reordered`,
   `tier_4_false_positive_bait`, `tier_5_algorithmic`,
   `edge_cases`).
2. Write `main.rs` — multiple top-level items that intentionally
   duplicate (or, for tier 4, look similar but differ).
   Cross-file fixtures use `producer.rs` + `consumer.rs` or
   similar names instead of `main.rs`; the harness accepts any `.rs`.
3. Write `README.md` — 2-4 paragraphs covering: what this
   demonstrates, why dry4rs should (or shouldn't) detect, how
   similar tools handle this. Describe structurally; the verdict
   column lives in `EXPECTED.md` and is empirically captured.
4. Run `BLESS=1 cargo test -p dry-examples` to generate
   `expected.json`. Review the diff before committing.
5. Add a row to `EXPECTED.md` in the canonical sort order
   (`(tier_number, fixture_path_lex)`, `edge_cases` last). The CI
   lint enforces this; an out-of-order add fails the test.

## Bless workflow

```bash
# Regenerate every fixture's expected.json from live dry4rs output.
BLESS=1 cargo test -p dry-examples

# Verify (no env var) — mismatches fail the test.
cargo test -p dry-examples
```

The harness:

- Walks `examples/` for fixture directories.
- For each fixture: invokes dry4rs via `cargo run -p dry4rs --`
  with `report --format json --no-fail --include-ignored
  <fixture_dir>/`. Sets `current_dir = <crate manifest>` so
  `forms[].file` emerges workspace-relative.
- Normalizes the envelope per ADR-5: `timestamp` →
  `"TIMESTAMP_REDACTED"`, `tool_version` → `"0.1.0"`, every
  `forms[].file` value `\\` → `/` (recursive walk).
- `BLESS=1`: writes the normalized envelope to `expected.json`
  (trailing `\n`). Default mode: diffs via
  `serde_json::Value` equality.

Editor's auto-insert-final-newline will NOT cause drift — the harness
writes a trailing `\n` and reads via `from_slice`.

## Wire shape

Each `expected.json` mirrors the v0.1 nested JSON envelope produced
by dry4rs (`schema_version: 1`). Sentinel substitution makes goldens
byte-stable across machines and OS:

- `timestamp` is hard-coded to `"TIMESTAMP_REDACTED"` (real value
  flutters per run).
- `tool_version` is hard-coded to `"0.1.0"` (real value bumps per
  release; pinning here avoids invalidating every golden).
- `forms[].file` paths are workspace-relative AND backslash-
  normalized to forward slashes (the latter handles Windows
  runners).

Wire-envelope ADR:
[`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/dry-rs/adr-nested-json-envelope.md).
Corpus-specific decisions (the four normalization layers, the per-
gate exclusion mechanisms, the observational/normative split):
[`adr-dry-examples-corpus`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/dry-rs/adr-dry-examples-corpus.md).

## Cross-tool comparison

`examples/bench.sh` is an offline script that runs each fixture
through dry4rs AND similarity-rs (when installed) and emits a
side-by-side markdown table to `examples/bench-output.md`. The
committed `bench-output.md` is the "what dry-rs catches that
competitors miss" empirical reference for the project README.

Refresh cadence: manual, pre-release. Trigger the
`refresh-bench.yml` `workflow_dispatch` workflow (or run `bash
examples/bench.sh > examples/bench-output.md` locally with
similarity-rs installed). The corpus-smoke CI job emits a
`::warning::` when `bench-output.md`'s `last_refreshed:` frontmatter
is more than 30 days old (passive staleness nag per ADR-8).

## What this is NOT

- **Not a binary.** No `[bin]` in `Cargo.toml`; the harness's spawn
  target is dry4rs, not anything in this crate.
- **Not a library shipping logic.** `src/lib.rs` is intentionally
  empty (docstring only).
- **Not a cargo-examples directory in the conventional sense.**
  `autoexamples = false`; files under `examples/` are PARSED by
  dry4rs, not compiled by cargo.
- **Not a replacement for the self-check.** The self-check at
  `crates/dry4rs/tests/self_check.rs` proves detection works on
  production-shaped code; this corpus proves detection works on
  curated patterns.
- **Not the load-bearing source of "what dry-rs catches" claims.**
  The `expected.json` files are the regression contract.
  `EXPECTED.md` is documentation; `bench-output.md` is the
  marketing-claim source. The three artifacts have distinct roles.

## Cross-references

- Issue: dry-rs#56
- ADR: `ops/decisions/dry-rs/adr-dry-examples-corpus.md`
- Sibling pattern: `crates/scrap-examples/` (scrap-rs#80)
- Sibling pattern: `crates/crap-examples/` (crap-rs#314)
- Walking-skeleton: dry-rs#10 (introduced `self_check.rs`)
