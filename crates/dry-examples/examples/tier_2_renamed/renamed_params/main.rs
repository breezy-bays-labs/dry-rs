//! Tier 2 renamed-duplication fixture: two free functions with the
//! same body shape but renamed parameters. Parameters drive both the
//! function signature AND the body's identifier usage, so this is a
//! slightly stronger renaming signal than `renamed_locals`.

fn distance(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    (dx * dx + dy * dy).sqrt()
}

fn separation(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    (dx * dx + dy * dy).sqrt()
}

fn main() {
    let d = distance(0.0, 0.0, 3.0, 4.0);
    let s = separation(0.0, 0.0, 3.0, 4.0);
    println!("{d} {s}");
}
