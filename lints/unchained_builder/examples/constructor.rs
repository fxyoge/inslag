#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Request;

fn configure() {
    let request = Request::default().header(1);
    let request = request.header(2);
    request.send();
}
fn main() {}
