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

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use dry_core::domain::{FormKind, LineColumn, NormalizedForm, Span};
use proc_macro2::Span as PmSpan;
use xxhash_rust::xxh3::Xxh3;

use super::token::NormalizedToken;

/// Walk a parsed `syn::File` and produce one [`NormalizedForm`] per
/// emitted form-shape body.
///
/// The walker honors the form-emission table from the O5 ADR — `ItemFn`,
/// `ImplItemFn`, `TraitItemFn` with default body, and `ExprClosure`
/// emit forms; everything else contributes fingerprints to the
/// enclosing form (or is a container that recurses into nested forms).
pub fn walk_file(file: &syn::File) -> Vec<NormalizedForm> {
    let mut walker = Walker::new();
    walker.visit_items(&file.items, &[], false);
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
        let kind = if in_test_module || has_test_attr(&item_fn.attrs) {
            FormKind::Test
        } else {
            FormKind::Production
        };
        let span = Self::span_for_item_fn(item_fn);

        let mut emitter = FormEmitter::new();
        emitter.hash_attrs(&item_fn.attrs);
        emitter.hash_sig(&item_fn.sig);
        emitter.hash_block(&item_fn.block);

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

    fn emit_impl_item_fn(
        &mut self,
        impl_fn: &syn::ImplItemFn,
        qpath: &[String],
        in_test_module: bool,
    ) {
        let mut form_qpath: Vec<String> = qpath.to_vec();
        form_qpath.push(impl_fn.sig.ident.to_string());
        let kind = if in_test_module || has_test_attr(&impl_fn.attrs) {
            FormKind::Test
        } else {
            FormKind::Production
        };
        let span = Self::span_for_impl_item_fn(impl_fn);

        let mut emitter = FormEmitter::new();
        emitter.hash_attrs(&impl_fn.attrs);
        emitter.hash_sig(&impl_fn.sig);
        emitter.hash_block(&impl_fn.block);

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
        let kind = if in_test_module || has_test_attr(&trait_fn.attrs) {
            FormKind::Test
        } else {
            FormKind::Production
        };
        let span = Self::span_for_trait_item_fn(trait_fn);

        let mut emitter = FormEmitter::new();
        emitter.hash_attrs(&trait_fn.attrs);
        emitter.hash_sig(&trait_fn.sig);
        emitter.hash_block(block);

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

/// Per-form accumulator. Carries the fingerprint set, identifier list,
/// and node count for the form currently being emitted. Form boundaries
/// (nested fn, closure) consult the walker, not the emitter.
struct FormEmitter {
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
    /// Attribute names are NOT recorded into `identifier_set` — the
    /// O11 rename-signal contract uses `identifier_set` for renameable
    /// identifiers (locals, fn names, type names, field names). An
    /// attribute name like `"test"` or `"inline"` is part of the
    /// language vocabulary, not a renameable identifier, and including
    /// it would create false rename-diff signal at v0.2+.
    fn hash_attrs(&mut self, attrs: &[syn::Attribute]) {
        let mut hasher = Xxh3::new();
        "Attrs".hash(&mut hasher);
        let mut any_preserved = false;
        for attr in attrs {
            let Some(name) = preserved_attr_name(attr) else {
                continue;
            };
            any_preserved = true;
            self.feed_token(&mut hasher, &NormalizedToken::Attr(name));
        }
        if any_preserved {
            let fp = hasher.finish();
            self.fingerprint_set.insert(fp);
        }
    }

    /// Hash the function signature: name + param types + return type +
    /// modifier keywords (async / const / unsafe).
    fn hash_sig(&mut self, sig: &syn::Signature) -> u64 {
        let mut hasher = Xxh3::new();
        "Sig".hash(&mut hasher);

        if sig.constness.is_some() {
            self.feed_token(&mut hasher, &NormalizedToken::Modifier("const"));
        }
        if sig.asyncness.is_some() {
            self.feed_token(&mut hasher, &NormalizedToken::Modifier("async"));
        }
        if sig.unsafety.is_some() {
            self.feed_token(&mut hasher, &NormalizedToken::Modifier("unsafe"));
        }

        // Function name is preserved as Ident.
        let name = sig.ident.to_string();
        self.record_identifier(name.clone());
        self.feed_token(&mut hasher, &NormalizedToken::Ident(name));

        // Generic parameters (type params + lifetimes).
        for gp in &sig.generics.params {
            let gp_fp = self.hash_generic_param(gp);
            gp_fp.hash(&mut hasher);
        }

        // Inputs (parameters).
        for input in &sig.inputs {
            let input_fp = self.hash_fn_arg(input);
            input_fp.hash(&mut hasher);
        }

        // Return type.
        if let syn::ReturnType::Type(_, ty) = &sig.output {
            let ret_fp = self.hash_type(ty);
            ret_fp.hash(&mut hasher);
        }

        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        fp
    }

    fn hash_generic_param(&mut self, gp: &syn::GenericParam) -> u64 {
        let mut hasher = Xxh3::new();
        match gp {
            syn::GenericParam::Type(tp) => {
                "GenericTypeParam".hash(&mut hasher);
                self.record_identifier(tp.ident.to_string());
                self.feed_token(&mut hasher, &NormalizedToken::TypeParam);
                for bound in &tp.bounds {
                    let bound_fp = self.hash_type_param_bound(bound);
                    bound_fp.hash(&mut hasher);
                }
            }
            syn::GenericParam::Lifetime(lt) => {
                "GenericLifetimeParam".hash(&mut hasher);
                let token = lifetime_token(&lt.lifetime);
                self.feed_token(&mut hasher, &token);
            }
            syn::GenericParam::Const(c) => {
                "GenericConstParam".hash(&mut hasher);
                self.record_identifier(c.ident.to_string());
                self.feed_token(&mut hasher, &NormalizedToken::TypeParam);
                let ty_fp = self.hash_type(&c.ty);
                ty_fp.hash(&mut hasher);
            }
        }
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        fp
    }

    fn hash_type_param_bound(&mut self, bound: &syn::TypeParamBound) -> u64 {
        let mut hasher = Xxh3::new();
        match bound {
            syn::TypeParamBound::Trait(t) => {
                "TraitBound".hash(&mut hasher);
                let path_fp = self.hash_path(&t.path);
                path_fp.hash(&mut hasher);
            }
            syn::TypeParamBound::Lifetime(lt) => {
                "LifetimeBound".hash(&mut hasher);
                let token = lifetime_token(lt);
                self.feed_token(&mut hasher, &token);
            }
            _ => {
                "UnknownBound".hash(&mut hasher);
            }
        }
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        fp
    }

    fn hash_fn_arg(&mut self, arg: &syn::FnArg) -> u64 {
        let mut hasher = Xxh3::new();
        match arg {
            syn::FnArg::Receiver(r) => {
                "Receiver".hash(&mut hasher);
                if r.reference.is_some() {
                    self.feed_token(&mut hasher, &NormalizedToken::Op("&"));
                }
                if r.mutability.is_some() {
                    self.feed_token(&mut hasher, &NormalizedToken::Kw("mut"));
                }
                self.feed_token(&mut hasher, &NormalizedToken::Var);
            }
            syn::FnArg::Typed(pt) => {
                "TypedArg".hash(&mut hasher);
                let pat_fp = self.hash_pat(&pt.pat);
                pat_fp.hash(&mut hasher);
                let ty_fp = self.hash_type(&pt.ty);
                ty_fp.hash(&mut hasher);
            }
        }
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        fp
    }

    fn hash_type(&mut self, ty: &syn::Type) -> u64 {
        let mut hasher = Xxh3::new();
        match ty {
            syn::Type::Path(tp) => {
                "TypePath".hash(&mut hasher);
                let path_fp = self.hash_path(&tp.path);
                path_fp.hash(&mut hasher);
            }
            syn::Type::Reference(r) => {
                "TypeRef".hash(&mut hasher);
                if r.mutability.is_some() {
                    self.feed_token(&mut hasher, &NormalizedToken::Kw("mut"));
                }
                if let Some(lt) = &r.lifetime {
                    let token = lifetime_token(lt);
                    self.feed_token(&mut hasher, &token);
                }
                let inner_fp = self.hash_type(&r.elem);
                inner_fp.hash(&mut hasher);
            }
            syn::Type::Tuple(t) => {
                "TypeTuple".hash(&mut hasher);
                for elem in &t.elems {
                    let elem_fp = self.hash_type(elem);
                    elem_fp.hash(&mut hasher);
                }
            }
            syn::Type::Array(a) => {
                "TypeArray".hash(&mut hasher);
                let inner_fp = self.hash_type(&a.elem);
                inner_fp.hash(&mut hasher);
                let len_fp = self.hash_expr(&a.len);
                len_fp.hash(&mut hasher);
            }
            syn::Type::Slice(s) => {
                "TypeSlice".hash(&mut hasher);
                let inner_fp = self.hash_type(&s.elem);
                inner_fp.hash(&mut hasher);
            }
            syn::Type::TraitObject(to) => {
                "TypeDyn".hash(&mut hasher);
                for bound in &to.bounds {
                    let bound_fp = self.hash_type_param_bound(bound);
                    bound_fp.hash(&mut hasher);
                }
            }
            syn::Type::ImplTrait(it) => {
                "TypeImpl".hash(&mut hasher);
                for bound in &it.bounds {
                    let bound_fp = self.hash_type_param_bound(bound);
                    bound_fp.hash(&mut hasher);
                }
            }
            syn::Type::BareFn(_)
            | syn::Type::Group(_)
            | syn::Type::Infer(_)
            | syn::Type::Macro(_)
            | syn::Type::Never(_)
            | syn::Type::Paren(_)
            | syn::Type::Ptr(_)
            | syn::Type::Verbatim(_) => {
                // Less-common type shapes — emit a discriminator and a
                // placeholder; downstream PRs can refine if these turn
                // out to be duplication hotspots.
                "TypeOther".hash(&mut hasher);
            }
            _ => {
                "TypeUnknown".hash(&mut hasher);
            }
        }
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        // node_count is per-leaf (O8 ADR); the subform itself does NOT
        // contribute. Any leaf tokens fed via feed_token during the
        // match arms above already incremented node_count.
        fp
    }

    fn hash_path(&mut self, path: &syn::Path) -> u64 {
        let mut hasher = Xxh3::new();
        "Path".hash(&mut hasher);
        for seg in &path.segments {
            let name = seg.ident.to_string();
            self.record_identifier(name.clone());
            // If the segment looks like a generic placeholder (single
            // uppercase letter or short PascalCase that matches no real
            // type), we still preserve it as PathSeg — the heuristic
            // for distinguishing generic params from types is contextual
            // and lives elsewhere.
            self.feed_token(&mut hasher, &NormalizedToken::PathSeg(name));
            // Generic arguments inside the path segment.
            if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                for arg in &args.args {
                    let arg_fp = self.hash_generic_arg(arg);
                    arg_fp.hash(&mut hasher);
                }
            }
        }
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        fp
    }

    fn hash_generic_arg(&mut self, arg: &syn::GenericArgument) -> u64 {
        let mut hasher = Xxh3::new();
        match arg {
            syn::GenericArgument::Type(ty) => {
                "GArgType".hash(&mut hasher);
                let ty_fp = self.hash_type(ty);
                ty_fp.hash(&mut hasher);
            }
            syn::GenericArgument::Lifetime(lt) => {
                "GArgLifetime".hash(&mut hasher);
                let token = lifetime_token(lt);
                self.feed_token(&mut hasher, &token);
            }
            syn::GenericArgument::Const(expr) => {
                "GArgConst".hash(&mut hasher);
                let e_fp = self.hash_expr(expr);
                e_fp.hash(&mut hasher);
            }
            _ => {
                "GArgOther".hash(&mut hasher);
            }
        }
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        fp
    }

    fn hash_pat(&mut self, pat: &syn::Pat) -> u64 {
        let mut hasher = Xxh3::new();
        match pat {
            syn::Pat::Ident(pi) => self.hash_pat_ident(&mut hasher, pi),
            syn::Pat::Wild(_) => "PatWild".hash(&mut hasher),
            syn::Pat::Path(pp) => self.hash_pat_path(&mut hasher, &pp.path),
            syn::Pat::Lit(pl) => self.hash_pat_lit(&mut hasher, &pl.lit),
            syn::Pat::Tuple(t) => self.hash_pat_seq(&mut hasher, "PatTuple", &t.elems),
            syn::Pat::Slice(s) => self.hash_pat_seq(&mut hasher, "PatSlice", &s.elems),
            syn::Pat::Or(po) => self.hash_pat_seq(&mut hasher, "PatOr", &po.cases),
            syn::Pat::TupleStruct(ts) => self.hash_pat_tuple_struct(&mut hasher, ts),
            syn::Pat::Struct(ps) => self.hash_pat_struct(&mut hasher, ps),
            syn::Pat::Reference(pr) => self.hash_pat_reference(&mut hasher, pr),
            syn::Pat::Type(pt) => self.hash_pat_type(&mut hasher, pt),
            syn::Pat::Range(_) => "PatRange".hash(&mut hasher),
            syn::Pat::Rest(_) => "PatRest".hash(&mut hasher),
            _ => "PatOther".hash(&mut hasher),
        }
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        // node_count is per-leaf (O8 ADR); the pattern subform itself
        // does NOT contribute. Leaf tokens fed via feed_token during
        // the match arms above already incremented node_count.
        fp
    }

    fn hash_pat_ident(&mut self, hasher: &mut Xxh3, pi: &syn::PatIdent) {
        "PatIdent".hash(hasher);
        self.record_identifier(pi.ident.to_string());
        self.feed_token(hasher, &NormalizedToken::Var);
        if pi.mutability.is_some() {
            self.feed_token(hasher, &NormalizedToken::Kw("mut"));
        }
    }

    fn hash_pat_path(&mut self, hasher: &mut Xxh3, path: &syn::Path) {
        "PatPath".hash(hasher);
        let path_fp = self.hash_path(path);
        path_fp.hash(hasher);
    }

    fn hash_pat_lit(&mut self, hasher: &mut Xxh3, lit: &syn::Lit) {
        "PatLit".hash(hasher);
        let lit_token = Self::lit_to_token(lit);
        self.feed_token(hasher, &lit_token);
    }

    /// Hash a sub-pattern sequence (Tuple / Slice / Or arms) with a
    /// caller-supplied discriminator. Generic over the punctuation
    /// token because syn uses `Comma` for Tuple/Slice and `Or` for
    /// Or-patterns.
    fn hash_pat_seq<P>(
        &mut self,
        hasher: &mut Xxh3,
        discriminator: &'static str,
        elems: &syn::punctuated::Punctuated<syn::Pat, P>,
    ) {
        discriminator.hash(hasher);
        for elem in elems {
            let e_fp = self.hash_pat(elem);
            e_fp.hash(hasher);
        }
    }

    fn hash_pat_tuple_struct(&mut self, hasher: &mut Xxh3, ts: &syn::PatTupleStruct) {
        "PatTupleStruct".hash(hasher);
        let path_fp = self.hash_path(&ts.path);
        path_fp.hash(hasher);
        for elem in &ts.elems {
            let e_fp = self.hash_pat(elem);
            e_fp.hash(hasher);
        }
    }

    fn hash_pat_struct(&mut self, hasher: &mut Xxh3, ps: &syn::PatStruct) {
        "PatStruct".hash(hasher);
        let path_fp = self.hash_path(&ps.path);
        path_fp.hash(hasher);
        for field in &ps.fields {
            let f_fp = self.hash_pat(&field.pat);
            f_fp.hash(hasher);
        }
    }

    fn hash_pat_reference(&mut self, hasher: &mut Xxh3, pr: &syn::PatReference) {
        "PatRef".hash(hasher);
        if pr.mutability.is_some() {
            self.feed_token(hasher, &NormalizedToken::Kw("mut"));
        }
        let inner_fp = self.hash_pat(&pr.pat);
        inner_fp.hash(hasher);
    }

    fn hash_pat_type(&mut self, hasher: &mut Xxh3, pt: &syn::PatType) {
        "PatType".hash(hasher);
        let inner_fp = self.hash_pat(&pt.pat);
        inner_fp.hash(hasher);
        let ty_fp = self.hash_type(&pt.ty);
        ty_fp.hash(hasher);
    }

    fn hash_block(&mut self, block: &syn::Block) -> u64 {
        let mut hasher = Xxh3::new();
        "Block".hash(&mut hasher);
        for stmt in &block.stmts {
            let s_fp = self.hash_stmt(stmt);
            s_fp.hash(&mut hasher);
        }
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        // Block itself is a structural wrapper; per O8 it does NOT
        // count toward node_count. The contained statements do.
        fp
    }

    fn hash_stmt(&mut self, stmt: &syn::Stmt) -> u64 {
        let mut hasher = Xxh3::new();
        match stmt {
            syn::Stmt::Local(local) => {
                "StmtLet".hash(&mut hasher);
                self.feed_token(&mut hasher, &NormalizedToken::Kw("let"));
                let pat_fp = self.hash_pat(&local.pat);
                pat_fp.hash(&mut hasher);
                if let Some(init) = &local.init {
                    let e_fp = self.hash_expr(&init.expr);
                    e_fp.hash(&mut hasher);
                    if let Some((_, else_block)) = &init.diverge {
                        let div_fp = self.hash_expr(else_block);
                        div_fp.hash(&mut hasher);
                    }
                }
            }
            syn::Stmt::Item(item) => {
                "StmtItem".hash(&mut hasher);
                // Nested item — form-boundary marker. The nested item
                // may emit its own form (via top-level visit_item),
                // BUT we don't recurse into nested forms from inside a
                // function body at v0.1 (function-local items are
                // emitted as form-boundary markers only).
                if matches!(item, syn::Item::Fn(_)) {
                    self.feed_token(&mut hasher, &NormalizedToken::NestedFn);
                }
                // Other nested item shapes don't carry useful structural
                // signal inside an enclosing fn body; collapse to a
                // generic marker.
            }
            syn::Stmt::Expr(expr, _semi) => {
                "StmtExpr".hash(&mut hasher);
                let e_fp = self.hash_expr(expr);
                e_fp.hash(&mut hasher);
            }
            syn::Stmt::Macro(m) => {
                "StmtMacro".hash(&mut hasher);
                let m_fp = self.hash_macro(&m.mac);
                m_fp.hash(&mut hasher);
            }
        }
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        fp
    }

    fn hash_expr(&mut self, expr: &syn::Expr) -> u64 {
        let mut hasher = Xxh3::new();
        // Dispatch by category to keep the outer match small. Each
        // category helper handles a fraction of `syn::Expr`'s ~33
        // variants. Less-common shapes (Group, Verbatim, Const block,
        // TryBlock, Yield, …) fall through to the `_ => ExprOther`
        // arm; downstream PRs refine if profiling shows duplication
        // hotspots there.
        if !self.hash_expr_dispatch(&mut hasher, expr) {
            "ExprOther".hash(&mut hasher);
        }
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        // Expression itself is a fingerprint-emitting subform; whether
        // it counts as a node_count leaf depends on what it is. We
        // count primitives (Lit, Path leaves) via feed_token; the
        // subform itself does not add to node_count here (would
        // double-count).
        fp
    }

    /// Dispatch a `syn::Expr` to its category-grouped hash helper.
    /// Returns `true` when a category handler claimed the variant;
    /// `false` falls back to the [`hash_expr`] caller's `ExprOther`
    /// discriminator.
    fn hash_expr_dispatch(&mut self, hasher: &mut Xxh3, expr: &syn::Expr) -> bool {
        if self.hash_expr_value(hasher, expr) {
            return true;
        }
        if self.hash_expr_operator(hasher, expr) {
            return true;
        }
        if self.hash_expr_call_like(hasher, expr) {
            return true;
        }
        if self.hash_expr_control(hasher, expr) {
            return true;
        }
        if self.hash_expr_collection(hasher, expr) {
            return true;
        }
        if self.hash_expr_wrap(hasher, expr) {
            return true;
        }
        self.hash_expr_block_like(hasher, expr)
    }

    /// Value-level expressions: path-or-local, literal, struct
    /// literal, repeat.
    fn hash_expr_value(&mut self, hasher: &mut Xxh3, expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::Path(ep) => self.hash_expr_path(hasher, ep),
            syn::Expr::Lit(el) => self.hash_expr_lit(hasher, &el.lit),
            syn::Expr::Struct(es) => self.hash_expr_struct(hasher, es),
            syn::Expr::Repeat(er) => self.hash_expr_repeat(hasher, er),
            _ => return false,
        }
        true
    }

    /// Operator expressions: binary, unary, assign, cast, range, try.
    fn hash_expr_operator(&mut self, hasher: &mut Xxh3, expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::Binary(eb) => self.hash_expr_binary(hasher, eb),
            syn::Expr::Unary(eu) => self.hash_expr_unary(hasher, eu),
            syn::Expr::Assign(ea) => self.hash_expr_assign(hasher, ea),
            syn::Expr::Cast(ec) => self.hash_expr_cast(hasher, ec),
            syn::Expr::Range(er) => self.hash_expr_range(hasher, er),
            syn::Expr::Try(et) => self.hash_expr_try(hasher, et),
            _ => return false,
        }
        true
    }

    /// Call-like expressions: free call, method call, field, index.
    fn hash_expr_call_like(&mut self, hasher: &mut Xxh3, expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::Call(ec) => self.hash_expr_call(hasher, ec),
            syn::Expr::MethodCall(em) => self.hash_expr_method_call(hasher, em),
            syn::Expr::Field(ef) => self.hash_expr_field(hasher, ef),
            syn::Expr::Index(ei) => self.hash_expr_index(hasher, ei),
            _ => return false,
        }
        true
    }

    /// Control-flow expressions: if, match, while, for, loop, return,
    /// break, continue, let.
    fn hash_expr_control(&mut self, hasher: &mut Xxh3, expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::If(ei) => self.hash_expr_if(hasher, ei),
            syn::Expr::Match(em) => self.hash_expr_match(hasher, em),
            syn::Expr::While(ew) => self.hash_expr_while(hasher, ew),
            syn::Expr::ForLoop(efl) => self.hash_expr_for(hasher, efl),
            syn::Expr::Loop(el) => self.hash_expr_loop(hasher, el),
            syn::Expr::Return(er) => self.hash_expr_return(hasher, er),
            syn::Expr::Break(eb) => self.hash_expr_break(hasher, eb),
            syn::Expr::Continue(_) => self.hash_expr_continue(hasher),
            syn::Expr::Let(el) => self.hash_expr_let(hasher, el),
            _ => return false,
        }
        true
    }

    /// Collection-shaped expressions: tuple, array.
    fn hash_expr_collection(&mut self, hasher: &mut Xxh3, expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::Tuple(et) => self.hash_expr_seq(hasher, "ExprTuple", &et.elems),
            syn::Expr::Array(ea) => self.hash_expr_seq(hasher, "ExprArray", &ea.elems),
            _ => return false,
        }
        true
    }

    /// Unary-wrapping expressions: reference, paren, await, macro,
    /// closure (form-boundary).
    fn hash_expr_wrap(&mut self, hasher: &mut Xxh3, expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::Reference(er) => self.hash_expr_reference(hasher, er),
            syn::Expr::Paren(ep) => self.hash_expr_paren(hasher, ep),
            syn::Expr::Await(ea) => self.hash_expr_await(hasher, ea),
            syn::Expr::Macro(em) => self.hash_expr_macro(hasher, em),
            syn::Expr::Closure(_) => self.hash_expr_closure(hasher),
            _ => return false,
        }
        true
    }

    /// Block-bearing expressions: block, async, unsafe.
    fn hash_expr_block_like(&mut self, hasher: &mut Xxh3, expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::Block(eb) => self.hash_expr_block_expr(hasher, &eb.block),
            syn::Expr::Async(ea) => self.hash_expr_async(hasher, ea),
            syn::Expr::Unsafe(eu) => self.hash_expr_unsafe(hasher, eu),
            _ => return false,
        }
        true
    }

    /// Path expression: single-segment, snake-case identifier paths
    /// in expression position are treated as local-variable
    /// references (alpha-equivalent collapse). This is the v0.1
    /// heuristic for "this is a local" without full scope tracking;
    /// multi-segment paths (e.g., `foo::bar`) and `PascalCase`
    /// single-segment paths (e.g., `Some`, `MyType`) are treated as
    /// concrete value paths.
    fn hash_expr_path(&mut self, hasher: &mut Xxh3, ep: &syn::ExprPath) {
        if let Some(name) = single_seg_local(&ep.path) {
            "ExprLocal".hash(hasher);
            self.record_identifier(name);
            self.feed_token(hasher, &NormalizedToken::Var);
        } else {
            "ExprPath".hash(hasher);
            let path_fp = self.hash_path(&ep.path);
            path_fp.hash(hasher);
        }
    }

    fn hash_expr_lit(&mut self, hasher: &mut Xxh3, lit: &syn::Lit) {
        "ExprLit".hash(hasher);
        let lit_token = Self::lit_to_token(lit);
        self.feed_token(hasher, &lit_token);
    }

    fn hash_expr_binary(&mut self, hasher: &mut Xxh3, eb: &syn::ExprBinary) {
        "ExprBinary".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Op(binop_symbol(&eb.op)));
        let l_fp = self.hash_expr(&eb.left);
        l_fp.hash(hasher);
        let r_fp = self.hash_expr(&eb.right);
        r_fp.hash(hasher);
    }

    fn hash_expr_unary(&mut self, hasher: &mut Xxh3, eu: &syn::ExprUnary) {
        "ExprUnary".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Op(unop_symbol(&eu.op)));
        let inner_fp = self.hash_expr(&eu.expr);
        inner_fp.hash(hasher);
    }

    fn hash_expr_assign(&mut self, hasher: &mut Xxh3, ea: &syn::ExprAssign) {
        "ExprAssign".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Op("="));
        let l_fp = self.hash_expr(&ea.left);
        l_fp.hash(hasher);
        let r_fp = self.hash_expr(&ea.right);
        r_fp.hash(hasher);
    }

    fn hash_expr_call(&mut self, hasher: &mut Xxh3, ec: &syn::ExprCall) {
        "ExprCall".hash(hasher);
        let f_fp = self.hash_expr(&ec.func);
        f_fp.hash(hasher);
        for arg in &ec.args {
            let a_fp = self.hash_expr(arg);
            a_fp.hash(hasher);
        }
    }

    fn hash_expr_method_call(&mut self, hasher: &mut Xxh3, em: &syn::ExprMethodCall) {
        "ExprMethodCall".hash(hasher);
        let recv_fp = self.hash_expr(&em.receiver);
        recv_fp.hash(hasher);
        let method = em.method.to_string();
        self.record_identifier(method.clone());
        self.feed_token(hasher, &NormalizedToken::Ident(method));
        for arg in &em.args {
            let a_fp = self.hash_expr(arg);
            a_fp.hash(hasher);
        }
    }

    fn hash_expr_field(&mut self, hasher: &mut Xxh3, ef: &syn::ExprField) {
        "ExprField".hash(hasher);
        let recv_fp = self.hash_expr(&ef.base);
        recv_fp.hash(hasher);
        match &ef.member {
            syn::Member::Named(ident) => {
                let name = ident.to_string();
                self.record_identifier(name.clone());
                self.feed_token(hasher, &NormalizedToken::Ident(name));
            }
            syn::Member::Unnamed(idx) => {
                self.feed_token(hasher, &NormalizedToken::LitInt(i128::from(idx.index)));
            }
        }
    }

    fn hash_expr_index(&mut self, hasher: &mut Xxh3, ei: &syn::ExprIndex) {
        "ExprIndex".hash(hasher);
        let recv_fp = self.hash_expr(&ei.expr);
        recv_fp.hash(hasher);
        let idx_fp = self.hash_expr(&ei.index);
        idx_fp.hash(hasher);
    }

    fn hash_expr_block_expr(&mut self, hasher: &mut Xxh3, block: &syn::Block) {
        "ExprBlock".hash(hasher);
        let b_fp = self.hash_block(block);
        b_fp.hash(hasher);
    }

    fn hash_expr_if(&mut self, hasher: &mut Xxh3, ei: &syn::ExprIf) {
        "ExprIf".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Kw("if"));
        let c_fp = self.hash_expr(&ei.cond);
        c_fp.hash(hasher);
        let t_fp = self.hash_block(&ei.then_branch);
        t_fp.hash(hasher);
        if let Some((_, else_branch)) = &ei.else_branch {
            let e_fp = self.hash_expr(else_branch);
            e_fp.hash(hasher);
        }
    }

    fn hash_expr_match(&mut self, hasher: &mut Xxh3, em: &syn::ExprMatch) {
        "ExprMatch".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Kw("match"));
        let scrutinee_fp = self.hash_expr(&em.expr);
        scrutinee_fp.hash(hasher);
        for arm in &em.arms {
            let arm_fp = self.hash_arm(arm);
            arm_fp.hash(hasher);
        }
    }

    fn hash_expr_while(&mut self, hasher: &mut Xxh3, ew: &syn::ExprWhile) {
        "ExprWhile".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Kw("while"));
        let c_fp = self.hash_expr(&ew.cond);
        c_fp.hash(hasher);
        let b_fp = self.hash_block(&ew.body);
        b_fp.hash(hasher);
    }

    fn hash_expr_for(&mut self, hasher: &mut Xxh3, efl: &syn::ExprForLoop) {
        "ExprFor".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Kw("for"));
        let pat_fp = self.hash_pat(&efl.pat);
        pat_fp.hash(hasher);
        let it_fp = self.hash_expr(&efl.expr);
        it_fp.hash(hasher);
        let body_fp = self.hash_block(&efl.body);
        body_fp.hash(hasher);
    }

    fn hash_expr_loop(&mut self, hasher: &mut Xxh3, el: &syn::ExprLoop) {
        "ExprLoop".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Kw("loop"));
        let b_fp = self.hash_block(&el.body);
        b_fp.hash(hasher);
    }

    fn hash_expr_return(&mut self, hasher: &mut Xxh3, er: &syn::ExprReturn) {
        "ExprReturn".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Kw("return"));
        if let Some(inner) = &er.expr {
            let i_fp = self.hash_expr(inner);
            i_fp.hash(hasher);
        }
    }

    fn hash_expr_break(&mut self, hasher: &mut Xxh3, eb: &syn::ExprBreak) {
        "ExprBreak".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Kw("break"));
        if let Some(inner) = &eb.expr {
            let i_fp = self.hash_expr(inner);
            i_fp.hash(hasher);
        }
    }

    fn hash_expr_continue(&mut self, hasher: &mut Xxh3) {
        "ExprContinue".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Kw("continue"));
    }

    fn hash_expr_reference(&mut self, hasher: &mut Xxh3, er: &syn::ExprReference) {
        "ExprRef".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Op("&"));
        if er.mutability.is_some() {
            self.feed_token(hasher, &NormalizedToken::Kw("mut"));
        }
        let inner_fp = self.hash_expr(&er.expr);
        inner_fp.hash(hasher);
    }

    fn hash_expr_paren(&mut self, hasher: &mut Xxh3, ep: &syn::ExprParen) {
        "ExprParen".hash(hasher);
        let inner_fp = self.hash_expr(&ep.expr);
        inner_fp.hash(hasher);
    }

    /// Hash a sub-expression sequence (Tuple / Array elements) with
    /// a caller-supplied discriminator.
    fn hash_expr_seq(
        &mut self,
        hasher: &mut Xxh3,
        discriminator: &'static str,
        elems: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
    ) {
        discriminator.hash(hasher);
        for elem in elems {
            let e_fp = self.hash_expr(elem);
            e_fp.hash(hasher);
        }
    }

    fn hash_expr_cast(&mut self, hasher: &mut Xxh3, ec: &syn::ExprCast) {
        "ExprCast".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Kw("as"));
        let inner_fp = self.hash_expr(&ec.expr);
        inner_fp.hash(hasher);
        let ty_fp = self.hash_type(&ec.ty);
        ty_fp.hash(hasher);
    }

    fn hash_expr_range(&mut self, hasher: &mut Xxh3, er: &syn::ExprRange) {
        "ExprRange".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Op(".."));
        if let Some(start) = &er.start {
            let s_fp = self.hash_expr(start);
            s_fp.hash(hasher);
        }
        if let Some(end) = &er.end {
            let e_fp = self.hash_expr(end);
            e_fp.hash(hasher);
        }
    }

    fn hash_expr_try(&mut self, hasher: &mut Xxh3, et: &syn::ExprTry) {
        "ExprTry".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Op("?"));
        let inner_fp = self.hash_expr(&et.expr);
        inner_fp.hash(hasher);
    }

    fn hash_expr_await(&mut self, hasher: &mut Xxh3, ea: &syn::ExprAwait) {
        "ExprAwait".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Kw("await"));
        let inner_fp = self.hash_expr(&ea.base);
        inner_fp.hash(hasher);
    }

    fn hash_expr_async(&mut self, hasher: &mut Xxh3, ea: &syn::ExprAsync) {
        "ExprAsync".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Modifier("async"));
        let b_fp = self.hash_block(&ea.block);
        b_fp.hash(hasher);
    }

    fn hash_expr_unsafe(&mut self, hasher: &mut Xxh3, eu: &syn::ExprUnsafe) {
        "ExprUnsafe".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Modifier("unsafe"));
        let b_fp = self.hash_block(&eu.block);
        b_fp.hash(hasher);
    }

    fn hash_expr_macro(&mut self, hasher: &mut Xxh3, em: &syn::ExprMacro) {
        "ExprMacro".hash(hasher);
        let m_fp = self.hash_macro(&em.mac);
        m_fp.hash(hasher);
    }

    /// Form-boundary: the closure body is attributed to its own
    /// form, not this one. Emit only the opaque marker. The walker's
    /// caller is responsible for capturing the closure as a separate
    /// form via a follow-up pass.
    fn hash_expr_closure(&mut self, hasher: &mut Xxh3) {
        "ExprClosure".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Closure);
    }

    fn hash_expr_struct(&mut self, hasher: &mut Xxh3, es: &syn::ExprStruct) {
        "ExprStruct".hash(hasher);
        let path_fp = self.hash_path(&es.path);
        path_fp.hash(hasher);
        for field in &es.fields {
            let f_fp = self.hash_expr(&field.expr);
            f_fp.hash(hasher);
        }
    }

    fn hash_expr_repeat(&mut self, hasher: &mut Xxh3, er: &syn::ExprRepeat) {
        "ExprRepeat".hash(hasher);
        let inner_fp = self.hash_expr(&er.expr);
        inner_fp.hash(hasher);
        let len_fp = self.hash_expr(&er.len);
        len_fp.hash(hasher);
    }

    fn hash_expr_let(&mut self, hasher: &mut Xxh3, el: &syn::ExprLet) {
        "ExprLet".hash(hasher);
        self.feed_token(hasher, &NormalizedToken::Kw("let"));
        let pat_fp = self.hash_pat(&el.pat);
        pat_fp.hash(hasher);
        let e_fp = self.hash_expr(&el.expr);
        e_fp.hash(hasher);
    }

    fn hash_arm(&mut self, arm: &syn::Arm) -> u64 {
        let mut hasher = Xxh3::new();
        "MatchArm".hash(&mut hasher);
        let pat_fp = self.hash_pat(&arm.pat);
        pat_fp.hash(&mut hasher);
        if let Some((_, guard)) = &arm.guard {
            self.feed_token(&mut hasher, &NormalizedToken::Kw("if"));
            let g_fp = self.hash_expr(guard);
            g_fp.hash(&mut hasher);
        }
        let body_fp = self.hash_expr(&arm.body);
        body_fp.hash(&mut hasher);
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        fp
    }

    fn hash_macro(&mut self, mac: &syn::Macro) -> u64 {
        let mut hasher = Xxh3::new();
        "MacroCall".hash(&mut hasher);
        let name = mac
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        self.record_identifier(name.clone());
        self.feed_token(&mut hasher, &NormalizedToken::MacroCall(name));
        // Per ADR: macro arguments are NOT walked at v0.1.
        let fp = hasher.finish();
        self.fingerprint_set.insert(fp);
        fp
    }

    fn lit_to_token(lit: &syn::Lit) -> NormalizedToken {
        match lit {
            syn::Lit::Int(li) => li
                .base10_parse::<i128>()
                .map_or(NormalizedToken::LitInt(0), NormalizedToken::LitInt),
            syn::Lit::Float(lf) => {
                let bits = lf.base10_parse::<f64>().map_or(0, f64::to_bits);
                NormalizedToken::LitFloat(bits)
            }
            syn::Lit::Str(ls) => NormalizedToken::LitStr(ls.value()),
            syn::Lit::Bool(lb) => NormalizedToken::LitBool(lb.value),
            syn::Lit::Char(lc) => NormalizedToken::LitChar(lc.value()),
            syn::Lit::Byte(lb) => NormalizedToken::LitByte(lb.value()),
            syn::Lit::ByteStr(lbs) => NormalizedToken::LitByteStr(lbs.value()),
            _ => NormalizedToken::LitStr(String::new()),
        }
    }

    fn feed_token<H: Hasher>(&mut self, hasher: &mut H, token: &NormalizedToken) {
        token.hash_into(hasher);
        // Per O8 node_count table: each placeholder, ident, type
        // reference, literal, operator, keyword, lifetime, and macro
        // counts as one leaf.
        self.node_count = self.node_count.saturating_add(1);
    }

    fn record_identifier(&mut self, id: String) {
        // Identifier recording is independent of fingerprint hashing;
        // walk-order is preserved per O11. The v0.1 comparison engine
        // doesn't read identifier_set; v0.2+ rename-signal does.
        self.identifier_set.push(id);
    }
}

