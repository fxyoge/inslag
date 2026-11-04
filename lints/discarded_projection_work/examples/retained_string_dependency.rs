#![expect(dead_code)]
struct Report {
    count: usize,
    text: String,
}
fn project(input: &str) -> Report {
    let text = input.to_owned();
    Report {
        count: text.len(),
        text,
    }
}
fn inspect(input: &str) -> usize {
    project(input).count
}
fn main() {}
