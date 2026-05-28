//! Edge-case fixture: one `async fn` and one sync `fn` with the same
//! body. Probes whether `async` is normalized away (so the bodies
//! match) or preserved as a fingerprint distinction.

async fn compute_async(x: i32) -> i32 {
    let scaled = x * 3;
    let total = scaled + 1;
    println!("computed {total}");
    total
}

fn compute_sync(x: i32) -> i32 {
    let scaled = x * 3;
    let total = scaled + 1;
    println!("computed {total}");
    total
}

fn main() {
    let _ = compute_sync(1);
    let _ = compute_async;
}
