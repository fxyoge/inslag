#![expect(dead_code)]
use std::ops::{Deref, DerefMut};
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/mod.rs"
    ));
}
use support::Settings;

struct Wrapper(Settings);

impl Deref for Wrapper {
    type Target = Settings;
    fn deref(&self) -> &Settings {
        &self.0
    }
}

impl DerefMut for Wrapper {
    fn deref_mut(&mut self) -> &mut Settings {
        &mut self.0
    }
}

impl Wrapper {
    fn outer(&mut self) -> &mut Self {
        self
    }
}

fn configure(wrapper: &mut Wrapper) {
    wrapper.option(1);
    wrapper.outer();
}
fn main() {}
