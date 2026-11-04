#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Clone)]
struct Element(i32);
impl Drop for Element {
    fn drop(&mut self) {
        println!("released {}", self.0);
    }
}
#[derive(Resource)]
struct State {
    tick: u64,
    objects: Vec<i32>,
}
struct Snapshot {
    tick: u64,
    objects: Vec<Element>,
}
impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            objects: self.objects.iter().map(|x| Element(*x)).collect(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.snapshot().tick
}
fn main() {}
