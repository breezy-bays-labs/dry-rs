//! Generic syn-subtree visitor — the single source of truth for *which*
//! syn nodes a form body is decomposed into, *in what order*, and *which
//! leaf tokens* each node contributes.
//!
//! ## Why a generic visitor
//!
//! Two consumers walk a function body's syn subtree: the v0.1
//! fingerprint fold ([`super::walker::FormEmitter`], which collapses each
//! subtree to a `u64` Merkle hash) and the future tree builder (PR 4,
//! which will materialise a `NormalizedTree`). Both MUST agree on the
//! exact set of subforms — which `syn::Expr` variants open a node, the
//! order children are folded, and which leaves are fed — or the fold and
//! the tree will silently disagree on what "the same form" means.
//!
//! Before this refactor that dispatch was inlined into the fold as a
//! ~500-line `hash_*` fan-out; a second consumer would have had to clone
//! it, and the two copies would drift. This module factors the dispatch
//! out behind the [`SubformSink`] trait so a single `walk_*` skeleton
//! drives any sink. [`super::walker::FormEmitter`] is the v0.1 sink that
//! reproduces the original fold byte-for-byte.
//!
//! ## The sink contract
//!
//! A [`SubformSink`] sees the walk as a stream of *node* lifecycles:
//!
//! 1. [`SubformSink::begin`] opens a node with a `&'static str` kind tag
//!    (`"Block"`, `"ExprBinary"`, …).
//! 2. Children fold in via [`SubformSink::fold`] (a sub-node's `Out`) and
//!    leaves via [`SubformSink::leaf`] (a [`NormalizedToken`]), in walk
//!    order.
//! 3. [`SubformSink::seal`] closes the node and yields its `Out`.
//!
//! The fingerprint fold maps this onto an `Xxh3` per node: `begin` hashes
//! the tag, `fold` hashes the child `u64`, `leaf` hashes the token (and
//! bumps `node_count`), `seal` finalises and inserts the `u64`. A future
//! tree builder maps the same lifecycle onto node construction. Because
//! the dispatch lives here and only the lifecycle mapping differs, the
//! two paths cannot diverge on subform structure.
//!
//! Identifier recording ([`SubformSink::record_identifier`]) is a
//! cross-cutting side channel (the O11 rename signal), independent of the
//! node lifecycle and preserved in walk order.

use std::hash::{Hash, Hasher};

use dry_core::domain::Span;
use proc_macro2::Span as PmSpan;
use xxhash_rust::xxh3::Xxh3;

use super::token::NormalizedToken;

/// Fold a form's ordered top-level subform fingerprints into a single
/// form-level root fingerprint.
///
/// A form has no single root in the fingerprint fold: form emission is a
/// fixed sequence of top-level subforms — the (conditional) `Attrs`
/// prelude, the `Sig` subform, then the `Block` subform — each sealing
/// its own `u64` into the form's `fingerprint_set`. This helper composes
/// those ordered top-level `u64`s into ONE root fingerprint by hashing a
/// `"Form"` discriminator tag followed by each top-level child `u64`, in
/// emission order — the SAME `tag`-then-`fold` primitive sequence the
/// fingerprint fold applies at every other node boundary
/// ([`super::walker::FormEmitter`]).
///
/// It is the single source of truth for the form-level root fold, shared
/// by the tree builder's synthetic root node and the fingerprint path's
/// form-fingerprint accessor, so the tree's `root.fp` cannot drift from
/// the fold — the P3 anti-drift bridge (`derive_tree`'s `root.fp`
/// equals the form's stored top-level fold) rests on this one function.
#[must_use]
pub(super) fn fold_form_fp(top_level: &[u64]) -> u64 {
    let mut hasher = Xxh3::new();
    "Form".hash(&mut hasher);
    for child in top_level {
        child.hash(&mut hasher);
    }
    hasher.finish()
}

/// A consumer of the generic syn-subtree walk.
///
/// Implementors decide what a "subform" *is* — a fingerprint `u64`
/// (v0.1, [`super::walker::FormEmitter`]) or a materialised tree node
/// (PR 4). The walk skeleton in this module decides which syn nodes exist
/// and in what order their children are visited; the sink decides what to
/// build from that stream.
///
/// `Out` is the value a fully-visited subtree collapses to (folded into
/// the parent via [`SubformSink::fold`]). `Node` is the per-node
/// in-progress accumulator handed back from [`SubformSink::begin`] and
/// consumed by [`SubformSink::seal`].
pub trait SubformSink {
    /// The value a sealed subtree yields and a parent folds in.
    type Out;
    /// The per-node accumulator held open between `begin` and `seal`.
    type Node;

    /// Open a fresh, untagged node. Most callers use the [`begin`]
    /// convenience (open + tag in one step); [`walk_expr`] opens untagged
    /// because its category dispatch decides the tag after the node is
    /// open (mirroring the original fold, which created the `Xxh3` before
    /// the `Expr` match selected its `"ExprXxx"` discriminator).
    ///
    /// [`begin`]: SubformSink::begin
    fn begin_node(&mut self) -> Self::Node;

