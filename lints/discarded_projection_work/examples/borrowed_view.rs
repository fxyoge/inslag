#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Resource)]
struct State {
    tick: u64,
    objects: Vec<i32>,
}
struct View<'a> {
    tick: u64,
    objects: &'a [i32],
}
impl State {
    fn view(&self) -> View<'_> {
        View {
            tick: self.tick,
            objects: &self.objects,
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.view().tick
}
fn main() {}
