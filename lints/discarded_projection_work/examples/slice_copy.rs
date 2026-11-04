#![expect(dead_code)]
struct Report {
    count: usize,
    values: Vec<u64>,
}
fn project(values: &[u64]) -> Report {
    Report {
        count: values.len(),
        values: values.to_vec(),
    }
}
fn inspect(values: &[u64]) -> usize {
    project(values).count
}
fn main() {}
