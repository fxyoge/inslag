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
        let mut count = 0;
        if !objects.is_empty() {
            count = objects.len();
        }
        Snapshot { count, objects }
    }
}
fn inspect(state: Res<State>) -> usize {
    state.snapshot().count
}
fn main() {}
