//! Tier 3 reordered-duplication fixture: two builder-pattern call
//! sequences with the same calls in a different order. Builder
//! patterns are intentionally order-tolerant for independent setters;
//! the two functions produce the same logical configuration.

struct ConfigBuilder {
    name: String,
    retries: u32,
    timeout_ms: u32,
}

impl ConfigBuilder {
    fn new() -> Self {
        Self {
            name: String::new(),
            retries: 0,
            timeout_ms: 0,
        }
    }
    fn name(mut self, n: &str) -> Self {
        self.name = n.to_string();
        self
    }
    fn retries(mut self, r: u32) -> Self {
        self.retries = r;
        self
    }
    fn timeout_ms(mut self, t: u32) -> Self {
        self.timeout_ms = t;
        self
    }
}

fn build_a() -> ConfigBuilder {
    ConfigBuilder::new()
        .name("svc")
        .retries(3)
        .timeout_ms(500)
}

fn build_b() -> ConfigBuilder {
    ConfigBuilder::new()
        .timeout_ms(500)
        .retries(3)
        .name("svc")
}

fn main() {
    let _ = build_a();
    let _ = build_b();
}
