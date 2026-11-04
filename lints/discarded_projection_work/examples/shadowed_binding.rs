#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

fn inspect(state: Res<State>) -> u64 {
    let snapshot = state.game.snapshot();
    let tick = snapshot.tick;
    let snapshot = state.game.snapshot();
    std::hint::black_box(snapshot);
    tick
}
fn main() {}
