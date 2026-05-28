# edge_cases / trait_impl_vs_free_fn

## What this demonstrates

A method `Counter::doubled(&self)` and a free function
`doubled_free(c: &Counter)` with the same body. Both bind
`c.value` (or `self.value`) to a local, double it, print, and return.
The behavioral output is identical; the calling convention differs.

This case is common when a developer extracts a method to a free
function (or vice versa) and forgets to delete the original. Both
forms continue compiling because their signatures are distinct;
duplication-detection should catch the redundancy.

## What dry4rs observes

The normalizer should treat the body's structure identically modulo
the `self.value` vs `c.value` access. Both resolve to a field-access
on a `Counter` (with the typed-placeholder substitution for
receiver vs param). The fingerprint sets should be near-equal.

Whether dry4rs's actual normalizer preserves the
method-vs-free-function distinction in a way that diverges the
fingerprints is empirical. The recorded `expected.json` documents the
behavior.

## How similar tools handle this

This is a case where tools' opinions differ: structurally, the two
are essentially the same code; syntactically, they're different
items at the AST level. Tools that walk method bodies and free-
function bodies through the same visitor surface this duplication;
tools that segregate them do not.
