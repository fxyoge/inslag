#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;

fn configure(settings: &mut Settings) {
    Settings::option(settings, 1);
    Settings::option(settings, 2);
}
fn main() {}
