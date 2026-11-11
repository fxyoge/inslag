#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::{Health, show};

type HealthQuery<'w, 's> = Query<'w, 's, &'static mut Health>;

fn update(mut query: HealthQuery) {
    for health in &mut query {
        show(health.0);
    }
}

fn main() {}
