#![expect(dead_code)]
struct Report {
    count: usize,
    text: String,
}
fn project(text: &str) -> Report {
    Report {
        count: text.len(),
        text: text.to_owned(),
    }
}
fn inspect() -> usize {
    project("").count
}
fn main() {}
