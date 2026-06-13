//! Fingerprint-determinism gate (dry-rs#121).
//!
//! The generic syn-walk visitor refactor (epic dry-rs#107, build-plan
//! PR 1) factored the inline `Xxh3` fold out of `parser::walker` into a
//! sink-driven dispatch in `parser::visitor`. The refactor is
//! **behavior-preserving for the fingerprint path**: for any given Rust
//! source, the walker MUST emit a byte-identical `fingerprint_set`,
//! `node_count`, and `identifier_set` as the pre-refactor inline fold.
//!
//! This test pins that contract with a concrete-value snapshot over a
//! representative corpus. The committed snapshot was captured from the
//! pre-refactor walker and confirmed byte-identical against the
//! refactored visitor; any future change to the dispatch (a reordered
//! child fold, a dropped leaf, a renamed discriminator) that perturbs
//! the emitted fingerprints will fail here.
//!
//! ## Why concrete `u64` values, not just structural relations
//!
//! The `parser::walker` unit tests and `normalizer_integration` assert
//! *relationships* ("these two bodies share fingerprints", "this
//! modifier changes the set"). Those catch many regressions but cannot
//! prove byte-identity — a refactor could shift every fingerprint by a
//! constant and still satisfy them. Snapshotting the actual sorted
//! `u64`s + `node_count` + ordered `identifier_set` is the only gate
//! that proves the fold is reproduced exactly.
//!
//! `xxh3` output is cross-toolchain stable per upstream contract (see
//! AGENTS.md § "Rust version + const fn rules" and the O5 ADR §
//! Hashing), so these values are stable across MSRV bumps within the
//! pinned hash algorithm. Should a future intentional change to the
//! normalization rules move them, re-bless with
//! `cargo insta test --accept -p dry4rs --test fingerprint_determinism`
//! AS PART OF the PR that changes the rules — never as a follow-up.

use std::path::PathBuf;

use dry4rs::domain::FilePath;
use dry4rs::parser::SynNormalizer;
use dry4rs::ports::NormalizerPort;
use serde_json::{Value, json};

/// Representative DRY-corpus fixtures spanning the O5 construct table:
/// signatures, control flow, patterns, generics, lifetimes, macros,
/// modules, traits, closures, async/unsafe blocks, casts, ranges. Each
/// exercises a distinct slice of the syn-dispatch fan-out.
fn corpus() -> Vec<&'static str> {
    vec![
        "fn empty() {}",
        "fn one() -> i32 { 1 }",
        "fn add(x: i32, y: i32) -> i32 { x + y }",
        "#[test] fn t() { assert_eq!(1, 1); }",
        "#[inline] fn host() { let _x = 0; }",
        "struct S; impl S { fn new() -> Self { S } }",
        "trait T { fn default_method(&self) -> i32 { 0 } }",
        "fn ret(x: i32) -> i32 { return x; }",
        "fn loops() { for i in 0..3 { let _ = i; } }",
        "fn cond(n: i32) -> bool { if n > 0 { true } else { false } }",
        "fn mch(n: i32) -> i32 { match n { 0 => 0, _ => 1 } }",
        "fn closure_host() { let _f = |x: i32| x + 1; }",
        "fn refs(r: &mut i32) { *r = *r + 1; }",
        "fn statics() { let _s: &'static str = \"hi\"; }",
        "async fn a() -> i32 { 1 }",
        "fn macros() { let _v = vec![1, 2, 3]; }",
        "fn paths() { let _ = std::collections::HashMap::<u32, u32>::new(); }",
        "fn classify(n: i32) -> &'static str { match n { 0 => \"zero\", _ => \"other\", } }",
        "fn cmp<T: Ord>(a: T, b: T) -> bool { a < b }",
        "mod outer { mod inner { fn deep() {} } }",
        "trait Greeter { fn name(&self) -> &str; fn greet(&self) -> String { let n = self.name(); format!(\"hi {}\", n) } }",
        "fn td() { let (a, b) = (1, 2); let _ = a + b; }",
        "struct P { x: i32, y: i32 } fn d(p: P) -> i32 { let P { x, y } = p; x + y }",
        "fn c(x: i64) -> i32 { x as i32 }",
        "fn de(p: &i32) -> i32 { *p + 1 }",
        "fn le(o: Option<i32>) -> i32 { let Some(n) = o else { return -1; }; n }",
        "fn dt(_b: Box<dyn std::fmt::Debug>) {}",
        "fn it() -> impl std::fmt::Debug { 1_i32 }",
        "fn cp<const N: usize>(_arr: [i32; N]) {}",
        "fn mp() { println!(\"hello\"); }",
        "fn tfa(p: (i32, bool)) -> i32 { p.0 }",
        "fn ct() { for i in 0..10 { if i % 2 == 0 { continue; } let _ = i; } }",
        "fn unsafe_host() -> i32 { unsafe { let x = 1; x } }",
        "fn async_host() { let _f = async { let x = 1; x }; }",
        "fn ar() -> [i32; 3] { [0; 3] }",
        "fn rg() { let _ = 0..10; let _ = ..; }",
    ]
}

/// Project a corpus run into a stable JSON shape: per fixture, per form,
/// the qualified name, node count, ordered identifier set, and the
/// SORTED fingerprint set. Sorting the fingerprints makes the snapshot
/// invariant to `HashSet` iteration order while still pinning every
/// concrete `u64` value.
fn project_corpus() -> Value {
    let n = SynNormalizer::new();
    let p = FilePath::from(PathBuf::from("fixture.rs"));
    let mut fixtures = Vec::new();
    for src in corpus() {
        let forms = n.normalize(src, &p).expect("corpus fixture must parse");
        let projected: Vec<Value> = forms
            .iter()
            .map(|f| {
                let mut fps: Vec<u64> = f.fingerprint_set.iter().copied().collect();
                fps.sort_unstable();
                json!({
                    "qualified_name": f.qualified_name,
                    "node_count": f.node_count,
                    "identifier_set": f.identifier_set,
                    "fingerprints": fps,
                })
            })
            .collect();
        fixtures.push(json!({ "source": src, "forms": projected }));
    }
    Value::Array(fixtures)
}

#[test]
fn walker_fingerprints_are_byte_identical_to_baseline() {
    // The committed snapshot is the pre-refactor inline fold's output,
    // confirmed byte-identical against the post-refactor visitor. A
    // failure here means the generic dispatch drifted from the fold.
    insta::assert_json_snapshot!("walker_corpus_fingerprints", project_corpus());
}

#[test]
fn corpus_normalization_is_deterministic_across_invocations() {
    // Belt-and-suspenders on top of the snapshot: two runs over the
    // whole corpus produce identical projections. Catches any accidental
    // `RandomState` leak in the dispatch independently of the snapshot
    // value.
    let a = project_corpus();
    let b = project_corpus();
    assert_eq!(a, b, "corpus normalization must be deterministic");
}
