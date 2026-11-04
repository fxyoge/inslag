#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Request;

fn configure(mut request: Request) {
    request = request.header(1);
    request = request.header(2).header(3);
    request.send();
}
fn main() {}
