#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::{prelude::*, query::QueryIter};
use model::Health;

fn inspect(_: QueryIter<&mut Health, ()>) {}

fn update(mut query: Query<&mut Health>) {
    inspect(query.iter_mut());
}

fn main() {}
