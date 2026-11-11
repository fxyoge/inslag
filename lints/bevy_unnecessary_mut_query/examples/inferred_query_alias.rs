#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::{Health, show};

fn update(query: Query<&mut Health>) {
    let mut alias = query;
    for health in &mut alias {
        show(health.0);
    }
}

fn main() {}
