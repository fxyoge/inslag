#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;
struct Guard(usize);
impl Drop for Guard {
    fn drop(&mut self) {}
}
fn guard() -> Guard {
    Guard(1)
}

fn configure(settings: &mut Settings) {
    settings.option(match guard() {
        Guard(value) => value,
    });
    settings.option(2);
}
fn main() {}
