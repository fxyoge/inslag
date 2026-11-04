#![expect(dead_code)]
struct Report {
    count: usize,
    values: Vec<u64>,
}
fn project(values: &[u64]) -> Report {
    Report {
        count: values.len(),
        values: values.iter().copied().collect(),
    }
}
fn inspect() -> usize {
    project(&[]).count
}
fn main() {}
