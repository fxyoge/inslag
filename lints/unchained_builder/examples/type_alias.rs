#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;
type OtherName = Settings;

fn configure(settings: &mut Settings) {
    OtherName::option(settings, 1);
    settings.option(2);
}
fn main() {}
