//! Tier 4 false-positive-bait fixture: two functions with the SAME
//! signature shape but DIFFERENT bodies. A duplication detector that
//! hashes on signatures alone (or that over-normalizes function
//! bodies) would flag these. dry4rs should NOT.

fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn subtract(a: i32, b: i32) -> i32 {
    a - b
}

fn main() {
    let s = add(2, 3);
    let d = subtract(5, 3);
    println!("{s} {d}");
}
