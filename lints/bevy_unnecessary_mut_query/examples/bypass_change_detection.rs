#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::Health;

fn update(mut query: Query<&mut Health>) {
    for mut health in &mut query {
        health.bypass_change_detection().reset();
    }
}

fn main() {}
