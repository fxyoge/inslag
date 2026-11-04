#![expect(dead_code)]
trait Observe {
    fn observe(&self) {}
}
struct Observer;
impl Observe for Observer {
    fn observe(&self) {
        println!("observed");
    }
}
struct Report {
    count: usize,
    values: Vec<i32>,
}
fn project<T: Observe>(values: &Vec<i32>, observer: &T) -> Report {
    observer.observe();
    Report {
        count: values.len(),
        values: values.clone(),
    }
}
fn inspect(values: &Vec<i32>, observer: &Observer) -> usize {
    project(values, observer).count
}
fn main() {}
