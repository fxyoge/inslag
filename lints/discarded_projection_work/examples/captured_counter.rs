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
        let mut count = 0;
        let objects = self
            .objects
            .iter()
            .map(|x| {
                count += 1;
                x + 1
            })
            .collect();
        Snapshot { count, objects }
    }
}
fn inspect(state: Res<State>) -> usize {
    state.snapshot().count
}
fn main() {}
