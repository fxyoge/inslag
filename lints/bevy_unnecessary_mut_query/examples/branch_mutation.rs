#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::{Health, show};

fn update(mut query: Query<&mut Health>, change: bool) {
    for mut health in &mut query {
        show(health.0);
        if change {
            health.reset();
        }
    }
}

fn main() {}
