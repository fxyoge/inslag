#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

fn inspect(state: Res<State>) -> (u64, Frog, Vec<i32>) {
    let snapshot = state.game.snapshot();
    (snapshot.tick, snapshot.frog, snapshot.objects)
}
fn main() {}
