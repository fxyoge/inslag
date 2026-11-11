#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::{Health, Shield};

fn update(mut query: Query<(&Shield, &mut Health)>) {
    for mut pair in &mut query {
        pair.1.0 += 1;
    }
}

fn main() {}
