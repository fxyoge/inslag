#![expect(dead_code)]
use bevy_ecs::prelude::*;
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

#[derive(Resource)]
struct History(Vec<Snapshot>);
fn checkpoint(state: Res<State>, mut history: ResMut<History>) {
    let snapshot = state.game.snapshot();
    let _tick = snapshot.tick;
    history.0.push(snapshot);
}
fn main() {}
