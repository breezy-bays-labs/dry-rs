# tier_1_exact / identical_async_fns

## What this demonstrates

Two `async fn` items with identical signatures and identical bodies.
The bodies are sync — no `.await` — so executor concerns are
irrelevant; what's at issue is whether the normalizer collapses the
`async` keyword and treats the function bodies as comparable to plain
`fn` bodies.

## Why dry4rs should (or shouldn't) detect

The `adr-rust-normalization-rules.md` ADR documents whether async/sync
asymmetry is normalized away or preserved. This fixture pins dry4rs's
actual behavior for the catalogue: whatever the normalizer does today,
the recorded `expected.json` makes the contract auditable.

Compare this fixture with `edge_cases/async_vs_sync_same_body/` —
that one mixes one `async` and one sync fn with the same body. The
two fixtures together document the full async/sync axis.

## How similar tools handle this

`similarity-rs` typically respects async vs sync as a structural
distinction. Both tools' verdicts populate `EXPECTED.md` after
`bench.sh` runs.
