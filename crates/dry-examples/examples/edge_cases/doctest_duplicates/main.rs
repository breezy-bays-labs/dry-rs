//! Edge-case fixture: two functions with identical doctest blocks.
//! Probes whether dry4rs's normalizer surfaces doctest bodies as
//! separate forms (`FormKind::Doctest`) and whether duplicate
//! doctests are detected.

/// Add one to the input.
///
/// ```
/// let result = 1 + 1;
/// assert_eq!(result, 2);
/// ```
pub fn add_one(x: i32) -> i32 {
    x + 1
}

/// Increment the input.
///
/// ```
/// let result = 1 + 1;
/// assert_eq!(result, 2);
/// ```
pub fn increment(x: i32) -> i32 {
    x + 1
}

fn main() {
    let a = add_one(1);
    let b = increment(2);
    println!("{a} {b}");
}
