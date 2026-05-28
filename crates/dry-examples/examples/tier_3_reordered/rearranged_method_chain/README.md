# tier_3_reordered / rearranged_method_chain

## What this demonstrates

Two functions that build the same `ConfigBuilder` via independent
setter methods, called in different orders. The chain
`new().name(...).retries(...).timeout_ms(...)` produces the
identical final builder as
`new().timeout_ms(...).retries(...).name(...)`.

Method chains are a frequent vector for "harmless" reorderings —
linters and reviewers don't usually flag swap-equivalent fluent
calls. A duplication detector that catches this surface area
distinguishes itself from string-based diff tools.

## Why dry4rs may or may not detect

Each method call contributes a fingerprint anchored to the
method-name string + receiver-type slot; under set-Jaccard, the two
fingerprint sets are equal modulo call order. Hash-bucket clustering
should pair them.

A position-aware detector (tree-edit, token-stream) would see the
method-chain as a sequence and call the order divergence a real
difference.

## How similar tools handle this

`similarity-rs` typically distinguishes call-chain order. The
catalogue records both tools' verdicts; the divergence is the
interesting signal.