    /// Hash a structural kind tag (`"Block"`, `"ExprBinary"`, …) into an
    /// open node. The tag is the AST-node-kind discriminator; the fold
    /// hashes it, a tree builder stores it.
    fn tag(&mut self, node: &mut Self::Node, tag: &'static str);

    /// Open a node and immediately tag it — the common case. Equivalent
    /// to [`begin_node`] followed by [`tag`].
    ///
    /// [`begin_node`]: SubformSink::begin_node
    /// [`tag`]: SubformSink::tag
    fn begin(&mut self, tag: &'static str) -> Self::Node {
        let mut node = self.begin_node();
        self.tag(&mut node, tag);
        node
    }

    /// Fold a fully-visited child subtree into the open node.
    fn fold(&mut self, node: &mut Self::Node, child: Self::Out);

    /// Contribute a leaf token to the open node. In the fold this hashes
    /// the token AND counts it toward `node_count` (the O8 per-leaf
    /// count); a tree builder records it as a leaf child.
    fn leaf(&mut self, node: &mut Self::Node, token: &NormalizedToken);

    /// Seal the open node and yield its `Out`. The fold finalises the
    /// `Xxh3` and inserts the resulting `u64` into `fingerprint_set`.
    fn seal(&mut self, node: Self::Node) -> Self::Out;

    /// Record a renameable identifier in walk order (O11 rename signal).
    /// Independent of the node lifecycle — locals, fn/method names, type
    /// names, path segments, and macro names flow here as they are seen.
    fn record_identifier(&mut self, id: String);
}

/// Walk a form's attribute prelude, driving the sink directly.
///
/// This is a FORM-LEVEL prelude, not a dispatched subform: the `Attrs`
/// node only seals (yields its `Out`) when at least one PRESERVED
/// attribute is seen, so an attribute-free form never gains a phantom
/// subform. Stripped attributes (`#[derive(...)]`, `#[doc(...)]`,
/// `#[allow(...)]`, `#[cfg(...)]`, …) contribute nothing; preserved
/// attributes (`#[test]`, `#[inline]`, `#[must_use]`, …) each contribute
/// an `Attr(<name>)` leaf.
///
/// Returns `Some(out)` when a preserved attribute was seen (the sealed
/// `Attrs` subform), `None` otherwise. Shared by every form-emission
/// site so the "only seal when a preserved attr was seen" rule lives in
/// ONE place — both the fingerprint fold ([`super::walker::FormEmitter`])
/// and the tree builder ([`super::tree`]) drive the identical lifecycle.
pub(super) fn walk_attrs<S: SubformSink>(sink: &mut S, attrs: &[syn::Attribute]) -> Option<S::Out> {
    let mut node = sink.begin("Attrs");
    let mut any_preserved = false;
    for attr in attrs {
        let Some(name) = preserved_attr_name(attr) else {
            continue;
        };
        any_preserved = true;
        sink.leaf(&mut node, &NormalizedToken::Attr(name));
    }
    if any_preserved {
        Some(sink.seal(node))
    } else {
        None
    }
}

/// Should this attribute be preserved in the subform stream?
///
/// Per O5 ADR § Attributes: preserve signal (`#[test]`, `#[inline]`,
/// `#[inline(always)]`, `#[cold]`, `#[must_use]`, `#[no_mangle]`,
/// `#[repr(...)]`); strip noise (`#[derive(...)]`, `#[doc(...)]`,
/// `#[allow(...)]`, `#[warn(...)]`, `#[cfg(...)]`,
/// `#[deprecated(...)]`).
///
/// Returns `Some(name)` for preserved attributes where `name` is the
/// last path segment (e.g. `Some("inline")` for `#[inline(always)]`),
/// `None` for stripped attributes. A pure syn → vocabulary mapping with
/// no sink state, so it lives with the dispatch (shared by every sink).
pub(super) fn preserved_attr_name(attr: &syn::Attribute) -> Option<String> {
    let last = attr.path().segments.last()?;
    let name = last.ident.to_string();
    match name.as_str() {
        // Preserved (positive list).
        "test" | "inline" | "cold" | "must_use" | "no_mangle" | "repr" => Some(name),
        // Stripped (everything else, including the explicit noise list).
        _ => None,
    }
}

/// Walk a function signature: name + generic params + inputs + return
/// type + modifier keywords (async / const / unsafe). Seals and returns
/// the signature subform's `Out`.
pub(super) fn walk_sig<S: SubformSink>(sink: &mut S, sig: &syn::Signature) -> S::Out {
    let mut node = sink.begin("Sig");

    if sig.constness.is_some() {
        sink.leaf(&mut node, &NormalizedToken::Modifier("const"));
    }
    if sig.asyncness.is_some() {
        sink.leaf(&mut node, &NormalizedToken::Modifier("async"));
    }
    if sig.unsafety.is_some() {
        sink.leaf(&mut node, &NormalizedToken::Modifier("unsafe"));
    }

    // Function name is preserved as Ident.
    let name = sig.ident.to_string();
    sink.record_identifier(name.clone());
    sink.leaf(&mut node, &NormalizedToken::Ident(name));

    // Generic parameters (type params + lifetimes).
    for gp in &sig.generics.params {
        let gp_out = walk_generic_param(sink, gp);
        sink.fold(&mut node, gp_out);
    }

    // Inputs (parameters).
    for input in &sig.inputs {
        let input_out = walk_fn_arg(sink, input);
        sink.fold(&mut node, input_out);
    }

    // Return type.
    if let syn::ReturnType::Type(_, ty) = &sig.output {
        let ret_out = walk_type(sink, ty);
        sink.fold(&mut node, ret_out);
    }

    sink.seal(node)
}

fn walk_generic_param<S: SubformSink>(sink: &mut S, gp: &syn::GenericParam) -> S::Out {
    match gp {
        syn::GenericParam::Type(tp) => {
            let mut node = sink.begin("GenericTypeParam");
            sink.record_identifier(tp.ident.to_string());
            sink.leaf(&mut node, &NormalizedToken::TypeParam);
            for bound in &tp.bounds {
                let bound_out = walk_type_param_bound(sink, bound);
                sink.fold(&mut node, bound_out);
            }
            sink.seal(node)
        }
        syn::GenericParam::Lifetime(lt) => {
            let mut node = sink.begin("GenericLifetimeParam");
            let token = lifetime_token(&lt.lifetime);
            sink.leaf(&mut node, &token);
            sink.seal(node)
        }
        syn::GenericParam::Const(c) => {
            let mut node = sink.begin("GenericConstParam");
            sink.record_identifier(c.ident.to_string());
            sink.leaf(&mut node, &NormalizedToken::TypeParam);
            let ty_out = walk_type(sink, &c.ty);
            sink.fold(&mut node, ty_out);
            sink.seal(node)
        }
    }
}

fn walk_type_param_bound<S: SubformSink>(sink: &mut S, bound: &syn::TypeParamBound) -> S::Out {
    match bound {
        syn::TypeParamBound::Trait(t) => {
            let mut node = sink.begin("TraitBound");
            let path_out = walk_path(sink, &t.path);
            sink.fold(&mut node, path_out);
            sink.seal(node)
        }
        syn::TypeParamBound::Lifetime(lt) => {
            let mut node = sink.begin("LifetimeBound");
            let token = lifetime_token(lt);
            sink.leaf(&mut node, &token);
            sink.seal(node)
        }
        _ => {
            let node = sink.begin("UnknownBound");
            sink.seal(node)
        }
    }
}

fn walk_fn_arg<S: SubformSink>(sink: &mut S, arg: &syn::FnArg) -> S::Out {
    match arg {
        syn::FnArg::Receiver(r) => {
            let mut node = sink.begin("Receiver");
            if r.reference.is_some() {
                sink.leaf(&mut node, &NormalizedToken::Op("&"));
            }
            if r.mutability.is_some() {
                sink.leaf(&mut node, &NormalizedToken::Kw("mut"));
            }
            sink.leaf(&mut node, &NormalizedToken::Var);
            sink.seal(node)
        }
        syn::FnArg::Typed(pt) => {
            let mut node = sink.begin("TypedArg");
            let pat_out = walk_pat(sink, &pt.pat);
            sink.fold(&mut node, pat_out);
            let ty_out = walk_type(sink, &pt.ty);
            sink.fold(&mut node, ty_out);
            sink.seal(node)
        }
    }
}

#[must_use]
pub(super) fn walk_type<S: SubformSink>(sink: &mut S, ty: &syn::Type) -> S::Out {
    let node = match ty {
        syn::Type::Path(tp) => {
            let mut node = sink.begin("TypePath");
            let path_out = walk_path(sink, &tp.path);
            sink.fold(&mut node, path_out);
            node
        }
        syn::Type::Reference(r) => {
            let mut node = sink.begin("TypeRef");
            if r.mutability.is_some() {
                sink.leaf(&mut node, &NormalizedToken::Kw("mut"));
            }
            if let Some(lt) = &r.lifetime {
                let token = lifetime_token(lt);
                sink.leaf(&mut node, &token);
            }
            let inner_out = walk_type(sink, &r.elem);
            sink.fold(&mut node, inner_out);
            node
        }
        syn::Type::Tuple(t) => {
            let mut node = sink.begin("TypeTuple");
            for elem in &t.elems {
                let elem_out = walk_type(sink, elem);
                sink.fold(&mut node, elem_out);
            }
            node
        }
        syn::Type::Array(a) => {
            let mut node = sink.begin("TypeArray");
            let inner_out = walk_type(sink, &a.elem);
            sink.fold(&mut node, inner_out);
            let len_out = walk_expr(sink, &a.len);
            sink.fold(&mut node, len_out);
            node
        }
        syn::Type::Slice(s) => {
            let mut node = sink.begin("TypeSlice");
            let inner_out = walk_type(sink, &s.elem);
            sink.fold(&mut node, inner_out);
            node
        }
        syn::Type::TraitObject(to) => {
            let mut node = sink.begin("TypeDyn");
            for bound in &to.bounds {
                let bound_out = walk_type_param_bound(sink, bound);
                sink.fold(&mut node, bound_out);
            }
            node
        }
        syn::Type::ImplTrait(it) => {
            let mut node = sink.begin("TypeImpl");
            for bound in &it.bounds {
                let bound_out = walk_type_param_bound(sink, bound);
                sink.fold(&mut node, bound_out);
            }
            node
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
            sink.begin("TypeOther")
        }
        _ => sink.begin("TypeUnknown"),
    };
    // node_count is per-leaf (O8 ADR); the subform itself does NOT
    // contribute. Any leaf tokens fed via `leaf` during the match arms
    // above already counted.
    sink.seal(node)
}

fn walk_path<S: SubformSink>(sink: &mut S, path: &syn::Path) -> S::Out {
    let mut node = sink.begin("Path");
    for seg in &path.segments {
        let name = seg.ident.to_string();
        sink.record_identifier(name.clone());
        // If the segment looks like a generic placeholder (single
        // uppercase letter or short PascalCase that matches no real
        // type), we still preserve it as PathSeg — the heuristic
        // for distinguishing generic params from types is contextual
        // and lives elsewhere.
        sink.leaf(&mut node, &NormalizedToken::PathSeg(name));
        // Generic arguments inside the path segment.
        if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
            for arg in &args.args {
                let arg_out = walk_generic_arg(sink, arg);
                sink.fold(&mut node, arg_out);
            }
        }
    }
    sink.seal(node)
}

