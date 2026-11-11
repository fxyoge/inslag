#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::Health;

fn change(health: &mut Health) {
    health.reset();
}

fn update(mut query: Query<&mut Health>) {
    for mut health in &mut query {
        change(&mut health);
    }
}

fn main() {}
