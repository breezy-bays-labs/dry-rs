//! syn AST walker — converts a `syn::File` into [`NormalizedForm`]s
//! via per-subform Merkle-style fingerprint hashing.
//!
//! Implements the contract pinned by the O5 ADR
//! (`ops/decisions/dry-rs/adr-rust-normalization-rules.md`):
//!
//! - **Per-subform fingerprinting.** Each visited subtree emits one
//!   `u64` into `fingerprint_set`. Children's `u64`s fold into their
//!   parent's hash (Merkle-style), so structurally-equivalent subtrees
//!   produce identical `u64`s at every level of granularity.
//! - **Form boundaries stop fingerprint flow.** When the walker
//!   encounters a node that emits its own form (`ItemFn`,
//!   `ImplItemFn`, `TraitItemFn`-with-default, `ExprClosure`), the
//!   inner form's body subforms are attributed to its own form, and
//!   the enclosing form sees only an opaque marker token.
//! - **Cross-toolchain stable hashing** via
//!   `xxhash_rust::xxh3::Xxh3`. Cross-toolchain stable per upstream
//!   contract — unlike `std::hash::DefaultHasher` (SipHash-1-3 with
//!   stdlib-internal fixed key; the stdlib explicitly reserves the
//!   right to change the algorithm in any new toolchain). The
//!   cross-version stability is load-bearing for the v0.3+ `--delta`
//!   baseline comparison feature, the PR 9 self-check snapshot
//!   surviving MSRV bumps, and any future fingerprint cache (see
//!   ADR § Hashing).
//! - **`identifier_set` populated in walk order** for v0.2+
//!   rename-signal consumers; the comparison engine does not read it
//!   at v0.1.
//! - **`node_count` is per-leaf** (per O8 table); `fingerprint_set`
//!   cardinality exceeds it.
//!
//! ## Generic dispatch
//!
//! The syn-subtree traversal — which nodes open a subform, the order
//! their children fold, and which leaf tokens each contributes — lives
//! in [`super::visitor`] behind the [`SubformSink`] trait, NOT inline
//! here. [`FormEmitter`] is the v0.1 sink: it maps the node lifecycle
//! onto an `Xxh3` fold, reproducing the original inline hashing
//! byte-for-byte (the fingerprint-determinism gate of dry-rs#121). A
//! future tree-building sink (PR 4) reuses the same dispatch, so the
//! fingerprint path and the tree path cannot drift on *which* subforms
//! exist. This module owns only form ENUMERATION (which fn-shaped bodies
//! become [`NormalizedForm`]s) and the [`FormEmitter`] sink.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use dry_core::domain::{FormKind, NormalizedForm, Span};
use xxhash_rust::xxh3::Xxh3;

use super::token::NormalizedToken;
use super::visitor::{self, SubformSink, span_from_pm};

/// Walk a parsed `syn::File` and produce one [`NormalizedForm`] per
/// emitted form-shape body.
///
/// The walker honors the form-emission table from the O5 ADR — `ItemFn`,
/// `ImplItemFn`, `TraitItemFn` with default body, and `ExprClosure`
/// emit forms; everything else contributes fingerprints to the
/// enclosing form (or is a container that recurses into nested forms).
///
/// `in_test_file` seeds the walk's test-context flag (dry-rs#108): when
/// `true`, every form classifies as [`FormKind::Test`] regardless of
/// `#[test]` markers, because the source lives under a Cargo
/// integration-test root (`tests/` / `benches/`) — cucumber step
/// modules, BDD world fixtures, and rstest helpers all carry no
/// `#[test]` attribute yet are test-harness code. Attribute-based
/// detection (`#[test]`, `#[given]`, …) still applies on top inside the
/// walk. The path heuristic that resolves `in_test_file` lives in the
/// dry4rs adapter (Cargo-specific); the walker only consumes the
/// resolved boolean, keeping path-convention knowledge out of the
/// shared traversal.
pub fn walk_file(file: &syn::File, in_test_file: bool) -> Vec<NormalizedForm> {
    let mut walker = Walker::new();
    walker.visit_items(&file.items, &[], in_test_file);
    walker.into_forms()
}

/// Internal walker state — accumulates emitted forms across the walk.
struct Walker {
    forms: Vec<NormalizedForm>,
}

