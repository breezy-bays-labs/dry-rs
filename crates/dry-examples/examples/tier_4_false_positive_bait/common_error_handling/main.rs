//! Tier 4 false-positive-bait fixture: two unrelated functions that
//! share a common Rust error-handling idiom (`if let Err(e) = ...`).
//! The shared pattern is intrinsic to Rust; flagging it as
//! duplication would be useless noise.

use std::num::ParseIntError;

fn parse_count(s: &str) -> i32 {
    if let Err(e) = s.parse::<i32>() {
        eprintln!("parse failed: {e}");
        return 0;
    }
    s.parse::<i32>().unwrap_or(0)
}

fn parse_offset(s: &str) -> i64 {
    if let Err(e) = s.parse::<i64>() {
        eprintln!("offset parse failed: {e}");
        return -1;
    }
    s.parse::<i64>().unwrap_or(-1)
}

fn main() {
    let c = parse_count("42");
    let o = parse_offset("7");
    println!("{c} {o}");
    let _: ParseIntError;
}
