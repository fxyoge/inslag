#![expect(dead_code)]
struct Board {
    values: Vec<u64>,
}
struct Report {
    count: usize,
    board: Board,
}
fn project(input: &[u64]) -> Report {
    let values: Vec<_> = input.iter().map(|&value| value + 1).collect();
    Report {
        count: values.len(),
        board: Board { values },
    }
}
fn inspect(input: &[u64]) -> usize {
    project(input).count
}
fn main() {}
