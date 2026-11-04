#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Request;

fn configure(request: Request) {
    let request = Request::header(request, 1);
    let request = Request::header(request, 2);
    request.send();
}
fn main() {}
