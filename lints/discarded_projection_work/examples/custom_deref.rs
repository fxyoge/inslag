#![expect(dead_code)]
use bevy_ecs::prelude::*;
use std::ops::Deref;

struct Objects(Vec<i32>);
impl Deref for Objects {
    type Target = Vec<i32>;
    fn deref(&self) -> &Self::Target {
        println!("accessed");
        &self.0
    }
}
#[derive(Resource)]
struct State {
    tick: u64,
    objects: Objects,
}
struct Snapshot {
    tick: u64,
    objects: Vec<i32>,
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
