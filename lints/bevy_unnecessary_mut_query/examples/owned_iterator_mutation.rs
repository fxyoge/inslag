#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::Health;

fn update(query: Query<&mut Health>) {
    for mut health in query {
        health.reset();
    }
}

fn main() {}
