#![expect(dead_code)]
mod model {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/model.rs"
    ));
}
use bevy_ecs::{prelude::*, query::QueryData};
use model::{Health, show};

#[derive(QueryData)]
#[query_data(mutable)]
struct Data {
    health: &'static mut Health,
}

fn update(mut query: Query<Data>) {
    for item in &mut query {
        show(item.health.0);
    }
}

fn main() {}
