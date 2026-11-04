#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Resource)]
struct State {
    objects: Vec<i32>,
}
struct Snapshot {
    count: usize,
    objects: Vec<i32>,
}
impl State {
    fn snapshot(&self) -> Snapshot {
        let objects = self.objects.clone();
        let count = objects.len();
        let alias = objects;
        Snapshot {
            count,
            objects: alias,
        }
    }
}
fn inspect(state: Res<State>) -> usize {
    state.snapshot().count
}
fn main() {}
