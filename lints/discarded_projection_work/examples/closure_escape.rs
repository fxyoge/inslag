#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

fn inspect(state: Res<State>) -> impl FnOnce() -> u64 {
    let snapshot = state.game.snapshot();
    move || snapshot.tick
}
fn main() {}
