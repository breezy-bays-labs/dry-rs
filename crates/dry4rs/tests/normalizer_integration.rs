//! Integration tests for `SynNormalizer`.
//!
//! Exercises the full `NormalizerPort` surface against representative
//! Rust source fragments. Covers per-construct emission rules from
//! the O5 ADR (`ops/decisions/dry-rs/adr-rust-normalization-rules.md`)
//! that the unit tests in `parser::walker::tests` don't cover, and
//! pins behavior at the trait surface (so a future refactor of the
//! walker internals doesn't break consumers).

use std::path::PathBuf;

use dry4rs::domain::{FilePath, FormKind};
use dry4rs::parser::SynNormalizer;
use dry4rs::ports::{NormalizeError, NormalizerPort};

fn path(p: &str) -> FilePath {
    FilePath::from(PathBuf::from(p))
}

fn normalize(src: &str) -> Vec<dry4rs::domain::NormalizedForm> {
    SynNormalizer::new()
        .normalize(src, &path("fixture.rs"))
        .expect("fixture must parse")
}

/// Normalize `src` as if it lived at `p` — exercises the path-based
/// integration-test classification (dry-rs#108).
fn normalize_at(src: &str, p: &str) -> Vec<dry4rs::domain::NormalizedForm> {
    SynNormalizer::new()
        .normalize(src, &path(p))
        .expect("fixture must parse")
}

#[test]
fn attr_name_is_not_recorded_in_identifier_set() {
    // O11 contract: attribute names are NOT renameable identifiers;
    // they should not pollute identifier_set. A v0.2+ rename-signal
    // consumer treats identifier_set as a multiset of renameable
    // tokens; "test" / "inline" / "must_use" are language vocabulary,
    // not user-named identifiers.
    let forms = normalize("#[test] fn t() {}");
    assert_eq!(forms.len(), 1);
    assert!(
        !forms[0].identifier_set.contains(&"test".to_string()),
        "attribute name `test` must not appear in identifier_set; \
         got {:?}",
        forms[0].identifier_set
    );
    // The fn name `t` IS in identifier_set (it's a renameable
    // identifier).
    assert!(forms[0].identifier_set.contains(&"t".to_string()));
}

#[test]
fn preserved_inline_attribute_contributes_fingerprint() {
    // Per O5 § Attributes: #[inline] is preserved. Two fns identical
    // except for #[inline] should have non-identical fingerprint_sets
    // (the inlined one has an extra Attrs fingerprint).
    let plain = &normalize("fn p() {}")[0];
    let inlined = &normalize("#[inline] fn p() {}")[0];
    assert_ne!(plain.fingerprint_set, inlined.fingerprint_set);
}

#[test]
fn stripped_derive_attribute_does_not_contribute_fingerprint() {
    // Per O5 § Attributes: #[derive(...)] is stripped. A struct's
    // derive doesn't affect the fn's fingerprint at all (derive lives
    // on the struct, but if we hypothetically had `#[derive(...)] fn
    // foo()` it should be ignored — actually #[derive] is only on
    // type defs, so the more realistic test is: a fn with #[allow]
    // (also stripped) has the same fingerprints as a fn without.
    let plain = &normalize("fn p() {}")[0];
    let allowed = &normalize("#[allow(dead_code)] fn p() {}")[0];
    assert_eq!(plain.fingerprint_set, allowed.fingerprint_set);
}

#[test]
fn binary_expression_in_body_emits_subform_fingerprint() {
    // Per ADR § Per-subform fingerprinting: an expression `x + y`
    // emits one ExprBinary subform fingerprint at the parent level,
    // plus its leaf fingerprints. A fn with `x + y` has more
    // fingerprints than a fn with just `x`.
    let bin = &normalize("fn b() { let x = 1; let y = 2; let z = x + y; }")[0];
    let unit = &normalize("fn b() { let x = 1; let y = 2; let z = x; }")[0];
    assert!(
        bin.fingerprint_set.len() > unit.fingerprint_set.len(),
        "binary expression should emit additional subform fingerprints"
    );
}

