#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;

fn plain(settings: &mut Settings) {
    settings.option(1);
    settings.option(2);
}

fn main() {}
