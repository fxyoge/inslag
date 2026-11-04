#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

fn inspect(state: Res<State>) -> Snapshot {
    let snapshot = state.game.snapshot();
    let _tick = snapshot.tick;
    snapshot
}
fn main() {}
