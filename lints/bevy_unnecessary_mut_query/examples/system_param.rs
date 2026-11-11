#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::{prelude::*, system::SystemParam};
use model::{Health, show};

#[derive(SystemParam)]
struct Params<'w, 's> {
    health: Query<'w, 's, &'static mut Health>,
}

fn update(mut params: Params) {
    for health in &mut params.health {
        show(health.0);
    }
}

fn main() {}