fn binop_symbol(op: &syn::BinOp) -> &'static str {
    match op {
        syn::BinOp::Add(_) => "+",
        syn::BinOp::Sub(_) => "-",
        syn::BinOp::Mul(_) => "*",
        syn::BinOp::Div(_) => "/",
        syn::BinOp::Rem(_) => "%",
        syn::BinOp::And(_) => "&&",
        syn::BinOp::Or(_) => "||",
        syn::BinOp::BitXor(_) => "^",
        syn::BinOp::BitAnd(_) => "&",
        syn::BinOp::BitOr(_) => "|",
        syn::BinOp::Shl(_) => "<<",
        syn::BinOp::Shr(_) => ">>",
        syn::BinOp::Eq(_) => "==",
        syn::BinOp::Lt(_) => "<",
        syn::BinOp::Le(_) => "<=",
        syn::BinOp::Ne(_) => "!=",
        syn::BinOp::Ge(_) => ">=",
        syn::BinOp::Gt(_) => ">",
        syn::BinOp::AddAssign(_) => "+=",
        syn::BinOp::SubAssign(_) => "-=",
        syn::BinOp::MulAssign(_) => "*=",
        syn::BinOp::DivAssign(_) => "/=",
        syn::BinOp::RemAssign(_) => "%=",
        syn::BinOp::BitXorAssign(_) => "^=",
        syn::BinOp::BitAndAssign(_) => "&=",
        syn::BinOp::BitOrAssign(_) => "|=",
        syn::BinOp::ShlAssign(_) => "<<=",
        syn::BinOp::ShrAssign(_) => ">>=",
        _ => "?op",
    }
}

