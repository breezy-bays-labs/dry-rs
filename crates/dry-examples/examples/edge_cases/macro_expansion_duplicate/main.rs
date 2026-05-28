//! Edge-case fixture: a `macro_rules!` invocation that expands to two
//! duplicate functions. dry4rs's normalizer (built on `syn`) walks
//! pre-expansion AST — `macro_rules!` invocations are opaque tokens
//! from its perspective. This fixture documents the limit.

macro_rules! double_fn {
    ($a:ident, $b:ident) => {
        fn $a(x: i32) -> i32 {
            x + 1
        }
        fn $b(x: i32) -> i32 {
            x + 1
        }
    };
}

double_fn!(foo, bar);

fn main() {
    let a = foo(1);
    let b = bar(2);
    println!("{a} {b}");
}
