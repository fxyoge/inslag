#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

fn capture(game: &Game) -> Snapshot {
    Snapshot {
        tick: game.tick,
        frog: game.frog,
        objects: game.objects.clone(),
    }
}
fn inspect(state: Res<State>) -> u64 {
    capture(&state.game).tick
}
fn main() {}
