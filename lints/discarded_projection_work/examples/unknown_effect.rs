#![expect(dead_code)]
use bevy_ecs::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

static CAPTURES: AtomicU64 = AtomicU64::new(0);
impl Game {
    fn counted(&self) -> Snapshot {
        CAPTURES.fetch_add(1, Ordering::Relaxed);
        Snapshot {
            tick: self.tick,
            frog: self.frog,
            objects: self.objects.clone(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.game.counted().tick
}
fn main() {}
