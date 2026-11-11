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
    for health in &mut query {
        let mut value = health.map_unchanged(|health| &mut health.0);
        *value = 1;
    }
}

fn main() {}
