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
fn constants(_state: &State) -> Vec<i32> {
    [1, 2, 3].iter().map(|x| x * 2).collect()
}
impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            objects: constants(self),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.snapshot().tick
}
fn main() {}
