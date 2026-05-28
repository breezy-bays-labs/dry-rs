//! Tier 2 renamed-duplication fixture: two struct constructors that
//! emit the same shape with different field names. The constructor
//! bodies populate fields in the same order with the same expression
//! shapes; the rename lives in the struct definitions, not in the
//! function bodies.

struct Point {
    x: f64,
    y: f64,
}

struct Vec2 {
    horizontal: f64,
    vertical: f64,
}

fn make_point(a: f64, b: f64) -> Point {
    let scaled_x = a * 2.0;
    let scaled_y = b * 2.0;
    Point {
        x: scaled_x,
        y: scaled_y,
    }
}

fn make_vec2(a: f64, b: f64) -> Vec2 {
    let scaled_h = a * 2.0;
    let scaled_v = b * 2.0;
    Vec2 {
        horizontal: scaled_h,
        vertical: scaled_v,
    }
}

fn main() {
    let _ = make_point(1.0, 2.0);
    let _ = make_vec2(1.0, 2.0);
}
