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
    let mut snapshot = state.game.snapshot();
    let tick = &mut snapshot.tick;
    *tick += 1;
    *tick
}
fn main() {}
