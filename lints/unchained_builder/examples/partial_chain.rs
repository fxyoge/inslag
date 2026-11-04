#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::{Configure, Settings};

fn configure(settings: &mut Settings) {
    settings.option(1).enabled();
    settings.option(2).option(3);
    settings.callback(|| {
        let value = 4;
        value + 1
    });
}
fn main() {}
