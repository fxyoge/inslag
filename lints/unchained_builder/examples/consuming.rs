#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Request;

fn rebound(request: Request) {
    let request = request.header(1);
    let request = request.header(2);
    let request = request.header(3);
    request.send();
}

fn main() {}
