#![expect(dead_code)]
struct Report {
    count: usize,
    values: Vec<u64>,
}
fn project(values: &Vec<u64>, ready: bool) -> Report {
    let values = values.clone();
    if ready {
        return Report {
            count: values.len(),
            values,
        };
    }
    Report { count: 0, values }
}
fn inspect(values: &Vec<u64>, ready: bool) -> usize {
    project(values, ready).count
}
fn main() {}
