#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::Health;

fn update(mut query: Query<&mut Health>) {
    let mut lens = query.transmute_lens::<&mut Health>();
    for mut health in &mut lens.query() {
        health.reset();
    }
}

fn main() {}