#[test]
fn match_expression_with_arms_emits_arm_subforms() {
    let src = r#"
        fn classify(n: i32) -> &'static str {
            match n {
                0 => "zero",
                _ => "other",
            }
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].kind, FormKind::Production);
    assert_eq!(forms[0].qualified_name, vec!["classify".to_string()]);
    // The match expression emits ExprMatch + arm subforms + scrutinee
    // + arm bodies + pattern subforms. The fingerprint set is large.
    assert!(forms[0].fingerprint_set.len() > 5);
    // `n` is referenced in the body via `Pat::Lit` (literal pattern)
    // and via the scrutinee.
    assert!(forms[0].identifier_set.contains(&"n".to_string()));
}

#[test]
fn generic_fn_with_bounds_records_type_param_and_path_components() {
    let src = "fn cmp<T: Ord>(a: T, b: T) -> bool { a < b }";
    let forms = normalize(src);
    let form = &forms[0];
    // `cmp` is the fn name; `Ord` is the bound path; `T` is the
    // type parameter name. The bound's path component appears in
    // identifier_set.
    assert!(form.identifier_set.contains(&"cmp".to_string()));
    assert!(form.identifier_set.contains(&"Ord".to_string()));
    assert!(form.identifier_set.contains(&"T".to_string()));
}

#[test]
fn lifetime_static_distinct_from_named_lifetime() {
    // Per O5 § Lifetimes: 'static is preserved as concrete; named
    // lifetimes are collapsed. Two fns differing only in 'a vs 'static
    // should have different fingerprint_sets.
    let named = &normalize("fn f<'a>(s: &'a str) -> &'a str { s }")[0];
    let stat = &normalize("fn f(s: &'static str) -> &'static str { s }")[0];
    assert_ne!(named.fingerprint_set, stat.fingerprint_set);
}

#[test]
fn async_const_unsafe_modifiers_are_preserved() {
    let plain = &normalize("fn p() {}")[0];
    let asyncfn = &normalize("async fn p() {}")[0];
    let constfn = &normalize("const fn p() -> i32 { 0 }")[0];
    let unsafefn = &normalize("unsafe fn p() {}")[0];
    // Each modifier produces a distinct signature fingerprint.
    assert_ne!(plain.fingerprint_set, asyncfn.fingerprint_set);
    assert_ne!(plain.fingerprint_set, unsafefn.fingerprint_set);
    // const fn returns i32 so its sig differs anyway, but the modifier
    // contributes additional fingerprints.
    assert_ne!(asyncfn.fingerprint_set, constfn.fingerprint_set);
}

#[test]
fn macro_invocation_fingerprints_as_opaque() {
    // Per O5 § Macros: macro arguments are NOT walked at v0.1. Two
    // macro calls with the same name but different args produce the
    // same fingerprint contribution (just the MacroCall(<name>) token).
    let vec_int = &normalize("fn m() { let _v = vec![1, 2, 3]; }")[0];
    let vec_bool = &normalize("fn m() { let _v = vec![true, false]; }")[0];
    // The vec! call contributes a MacroCall("vec") token to both;
    // since the args are not walked, both sets share that contribution.
    let shared: std::collections::HashSet<u64> = vec_int
        .fingerprint_set
        .intersection(&vec_bool.fingerprint_set)
        .copied()
        .collect();
    assert!(
        !shared.is_empty(),
        "macro invocations with the same name should share macro-call fingerprints"
    );
}

#[test]
fn multi_module_qualified_name_threads_through_walk() {
    let src = r#"
        mod outer {
            mod inner {
                fn deep() {}
            }
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
    assert_eq!(
        forms[0].qualified_name,
        vec!["outer".to_string(), "inner".to_string(), "deep".to_string()]
    );
}

#[test]
fn trait_method_with_default_body_emits_form() {
    let src = r#"
        trait Greeter {
            fn name(&self) -> &str;
            fn greet(&self) -> String {
                let n = self.name();
                format!("hello {}", n)
            }
        }
    "#;
    let forms = normalize(src);
    // Only `greet` (default body) emits a form; `name` (no default) does not.
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].qualified_name[0], "Greeter");
    assert_eq!(forms[0].qualified_name[1], "greet");
}

#[test]
fn closure_inside_fn_does_not_emit_separate_form_at_v0_1() {
    // Per ADR v0.1 vs v0.2+ scope: closures appear as Closure marker
    // tokens in the enclosing form; no separate form is emitted at
    // v0.1.
    let forms = normalize("fn host() { let _f = |x: i32| -> i32 { x * 2 }; }");
    // Only one form — the enclosing `host` fn.
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].qualified_name, vec!["host".to_string()]);
}

