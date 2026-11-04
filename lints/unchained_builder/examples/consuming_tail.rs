#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Request;

fn configure(request: Request) -> Request {
    let request = request.header(1);
    request.header(2)
}
fn main() {}
