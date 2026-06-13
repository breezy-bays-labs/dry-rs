//! Integration tests for the run-loop tree re-derive + anti-unification
//! template attach (epic #107, dry-rs#135).
//!
//! These tests drive the REAL `dry4rs` binary (`CARGO_BIN_EXE_dry4rs`)
//! end-to-end — clap parse → walker → normalize → compare → template
//! attach → JSON envelope — against on-disk fixture clusters, plus the
//! REAL `SynNormalizer` adapter at the library level for the
//! edited-on-disk failure path. The PURE attach/skip decision surface
//! (`decide_template`, `tree_top_level_fps_in_bag`, the re-read failure
//! arm) is unit-tested in `dry-core::cli::run`.

use std::path::PathBuf;
use std::process::Command;

use dry4rs::domain::FilePath;
use dry4rs::parser::SynNormalizer;
use dry4rs::ports::{NormalizerPort, TreeDeriverPort};
use serde_json::Value;

/// Write a fixture crate with three near-identical functions under a
/// fresh tempdir's `src/` and return the dir handle. The three bodies
/// are structurally identical apart from the function name and the
/// helper they call, so they cluster as ONE 3-member near-duplicate
/// match at the default threshold.
fn three_member_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("mkdir src");
    std::fs::write(
        src.join("lib.rs"),
        r"
pub fn alpha(input: i32) -> i32 {
    let total = input + 1;
    let scaled = total * 2;
    let result = scaled - 3;
    result + total
}

pub fn beta(value: i32) -> i32 {
    let total = value + 1;
    let scaled = total * 2;
    let result = scaled - 3;
    result + total
}

pub fn gamma(number: i32) -> i32 {
    let total = number + 1;
    let scaled = total * 2;
    let result = scaled - 3;
    result + total
}
",
    )
    .expect("write lib.rs");
    dir
}

