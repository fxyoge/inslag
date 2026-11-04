#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Resource)]
struct State {
    tick: u64,
    objects: Vec<i32>,
}
struct Snapshot {
    tick: u64,
    objects: Vec<i32>,
}
impl State {
    fn snapshot(&self) -> Snapshot {
        let _count = self.objects.len();
        Snapshot {
            tick: self.tick,
            objects: {
                let _tick = self.tick;
                Vec::new()
            },
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.snapshot().tick
}
fn main() {}
