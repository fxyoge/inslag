#![expect(dead_code)]
struct Report {
    count: usize,
    values: Vec<u64>,
}
fn project(values: &Vec<u64>, fallback: Report, ready: bool) -> Report {
    if !ready {
        return fallback;
    }
    Report {
        count: values.len(),
        values: values.clone(),
    }
}
fn inspect(values: &Vec<u64>, fallback: Report, ready: bool) -> usize {
    project(values, fallback, ready).count
}
fn main() {}
