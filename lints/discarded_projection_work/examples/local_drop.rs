#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        println!("capture finished");
    }
}
impl Game {
    fn guarded(&self) -> Snapshot {
        let _guard = Guard;
        Snapshot {
            tick: self.tick,
            frog: self.frog,
            objects: self.objects.clone(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.game.guarded().tick
}
fn main() {}