fn run_dry4rs_json(path: &std::path::Path) -> Value {
    let bin = env!("CARGO_BIN_EXE_dry4rs");
    let output = Command::new(bin)
        .args(["report", "--format", "json", "--no-fail"])
        .arg(path)
        .output()
        .expect("dry4rs binary should execute");
    assert!(
        output.status.success(),
        "dry4rs --no-fail must exit 0: status={:?}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "dry4rs JSON must parse: {err}, stdout={}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn three_member_cluster_carries_a_template_with_derived_scores() {
    // INTEGRATION: a known 3-member fixture cluster, run through the real
    // binary, surfaces a single multi-member match that CARRIES an
    // anti-unification template with the reserved score slots derived.
    let dir = three_member_fixture();
    let envelope = run_dry4rs_json(&dir.path().join("src"));
    let matches = envelope["result"]["matches"]
        .as_array()
        .expect("result.matches array");

    // Exactly one near-duplicate cluster surfaces across the three fns.
    let cluster = matches
        .iter()
        .find(|m| m["forms"].as_array().is_some_and(|f| f.len() == 3))
        .expect("a 3-member cluster must surface");

    // The template attached (graceful path succeeded — same source on
    // disk, no fp drift), and the reserved score slots are now DERIVED
    // (numbers, not null) per `Match::with_template`.
    let template = &cluster["template"];
    assert!(
        template.is_object(),
        "the 3-member cluster must carry a template, got: {cluster}"
    );
    let holes = template["holes"].as_array().expect("template.holes array");
    assert!(
        !holes.is_empty(),
        "the three bodies diverge (function/param names) so the template \
         must carry at least one hole"
    );

    // Derived score slots: all three populated (reserved-then-derived),
    // structural_score >= score, every value in [0, 1].
    let score = cluster["score"].as_f64().expect("score f64");
    let structural = cluster["structural_score"]
        .as_f64()
        .expect("structural_score must be derived (non-null) once a template attaches");
    let rename_count = cluster["rename_count"]
        .as_u64()
        .expect("rename_count must be derived (non-null)");
    assert!(
        structural >= score - 1e-12,
        "structural_score {structural} must be >= score {score}"
    );
    assert!((0.0..=1.0).contains(&structural));
    assert!((0.0..=1.0).contains(&score));
    // rename_density is null only for a hole-free template; this cluster
    // has holes, so it is present.
    assert!(
        cluster["rename_density"].is_number(),
        "rename_density must be derived for a hole-bearing template"
    );

    // Substitutions are index-joined with Match.forms (3 members).
    for hole in holes {
        let subs = hole["substitutions"]
            .as_array()
            .expect("substitutions array");
        assert_eq!(
            subs.len(),
            3,
            "each hole's substitutions must be index-joined with the 3 forms"
        );
        let divergence = &hole["divergence"];
        assert_eq!(divergence["members"], 3, "divergence.members == 3");
    }

    // The wire carried rename_count as a concrete value (the assertion
    // above already unwrapped it); keep the binding meaningful for the
    // reader.
    let _ = rename_count;
}

#[test]
fn single_member_paths_carry_no_template() {
    // A source with no duplication produces no multi-member match, so no
    // template attaches anywhere (singletons never get a template).
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("mkdir src");
    std::fs::write(
        src.join("lib.rs"),
        "pub fn lonely(x: i32) -> i32 { x.wrapping_mul(7).rotate_left(3) ^ 0x55 }\n",
    )
    .expect("write lib.rs");

    let envelope = run_dry4rs_json(&src);
    for m in envelope["result"]["matches"]
        .as_array()
        .expect("matches array")
    {
        assert!(
            m.get("template").is_none(),
            "no multi-member cluster exists, so no template should attach: {m}"
        );
    }
}

#[test]
fn edited_on_disk_span_no_longer_resolves_to_a_form() {
    // EDITED-ON-DISK foundation: the run loop locates each member's tree
    // by re-parsing the file and matching `form_span`. If the source
    // changed between detection and re-derive, the stored span no longer
    // addresses a form and `derive_tree` returns `Err` — which the run
    // loop turns into a template-None (never a panic, never a wrong
    // template). This drives the REAL `SynNormalizer` adapter to prove
    // the failure path's foundation end-to-end.
    let normalizer = SynNormalizer::new();
    let path = FilePath::from(PathBuf::from("fixture.rs"));

    // Two forms of DIFFERENT lengths so a shift can never make one form's
    // new span coincide with the other's stored span.
    let original = "fn a() -> i32 {\n    let t = 1;\n    t + 2\n}\nfn b() -> i32 { 7 }\n";
    let forms = normalizer.normalize(original, &path).expect("normalize");
    assert_eq!(forms.len(), 2, "two forms in the original source");
    let stored_span = forms[1].span;

    // The user edits the file: insert a fresh function at the top,
    // shifting every form below it down by several lines. The
    // previously-stored span of `b` now addresses no form.
    let edited = format!("fn inserted() {{\n    let _ = 0;\n    let _ = 1;\n}}\n{original}");
    let result = normalizer.derive_tree(&edited, stored_span);
    assert!(
        result.is_err(),
        "a stored span that no longer addresses a form after an on-disk \
         edit must Err (-> template None), never panic; got Ok"
    );
}

#[test]
fn edited_on_disk_unparseable_source_errs_not_panics() {
    // A second edited-on-disk shape: the file no longer parses at all
    // (mid-edit save). `derive_tree` returns `Err` rather than
    // panicking, so the run loop degrades to template None.
    let normalizer = SynNormalizer::new();
    let path = FilePath::from(PathBuf::from("fixture.rs"));
    let original = "fn a() -> i32 { 1 }\n";
    let span = normalizer.normalize(original, &path).expect("normalize")[0].span;

    let broken = "fn a( -> i32 { this is not valid rust ;;;";
    let result = normalizer.derive_tree(broken, span);
    assert!(
        result.is_err(),
        "unparseable on-disk source must Err (-> template None), never panic"
    );
}
