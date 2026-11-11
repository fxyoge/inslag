#![expect(dead_code)]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/support/model.rs"
));

fn unknown_component<T: Component>(query: Query<&T, Changed<Health>>) -> bool {
    query.is_empty()
}

fn unknown_filter_component<T: Component>(query: Query<&Health, Added<T>>) -> bool {
    query.is_empty()
}

fn unknown_both<T: Component, U: Component>(query: Query<&T, Changed<U>>) -> bool {
    query.is_empty()
}

trait HasHealth {
    type Value: Component;
}

fn projection<T: HasHealth<Value = Health>>(query: Query<&T::Value, Changed<Health>>) -> bool {
    query.is_empty()
}

#[derive(Component)]
struct Wrapper<T: Component>(T);

fn nested_generic<T: Component>(query: Query<&Wrapper<T>, Changed<Wrapper<Health>>>) -> bool {
    query.is_empty()
}

fn main() {}
