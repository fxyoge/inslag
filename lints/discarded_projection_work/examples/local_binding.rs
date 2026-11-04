#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

fn inspect(state: Res<State>) -> (u64, bool) {
    let snapshot = state.game.snapshot();
    (snapshot.tick, snapshot.frog.hopping)
}
fn main() {}