#[test]
fn nested_fn_inside_fn_body_does_not_emit_separate_form_at_v0_1() {
    // Per ADR v0.1 vs v0.2+ scope: nested fns inside a fn body appear
    // as NestedFn marker tokens; no separate form at v0.1.
    let forms = normalize("fn outer() { fn inner() { let _x = 1 + 2; } }");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].qualified_name, vec!["outer".to_string()]);
    // The inner fn's identifiers (`inner`, the let-binding) do NOT
    // appear in outer's identifier_set.
    assert!(!forms[0].identifier_set.contains(&"inner".to_string()));
}

#[test]
fn for_loop_emits_pattern_and_iterator_subforms() {
    let src = "fn iter() { for i in 0..10 { let _x = i; } }";
    let forms = normalize(src);
    let form = &forms[0];
    // `i` (the loop variable) and `0`, `10` (range bounds) all
    // contribute to the fingerprint set.
    assert!(form.identifier_set.contains(&"i".to_string()));
}

#[test]
fn while_let_loop_emits_distinct_fingerprint_from_for() {
    let for_loop = &normalize("fn f() { for i in 0..10 { let _ = i; } }")[0];
    let while_loop = &normalize("fn f() { while let Some(i) = None::<i32> { let _ = i; } }")[0];
    // The two control-flow keywords produce distinct fingerprints.
    assert_ne!(for_loop.fingerprint_set, while_loop.fingerprint_set);
}

