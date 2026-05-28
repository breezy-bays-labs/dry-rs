//! Tier 3 reordered-duplication fixture: two functions matching on
//! the same enum with the arms in a different order. The arm bodies
//! and patterns are identical; only the source order differs.

enum Direction {
    North,
    South,
    East,
    West,
}

fn label_a(d: Direction) -> &'static str {
    match d {
        Direction::North => "up",
        Direction::South => "down",
        Direction::East => "right",
        Direction::West => "left",
    }
}

fn label_b(d: Direction) -> &'static str {
    match d {
        Direction::West => "left",
        Direction::East => "right",
        Direction::South => "down",
        Direction::North => "up",
    }
}

fn main() {
    let _ = label_a(Direction::North);
    let _ = label_b(Direction::West);
}
