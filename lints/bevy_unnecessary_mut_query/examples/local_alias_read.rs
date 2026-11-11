#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::{Health, show};

fn update(mut query: Query<&mut Health>) {
    for health in &mut query {
        let alias = health;
        show(alias.0);
    }
}

fn main() {}
