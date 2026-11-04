#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;

fn configure(first: &mut Settings, second: &mut Settings) {
    first.option(1);
    second.option(2);
}
fn main() {}