fn walk_generic_arg<S: SubformSink>(sink: &mut S, arg: &syn::GenericArgument) -> S::Out {
    match arg {
        syn::GenericArgument::Type(ty) => {
            let mut node = sink.begin("GArgType");
            let ty_out = walk_type(sink, ty);
            sink.fold(&mut node, ty_out);
            sink.seal(node)
        }
        syn::GenericArgument::Lifetime(lt) => {
            let mut node = sink.begin("GArgLifetime");
            let token = lifetime_token(lt);
            sink.leaf(&mut node, &token);
            sink.seal(node)
        }
        syn::GenericArgument::Const(expr) => {
            let mut node = sink.begin("GArgConst");
            let e_out = walk_expr(sink, expr);
            sink.fold(&mut node, e_out);
            sink.seal(node)
        }
        _ => {
            let node = sink.begin("GArgOther");
            sink.seal(node)
        }
    }
}

fn walk_pat<S: SubformSink>(sink: &mut S, pat: &syn::Pat) -> S::Out {
    let node = match pat {
        syn::Pat::Ident(pi) => {
            let mut node = sink.begin("PatIdent");
            sink.record_identifier(pi.ident.to_string());
            sink.leaf(&mut node, &NormalizedToken::Var);
            if pi.mutability.is_some() {
                sink.leaf(&mut node, &NormalizedToken::Kw("mut"));
            }
            node
        }
        syn::Pat::Wild(_) => sink.begin("PatWild"),
        syn::Pat::Path(pp) => {
            let mut node = sink.begin("PatPath");
            let path_out = walk_path(sink, &pp.path);
            sink.fold(&mut node, path_out);
            node
        }
        syn::Pat::Lit(pl) => {
            let mut node = sink.begin("PatLit");
            let lit_token = lit_to_token(&pl.lit);
            sink.leaf(&mut node, &lit_token);
            node
        }
        syn::Pat::Tuple(t) => walk_pat_seq(sink, "PatTuple", &t.elems),
        syn::Pat::Slice(s) => walk_pat_seq(sink, "PatSlice", &s.elems),
        syn::Pat::Or(po) => walk_pat_seq(sink, "PatOr", &po.cases),
        syn::Pat::TupleStruct(ts) => {
            let mut node = sink.begin("PatTupleStruct");
            let path_out = walk_path(sink, &ts.path);
            sink.fold(&mut node, path_out);
            for elem in &ts.elems {
                let e_out = walk_pat(sink, elem);
                sink.fold(&mut node, e_out);
            }
            node
        }
        syn::Pat::Struct(ps) => {
            let mut node = sink.begin("PatStruct");
            let path_out = walk_path(sink, &ps.path);
            sink.fold(&mut node, path_out);
            for field in &ps.fields {
                let f_out = walk_pat(sink, &field.pat);
                sink.fold(&mut node, f_out);
            }
            node
        }
        syn::Pat::Reference(pr) => {
            let mut node = sink.begin("PatRef");
            if pr.mutability.is_some() {
                sink.leaf(&mut node, &NormalizedToken::Kw("mut"));
            }
            let inner_out = walk_pat(sink, &pr.pat);
            sink.fold(&mut node, inner_out);
            node
        }
        syn::Pat::Type(pt) => {
            let mut node = sink.begin("PatType");
            let inner_out = walk_pat(sink, &pt.pat);
            sink.fold(&mut node, inner_out);
            let ty_out = walk_type(sink, &pt.ty);
            sink.fold(&mut node, ty_out);
            node
        }
        syn::Pat::Range(_) => sink.begin("PatRange"),
        syn::Pat::Rest(_) => sink.begin("PatRest"),
        _ => sink.begin("PatOther"),
    };
    // node_count is per-leaf (O8 ADR); the pattern subform itself does
    // NOT contribute. Leaf tokens fed via `leaf` during the match arms
    // above already counted.
    sink.seal(node)
}

