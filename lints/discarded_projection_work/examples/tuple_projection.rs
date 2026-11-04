#![expect(dead_code)]
fn project(values: &Vec<u64>) -> (usize, Vec<u64>) {
    (values.len(), values.clone())
}
fn inspect(values: &Vec<u64>) -> usize {
    project(values).0
}
fn main() {}
