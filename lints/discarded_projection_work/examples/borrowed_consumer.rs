#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

fn consume(snapshot: &Snapshot) {
    std::hint::black_box(snapshot);
}
fn inspect(state: Res<State>) -> u64 {
    let snapshot = state.game.snapshot();
    consume(&snapshot);
    snapshot.tick
}
fn main() {}
