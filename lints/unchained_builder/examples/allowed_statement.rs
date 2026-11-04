#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;

fn configure(settings: &mut Settings) {
    #[cfg_attr(dylint_lib = "unchained_builder", expect(unchained_builder))]
    settings.option(1);
    settings.option(2);
}
fn main() {}
