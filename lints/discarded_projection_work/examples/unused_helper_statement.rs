#![expect(dead_code)]
struct Report {
    count: usize,
    values: Vec<u64>,
}
fn project(input: &Vec<u64>) -> Report {
    Report {
        count: input.len(),
        values: {
            let _unused = input.clone();
            Vec::new()
        },
    }
}
fn inspect(input: &Vec<u64>) -> usize {
    project(input).count
}
fn main() {}
