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
    fn copied(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            frog: self.frog,
            objects: self.objects.iter().copied().map(|x| x + 1).collect(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.game.copied().tick
}
fn main() {}
