#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Resource)]
struct State {
    tick: u64,
    cars: Vec<i32>,
    logs: Vec<i32>,
}
struct Snapshot {
    tick: u64,
    cars: Vec<i32>,
    logs: Vec<i32>,
}
impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            cars: self.cars.clone(),
            logs: self.logs.clone(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.snapshot().tick
}
fn main() {}
