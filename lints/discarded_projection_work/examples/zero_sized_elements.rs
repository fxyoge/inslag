#![expect(dead_code)]
struct Report {
    count: usize,
    values: Vec<()>,
}
fn project(values: &Vec<()>) -> Report {
    Report {
        count: values.len(),
        values: values.clone(),
    }
}
fn inspect(values: &Vec<()>) -> usize {
    project(values).count
}
fn main() {}
