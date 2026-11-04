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
    let game = Game {
        tick: state.game.tick,
        frog: state.game.frog,
        objects: Vec::new(),
    };
    game.snapshot().tick
}
fn main() {}
