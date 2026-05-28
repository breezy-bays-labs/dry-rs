//! Tier 2 renamed-duplication fixture: two free functions with the
//! same structural body but different local variable names. The
//! statements, types, control flow, and operators all match; only
//! the bindings differ.

fn area_a(width: f64, height: f64) -> f64 {
    let w = width;
    let h = height;
    let product = w * h;
    println!("area {product}");
    product
}

fn area_b(width: f64, height: f64) -> f64 {
    let span = width;
    let depth = height;
    let total = span * depth;
    println!("area {total}");
    total
}

fn main() {
    let a = area_a(3.0, 4.0);
    let b = area_b(3.0, 4.0);
    println!("{a} {b}");
}
