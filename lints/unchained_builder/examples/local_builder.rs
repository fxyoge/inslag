#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::{Configure, Settings};

fn configure() {
    let mut settings = Settings::default();
    settings.option(1);
    settings.enabled();
}
fn main() {}
