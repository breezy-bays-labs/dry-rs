//! Tier 1 exact-duplication fixture: two `async fn`s with identical
//! bodies. Probes whether the normalizer treats `async fn` and `fn`
//! bodies the same way structurally.
//!
//! Compiles standalone because this file is never built by cargo
//! (`autoexamples = false` on the crate) — dry4rs parses it as text.
//! The body has no `.await` so any executor is irrelevant.

async fn fetch_a(id: u32) -> Result<u32, String> {
    let key = id * 2;
    println!("fetching {key}");
    Ok(key)
}

async fn fetch_b(id: u32) -> Result<u32, String> {
    let key = id * 2;
    println!("fetching {key}");
    Ok(key)
}

fn main() {
    let _ = (fetch_a, fetch_b);
}
