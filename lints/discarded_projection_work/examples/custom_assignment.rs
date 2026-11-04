#![expect(dead_code)]
use bevy_ecs::prelude::*;
use std::ops::AddAssign;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

struct Counter(u64);
impl AddAssign<u64> for Counter {
    fn add_assign(&mut self, value: u64) {
        println!("counting");
        self.0 += value;
    }
}
impl Game {
    fn counted(&self) -> Snapshot {
        let mut count = Counter(0);
        count += self.tick;
        Snapshot {
            tick: count.0,
            frog: self.frog,
            objects: self.objects.clone(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.game.counted().tick
}
fn main() {}
