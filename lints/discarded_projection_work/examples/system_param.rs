#![expect(dead_code)]
use bevy_ecs::{prelude::*, system::SystemParam};
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use model::*;

#[derive(SystemParam)]
struct Simulation<'w> {
    state: Res<'w, State>,
}
fn inspect(sim: Simulation) -> u64 {
    sim.state.game.snapshot().tick
}
fn main() {}
