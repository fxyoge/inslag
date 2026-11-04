#![expect(dead_code)]
struct Board {
    values: Vec<u64>,
}
struct Report {
    count: usize,
    board: Board,
}
fn project(mut values: Vec<u64>) -> Report {
    values.push(1);
    Report {
        count: values.len(),
        board: Board { values },
    }
}
fn inspect(input: Vec<u64>) -> usize {
    project(input).count
}
fn main() {}
