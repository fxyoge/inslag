#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Request;

fn configure(request: Request) {
    let first = request.header(1);
    let second = first.header(2);
    second.send();
}
fn main() {}
