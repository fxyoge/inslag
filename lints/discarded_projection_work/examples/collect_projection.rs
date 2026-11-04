#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

impl Game {
    fn projected(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            frog: self.frog,
            objects: self.objects.iter().map(|x| x + 1).collect(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.game.projected().tick
}
fn main() {}
