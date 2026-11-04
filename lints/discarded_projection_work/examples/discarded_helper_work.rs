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
fn empty(state: &State) -> Vec<i32> {
    let _unused = state.objects.clone();
    Vec::new()
}
impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            objects: empty(self),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.snapshot().tick
}
fn main() {}
