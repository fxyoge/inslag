#![expect(dead_code)]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/support/model.rs"
));
use bevy_ecs::system::Query as Q;

fn explicit<'w, 's>(query: Q<'w, 's, &'static Health>) -> bool {
    query.is_empty()
}

fn generic<T: Component>(query: Q<&T, With<Shield>>) -> bool {
    query.is_empty()
}

fn main() {}
