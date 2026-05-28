//! Edge-case fixture: a method on a trait impl and a free function
//! with the same body. Probes whether the normalizer treats the
//! syntactic difference (method-on-impl vs free fn) as a structural
//! divergence.

struct Counter {
    value: i32,
}

impl Counter {
    fn doubled(&self) -> i32 {
        let n = self.value;
        let result = n * 2;
        println!("doubled {result}");
        result
    }
}

fn doubled_free(c: &Counter) -> i32 {
    let n = c.value;
    let result = n * 2;
    println!("doubled {result}");
    result
}

fn main() {
    let c = Counter { value: 21 };
    let m = c.doubled();
    let f = doubled_free(&c);
    println!("{m} {f}");
}