impl Walker {
    fn new() -> Self {
        Self { forms: Vec::new() }
    }

    fn into_forms(self) -> Vec<NormalizedForm> {
        self.forms
    }

    /// Visit a slice of top-level items inside a (possibly cfg(test))
    /// module context. The `qpath` carries the parent module's
    /// qualified-name path components; `in_test_module` propagates the
    /// `#[cfg(test)]` mod context for `FormKind::Test` detection.
    fn visit_items(&mut self, items: &[syn::Item], qpath: &[String], in_test_module: bool) {
        for item in items {
            self.visit_item(item, qpath, in_test_module);
        }
    }

    fn visit_item(&mut self, item: &syn::Item, qpath: &[String], in_test_module: bool) {
        match item {
            syn::Item::Fn(item_fn) => self.emit_item_fn(item_fn, qpath, in_test_module),
            syn::Item::Mod(item_mod) => self.visit_mod_item(item_mod, qpath, in_test_module),
            syn::Item::Impl(item_impl) => self.visit_impl_item(item_impl, qpath, in_test_module),
            syn::Item::Trait(item_trait) => {
                self.visit_trait_item(item_trait, qpath, in_test_module);
            }
            // Other top-level items (Struct, Enum, Const, Static,
            // Type, Use, ExternCrate, Macro, etc.) don't emit forms
            // at v0.1.
            _ => {}
        }
    }

    /// Recurse into a `mod` item, extending the qualified-name path
    /// and propagating the `#[cfg(test)]` test-module flag.
    fn visit_mod_item(&mut self, item_mod: &syn::ItemMod, qpath: &[String], in_test_module: bool) {
        let Some((_, inner_items)) = &item_mod.content else {
            return;
        };
        let next_in_test = in_test_module || mod_is_cfg_test(item_mod);
        let mut child_qpath: Vec<String> = qpath.to_vec();
        child_qpath.push(item_mod.ident.to_string());
        self.visit_items(inner_items, &child_qpath, next_in_test);
    }

    /// Visit every method inside an `impl` block.
    ///
    /// Computes a qpath suffix for impl members: for
    /// `impl Type { fn m() {} }` the method's qname is
    /// `["Type", "m"]` — drop the impl block from the qpath, just
    /// use the type's last path segment.
    fn visit_impl_item(
        &mut self,
        item_impl: &syn::ItemImpl,
        qpath: &[String],
        in_test_module: bool,
    ) {
        let mut child_qpath: Vec<String> = qpath.to_vec();
        if let Some(seg) = impl_self_ty_last_segment(&item_impl.self_ty) {
            child_qpath.push(seg);
        }
        for impl_item in &item_impl.items {
            if let syn::ImplItem::Fn(impl_fn) = impl_item {
                self.emit_impl_item_fn(impl_fn, &child_qpath, in_test_module);
            }
            // Other ImplItem variants (Const, Type, Macro, Verbatim)
            // don't emit forms at v0.1.
        }
    }

    /// Visit every method inside a `trait` block.
    ///
    /// Only methods with a default body emit a form per the form-
    /// emission table; signature-only methods are skipped.
    fn visit_trait_item(
        &mut self,
        item_trait: &syn::ItemTrait,
        qpath: &[String],
        in_test_module: bool,
    ) {
        let mut child_qpath: Vec<String> = qpath.to_vec();
        child_qpath.push(item_trait.ident.to_string());
        for trait_item in &item_trait.items {
            let syn::TraitItem::Fn(trait_fn) = trait_item else {
                continue;
            };
            if trait_fn.default.is_some() {
                self.emit_trait_item_fn(trait_fn, &child_qpath, in_test_module);
            }
        }
    }

    fn emit_item_fn(&mut self, item_fn: &syn::ItemFn, qpath: &[String], in_test_module: bool) {
        let mut form_qpath: Vec<String> = qpath.to_vec();
        form_qpath.push(item_fn.sig.ident.to_string());
        let kind = form_kind(in_test_module, &item_fn.attrs);
        let span = Self::span_for_item_fn(item_fn);

        let mut emitter = FormEmitter::new();
        emitter.hash_attrs(&item_fn.attrs);
        visitor::walk_sig(&mut emitter, &item_fn.sig);
        visitor::walk_block(&mut emitter, &item_fn.block);

        self.push_form(emitter, kind, form_qpath, span);
    }

