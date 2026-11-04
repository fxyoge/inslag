#![expect(dead_code)]
use bevy_ecs::prelude::*;

struct Element(i32);
impl Clone for Element {
    fn clone(&self) -> Self {
        println!("cloned");
        Self(self.0)
    }
}
#[derive(Resource)]
struct State {
    tick: u64,
    objects: Vec<Element>,
}
struct Snapshot {
    tick: u64,
    objects: Vec<Element>,
}
impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            objects: self.objects.clone(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.snapshot().tick
}
fn main() {}
