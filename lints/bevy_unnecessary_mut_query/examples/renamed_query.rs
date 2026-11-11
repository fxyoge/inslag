#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::system::Query as Entities;
use model::{Health, show};

fn update(mut query: Entities<&mut Health>) {
    for health in &mut query {
        show(health.0);
    }
}

fn main() {}
