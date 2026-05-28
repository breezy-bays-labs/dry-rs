//! Tier 5 algorithmic fixture: two factorial implementations — one
//! iterative, one recursive. Both compute the same value but their
//! control flow is structurally unrelated. dry4rs v0.1 should NOT
//! detect them as duplicates; semantic equivalence under different
//! algorithms is a known v0.1 limit.

fn factorial_iterative(n: u64) -> u64 {
    let mut product: u64 = 1;
    for i in 1..=n {
        product *= i;
    }
    product
}

fn factorial_recursive(n: u64) -> u64 {
    if n <= 1 {
        return 1;
    }
    n * factorial_recursive(n - 1)
}

fn main() {
    let a = factorial_iterative(5);
    let b = factorial_recursive(5);
    println!("{a} {b}");
}
