#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::{Health, show};

#[cfg_attr(
    dylint_lib = "bevy_unnecessary_mut_query",
    allow(bevy_unnecessary_mut_query)
)]
fn update(mut query: Query<&mut Health>) {
    for health in &mut query {
        show(health.0);
    }
}

fn main() {}
