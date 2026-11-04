#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

fn inspect(mut state: ResMut<State>) {
    state.game.tick = state.game.snapshot().tick + 1;
}
fn main() {}
