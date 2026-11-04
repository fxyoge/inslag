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
fn count(objects: &[i32]) -> usize {
    objects.len()
}
impl State {
    fn snapshot(&self) -> Snapshot {
        let objects = self.objects.clone();
        Snapshot {
            count: count(&objects),
            objects,
        }
    }
}
fn inspect(state: Res<State>) -> usize {
    state.snapshot().count
}
fn main() {}
