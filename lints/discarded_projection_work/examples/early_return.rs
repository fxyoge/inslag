#![expect(dead_code)]
struct Report {
    count: usize,
    values: Vec<u64>,
}
fn project(values: &Vec<u64>, ready: bool) -> Report {
    if ready {
        return Report {
            count: values.len(),
            values: values.clone(),
        };
    }
    Report {
        count: values.len(),
        values: Vec::new(),
    }
}
fn inspect(values: &Vec<u64>, ready: bool) -> usize {
    project(values, ready).count
}
fn main() {}