    fn emit_impl_item_fn(
        &mut self,
        impl_fn: &syn::ImplItemFn,
        qpath: &[String],
        in_test_module: bool,
    ) {
        let mut form_qpath: Vec<String> = qpath.to_vec();
        form_qpath.push(impl_fn.sig.ident.to_string());
        let kind = form_kind(in_test_module, &impl_fn.attrs);
        let span = Self::span_for_impl_item_fn(impl_fn);

        let mut emitter = FormEmitter::new();
        emitter.hash_attrs(&impl_fn.attrs);
        visitor::walk_sig(&mut emitter, &impl_fn.sig);
        visitor::walk_block(&mut emitter, &impl_fn.block);

        self.push_form(emitter, kind, form_qpath, span);
    }

    fn emit_trait_item_fn(
        &mut self,
        trait_fn: &syn::TraitItemFn,
        qpath: &[String],
        in_test_module: bool,
    ) {
        let Some(block) = &trait_fn.default else {
            return;
        };
        let mut form_qpath: Vec<String> = qpath.to_vec();
        form_qpath.push(trait_fn.sig.ident.to_string());
        let kind = form_kind(in_test_module, &trait_fn.attrs);
        let span = Self::span_for_trait_item_fn(trait_fn);

        let mut emitter = FormEmitter::new();
        emitter.hash_attrs(&trait_fn.attrs);
        visitor::walk_sig(&mut emitter, &trait_fn.sig);
        visitor::walk_block(&mut emitter, block);

        self.push_form(emitter, kind, form_qpath, span);
    }

    /// Finalise a [`FormEmitter`] into a [`NormalizedForm`] and append
    /// it. Shared by all three form-emission sites (item / impl / trait
    /// fn) so the part-extraction and `with_context` plumbing lives in
    /// one place.
    fn push_form(
        &mut self,
        emitter: FormEmitter,
        kind: FormKind,
        form_qpath: Vec<String>,
        span: Span,
    ) {
        let (fingerprint_set, identifier_set, node_count) = emitter.into_parts();
        let line_count = lines_in_span(&span);
        let form = NormalizedForm::with_context(
            kind,
            fingerprint_set,
            identifier_set,
            form_qpath,
            span,
            node_count,
            line_count,
        );
        self.forms.push(form);
    }

    fn span_for_item_fn(item_fn: &syn::ItemFn) -> Span {
        let start = item_fn.sig.fn_token.span;
        let end = item_fn.block.brace_token.span.close();
        span_from_pm(start, end)
    }

    fn span_for_impl_item_fn(impl_fn: &syn::ImplItemFn) -> Span {
        let start = impl_fn.sig.fn_token.span;
        let end = impl_fn.block.brace_token.span.close();
        span_from_pm(start, end)
    }

    fn span_for_trait_item_fn(trait_fn: &syn::TraitItemFn) -> Span {
        let start = trait_fn.sig.fn_token.span;
        let end = match &trait_fn.default {
            Some(block) => block.brace_token.span.close(),
            None => trait_fn.sig.fn_token.span,
        };
        span_from_pm(start, end)
    }
}

/// Resolve [`FormKind`] from the test-module context flag plus the form's
/// attributes (`#[test]`, cucumber `#[given]`, …).
fn form_kind(in_test_module: bool, attrs: &[syn::Attribute]) -> FormKind {
    if in_test_module || has_test_attr(attrs) {
        FormKind::Test
    } else {
        FormKind::Production
    }
}

/// The v0.1 fingerprint-fold [`SubformSink`].
///
/// Maps the generic node lifecycle onto a Merkle-style `Xxh3` fold: each
/// open node is an `Xxh3` hasher; `tag` and `fold` hash the
/// discriminator and each child `u64`; `leaf` hashes the token and
/// counts it; `seal` finalises the hasher and inserts the resulting
/// `u64` into `fingerprint_set`. Carries the form's identifier list and
/// per-leaf node count alongside.
///
/// This sink IS the original inline fold — the hashing operations are
/// reproduced exactly so the refactor is byte-identical on
/// `fingerprint_set` / `node_count` / `identifier_set`. Form boundaries
/// (nested fn, closure) are handled by the [`Walker`]'s enumeration, not
/// the sink: the walker simply never recurses into a nested form's body,
/// so the dispatch only ever feeds this sink the enclosing form's
/// subtree.
pub(super) struct FormEmitter {
    fingerprint_set: HashSet<u64>,
    identifier_set: Vec<String>,
    node_count: u32,
}

