#![allow(dead_code)]

pub struct StateTreeInstance;

impl StateTreeInstance {
    pub fn data(&self, _: &str) -> Option<()> {
        None
    }
}

fn main() {
    let _ = StateTreeInstance.data("private");
}
