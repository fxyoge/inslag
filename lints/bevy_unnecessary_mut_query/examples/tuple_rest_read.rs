#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::{Health, Shield, show};

fn update(mut query: Query<(Entity, &Shield, &mut Health)>) {
    for (.., health) in &mut query {
        show(health.0);
    }
}

fn main() {}
