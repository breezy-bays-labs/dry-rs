//! Edge-case fixture (multi-file half 2 of 2): consumer side. Pairs
//! with `producer.rs` to exercise dry4rs's hash-bucket clustering
//! across multiple files. The fixture's harness invocation passes
//! the parent directory, not a single file.

pub fn pack_payload(content: &str, priority: u32) -> String {
    let header = format!("priority={priority}");
    let body = content.to_string();
    format!("{header}\n{body}")
}
