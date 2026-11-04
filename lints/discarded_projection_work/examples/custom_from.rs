#![expect(dead_code)]
use bevy_ecs::prelude::*;

struct Count(u64);
impl From<u64> for Count {
    fn from(value: u64) -> Self {
        println!("converted");
        Self(value)
    }
}
#[derive(Resource)]
struct State {
    tick: u64,
    objects: Vec<i32>,
}
struct Snapshot {
    tick: Count,
    objects: Vec<i32>,
}
impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: Count::from(self.tick),
            objects: self.objects.clone(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.snapshot().tick.0
}
fn main() {}
