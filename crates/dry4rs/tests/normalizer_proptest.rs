//! Property tests for `SynNormalizer`.
//!
//! Pins the invariants from the O5 ADR
//! (`ops/decisions/dry-rs/adr-rust-normalization-rules.md`):
//!
//! - Determinism: `normalize(s).len() == normalize(s).len()` and the
//!   ordered `Vec<NormalizedForm>` round-trips byte-equally across
//!   invocations.
//! - `identifier_set` ordering is stable across runs (Vec equality,
//!   not just HashSet equality).
//! - Spans carry real coordinates (line >= 1) when the source parses,
//!   proving `proc-macro2`'s `span-locations` feature is active.
//! - Parse errors never panic (the adapter contractually returns
//!   `NormalizeError::Parse` on invalid Rust).

use std::path::PathBuf;

use dry4rs::domain::FilePath;
use dry4rs::parser::SynNormalizer;
use dry4rs::ports::NormalizerPort;
use proptest::prelude::*;

fn path(p: &str) -> FilePath {
    FilePath::from(PathBuf::from(p))
}

/// Pre-canned valid Rust fixtures the normalizer should handle
/// deterministically. Larger than a single literal; representative of
/// the constructs in the O5 ADR's 17-row table.
fn rust_fixtures() -> Vec<&'static str> {
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
    ]
}

#[test]
fn deterministic_across_invocations_for_curated_fixtures() {
    // The byte-equal forms invariant: same source produces an
    // identical Vec<NormalizedForm> on repeated calls. The exact
    // u64 values may shift on MSRV bumps (per ADR § Hashing), but
    // within a single test run they are stable.
    let n = SynNormalizer::new();
    let p = path("fixture.rs");
    for src in rust_fixtures() {
        let a = n.normalize(src, &p).expect("fixture must parse");
        let b = n.normalize(src, &p).expect("fixture must parse");
        assert_eq!(a, b, "non-deterministic normalize on:\n{src}");
    }
}

#[test]
fn identifier_set_order_is_stable_across_invocations() {
    // Vec equality on identifier_set proves order is preserved
    // (HashSet equality would not catch ordering bugs).
    let n = SynNormalizer::new();
    let p = path("fixture.rs");
    let src = "fn order(a: i32, b: i32) -> i32 { let c = a + b; c }";
    let a = &n.normalize(src, &p).unwrap()[0];
    let b = &n.normalize(src, &p).unwrap()[0];
    assert_eq!(
        a.identifier_set, b.identifier_set,
        "identifier_set Vec ordering must be stable"
    );
    // The fn name `order` is the first emitted identifier.
    assert_eq!(a.identifier_set[0], "order");
}

#[test]
fn span_locations_feature_is_active() {
    // If proc-macro2 ever loses the `span-locations` feature flag,
    // Span::start() / end() silently return LineColumn { line: 0, column: 0 }.
    // The CI span-locations-check enforces the feature at the dep
    // level; this is the runtime backstop.
    let n = SynNormalizer::new();
    let src = "fn at_line_one() {}\nfn at_line_two() {}\n";
    let forms = n.normalize(src, &path("two.rs")).unwrap();
    assert_eq!(forms.len(), 2);
    assert_eq!(forms[0].span.start.line, 1);
    assert_eq!(forms[1].span.start.line, 2);
}

proptest! {
    /// Property: random valid `fn name() {}` declarations parse and
    /// produce exactly one form each. The fn name varies; structure
    /// is constant. This proves the walker handles arbitrary
    /// identifier shapes (not just the test fixtures). The
    /// `_fn[a-z0-9_]+` shape sidesteps both Rust keywords (unprefixed
    /// lowercase ASCII) and the bare `_` reserved identifier.
    #[test]
    fn arbitrary_fn_name_produces_one_form(name in "_fn[a-z0-9_]{0,12}") {
        let n = SynNormalizer::new();
        let src = format!("fn {name}() {{}}");
        let forms = n.normalize(&src, &path("arb.rs")).unwrap();
        prop_assert_eq!(forms.len(), 1);
        prop_assert_eq!(&forms[0].qualified_name, &vec![name]);
    }

    /// Property: random source strings either parse successfully or
    /// return `NormalizeError::Parse` — never panic. This catches
    /// any unwrap() / panic!() that might creep into the walker.
    #[test]
    fn arbitrary_source_does_not_panic(src in "[a-zA-Z0-9_(){}; \n]{0,200}") {
        let n = SynNormalizer::new();
        // The walker either parses + walks (Ok) or returns a Parse
        // error (Err). Either is fine; a panic is a bug.
        let _ = n.normalize(&src, &path("fuzz.rs"));
    }

    /// Property: identifier_set length is bounded by the source size.
    /// Catches infinite-recursion / accidental cloning bugs in the
    /// identifier-emission path. The `_fn[a-z]+` shape sidesteps both
    /// Rust keywords and the bare `_` reserved identifier.
    #[test]
    fn identifier_set_is_bounded_by_source_size(name in "_fn[a-z]{1,8}") {
        let n = SynNormalizer::new();
        let src = format!("fn {name}() {{}}");
        let forms = n.normalize(&src, &path("bounded.rs")).unwrap();
        prop_assert!(forms[0].identifier_set.len() <= src.len());
    }

    /// Property: empty source always produces an empty form list.
    /// Trivially true but worth pinning so a future refactor doesn't
    /// accidentally emit a phantom form for an empty file.
    #[test]
    fn empty_source_always_empty(prefix in "[ \n\t]{0,20}") {
        let n = SynNormalizer::new();
        let forms = n.normalize(&prefix, &path("empty.rs")).unwrap();
        prop_assert!(forms.is_empty());
    }
}
