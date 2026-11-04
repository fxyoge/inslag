#![expect(dead_code)]
struct Report {
    count: usize,
    values: Vec<u64>,
}
fn project(input: &Vec<u64>) -> Report {
    let values;
    values = input.clone();
    Report {
        count: values.len(),
        values,
    }
}
fn inspect(input: &Vec<u64>) -> usize {
    project(input).count
}
fn main() {}
