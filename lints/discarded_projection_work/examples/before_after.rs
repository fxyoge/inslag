#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

fn inspect(mut state: ResMut<State>) -> bool {
    let before = state.game.snapshot();
    state.game.tick += 1;
    let after = state.game.snapshot();
    before.tick != after.tick
}
fn main() {}
