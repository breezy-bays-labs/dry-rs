//! `dry-examples` — curated DRY-violation corpus + cross-tool
//! benchmark harness for the dry-rs ecosystem.
//!
//! See [`README.md`](https://github.com/breezy-bays-labs/dry-rs/blob/main/crates/dry-examples/README.md)
//! for the layout convention, BLESS workflow, and wire-shape contract.
//!
//! This crate is `publish = false` and intentionally has zero library
//! surface — the fixture source files under
//! `examples/<tier>/<fixture>/main.rs` are the artifact, exercised by
//! `tests/snapshots.rs`. The corpus-exclusion ADR lives at
//! [`adr-dry-examples-corpus.md`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/dry-rs/adr-dry-examples-corpus.md).
#![warn(missing_docs)]
