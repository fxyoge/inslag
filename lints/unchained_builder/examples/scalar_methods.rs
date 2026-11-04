#![expect(dead_code)]

fn configure(mut value: i32) {
    value = value.abs();
    value = value.abs();
    std::hint::black_box(value);
}
fn main() {}