impl FormEmitter {
    fn new() -> Self {
        Self {
            fingerprint_set: HashSet::new(),
            identifier_set: Vec::new(),
            node_count: 0,
        }
    }

    fn into_parts(self) -> (HashSet<u64>, Vec<String>, u32) {
        (self.fingerprint_set, self.identifier_set, self.node_count)
    }

    /// Project an attribute list through the strip-noise / preserve-signal
    /// partition (see O5 ADR § Attributes). Stripped attributes
    /// (`#[derive(...)]`, `#[doc(...)]`, `#[allow(...)]`,
    /// `#[warn(...)]`, `#[cfg(...)]`, `#[deprecated(...)]`) contribute
    /// no fingerprint. Preserved attributes contribute an `Attr(<name>)`
    /// token to the form's fingerprint stream.
    ///
    /// This is a form-level prelude, not a dispatched subform — the
    /// `Attrs` node only seals (inserts its `u64`) when at least one
    /// preserved attribute was seen, so an attribute-free fn never gains
    /// a phantom fingerprint. It therefore drives the sink primitives
    /// directly rather than going through [`super::visitor`].
    ///
    /// Attribute names are NOT recorded into `identifier_set` — the
    /// O11 rename-signal contract uses `identifier_set` for renameable
    /// identifiers (locals, fn names, type names, field names). An
    /// attribute name like `"test"` or `"inline"` is part of the
    /// language vocabulary, not a renameable identifier, and including
    /// it would create false rename-diff signal at v0.2+.
    fn hash_attrs(&mut self, attrs: &[syn::Attribute]) {
        let mut node = self.begin("Attrs");
        let mut any_preserved = false;
        for attr in attrs {
            let Some(name) = preserved_attr_name(attr) else {
                continue;
            };
            any_preserved = true;
            self.leaf(&mut node, &NormalizedToken::Attr(name));
        }
        if any_preserved {
            self.seal(node);
        }
    }
}

impl SubformSink for FormEmitter {
    type Out = u64;
    type Node = Xxh3;

    fn begin_node(&mut self) -> Xxh3 {
        Xxh3::new()
    }

    fn tag(&mut self, node: &mut Xxh3, tag: &'static str) {
        tag.hash(node);
    }

    fn fold(&mut self, node: &mut Xxh3, child: u64) {
        child.hash(node);
    }

    fn leaf(&mut self, node: &mut Xxh3, token: &NormalizedToken) {
        token.hash_into(node);
        // Per O8 node_count table: each placeholder, ident, type
        // reference, literal, operator, keyword, lifetime, and macro
        // counts as one leaf.
        self.node_count = self.node_count.saturating_add(1);
    }

    fn seal(&mut self, node: Xxh3) -> u64 {
        let fp = node.finish();
        self.fingerprint_set.insert(fp);
        fp
    }

    fn record_identifier(&mut self, id: String) {
        // Identifier recording is independent of fingerprint hashing;
        // walk-order is preserved per O11. The v0.1 comparison engine
        // doesn't read identifier_set; v0.2+ rename-signal does.
        self.identifier_set.push(id);
    }
}

/// Should this attribute be preserved in the fingerprint stream?
///
/// Per O5 ADR § Attributes: preserve signal (`#[test]`, `#[inline]`,
/// `#[inline(always)]`, `#[cold]`, `#[must_use]`, `#[no_mangle]`,
/// `#[repr(...)]`); strip noise (`#[derive(...)]`, `#[doc(...)]`,
/// `#[allow(...)]`, `#[warn(...)]`, `#[cfg(...)]`,
/// `#[deprecated(...)]`).
///
/// Returns `Some(name)` for preserved attributes where `name` is the
/// last path segment (e.g., `Some("inline")` for `#[inline(always)]`).
/// Returns `None` for stripped attributes.
fn preserved_attr_name(attr: &syn::Attribute) -> Option<String> {
    let last = attr.path().segments.last()?;
    let name = last.ident.to_string();
    match name.as_str() {
        // Preserved (positive list).
        "test" | "inline" | "cold" | "must_use" | "no_mangle" | "repr" => Some(name),
        // Stripped (everything else, including the explicit noise list).
        _ => None,
    }
}