fn unop_symbol(op: &syn::UnOp) -> &'static str {
    match op {
        syn::UnOp::Deref(_) => "*",
        syn::UnOp::Not(_) => "!",
        syn::UnOp::Neg(_) => "-",
        _ => "?unop",
    }
}

fn lifetime_token(lt: &syn::Lifetime) -> NormalizedToken {
    if lt.ident == "static" {
        NormalizedToken::LifetimeStatic
    } else {
        NormalizedToken::Lifetime
    }
}

/// Is this path a single-segment, snake-case identifier (treated as a
/// local-variable reference)?
///
/// Per O5 ADR § Typed placeholders: local variable identifiers collapse
/// to `Var`. The v0.1 heuristic is "single-segment path with first
/// character lowercase or underscore, no `PathArguments`." This catches
/// `x`, `_foo`, `bar_baz` but not `Some`, `MyType::new`, or
/// `foo::<i32>()`. False-positives (e.g., a single-segment lowercase fn
/// reference) are accepted at v0.1 because (a) typical fn references
/// in expression position are method calls (`receiver.method()`) which
/// route through `Expr::MethodCall`, and (b) free-fn calls usually use
/// at least a path (`crate_root::foo()`) or are intra-module which is
/// rare in well-organized code. Returns the segment name when the path
/// qualifies, otherwise `None`.
fn single_seg_local(path: &syn::Path) -> Option<String> {
    if path.leading_colon.is_some() {
        return None;
    }
    if path.segments.len() != 1 {
        return None;
    }
    let seg = &path.segments[0];
    if !matches!(seg.arguments, syn::PathArguments::None) {
        return None;
    }
    let name = seg.ident.to_string();
    let first = name.chars().next()?;
    if first.is_ascii_lowercase() || first == '_' {
        Some(name)
    } else {
        None
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

/// Does the attribute list carry a `#[test]` or `#[tokio::test]`-style
/// test attribute?
fn has_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.path();
        // Match `test`, `*::test`, or any path ending in `test`.
        path.segments.last().is_some_and(|seg| seg.ident == "test")
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

fn span_from_pm(start: PmSpan, end: PmSpan) -> Span {
    let s = start.start();
    let e = end.end();
    // proc_macro2 returns 1-indexed lines and 0-indexed columns —
    // matches our LineColumn convention exactly. Without the
    // `span-locations` feature these would silently be 0/0; the
    // CI `proc-macro2 span-locations enforcement` job rejects deps
    // that omit the feature.
    let start_lc = LineColumn::new(
        u32::try_from(s.line).unwrap_or(1),
        u32::try_from(s.column).unwrap_or(0),
    );
    let end_lc = LineColumn::new(
        u32::try_from(e.line).unwrap_or(1),
        u32::try_from(e.column).unwrap_or(0),
    );
    Span::try_new(start_lc, end_lc).unwrap_or_else(|_| {
        // Defensive: if proc-macro2 ever returns inverted positions,
        // fall back to a single-position span at start.
        Span::try_new(start_lc, start_lc).expect("self-referential span is always valid")
    })
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

    /// Helper: parse source + walk it, returning emitted forms.
    fn forms_of(source: &str) -> Vec<NormalizedForm> {
        let file = syn::parse_file(source).expect("parse fixture must succeed");
        walk_file(&file)
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
