#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

fn inspect(state: Res<State>, capture: fn(&Game) -> Snapshot) -> u64 {
    capture(&state.game).tick
}
fn main() {}