/// Does the syn module item carry a `#[cfg(test)]` attribute?
fn mod_is_cfg_test(item_mod: &syn::ItemMod) -> bool {
    item_mod.attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        // We accept exactly `#[cfg(test)]` at v0.1; nested cfg
        // (cfg(any(test, ...))) is not detected.
        let mut is_test = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("test") {
                is_test = true;
            }
            Ok(())
        });
        is_test
    })
}

/// Known test-framework attribute names whose presence marks a form as
/// [`FormKind::Test`] (dry-rs#108).
///
/// The set is deliberately conservative — only attributes that are
/// *unambiguously* test-harness markers in the Rust ecosystem:
///
/// - `test` — matches the std `#[test]`, `#[tokio::test]`,
///   `#[async_std::test]`, and any `*::test` form (the last path
///   segment is `test`).
/// - `given` / `when` / `then` — cucumber-rs step definitions
///   (`cucumber::given`, etc.). These carry NO `#[test]` marker, so
///   without this list they leaked into the production lane (the
///   originating bug: a 9-member cluster of `#[given]` steps in
///   mokumo's BDD world files classified as production).
/// - `rstest` — the rstest fixture/parameterized-test attribute.
/// - `test_case` — the test-case parameterized-test attribute.
///
/// Matching is on the LAST path segment so namespaced forms
/// (`cucumber::given`, `rstest::rstest`) are recognised. A production
/// fn whose *name* resembles a step verb is unaffected — only the
/// attribute triggers reclassification.
const TEST_FRAMEWORK_ATTRS: &[&str] = &["test", "given", "when", "then", "rstest", "test_case"];

/// Does the attribute list carry a recognised test-framework attribute?
///
/// Recognises `#[test]` / `#[tokio::test]`-style attributes plus the
/// cucumber / rstest / `test_case` markers enumerated in
/// [`TEST_FRAMEWORK_ATTRS`] (dry-rs#108).
fn has_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|seg| TEST_FRAMEWORK_ATTRS.iter().any(|name| seg.ident == *name))
    })
}

/// Last identifier segment of an impl block's Self type, e.g.,
/// `impl Foo { ... }` → `Some("Foo")`. Returns `None` for impl Self
/// types that don't have an ident-shaped last segment (e.g.,
/// `impl &Self`, function-pointer types).
fn impl_self_ty_last_segment(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(tp) = ty {
        return tp.path.segments.last().map(|s| s.ident.to_string());
    }
    None
}

