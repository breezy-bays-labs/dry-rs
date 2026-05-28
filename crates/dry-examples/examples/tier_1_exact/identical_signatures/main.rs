//! Tier 1 exact-duplication fixture: two free functions with identical
//! signatures and identical bodies. The only difference is the function
//! names — every statement, every expression, every token (after the
//! function name) matches.

fn add_one(x: i32) -> i32 {
    let result = x + 1;
    println!("computed {result}");
    result
}

fn increment(value: i32) -> i32 {
    let result = value + 1;
    println!("computed {result}");
    result
}

fn main() {
    let a = add_one(1);
    let b = increment(2);
    println!("{a} {b}");
}
