//! Tier 1 exact-duplication fixture: two methods on different structs
//! with identical bodies. Probes whether the normalizer treats method
//! bodies and free-function bodies the same way structurally.

struct Counter {
    value: i32,
}

struct Tally {
    total: i32,
}

impl Counter {
    fn step(&mut self, delta: i32) -> i32 {
        self.value += delta;
        let snapshot = self.value;
        println!("step → {snapshot}");
        snapshot
    }
}

impl Tally {
    fn step(&mut self, delta: i32) -> i32 {
        self.total += delta;
        let snapshot = self.total;
        println!("step → {snapshot}");
        snapshot
    }
}

fn main() {
    let mut c = Counter { value: 0 };
    let mut t = Tally { total: 0 };
    c.step(1);
    t.step(2);
}