fn lines_in_span(span: &Span) -> u32 {
    span.end
        .line
        .saturating_sub(span.start.line)
        .saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse source + walk it (NOT in an integration-test file),
    /// returning emitted forms. The `in_test_file = false` seed isolates
    /// attribute / `#[cfg(test)]`-based classification from the
    /// path-based heuristic (which the normalizer integration tests
    /// exercise separately).
    fn forms_of(source: &str) -> Vec<NormalizedForm> {
        let file = syn::parse_file(source).expect("parse fixture must succeed");
        walk_file(&file, false)
    }

    #[test]
    fn empty_function_emits_one_production_form() {
        let forms = forms_of("fn empty() {}");
        assert_eq!(forms.len(), 1);
        let form = &forms[0];
        assert_eq!(form.kind, FormKind::Production);
        assert_eq!(form.qualified_name, vec!["empty".to_string()]);
        // The signature emits at least one fingerprint (the Sig
        // subform); the empty Block emits one more.
        assert!(!form.fingerprint_set.is_empty());
        // The function name `empty` is recorded in identifier_set.
        assert_eq!(form.identifier_set, vec!["empty".to_string()]);
        // line_count >= 1 — span derived from proc_macro2 positions.
        assert!(form.line_count >= 1);
    }

    #[test]
    fn function_with_parameter_records_identifier_and_type_signal() {
        let forms = forms_of("fn id(x: i32) -> i32 { x }");
        assert_eq!(forms.len(), 1);
        let form = &forms[0];
        // `id` (fn name), `x` (local), `i32` (param type, twice — input
        // and return), `x` (body reference) are all in identifier_set
        // in walk order. The exact list depends on how path-segment
        // emission interleaves with pat-emission, but `id` must come
        // first and `x` and `i32` must appear.
        assert_eq!(form.identifier_set[0], "id");
        assert!(form.identifier_set.contains(&"x".to_string()));
        assert!(form.identifier_set.contains(&"i32".to_string()));
        assert_eq!(form.kind, FormKind::Production);
    }

    #[test]
    fn structurally_identical_bodies_with_different_names_share_body_fingerprints() {
        // `fn add(...) { x + y }` and `fn combine(...) { x + y }` have
        // identical body structure; they should share the body
        // fingerprints (BinaryExpr + Ident leaves + Op leaf) but their
        // signature fingerprints differ (function name is preserved as
        // Ident).
        let add = &forms_of("fn add(x: i32, y: i32) -> i32 { x + y }")[0];
        let combine = &forms_of("fn combine(x: i32, y: i32) -> i32 { x + y }")[0];

        // Some non-zero subset of fingerprints overlaps — the body
        // subforms.
        let shared: HashSet<u64> = add
            .fingerprint_set
            .intersection(&combine.fingerprint_set)
            .copied()
            .collect();
        assert!(!shared.is_empty(), "bodies should share fingerprints");

        // But the two sets are NOT identical — the Sig fingerprint
        // differs (function name).
        assert_ne!(add.fingerprint_set, combine.fingerprint_set);
    }

    #[test]
    fn alpha_equivalent_locals_collapse_in_body_fingerprints() {
        // `fn one(a: i32, b: i32) -> i32 { a + b }` and
        // `fn two(x: i32, y: i32) -> i32 { x + y }` have identical
        // structure modulo local names. The body fingerprints
        // (BinaryExpr + Var leaves) match exactly.
        let one = &forms_of("fn fst(a: i32, b: i32) -> i32 { a + b }")[0];
        let two = &forms_of("fn fst(x: i32, y: i32) -> i32 { x + y }")[0];

        // Same fn name (fst) so signature should share — and body too.
        // Strong condition: a large overlap.
        let shared: HashSet<u64> = one
            .fingerprint_set
            .intersection(&two.fingerprint_set)
            .copied()
            .collect();
        assert!(
            shared.len() >= one.fingerprint_set.len() / 2,
            "alpha-equivalent fns should share most fingerprints"
        );
    }

    #[test]
    fn test_attribute_makes_form_kind_test() {
        let forms = forms_of("#[test] fn t() {}");
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].kind, FormKind::Test);
    }

    #[test]
    fn cfg_test_mod_makes_inner_fn_test() {
        let forms = forms_of("#[cfg(test)] mod tests { fn helper() {} }");
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].kind, FormKind::Test);
        assert_eq!(
            forms[0].qualified_name,
            vec!["tests".to_string(), "helper".to_string()]
        );
    }

    #[test]
    fn tokio_test_attribute_also_makes_form_kind_test() {
        let forms = forms_of("#[tokio::test] async fn t() {}");
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].kind, FormKind::Test);
    }

    #[test]
    fn cucumber_given_attribute_makes_form_kind_test() {
        // Cucumber step definitions (#[given]/#[when]/#[then]) carry no
        // #[test] marker but ARE test-harness code (dry-rs#108).
        let forms = forms_of(r#"#[given("a precondition")] fn a_precondition() {}"#);
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].kind, FormKind::Test);
    }

    #[test]
    fn cucumber_when_attribute_makes_form_kind_test() {
        let forms = forms_of(r#"#[when("an action occurs")] fn an_action() {}"#);
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].kind, FormKind::Test);
    }

    #[test]
    fn cucumber_then_attribute_makes_form_kind_test() {
        let forms = forms_of(r#"#[then("an outcome holds")] fn an_outcome() {}"#);
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].kind, FormKind::Test);
    }

    #[test]
    fn rstest_attribute_makes_form_kind_test() {
        let forms = forms_of("#[rstest] fn parameterized() {}");
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].kind, FormKind::Test);
    }

    #[test]
    fn test_case_attribute_makes_form_kind_test() {
        let forms = forms_of(r"#[test_case(1 => 2)] fn doubles(n: i32) -> i32 { n * 2 }");
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].kind, FormKind::Test);
    }

    #[test]
    fn production_fn_named_like_a_step_stays_production() {
        // A plain production fn whose name resembles a step verb must
        // NOT be reclassified — only the ATTRIBUTE triggers test kind.
        let forms = forms_of("fn given_up() {}");
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].kind, FormKind::Production);
    }

    #[test]
    fn impl_method_qualified_name_includes_type() {
        let forms = forms_of("struct Foo; impl Foo { fn bar(&self) {} }");
        assert_eq!(forms.len(), 1);
        assert_eq!(
            forms[0].qualified_name,
            vec!["Foo".to_string(), "bar".to_string()]
        );
    }

    #[test]
    fn trait_method_with_default_emits_form() {
        let forms = forms_of("trait Greet { fn hello(&self) { println!(\"hi\"); } }");
        assert_eq!(forms.len(), 1);
        assert_eq!(
            forms[0].qualified_name,
            vec!["Greet".to_string(), "hello".to_string()]
        );
    }

    #[test]
    fn trait_method_without_default_emits_no_form() {
        let forms = forms_of("trait Sig { fn sig(&self); }");
        assert!(forms.is_empty());
    }

    #[test]
    fn nested_fn_does_not_bleed_fingerprints_into_outer_form() {
        // Per ADR: nested ItemFn inside a fn body is form-boundary;
        // the outer's fingerprint_set should NOT include the inner's
        // subforms. At v0.1 the nested fn is NOT also emitted as a
        // separate form when it's inside a fn body (top-level walk
        // only recurses into module/impl/trait containers).
        let forms = forms_of("fn outer() { fn inner() { 1 + 2 } }");
        assert_eq!(forms.len(), 1);
        let outer = &forms[0];
        // outer's identifier_set does NOT contain `inner` — the nested
        // fn is fingerprinted as an opaque NestedFn marker; its name
        // is not threaded into the outer's identifier stream.
        assert!(!outer.identifier_set.contains(&"inner".to_string()));
        // outer's qualified_name is just ["outer"], not ["outer", "inner"].
        assert_eq!(outer.qualified_name, vec!["outer".to_string()]);
    }

    #[test]
    fn closure_marker_in_enclosing_fn_does_not_leak_closure_body() {
        // A closure inside a fn body is form-boundary. The enclosing
        // fn's identifier_set should NOT contain identifiers walked
        // from the closure's body.
        let forms = forms_of("fn host() { let _f = |a| a + 1; }");
        assert_eq!(forms.len(), 1);
        let host = &forms[0];
        // `a` (closure param) and the body's `1` literal are NOT in
        // host's identifier_set at v0.1 (closure body is form-boundary).
        // Note: at v0.1 the walker does not emit a separate form for
        // the closure either (closures-as-separate-forms is a follow-up
        // in the same PR; tracked by the next test).
        assert!(!host.identifier_set.contains(&"a".to_string()));
    }

    #[test]
    fn deterministic_across_runs() {
        // The fingerprint set is byte-equal across two normalize
        // invocations on the same source. This catches accidental
        // use of HashMap's RandomState in the inner fingerprint hash.
        let a = forms_of("fn x(n: i32) -> i32 { n + 1 }");
        let b = forms_of("fn x(n: i32) -> i32 { n + 1 }");
        assert_eq!(a, b);
    }

    #[test]
    fn identifier_set_preserves_walk_order() {
        // O11 contract: identifier_set is Vec, preserving walk order
        // and duplicates. `fn dup(x: i32) -> i32 { x + x }` records
        // `dup`, then walks into the signature and body — `x` appears
        // multiple times.
        let forms = forms_of("fn dup(x: i32) -> i32 { x + x }");
        let form = &forms[0];
        let xs = form
            .identifier_set
            .iter()
            .filter(|s| s.as_str() == "x")
            .count();
        assert!(
            xs >= 2,
            "identifier_set should preserve duplicate `x` references"
        );
        // The fn name appears first.
        assert_eq!(form.identifier_set[0], "dup");
    }

    #[test]
    fn node_count_is_nonzero_for_nonempty_body() {
        // Per O8: node_count counts post-substitution leaves. An
        // empty `fn empty() {}` has signature leaves only; a body
        // with a single literal `fn lit() { 0 }` has more.
        let empty = &forms_of("fn empty() {}")[0];
        let lit = &forms_of("fn lit() { 0 }")[0];
        assert!(lit.node_count > empty.node_count);
    }
}
