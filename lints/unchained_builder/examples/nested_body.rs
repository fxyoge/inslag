#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;

fn configure(settings: &mut Settings) {
    let mut configure = || {
        settings.option(1);
        settings.option(2);
    };
    configure();
}
fn main() {}
