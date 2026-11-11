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
    if let Ok(health) = query.get_mut(entity) {
        show(health.0);
    }
    query.get_mut(entity).unwrap().reset();
}

fn main() {}
