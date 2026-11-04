#![expect(dead_code)]
use bevy_ecs::prelude::*;

struct Values;
impl Iterator for Values {
    type Item = i32;
    fn next(&mut self) -> Option<i32> {
        println!("advanced");
        None
    }
}
#[derive(Resource)]
struct State {
    tick: u64,
    objects: Vec<i32>,
}
struct Snapshot {
    tick: u64,
    objects: Vec<i32>,
    extra: Vec<i32>,
}
impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            objects: self.objects.clone(),
            extra: Values.collect(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.snapshot().tick
}
fn main() {}
