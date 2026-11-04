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
        Snapshot {
            count: self.objects.len(),
            objects: self.objects.clone(),
        }
    }
}
fn inspect(state: Res<State>) -> usize {
    state.snapshot().count
}
fn main() {}
