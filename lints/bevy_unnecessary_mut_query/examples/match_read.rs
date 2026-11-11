#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::{Health, show};

fn update(mut query: Query<&mut Health>, entity: Entity) {
    match query.get_mut(entity) {
        Ok(health) => show(health.0),
        Err(_) => show(0),
    }
}

fn main() {}
