//! Tier 5 algorithmic fixture: two sort implementations producing
//! the same output via different algorithms. Quicksort and mergesort
//! sort the same vector but via fundamentally different recursive
//! structures. Semantic-equivalence detection is out of scope at v0.1.

fn sort_quicksort(input: Vec<i32>) -> Vec<i32> {
    if input.len() <= 1 {
        return input;
    }
    let pivot = input[0];
    let mut less = Vec::new();
    let mut greater = Vec::new();
    for x in input.iter().skip(1) {
        if *x < pivot {
            less.push(*x);
        } else {
            greater.push(*x);
        }
    }
    let mut out = sort_quicksort(less);
    out.push(pivot);
    out.extend(sort_quicksort(greater));
    out
}

fn sort_mergesort(input: Vec<i32>) -> Vec<i32> {
    if input.len() <= 1 {
        return input;
    }
    let mid = input.len() / 2;
    let left = sort_mergesort(input[..mid].to_vec());
    let right = sort_mergesort(input[mid..].to_vec());
    let mut out = Vec::with_capacity(left.len() + right.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if left[i] <= right[j] {
            out.push(left[i]);
            i += 1;
        } else {
            out.push(right[j]);
            j += 1;
        }
    }
    out.extend_from_slice(&left[i..]);
    out.extend_from_slice(&right[j..]);
    out
}

fn main() {
    let q = sort_quicksort(vec![3, 1, 4, 1, 5, 9, 2, 6]);
    let m = sort_mergesort(vec![3, 1, 4, 1, 5, 9, 2, 6]);
    println!("{q:?} {m:?}");
}
