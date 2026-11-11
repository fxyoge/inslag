#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::Health;

fn inspect(_: &Mut<Health>) {}

fn update(mut query: Query<&mut Health>) {
    for health in &mut query {
        inspect(&health);
    }
}

fn main() {}
