#![expect(dead_code)]
struct Report {
    count: usize,
    values: Vec<u64>,
}
fn count(values: &[u64]) -> usize {
    if values.is_empty() {
        return 0;
    }
    1
}
fn project(input: &Vec<u64>) -> Report {
    let values = input.clone();
    Report {
        count: count(&values),
        values,
    }
}
fn inspect(input: &Vec<u64>) -> usize {
    project(input).count
}
fn main() {}
