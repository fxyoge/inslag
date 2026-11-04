#![expect(dead_code)]
use std::collections::VecDeque;
struct Board {
    values: VecDeque<u64>,
}
struct Report {
    count: usize,
    board: Board,
}
fn project(values: &[u64]) -> Report {
    Report {
        count: values.len(),
        board: Board {
            values: values.iter().copied().collect(),
        },
    }
}
fn inspect(values: &[u64]) -> usize {
    project(values).count
}
fn main() {}