/// Walk a sub-pattern sequence (Tuple / Slice / Or arms) with a
/// caller-supplied discriminator. Generic over the punctuation token
/// because syn uses `Comma` for Tuple/Slice and `Or` for Or-patterns.
/// Returns the *open* node so the caller can seal it inside the shared
/// per-pattern seal.
fn walk_pat_seq<S: SubformSink, P>(
    sink: &mut S,
    discriminator: &'static str,
    elems: &syn::punctuated::Punctuated<syn::Pat, P>,
) -> S::Node {
    let mut node = sink.begin(discriminator);
    for elem in elems {
        let e_out = walk_pat(sink, elem);
        sink.fold(&mut node, e_out);
    }
    node
}

/// Walk a `{ … }` block: open a node, fold each statement subform.
pub(super) fn walk_block<S: SubformSink>(sink: &mut S, block: &syn::Block) -> S::Out {
    let mut node = sink.begin("Block");
    for stmt in &block.stmts {
        let s_out = walk_stmt(sink, stmt);
        sink.fold(&mut node, s_out);
    }
    // Block itself is a structural wrapper; per O8 it does NOT count
    // toward node_count. The contained statements do.
    sink.seal(node)
}

fn walk_stmt<S: SubformSink>(sink: &mut S, stmt: &syn::Stmt) -> S::Out {
    let node = match stmt {
        syn::Stmt::Local(local) => {
            let mut node = sink.begin("StmtLet");
            sink.leaf(&mut node, &NormalizedToken::Kw("let"));
            let pat_out = walk_pat(sink, &local.pat);
            sink.fold(&mut node, pat_out);
            if let Some(init) = &local.init {
                let e_out = walk_expr(sink, &init.expr);
                sink.fold(&mut node, e_out);
                if let Some((_, else_block)) = &init.diverge {
                    let div_out = walk_expr(sink, else_block);
                    sink.fold(&mut node, div_out);
                }
            }
            node
        }
        syn::Stmt::Item(item) => {
            let mut node = sink.begin("StmtItem");
            // Nested item — form-boundary marker. The nested item may
            // emit its own form (via top-level visit_item), BUT we don't
            // recurse into nested forms from inside a function body at
            // v0.1 (function-local items are emitted as form-boundary
            // markers only).
            if matches!(item, syn::Item::Fn(_)) {
                sink.leaf(&mut node, &NormalizedToken::NestedFn);
            }
            // Other nested item shapes don't carry useful structural
            // signal inside an enclosing fn body; collapse to a generic
            // marker.
            node
        }
        syn::Stmt::Expr(expr, _semi) => {
            let mut node = sink.begin("StmtExpr");
            let e_out = walk_expr(sink, expr);
            sink.fold(&mut node, e_out);
            node
        }
        syn::Stmt::Macro(m) => {
            let mut node = sink.begin("StmtMacro");
            let m_out = walk_macro(sink, &m.mac);
            sink.fold(&mut node, m_out);
            node
        }
    };
    sink.seal(node)
}

