#![expect(dead_code)]
use bevy_ecs::prelude::*;

struct Count(usize);
impl FromIterator<i32> for Count {
    fn from_iter<T: IntoIterator<Item = i32>>(iter: T) -> Self {
        println!("counting");
        Self(iter.into_iter().count())
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
    count: Count,
}
impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            objects: self.objects.clone(),
            count: self.objects.iter().copied().collect(),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.snapshot().tick
}
fn main() {}
