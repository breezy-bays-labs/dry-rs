# edge_cases / async_vs_sync_same_body

## What this demonstrates

One `async fn compute_async(x: i32) -> i32` and one sync
`fn compute_sync(x: i32) -> i32` with identical bodies (no `.await`,
so the async machinery is unused). Behaviorally interchangeable in
this stripped-down form.

This fixture probes the boundary that `async` introduces. The keyword
changes the function's return type from `i32` to
`impl Future<Output = i32>`, but the body source is the same.

## What dry4rs observes

Two design choices the normalizer can take:

- **Preserve `async`**: the keyword contributes a fingerprint token,
  the bodies diverge by exactly one token. Score lands close to but
  below 1.0.
- **Normalize `async` away**: the bodies are structurally identical.
  Score lands at or near 1.0.

Compare with `tier_1_exact/identical_async_fns/` — that fixture is
two `async` fns with identical bodies. The async-vs-sync split in
this fixture pins what happens when ONE side is async.

The captured `expected.json` makes the v0.1 normalizer's choice
auditable. Whatever it does, this fixture is the regression sentinel.

## How similar tools handle this

Most token-stream detectors ignore the `async` keyword by accident
(it normalizes to whitespace-equivalent in many tokenization passes).
Type-aware detectors preserve the distinction. Neither is
unambiguously correct — it depends on whether the consumer wants the
"these can be merged into one" or "these are intentionally separate"
verdict.