/// Walk a `syn::Expr`. Dispatches by category to keep each helper small;
/// less-common shapes (`Group`, `Verbatim`, `Const` block, `TryBlock`,
/// `Yield`, …) fall through to the `ExprOther` discriminator.
#[must_use]
pub(super) fn walk_expr<S: SubformSink>(sink: &mut S, expr: &syn::Expr) -> S::Out {
    let mut node = sink.begin_node();
    if !walk_expr_dispatch(sink, &mut node, expr) {
        sink.tag(&mut node, "ExprOther");
    }
    // Expression itself is a fingerprint-emitting subform; whether it
    // counts as a node_count leaf depends on what it is. Primitives
    // (Lit, Path leaves) count via `leaf`; the subform itself does not
    // add to node_count here (would double-count).
    sink.seal(node)
}

/// Dispatch a `syn::Expr` to its category-grouped walk helper. Returns
/// `true` when a category handler claimed the variant; `false` falls
/// back to the [`walk_expr`] caller's `ExprOther` discriminator.
fn walk_expr_dispatch<S: SubformSink>(sink: &mut S, node: &mut S::Node, expr: &syn::Expr) -> bool {
    walk_expr_value(sink, node, expr)
        || walk_expr_operator(sink, node, expr)
        || walk_expr_call_like(sink, node, expr)
        || walk_expr_control(sink, node, expr)
        || walk_expr_collection(sink, node, expr)
        || walk_expr_wrap(sink, node, expr)
        || walk_expr_block_like(sink, node, expr)
}

/// Value-level expressions: path-or-local, literal, struct literal,
/// repeat.
fn walk_expr_value<S: SubformSink>(sink: &mut S, node: &mut S::Node, expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Path(ep) => walk_expr_path(sink, node, ep),
        syn::Expr::Lit(el) => walk_expr_lit(sink, node, &el.lit),
        syn::Expr::Struct(es) => walk_expr_struct(sink, node, es),
        syn::Expr::Repeat(er) => walk_expr_repeat(sink, node, er),
        _ => return false,
    }
    true
}

/// Operator expressions: binary, unary, assign, cast, range, try.
fn walk_expr_operator<S: SubformSink>(sink: &mut S, node: &mut S::Node, expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Binary(eb) => walk_expr_binary(sink, node, eb),
        syn::Expr::Unary(eu) => walk_expr_unary(sink, node, eu),
        syn::Expr::Assign(ea) => walk_expr_assign(sink, node, ea),
        syn::Expr::Cast(ec) => walk_expr_cast(sink, node, ec),
        syn::Expr::Range(er) => walk_expr_range(sink, node, er),
        syn::Expr::Try(et) => walk_expr_try(sink, node, et),
        _ => return false,
    }
    true
}

/// Call-like expressions: free call, method call, field, index.
fn walk_expr_call_like<S: SubformSink>(sink: &mut S, node: &mut S::Node, expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Call(ec) => walk_expr_call(sink, node, ec),
        syn::Expr::MethodCall(em) => walk_expr_method_call(sink, node, em),
        syn::Expr::Field(ef) => walk_expr_field(sink, node, ef),
        syn::Expr::Index(ei) => walk_expr_index(sink, node, ei),
        _ => return false,
    }
    true
}

/// Control-flow expressions: if, match, while, for, loop, return, break,
/// continue, let.
fn walk_expr_control<S: SubformSink>(sink: &mut S, node: &mut S::Node, expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::If(ei) => walk_expr_if(sink, node, ei),
        syn::Expr::Match(em) => walk_expr_match(sink, node, em),
        syn::Expr::While(ew) => walk_expr_while(sink, node, ew),
        syn::Expr::ForLoop(efl) => walk_expr_for(sink, node, efl),
        syn::Expr::Loop(el) => walk_expr_loop(sink, node, el),
        syn::Expr::Return(er) => walk_expr_return(sink, node, er),
        syn::Expr::Break(eb) => walk_expr_break(sink, node, eb),
        syn::Expr::Continue(_) => walk_expr_continue(sink, node),
        syn::Expr::Let(el) => walk_expr_let(sink, node, el),
        _ => return false,
    }
    true
}

