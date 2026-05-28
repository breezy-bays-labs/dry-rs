//! Tier 4 false-positive-bait fixture: two unrelated functions that
//! both happen to use `for i in 0..10` as a scaffolding loop. The
//! shared idiom is incidental; the functions compute different things.

fn sum_squares() -> i32 {
    let mut total = 0;
    for i in 0..10 {
        total += i * i;
    }
    total
}

fn print_messages() {
    for i in 0..10 {
        println!("hello world {i}");
    }
}

fn main() {
    let s = sum_squares();
    print_messages();
    println!("{s}");
}
