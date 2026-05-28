//! Tier 3 reordered-duplication fixture: two functions with the same
//! statements in a different order. Both compute the same final value
//! but the let-bindings are permuted.

fn weighted_sum_a(x: f64, y: f64) -> f64 {
    let wx = x * 0.6;
    let wy = y * 0.4;
    let sum = wx + wy;
    println!("weighted_sum {sum}");
    sum
}

fn weighted_sum_b(x: f64, y: f64) -> f64 {
    let wy = y * 0.4;
    let wx = x * 0.6;
    let sum = wx + wy;
    println!("weighted_sum {sum}");
    sum
}

fn main() {
    let a = weighted_sum_a(1.0, 2.0);
    let b = weighted_sum_b(1.0, 2.0);
    println!("{a} {b}");
}