/// Collection-shaped expressions: tuple, array.
fn walk_expr_collection<S: SubformSink>(
    sink: &mut S,
    node: &mut S::Node,
    expr: &syn::Expr,
) -> bool {
    match expr {
        syn::Expr::Tuple(et) => walk_expr_seq(sink, node, "ExprTuple", &et.elems),
        syn::Expr::Array(ea) => walk_expr_seq(sink, node, "ExprArray", &ea.elems),
        _ => return false,
    }
    true
}

/// Unary-wrapping expressions: reference, paren, await, macro, closure
/// (form-boundary).
fn walk_expr_wrap<S: SubformSink>(sink: &mut S, node: &mut S::Node, expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Reference(er) => walk_expr_reference(sink, node, er),
        syn::Expr::Paren(ep) => walk_expr_paren(sink, node, ep),
        syn::Expr::Await(ea) => walk_expr_await(sink, node, ea),
        syn::Expr::Macro(em) => walk_expr_macro(sink, node, em),
        syn::Expr::Closure(_) => walk_expr_closure(sink, node),
        _ => return false,
    }
    true
}

/// Block-bearing expressions: block, async, unsafe.
fn walk_expr_block_like<S: SubformSink>(
    sink: &mut S,
    node: &mut S::Node,
    expr: &syn::Expr,
) -> bool {
    match expr {
        syn::Expr::Block(eb) => walk_expr_block_expr(sink, node, &eb.block),
        syn::Expr::Async(ea) => walk_expr_async(sink, node, ea),
        syn::Expr::Unsafe(eu) => walk_expr_unsafe(sink, node, eu),
        _ => return false,
    }
    true
}

/// Path expression: single-segment, snake-case identifier paths in
/// expression position are treated as local-variable references
/// (alpha-equivalent collapse). This is the v0.1 heuristic for "this is a
/// local" without full scope tracking; multi-segment paths (e.g.,
/// `foo::bar`) and `PascalCase` single-segment paths (e.g., `Some`,
/// `MyType`) are treated as concrete value paths.
fn walk_expr_path<S: SubformSink>(sink: &mut S, node: &mut S::Node, ep: &syn::ExprPath) {
    if let Some(name) = single_seg_local(&ep.path) {
        sink.tag(node, "ExprLocal");
        sink.record_identifier(name);
        sink.leaf(node, &NormalizedToken::Var);
    } else {
        sink.tag(node, "ExprPath");
        let path_out = walk_path(sink, &ep.path);
        sink.fold(node, path_out);
    }
}

fn walk_expr_lit<S: SubformSink>(sink: &mut S, node: &mut S::Node, lit: &syn::Lit) {
    sink.tag(node, "ExprLit");
    let lit_token = lit_to_token(lit);
    sink.leaf(node, &lit_token);
}

fn walk_expr_binary<S: SubformSink>(sink: &mut S, node: &mut S::Node, eb: &syn::ExprBinary) {
    sink.tag(node, "ExprBinary");
    sink.leaf(node, &NormalizedToken::Op(binop_symbol(&eb.op)));
    let l_out = walk_expr(sink, &eb.left);
    sink.fold(node, l_out);
    let r_out = walk_expr(sink, &eb.right);
    sink.fold(node, r_out);
}

fn walk_expr_unary<S: SubformSink>(sink: &mut S, node: &mut S::Node, eu: &syn::ExprUnary) {
    sink.tag(node, "ExprUnary");
    sink.leaf(node, &NormalizedToken::Op(unop_symbol(&eu.op)));
    let inner_out = walk_expr(sink, &eu.expr);
    sink.fold(node, inner_out);
}

fn walk_expr_assign<S: SubformSink>(sink: &mut S, node: &mut S::Node, ea: &syn::ExprAssign) {
    sink.tag(node, "ExprAssign");
    sink.leaf(node, &NormalizedToken::Op("="));
    let l_out = walk_expr(sink, &ea.left);
    sink.fold(node, l_out);
    let r_out = walk_expr(sink, &ea.right);
    sink.fold(node, r_out);
}

fn walk_expr_call<S: SubformSink>(sink: &mut S, node: &mut S::Node, ec: &syn::ExprCall) {
    sink.tag(node, "ExprCall");
    let f_out = walk_expr(sink, &ec.func);
    sink.fold(node, f_out);
    for arg in &ec.args {
        let a_out = walk_expr(sink, arg);
        sink.fold(node, a_out);
    }
}

fn walk_expr_method_call<S: SubformSink>(
    sink: &mut S,
    node: &mut S::Node,
    em: &syn::ExprMethodCall,
) {
    sink.tag(node, "ExprMethodCall");
    let recv_out = walk_expr(sink, &em.receiver);
    sink.fold(node, recv_out);
    let method = em.method.to_string();
    sink.record_identifier(method.clone());
    sink.leaf(node, &NormalizedToken::Ident(method));
    for arg in &em.args {
        let a_out = walk_expr(sink, arg);
        sink.fold(node, a_out);
    }
}

fn walk_expr_field<S: SubformSink>(sink: &mut S, node: &mut S::Node, ef: &syn::ExprField) {
    sink.tag(node, "ExprField");
    let recv_out = walk_expr(sink, &ef.base);
    sink.fold(node, recv_out);
    match &ef.member {
        syn::Member::Named(ident) => {
            let name = ident.to_string();
            sink.record_identifier(name.clone());
            sink.leaf(node, &NormalizedToken::Ident(name));
        }
        syn::Member::Unnamed(idx) => {
            sink.leaf(node, &NormalizedToken::LitInt(i128::from(idx.index)));
        }
    }
}

