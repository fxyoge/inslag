#![expect(dead_code)]
struct Report {
    count: usize,
    checksum: u64,
}
fn project(values: &[u64]) -> Report {
    Report {
        count: values.len(),
        checksum: values.iter().copied().sum(),
    }
}
fn inspect(values: &[u64]) -> usize {
    project(values).count
}
fn main() {}