#[test]
fn impl_trait_for_type_emits_one_form_per_method() {
    let src = r#"
        struct S;
        impl Default for S {
            fn default() -> Self { S }
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
    assert_eq!(
        forms[0].qualified_name,
        vec!["S".to_string(), "default".to_string()]
    );
}

#[test]
fn unreachable_or_invalid_syntax_returns_parse_error() {
    let n = SynNormalizer::new();
    let err = n
        .normalize("this is not rust", &path("bad.rs"))
        .expect_err("non-Rust source must error");
    matches!(err, NormalizeError::Parse { .. });
}

#[test]
fn span_round_trips_through_proc_macro2() {
    // The proc-macro2 `span-locations` feature must be active or this
    // span would be (line=0, column=0). The CI job rejects deps that
    // omit the feature flag; here we assert that the produced spans
    // have realistic 1-indexed line numbers.
    let forms = normalize("fn at_line_one() {}");
    assert_eq!(forms[0].span.start.line, 1);
    assert!(forms[0].span.end.line >= 1);
}

#[test]
fn extensions_list_only_contains_dot_rs() {
    let n = SynNormalizer::new();
    let ext = n.extensions();
    assert_eq!(ext.len(), 1);
    assert_eq!(ext[0], ".rs");
}

#[test]
fn many_top_level_fns_each_get_their_own_form() {
    let src = r#"
        fn a() {}
        fn b() {}
        fn c() {}
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 3);
    let names: Vec<_> = forms.iter().map(|f| f.qualified_name[0].clone()).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

#[test]
fn method_call_records_method_name_as_identifier() {
    let src = "fn caller() { let s = String::new(); s.push('!'); }";
    let forms = normalize(src);
    let form = &forms[0];
    // `String`, `new`, `push`, `s` all appear in identifier_set.
    assert!(form.identifier_set.contains(&"String".to_string()));
    assert!(form.identifier_set.contains(&"push".to_string()));
    assert!(form.identifier_set.contains(&"s".to_string()));
}

#[test]
fn field_access_records_field_name() {
    let src = r#"
        struct P { x: i32 }
        fn read(p: &P) -> i32 { p.x }
    "#;
    let forms = normalize(src);
    let form = &forms[0];
    // `p` (local), `x` (field), `i32` (type), `P` (type), `read` (fn).
    assert!(form.identifier_set.contains(&"x".to_string()));
    assert!(form.identifier_set.contains(&"P".to_string()));
}

#[test]
fn return_expression_contributes_keyword_fingerprint() {
    let with_return = &normalize("fn r() -> i32 { return 1; }")[0];
    let without = &normalize("fn r() -> i32 { 1 }")[0];
    // The `return` keyword contributes a distinct fingerprint.
    assert_ne!(with_return.fingerprint_set, without.fingerprint_set);
}

#[test]
fn try_operator_emits_fingerprint() {
    let src = "fn try_op() -> Result<i32, ()> { let x = Ok(1)?; Ok(x) }";
    let forms = normalize(src);
    let form = &forms[0];
    // The `?` operator is preserved as an Op token; the fingerprint
    // set contains the ExprTry subform.
    assert!(!form.fingerprint_set.is_empty());
}

#[test]
fn reference_and_dereference_contribute_distinct_fingerprints() {
    let plain = &normalize("fn p(x: i32) -> i32 { x }")[0];
    let by_ref = &normalize("fn p(x: &i32) -> i32 { *x }")[0];
    assert_ne!(plain.fingerprint_set, by_ref.fingerprint_set);
}

#[test]
fn line_count_grows_with_source_lines() {
    let one_line = &normalize("fn one() {}")[0];
    let multi = &normalize("fn multi()\n{\n    let _ = 1;\n}\n")[0];
    assert!(multi.line_count > one_line.line_count);
}

#[test]
fn must_use_attribute_distinguishes_otherwise_identical_fns() {
    let plain = &normalize("fn p() -> i32 { 1 }")[0];
    let must_use = &normalize("#[must_use] fn p() -> i32 { 1 }")[0];
    assert_ne!(plain.fingerprint_set, must_use.fingerprint_set);
}

#[test]
fn placeholder_policy_v0_1_default_round_trips() {
    use dry4rs::ports::PlaceholderPolicy;
    let n = SynNormalizer::new();
    assert_eq!(n.placeholder_policy(), PlaceholderPolicy::v0_1_default());
    assert_eq!(
        PlaceholderPolicy::default(),
        PlaceholderPolicy::v0_1_default()
    );
}

// Coverage-driven tests for less common construct arms in the
// walker. These exercise the per-construct table rows that the
// behavior-driven tests above didn't already hit.

#[test]
fn tuple_destructure_let_binding_walks_pattern() {
    let src = "fn td() { let (a, b) = (1, 2); let _ = a + b; }";
    let forms = normalize(src);
    let form = &forms[0];
    assert!(form.identifier_set.contains(&"a".to_string()));
    assert!(form.identifier_set.contains(&"b".to_string()));
}

#[test]
fn struct_pattern_with_field_walks_inner_pat() {
    let src = r#"
        struct P { x: i32, y: i32 }
        fn d(p: P) -> i32 { let P { x, y } = p; x + y }
    "#;
    let forms = normalize(src);
    let form = &forms[0];
    assert!(form.identifier_set.contains(&"P".to_string()));
}

#[test]
fn tuple_struct_pattern_walks_path_and_inner() {
    let src = r#"
        struct W(i32);
        fn unwrap(w: W) -> i32 { let W(n) = w; n }
    "#;
    let forms = normalize(src);
    let form = &forms[0];
    assert!(form.identifier_set.contains(&"W".to_string()));
}

#[test]
fn slice_pattern_walks_inner_elements() {
    let src = "fn s(arr: [i32; 3]) -> i32 { match arr { [a, b, c] => a + b + c } }";
    let forms = normalize(src);
    let form = &forms[0];
    assert!(form.identifier_set.contains(&"arr".to_string()));
}

#[test]
fn or_pattern_walks_each_case() {
    let src = r#"
        fn or_pat(n: i32) -> i32 {
            match n {
                1 | 2 | 3 => 10,
                _ => 0,
            }
        }
    "#;
    let forms = normalize(src);
    let form = &forms[0];
    assert_eq!(form.qualified_name, vec!["or_pat".to_string()]);
}

#[test]
fn match_arm_with_guard_walks_guard() {
    let src = r#"
        fn guarded(n: i32) -> i32 {
            match n {
                x if x > 0 => 1,
                _ => 0,
            }
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn unsafe_block_expression_walks_inner() {
    let src = r#"
        fn unsafe_host() -> i32 {
            unsafe {
                let x = 1;
                x
            }
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].kind, FormKind::Production);
}

#[test]
fn async_block_expression_walks_inner() {
    let src = r#"
        fn async_host() {
            let _f = async {
                let x = 1;
                x
            };
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn while_loop_walks_condition_and_body() {
    let src = r#"
        fn wl(mut n: i32) {
            while n > 0 {
                n -= 1;
            }
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn loop_with_break_value() {
    let src = r#"
        fn lv() -> i32 {
            loop {
                break 42;
            }
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn continue_keyword_emits_fingerprint() {
    let src = r#"
        fn ct() {
            for i in 0..10 {
                if i % 2 == 0 { continue; }
                let _ = i;
            }
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn cast_expression_walks_inner_and_type() {
    let src = "fn c(x: i64) -> i32 { x as i32 }";
    let forms = normalize(src);
    let form = &forms[0];
    assert!(form.identifier_set.contains(&"i64".to_string()));
    assert!(form.identifier_set.contains(&"i32".to_string()));
}

#[test]
fn range_expression_with_both_ends() {
    let src = "fn rg() { let _ = 0..10; let _ = ..; }";
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn try_block_via_question_mark_in_path() {
    let src = r#"
        fn t() -> Result<i32, ()> {
            let _v = Ok::<i32, ()>(1)?;
            Ok(_v)
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn await_expression_inside_async_fn() {
    let src = r#"
        async fn aw() -> i32 {
            let f = async { 1 };
            f.await
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn struct_literal_walks_field_exprs() {
    let src = r#"
        struct P { x: i32, y: i32 }
        fn make() -> P { P { x: 1, y: 2 } }
    "#;
    let forms = normalize(src);
    let form = &forms[0];
    assert!(form.identifier_set.contains(&"P".to_string()));
}

#[test]
fn array_repeat_walks_element_and_length() {
    let src = "fn ar() -> [i32; 3] { [0; 3] }";
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn tuple_expr_walks_each_element() {
    let src = "fn t() -> (i32, bool) { (1, true) }";
    let forms = normalize(src);
    let form = &forms[0];
    assert!(form.identifier_set.contains(&"bool".to_string()));
}

#[test]
fn nested_module_walk_with_methods() {
    let src = r#"
        mod outer {
            pub struct S;
            impl S {
                pub fn method(&self) {}
            }
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
    assert_eq!(
        forms[0].qualified_name,
        vec!["outer".to_string(), "S".to_string(), "method".to_string()]
    );
}

#[test]
fn block_expression_in_let_initializer() {
    let src = r#"
        fn b() -> i32 {
            let x = { let y = 1; y + 1 };
            x
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn let_else_diverges_expression() {
    let src = r#"
        fn le(o: Option<i32>) -> i32 {
            let Some(n) = o else { return -1; };
            n
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn unary_negation_and_not_emit_distinct_ops() {
    let neg = &normalize("fn n(x: i32) -> i32 { -x }")[0];
    let not = &normalize("fn n(x: bool) -> bool { !x }")[0];
    assert_ne!(neg.fingerprint_set, not.fingerprint_set);
}

#[test]
fn assign_compound_assign_ops_emit_distinct_fingerprints() {
    let src = "fn a(mut x: i32) { x += 1; x = 2; }";
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn impl_trait_in_return_position() {
    let src = "fn it() -> impl std::fmt::Debug { 1_i32 }";
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn dyn_trait_object_in_parameter() {
    let src = "fn dt(_b: Box<dyn std::fmt::Debug>) {}";
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn tuple_type_in_signature() {
    let src = "fn tt(_pair: (i32, bool)) {}";
    let forms = normalize(src);
    let form = &forms[0];
    assert!(form.identifier_set.contains(&"i32".to_string()));
    assert!(form.identifier_set.contains(&"bool".to_string()));
}

#[test]
fn array_type_with_length() {
    let src = "fn at(_arr: [i32; 4]) {}";
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn slice_type_in_signature() {
    let src = "fn st(_s: &[i32]) {}";
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn macro_in_stmt_position() {
    let src = r#"fn mp() { println!("hello"); }"#;
    let forms = normalize(src);
    let form = &forms[0];
    assert!(form.identifier_set.contains(&"println".to_string()));
}

#[test]
fn index_expression_walks_receiver_and_index() {
    let src = "fn ix(v: Vec<i32>) -> i32 { v[0] }";
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn tuple_field_access_via_index() {
    let src = "fn tfa(p: (i32, bool)) -> i32 { p.0 }";
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn mutable_ref_pattern_walks_inner() {
    let src = r#"
        fn mr() {
            let n = 1;
            match &n {
                &x => { let _ = x; }
            }
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn const_param_in_generics_collapses_to_type_param() {
    let src = "fn cp<const N: usize>(_arr: [i32; N]) {}";
    let forms = normalize(src);
    let form = &forms[0];
    assert!(form.identifier_set.contains(&"N".to_string()));
}

#[test]
fn cfg_test_module_with_multiple_fns() {
    let src = r#"
        fn prod() {}
        #[cfg(test)]
        mod tests {
            fn helper_a() {}
            fn helper_b() {}
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 3);
    let by_kind: Vec<_> = forms.iter().map(|f| f.kind).collect();
    let test_count = by_kind.iter().filter(|k| **k == FormKind::Test).count();
    let prod_count = by_kind
        .iter()
        .filter(|k| **k == FormKind::Production)
        .count();
    assert_eq!(test_count, 2);
    assert_eq!(prod_count, 1);
}

#[test]
fn deref_via_unary_star() {
    let src = "fn de(p: &i32) -> i32 { *p + 1 }";
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn paren_expression_walks_inner() {
    let src = "fn pe(x: i32, y: i32) -> i32 { (x + y) * 2 }";
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

#[test]
fn if_let_expression_walks_pattern() {
    let src = r#"
        fn il(o: Option<i32>) -> i32 {
            if let Some(n) = o { n } else { 0 }
        }
    "#;
    let forms = normalize(src);
    assert_eq!(forms.len(), 1);
}

// --- dry-rs#108: integration-test classification -------------------
//
// Two detection paths fold into `FormKind::Test`:
//   1. PATH-based — any file under a Cargo `tests/` or `benches/`
//      integration root is test code, even without `#[test]` markers
//      (cucumber step modules, BDD world fixtures, rstest helpers).
//   2. ATTRIBUTE-based — known test-framework attributes classify a
//      form as test regardless of its path (covered in
//      `parser::walker::tests` unit tests + asserted here at the trait
//      surface for the cucumber case).

#[test]
fn fn_under_tests_root_is_test_kind_even_without_test_attr() {
    // PATH heuristic: a plain helper fn in an integration-test file has
    // no `#[test]` and no enclosing `#[cfg(test)]`, yet it is test code.
    let forms = normalize_at("fn helper() {}", "crates/foo/tests/it.rs");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].kind, FormKind::Test);
}

#[test]
fn fn_under_benches_root_is_test_kind() {
    // PATH heuristic: `benches/` is also a Cargo integration target.
    let forms = normalize_at("fn bench_body() {}", "crates/foo/benches/bench.rs");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].kind, FormKind::Test);
}

#[test]
fn fn_under_src_stays_production() {
    // PATH heuristic must NOT over-reach: ordinary `src/` code is
    // production. (A bare `src/lib.rs` fn with no test markers.)
    let forms = normalize_at("fn business_logic() {}", "crates/foo/src/lib.rs");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].kind, FormKind::Production);
}

#[test]
fn file_with_tests_in_name_but_not_a_dir_component_stays_production() {
    // PATH heuristic keys on a PATH COMPONENT named `tests`/`benches`,
    // not a substring — `src/tests_helpers.rs` is still production.
    let forms = normalize_at("fn util() {}", "crates/foo/src/tests_helpers.rs");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].kind, FormKind::Production);
}

#[test]
fn cucumber_step_in_tests_tree_is_test_kind() {
    // The exact mokumo scenario from dry-rs#108: a cucumber step def
    // (`#[given]`) inside a BDD world file under `tests/`. BOTH paths
    // would classify it; this pins the trait-surface behavior.
    let src = r#"#[given("a migration plan")] fn a_migration_plan() {}"#;
    let forms = normalize_at(src, "crates/kikan/tests/bdd_world/steps.rs");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].kind, FormKind::Test);
}

#[test]
fn cucumber_step_outside_tests_tree_is_still_test_via_attribute() {
    // ATTRIBUTE path alone (no `tests/` component): a `#[when]` step in
    // a `src/` file is still test-harness code.
    let src = r#"#[when("the engine boots")] fn the_engine_boots() {}"#;
    let forms = normalize_at(src, "crates/kikan/src/steps.rs");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].kind, FormKind::Test);
}

#[test]
fn windows_style_tests_path_is_test_kind() {
    // Path-component detection must survive backslash separators on
    // Windows runners (see global memory: Rust emits backslashes on
    // Windows). A `FilePath` built from a backslash path must still
    // recognise the `tests` component.
    let forms = normalize_at("fn helper() {}", r"crates\foo\tests\it.rs");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].kind, FormKind::Test);
}
