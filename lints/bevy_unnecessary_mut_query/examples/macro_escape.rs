#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::prelude::*;
use model::Health;

macro_rules! inspect {
    ($value:expr) => {
        std::mem::size_of_val(&$value)
    };
}

fn update(mut query: Query<&mut Health>) {
    for health in &mut query {
        let _size = inspect!(health);
    }
}

fn main() {}
