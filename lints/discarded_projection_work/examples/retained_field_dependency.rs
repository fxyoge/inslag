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
        let objects: Vec<_> = self.objects.iter().map(|x| x + 1).collect();
        let count = objects.len();
        Snapshot { count, objects }
    }
}
fn inspect(state: Res<State>) -> usize {
    state.snapshot().count
}
fn main() {}
