#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::{Health, Shield, show};

fn update(mut query: Query<(&Shield, &mut Health)>) {
    for pair in &mut query {
        show(pair.1.0);
    }
}

fn main() {}
