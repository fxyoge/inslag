#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;

fn configure(settings: &mut Settings) {
    settings.option(1).option(2);
    settings.option(settings.value()).option(3);
}
fn main() {}
