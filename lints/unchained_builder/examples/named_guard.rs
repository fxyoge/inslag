#![expect(dead_code)]
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {}
}
impl Guard {
    fn value(&self) -> usize {
        1
    }
}

fn configure(settings: &mut Settings, guard: Guard) {
    settings.option(guard.value());
    settings.option(2);
}
fn main() {}