fn walk_expr_index<S: SubformSink>(sink: &mut S, node: &mut S::Node, ei: &syn::ExprIndex) {
    sink.tag(node, "ExprIndex");
    let recv_out = walk_expr(sink, &ei.expr);
    sink.fold(node, recv_out);
    let idx_out = walk_expr(sink, &ei.index);
    sink.fold(node, idx_out);
}

fn walk_expr_block_expr<S: SubformSink>(sink: &mut S, node: &mut S::Node, block: &syn::Block) {
    sink.tag(node, "ExprBlock");
    let b_out = walk_block(sink, block);
    sink.fold(node, b_out);
}

fn walk_expr_if<S: SubformSink>(sink: &mut S, node: &mut S::Node, ei: &syn::ExprIf) {
    sink.tag(node, "ExprIf");
    sink.leaf(node, &NormalizedToken::Kw("if"));
    let c_out = walk_expr(sink, &ei.cond);
    sink.fold(node, c_out);
    let t_out = walk_block(sink, &ei.then_branch);
    sink.fold(node, t_out);
    if let Some((_, else_branch)) = &ei.else_branch {
        let e_out = walk_expr(sink, else_branch);
        sink.fold(node, e_out);
    }
}

fn walk_expr_match<S: SubformSink>(sink: &mut S, node: &mut S::Node, em: &syn::ExprMatch) {
    sink.tag(node, "ExprMatch");
    sink.leaf(node, &NormalizedToken::Kw("match"));
    let scrutinee_out = walk_expr(sink, &em.expr);
    sink.fold(node, scrutinee_out);
    for arm in &em.arms {
        let arm_out = walk_arm(sink, arm);
        sink.fold(node, arm_out);
    }
}

fn walk_expr_while<S: SubformSink>(sink: &mut S, node: &mut S::Node, ew: &syn::ExprWhile) {
    sink.tag(node, "ExprWhile");
    sink.leaf(node, &NormalizedToken::Kw("while"));
    let c_out = walk_expr(sink, &ew.cond);
    sink.fold(node, c_out);
    let b_out = walk_block(sink, &ew.body);
    sink.fold(node, b_out);
}

fn walk_expr_for<S: SubformSink>(sink: &mut S, node: &mut S::Node, efl: &syn::ExprForLoop) {
    sink.tag(node, "ExprFor");
    sink.leaf(node, &NormalizedToken::Kw("for"));
    let pat_out = walk_pat(sink, &efl.pat);
    sink.fold(node, pat_out);
    let it_out = walk_expr(sink, &efl.expr);
    sink.fold(node, it_out);
    let body_out = walk_block(sink, &efl.body);
    sink.fold(node, body_out);
}

fn walk_expr_loop<S: SubformSink>(sink: &mut S, node: &mut S::Node, el: &syn::ExprLoop) {
    sink.tag(node, "ExprLoop");
    sink.leaf(node, &NormalizedToken::Kw("loop"));
    let b_out = walk_block(sink, &el.body);
    sink.fold(node, b_out);
}

fn walk_expr_return<S: SubformSink>(sink: &mut S, node: &mut S::Node, er: &syn::ExprReturn) {
    sink.tag(node, "ExprReturn");
    sink.leaf(node, &NormalizedToken::Kw("return"));
    if let Some(inner) = &er.expr {
        let i_out = walk_expr(sink, inner);
        sink.fold(node, i_out);
    }
}

fn walk_expr_break<S: SubformSink>(sink: &mut S, node: &mut S::Node, eb: &syn::ExprBreak) {
    sink.tag(node, "ExprBreak");
    sink.leaf(node, &NormalizedToken::Kw("break"));
    if let Some(inner) = &eb.expr {
        let i_out = walk_expr(sink, inner);
        sink.fold(node, i_out);
    }
}

fn walk_expr_continue<S: SubformSink>(sink: &mut S, node: &mut S::Node) {
    sink.tag(node, "ExprContinue");
    sink.leaf(node, &NormalizedToken::Kw("continue"));
}

fn walk_expr_reference<S: SubformSink>(sink: &mut S, node: &mut S::Node, er: &syn::ExprReference) {
    sink.tag(node, "ExprRef");
    sink.leaf(node, &NormalizedToken::Op("&"));
    if er.mutability.is_some() {
        sink.leaf(node, &NormalizedToken::Kw("mut"));
    }
    let inner_out = walk_expr(sink, &er.expr);
    sink.fold(node, inner_out);
}

fn walk_expr_paren<S: SubformSink>(sink: &mut S, node: &mut S::Node, ep: &syn::ExprParen) {
    sink.tag(node, "ExprParen");
    let inner_out = walk_expr(sink, &ep.expr);
    sink.fold(node, inner_out);
}

/// Walk a sub-expression sequence (Tuple / Array elements) with a
/// caller-supplied discriminator.
fn walk_expr_seq<S: SubformSink>(
    sink: &mut S,
    node: &mut S::Node,
    discriminator: &'static str,
    elems: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
) {
    sink.tag(node, discriminator);
    for elem in elems {
        let e_out = walk_expr(sink, elem);
        sink.fold(node, e_out);
    }
}

fn walk_expr_cast<S: SubformSink>(sink: &mut S, node: &mut S::Node, ec: &syn::ExprCast) {
    sink.tag(node, "ExprCast");
    sink.leaf(node, &NormalizedToken::Kw("as"));
    let inner_out = walk_expr(sink, &ec.expr);
    sink.fold(node, inner_out);
    let ty_out = walk_type(sink, &ec.ty);
    sink.fold(node, ty_out);
}

