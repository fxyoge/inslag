#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Resource)]
struct State {
    tick: u64,
    first: i64,
    last: i64,
}
struct Snapshot {
    tick: u64,
    objects: Vec<i64>,
}
impl State {
    fn snapshot(&self) -> Snapshot {
        let mut objects = Vec::new();
        for x in self.first..=self.last {
            objects.push(x);
        }
        Snapshot {
            tick: self.tick,
            objects,
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.snapshot().tick
}
fn main() {}
