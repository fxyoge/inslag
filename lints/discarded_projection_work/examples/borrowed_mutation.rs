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
fn increment(value: &mut u64) {
    *value += 1;
}
impl State {
    fn snapshot(&mut self) -> Snapshot {
        increment(&mut self.tick);
        Snapshot {
            tick: self.tick,
            objects: self.objects.clone(),
        }
    }
}
fn inspect(mut state: ResMut<State>) -> u64 {
    state.snapshot().tick
}
fn main() {}
