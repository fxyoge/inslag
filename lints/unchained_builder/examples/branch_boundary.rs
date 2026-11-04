#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;

fn configure(settings: &mut Settings, condition: bool) {
    settings.option(1);
    if condition {
        settings.option(2);
    }
    settings.option(3);
}
fn main() {}