fn walk_expr_range<S: SubformSink>(sink: &mut S, node: &mut S::Node, er: &syn::ExprRange) {
    sink.tag(node, "ExprRange");
    sink.leaf(node, &NormalizedToken::Op(".."));
    if let Some(start) = &er.start {
        let s_out = walk_expr(sink, start);
        sink.fold(node, s_out);
    }
    if let Some(end) = &er.end {
        let e_out = walk_expr(sink, end);
        sink.fold(node, e_out);
    }
}

fn walk_expr_try<S: SubformSink>(sink: &mut S, node: &mut S::Node, et: &syn::ExprTry) {
    sink.tag(node, "ExprTry");
    sink.leaf(node, &NormalizedToken::Op("?"));
    let inner_out = walk_expr(sink, &et.expr);
    sink.fold(node, inner_out);
}

fn walk_expr_await<S: SubformSink>(sink: &mut S, node: &mut S::Node, ea: &syn::ExprAwait) {
    sink.tag(node, "ExprAwait");
    sink.leaf(node, &NormalizedToken::Kw("await"));
    let inner_out = walk_expr(sink, &ea.base);
    sink.fold(node, inner_out);
}

fn walk_expr_async<S: SubformSink>(sink: &mut S, node: &mut S::Node, ea: &syn::ExprAsync) {
    sink.tag(node, "ExprAsync");
    sink.leaf(node, &NormalizedToken::Modifier("async"));
    let b_out = walk_block(sink, &ea.block);
    sink.fold(node, b_out);
}

fn walk_expr_unsafe<S: SubformSink>(sink: &mut S, node: &mut S::Node, eu: &syn::ExprUnsafe) {
    sink.tag(node, "ExprUnsafe");
    sink.leaf(node, &NormalizedToken::Modifier("unsafe"));
    let b_out = walk_block(sink, &eu.block);
    sink.fold(node, b_out);
}

fn walk_expr_macro<S: SubformSink>(sink: &mut S, node: &mut S::Node, em: &syn::ExprMacro) {
    sink.tag(node, "ExprMacro");
    let m_out = walk_macro(sink, &em.mac);
    sink.fold(node, m_out);
}

/// Form-boundary: the closure body is attributed to its own form, not
/// this one. Emit only the opaque marker. The walker's caller is
/// responsible for capturing the closure as a separate form via a
/// follow-up pass.
fn walk_expr_closure<S: SubformSink>(sink: &mut S, node: &mut S::Node) {
    sink.tag(node, "ExprClosure");
    sink.leaf(node, &NormalizedToken::Closure);
}

fn walk_expr_struct<S: SubformSink>(sink: &mut S, node: &mut S::Node, es: &syn::ExprStruct) {
    sink.tag(node, "ExprStruct");
    let path_out = walk_path(sink, &es.path);
    sink.fold(node, path_out);
    for field in &es.fields {
        let f_out = walk_expr(sink, &field.expr);
        sink.fold(node, f_out);
    }
}

fn walk_expr_repeat<S: SubformSink>(sink: &mut S, node: &mut S::Node, er: &syn::ExprRepeat) {
    sink.tag(node, "ExprRepeat");
    let inner_out = walk_expr(sink, &er.expr);
    sink.fold(node, inner_out);
    let len_out = walk_expr(sink, &er.len);
    sink.fold(node, len_out);
}

fn walk_expr_let<S: SubformSink>(sink: &mut S, node: &mut S::Node, el: &syn::ExprLet) {
    sink.tag(node, "ExprLet");
    sink.leaf(node, &NormalizedToken::Kw("let"));
    let pat_out = walk_pat(sink, &el.pat);
    sink.fold(node, pat_out);
    let e_out = walk_expr(sink, &el.expr);
    sink.fold(node, e_out);
}

fn walk_arm<S: SubformSink>(sink: &mut S, arm: &syn::Arm) -> S::Out {
    let mut node = sink.begin("MatchArm");
    let pat_out = walk_pat(sink, &arm.pat);
    sink.fold(&mut node, pat_out);
    if let Some((_, guard)) = &arm.guard {
        sink.leaf(&mut node, &NormalizedToken::Kw("if"));
        let g_out = walk_expr(sink, guard);
        sink.fold(&mut node, g_out);
    }
    let body_out = walk_expr(sink, &arm.body);
    sink.fold(&mut node, body_out);
    sink.seal(node)
}

fn walk_macro<S: SubformSink>(sink: &mut S, mac: &syn::Macro) -> S::Out {
    let mut node = sink.begin("MacroCall");
    let name = mac
        .path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();
    sink.record_identifier(name.clone());
    sink.leaf(&mut node, &NormalizedToken::MacroCall(name));
    // Per ADR: macro arguments are NOT walked at v0.1.
    sink.seal(node)
}

// --- shared leaf helpers (language-vocabulary mappings) ---------------
//
// These are pure syn → NormalizedToken / symbol mappings shared by every
// sink. They carry no accumulator state, so they live with the dispatch.

/// Convert a `syn::Lit` to its [`NormalizedToken`] leaf.
pub(super) fn lit_to_token(lit: &syn::Lit) -> NormalizedToken {
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

/// Build a `domain::Span` from a pair of proc-macro2 spans. Shared by
/// every form-emission site; lives with the visitor because the
/// span-coordinate mapping is language-vocabulary, not sink state.
pub(super) fn span_from_pm(start: PmSpan, end: PmSpan) -> Span {
    use dry_core::domain::LineColumn;

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
