#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

fn project(objects: &[i32]) -> Vec<i32> {
    let mut values = Vec::new();
    for value in objects {
        values.push(*value + 1);
    }
    values
}
impl Game {
    fn projected(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            frog: self.frog,
            objects: project(&self.objects),
        }
    }
}
fn inspect(state: Res<State>) -> u64 {
    state.game.projected().tick
}
fn main() {}
