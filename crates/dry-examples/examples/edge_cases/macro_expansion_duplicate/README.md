# edge_cases / macro_expansion_duplicate

## What this demonstrates

A `macro_rules!` invocation (`double_fn!(foo, bar)`) that, after
expansion, produces two identical-body functions. From the source
text alone, there's only one item: the macro invocation. After the
compiler expands it, there are two duplicate `fn` definitions.

This fixture probes dry4rs's pre-expansion blindness. The syn parser
sees the macro invocation as a single opaque `Item::Macro`, not the
two functions it generates.

## What dry4rs observes (documented v0.1 limit)

dry4rs walks the syn AST without invoking the macro expander
(`syn::ExprMacro` / `syn::ItemMacro` are leaves). The pre-expansion
AST has one item — the `double_fn!()` invocation. There's no
duplication to detect at this layer.

The captured `expected.json` documents the actual count of detected
duplications (likely zero). This is NOT a "tool fails to detect" claim
— it's a documented limit of pre-expansion analysis. A future v0.x
might add a `cargo expand`-based pass for opt-in expansion; this
fixture would then move tiers.

## How similar tools handle this

Most pre-expansion duplication detectors (PMD CPD, jscpd, structural-
search) have the same limit. `cargo expand` + rerun is the
workaround consumers can apply today.

The catalogue documents both verdicts; the absence of detection on
this fixture is the *expected, intentional behavior at v0.1*.
