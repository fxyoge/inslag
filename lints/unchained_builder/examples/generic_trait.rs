#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Configure;

fn configure<T: Configure>(settings: &mut T) {
    settings.enabled();
    settings.enabled();
}
fn main() {}
