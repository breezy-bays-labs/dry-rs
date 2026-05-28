//! Edge-case fixture: two generic functions with the same body but
//! different trait bounds. `fn foo<T: Display>` and `fn bar<T: Debug>`
//! print their input; only the bound differs.

use std::fmt::{Debug, Display};

fn announce_display<T: Display>(value: T) {
    println!("got {value}");
    println!("done");
}

fn announce_debug<T: Debug>(value: T) {
    println!("got {value:?}");
    println!("done");
}

fn main() {
    announce_display(42);
    announce_debug(42);
}
