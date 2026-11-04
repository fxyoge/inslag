#![expect(dead_code)]
use std::ops::{Deref, DerefMut};
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;

struct Wrapper {
    settings: Settings,
    accesses: usize,
}
impl Deref for Wrapper {
    type Target = Settings;
    fn deref(&self) -> &Settings {
        &self.settings
    }
}
impl DerefMut for Wrapper {
    fn deref_mut(&mut self) -> &mut Settings {
        self.accesses += 1;
        &mut self.settings
    }
}
fn configure(wrapper: &mut Wrapper) {
    wrapper.option(1);
    wrapper.option(2);
}
fn main() {}
